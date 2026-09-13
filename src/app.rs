use std::time::Duration;

use iced::widget::{button, column, container, progress_bar, row, scrollable, slider, space, text};
use iced::{Alignment, Element, Fill, Length, Subscription, Task, Theme};

use crate::device::{
    DetectedDevice, DeviceLocation, DeviceWatchEvent, DiscoveryError, DiscoveryReport,
    FeedbackSnapshot, MonitorToggle, UnknownAudientDevice, discover, read_feedback,
    send_channel_level, send_channel_polarity, send_digital_output_mode, send_headphone_level,
    send_monitor_toggle, send_output_route, send_speaker_level, support_label, support_level,
    watch_events,
};
use crate::mixer::{
    ChannelStrip, channel_name, meter_feedback_available, mixer_available, mixer_channel_count,
};
use crate::monitor::{
    ToggleControl, VolumeControl, monitor_controls_available, monitor_toggles_available,
    speaker_feedback_available, toggle_feedback_available,
};
use crate::routing::{
    DigitalOutputMode, OutputRouteControl, Route, RouteStatus, RoutingDestination,
    digital_output_mode_available, reset_route, routing_available,
};

mod route_state;
use route_state::fresh_routes_for;

/// Meter polls run at 10 Hz: the slowest rate that still reads as motion.
/// Monitor state rides the same timer at ~1 Hz (see [`monitor_due`]).
const METER_CADENCE_MS: u64 = 100;
const MONITOR_CADENCE_MS: u64 = 1_000;

struct App {
    status: DeviceStatus,
    selected: Option<DeviceLocation>,
    scan_in_flight: bool,
    rescan_requested: bool,
    watch_status: WatchStatus,
    speaker: VolumeControl,
    headphone: VolumeControl,
    routes: Vec<OutputRouteControl>,
    digital_output_mode: ToggleControl,
    toggles: [ToggleControl; 5],
    channels: Vec<ChannelStrip>,
    /// One level byte per mixer input while meters are supported, `None`
    /// per input while the level is unknown. Unknown is never shown as zero.
    meters: Vec<Option<u8>>,
    /// Whether a background feedback poll currently owns a device session.
    /// Ticks arriving while this is set are skipped, so polls can never
    /// overlap or queue behind each other.
    feedback_in_flight: bool,
    /// Counts feedback ticks to fold the ~1 Hz monitor refresh into the
    /// faster meter cadence on one session per tick.
    feedback_tick: u64,
    /// The last feedback failure, shown once until a poll succeeds or the
    /// device set changes. Last confirmed values stay visible underneath.
    feedback_notice: Option<String>,
}

/// Outcome of one background speaker-volume send, paired with the level it
/// attempted so stale completions can be ignored after a rescan.
#[derive(Clone, Debug)]
struct SpeakerVolumeOutcome {
    level: f32,
    result: Result<(), String>,
}

/// Outcome of one background headphone-volume send, paired with the level
/// it attempted so stale completions can be ignored after a rescan.
#[derive(Clone, Debug)]
struct HeadphoneVolumeOutcome {
    level: f32,
    result: Result<(), String>,
}

/// Outcome of one background monitor-toggle send, paired with the toggle and
/// value it attempted so stale completions can be ignored after a rescan.
#[derive(Clone, Debug)]
struct MonitorToggleOutcome {
    toggle: MonitorToggle,
    on: bool,
    result: Result<(), String>,
}

/// Outcome of one background channel-level send, paired with the channel
/// and level it attempted so stale completions can be ignored.
#[derive(Clone, Debug)]
struct ChannelLevelOutcome {
    channel: u8,
    level: f32,
    result: Result<(), String>,
}

/// Outcome of one background channel-polarity send, paired with the channel
/// and value it attempted so stale completions can be ignored.
#[derive(Clone, Debug)]
struct ChannelPolarityOutcome {
    channel: u8,
    flipped: bool,
    result: Result<(), String>,
}

/// Outcome of one output-pair routing send. The complete typed route is its
/// identity, so a completion from an older attachment cannot update new state.
#[derive(Clone, Debug)]
struct RoutingOutcome {
    route: Route,
    result: Result<(), String>,
}

#[derive(Clone, Debug)]
struct DigitalOutputModeOutcome {
    mode: DigitalOutputMode,
    result: Result<(), String>,
}

/// Outcome of one background feedback poll, paired with the attachment it
/// read so completions from a replaced device are ignored.
#[derive(Clone, Debug)]
struct FeedbackOutcome {
    location: DeviceLocation,
    read_monitor: bool,
    result: Result<FeedbackSnapshot, String>,
}

#[derive(Clone, Debug)]
enum Message {
    Refresh,
    DeviceSelected(DeviceLocation),
    DeviceWatch(DeviceWatchEvent),
    DiscoveryFinished(Result<DiscoveryReport, DiscoveryError>),
    SpeakerVolumeChanged(f32),
    SpeakerVolumeFinished(SpeakerVolumeOutcome),
    HeadphoneVolumeChanged(f32),
    HeadphoneVolumeFinished(HeadphoneVolumeOutcome),
    RouteSelected(Route),
    RouteReset(RoutingDestination),
    RoutingFinished(RoutingOutcome),
    DigitalOutputModeSelected(DigitalOutputMode),
    DigitalOutputModeFinished(DigitalOutputModeOutcome),
    MonitorToggleChanged { toggle: MonitorToggle, on: bool },
    MonitorToggleFinished(MonitorToggleOutcome),
    ChannelLevelChanged { channel: u8, level: f32 },
    ChannelLevelFinished(ChannelLevelOutcome),
    ChannelPolarityChanged { channel: u8, flipped: bool },
    ChannelPolarityFinished(ChannelPolarityOutcome),
    FeedbackTick,
    FeedbackFinished(FeedbackOutcome),
}

enum DeviceStatus {
    Scanning,
    Ready(DiscoveryReport),
    Unsupported(DiscoveryReport),
    Empty,
    Failed(DiscoveryError),
}

enum WatchStatus {
    Starting,
    Active,
    Failed,
}

pub(crate) fn run() -> iced::Result {
    iced::application(App::new, update, view)
        .title("Selah")
        .theme(theme)
        .subscription(subscription)
        .window_size((760.0, 520.0))
        .centered()
        .run()
}

impl App {
    fn new() -> (Self, Task<Message>) {
        (
            Self {
                status: DeviceStatus::Scanning,
                selected: None,
                scan_in_flight: false,
                rescan_requested: false,
                watch_status: WatchStatus::Starting,
                speaker: VolumeControl::default(),
                headphone: VolumeControl::default(),
                routes: Vec::new(),
                digital_output_mode: ToggleControl::default(),
                toggles: Default::default(),
                channels: Vec::new(),
                meters: Vec::new(),
                feedback_in_flight: false,
                feedback_tick: 0,
                feedback_notice: None,
            },
            Task::none(),
        )
    }
}

/// Index of a toggle in `App::toggles`, matching `MonitorToggle::ALL` order.
fn toggle_index(toggle: MonitorToggle) -> usize {
    MonitorToggle::ALL
        .iter()
        .position(|candidate| *candidate == toggle)
        .expect("every monitor toggle is listed in ALL")
}

/// Drops per-device state for a new or rescanned selection, then schedules
/// one hardware refresh. Shared by explicit picks and discovery completions
/// so both rebuild routes, channels, and meters for the same device.
fn adopt_selection(app: &mut App) -> Task<Message> {
    let selected = selected_device(app);
    app.speaker = VolumeControl::default();
    app.headphone = VolumeControl::default();
    app.toggles = Default::default();
    app.routes = selected.as_ref().map(fresh_routes_for).unwrap_or_default();
    app.digital_output_mode = ToggleControl::default();
    app.channels = selected
        .as_ref()
        .map(fresh_channels_for)
        .unwrap_or_default();
    // A (re)connected device starts unknown: old local values are
    // never restored as confirmed, and the refresh below adopts
    // whatever the hardware actually reports.
    app.meters = selected.as_ref().map(fresh_meters_for).unwrap_or_default();
    app.feedback_notice = None;
    app.feedback_in_flight = false;
    app.feedback_tick = 0;
    let scan = finish_scan(app);
    Task::batch([scan, refresh_task(app, true)])
}

fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::DeviceSelected(location) => {
            app.selected = Some(location);
            adopt_selection(app)
        }
        Message::DeviceWatch(event) => device_watch_event(app, event),
        Message::Refresh => request_scan(app),
        Message::DiscoveryFinished(Ok(report)) if !report.supported.is_empty() => {
            tracing::info!(
                supported = report.supported.len(),
                unsupported = report.unsupported.len(),
                "Audient device scan completed"
            );
            let still_there = app
                .selected
                .as_ref()
                .is_some_and(|wanted| report.supported.iter().any(|d| &d.location == wanted));
            if !still_there {
                app.selected = None;
            }
            app.status = DeviceStatus::Ready(report);
            adopt_selection(app)
        }
        Message::DiscoveryFinished(Ok(report)) if !report.unsupported.is_empty() => {
            tracing::warn!(
                unsupported = report.unsupported.len(),
                "Found an unrecognized Audient interface"
            );
            app.status = DeviceStatus::Unsupported(report);
            app.selected = None;
            app.speaker = VolumeControl::default();
            app.headphone = VolumeControl::default();
            app.toggles = Default::default();
            app.routes = Vec::new();
            app.digital_output_mode = ToggleControl::default();
            app.channels = Vec::new();
            app.meters = Vec::new();
            app.feedback_notice = None;
            app.feedback_in_flight = false;
            finish_scan(app)
        }
        Message::DiscoveryFinished(Ok(_)) => {
            tracing::info!("No Audient interface detected");
            app.status = DeviceStatus::Empty;
            app.selected = None;
            app.speaker = VolumeControl::default();
            app.headphone = VolumeControl::default();
            app.toggles = Default::default();
            app.routes = Vec::new();
            app.digital_output_mode = ToggleControl::default();
            app.channels = Vec::new();
            app.meters = Vec::new();
            app.feedback_notice = None;
            app.feedback_in_flight = false;
            finish_scan(app)
        }
        Message::DiscoveryFinished(Err(error)) => {
            tracing::error!(%error, "Audient device scan failed");
            app.status = DeviceStatus::Failed(error);
            app.selected = None;
            app.speaker = VolumeControl::default();
            app.headphone = VolumeControl::default();
            app.toggles = Default::default();
            app.routes = Vec::new();
            app.digital_output_mode = ToggleControl::default();
            app.channels = Vec::new();
            app.meters = Vec::new();
            app.feedback_notice = None;
            app.feedback_in_flight = false;
            finish_scan(app)
        }
        Message::SpeakerVolumeChanged(level) => request_speaker_volume(app, level),
        Message::SpeakerVolumeFinished(outcome) => finish_speaker_volume(app, outcome),
        Message::HeadphoneVolumeChanged(level) => request_headphone_volume(app, level),
        Message::HeadphoneVolumeFinished(outcome) => finish_headphone_volume(app, outcome),
        Message::RouteSelected(route) => request_routing(app, route),
        Message::RouteReset(destination) => request_route_reset(app, destination),
        Message::RoutingFinished(outcome) => finish_routing(app, outcome),
        Message::DigitalOutputModeSelected(mode) => request_digital_output_mode(app, mode),
        Message::DigitalOutputModeFinished(outcome) => finish_digital_output_mode(app, outcome),
        Message::MonitorToggleChanged { toggle, on } => request_monitor_toggle(app, toggle, on),
        Message::MonitorToggleFinished(outcome) => finish_monitor_toggle(app, outcome),
        Message::ChannelLevelChanged { channel, level } => {
            request_channel_level(app, channel, level)
        }
        Message::ChannelLevelFinished(outcome) => finish_channel_level(app, outcome),
        Message::ChannelPolarityChanged { channel, flipped } => {
            request_channel_polarity(app, channel, flipped)
        }
        Message::ChannelPolarityFinished(outcome) => finish_channel_polarity(app, outcome),
        Message::FeedbackTick => request_feedback(app),
        Message::FeedbackFinished(outcome) => finish_feedback(app, outcome),
    }
}

/// Handles the operating-system USB monitor without polling.
///
/// Connection changes trigger a fresh descriptor scan; a failed monitor
/// falls back to the manual scan button.
fn device_watch_event(app: &mut App, event: DeviceWatchEvent) -> Task<Message> {
    match event {
        DeviceWatchEvent::Started => {
            tracing::info!("Watching for USB device changes");
            app.watch_status = WatchStatus::Active;
            request_scan(app)
        }
        DeviceWatchEvent::DevicesChanged => request_scan(app),
        DeviceWatchEvent::Failed(error) => {
            tracing::warn!(%error, "Automatic USB device detection is unavailable");
            app.watch_status = WatchStatus::Failed;
            request_scan(app)
        }
    }
}

/// Builds a fresh strip per input of one explicitly selected device.
fn fresh_channels_for(device: &DetectedDevice) -> Vec<ChannelStrip> {
    let count = mixer_channel_count(device.model) as usize;
    std::iter::repeat_with(ChannelStrip::default)
        .take(count)
        .collect()
}

/// Builds one unknown meter slot per mixer input of one explicitly
/// selected device.
fn fresh_meters_for(device: &DetectedDevice) -> Vec<Option<u8>> {
    let count = mixer_channel_count(device.model) as usize;
    std::iter::repeat_with(|| None).take(count).collect()
}

/// How often the device may be polled, or `None` when no feedback is
/// supported. Meters need ~10 Hz to read as motion; monitor state follows
/// at ~1 Hz. Anything without a readable control or a meter source gets no
/// timer at all, so Selah stays quiet instead of polling blindly.
fn feedback_cadence(app: &App) -> Option<Duration> {
    feedback_cadence_ms(app).map(Duration::from_millis)
}

fn feedback_cadence_ms(app: &App) -> Option<u64> {
    let device = selected_device(app)?;
    device.control_interface?;
    if meter_feedback_available(&device) {
        Some(METER_CADENCE_MS)
    } else if speaker_feedback_available(&device)
        || toggle_feedback_available(&device)
        || digital_output_mode_available(&device)
    {
        Some(MONITOR_CADENCE_MS)
    } else {
        None
    }
}

/// Whether this tick also refreshes monitor state. The monitor snapshot
/// rides every ~1 s on top of the meter cadence, on the same session.
fn monitor_due(tick: u64, cadence_ms: u64) -> bool {
    tick.is_multiple_of((MONITOR_CADENCE_MS / cadence_ms.max(1)).max(1))
}

/// Starts one bounded feedback poll unless one already owns a session.
///
/// Ticks arriving while a poll is in flight are dropped, never queued: at
/// most one session exists per tick, each tick holds it briefly, and the
/// subscription disappears entirely when no supported device is present.
fn request_feedback(app: &mut App) -> Task<Message> {
    let Some(cadence_ms) = feedback_cadence_ms(app) else {
        return Task::none();
    };
    if app.feedback_in_flight {
        return Task::none();
    }
    app.feedback_tick = app.feedback_tick.wrapping_add(1);
    refresh_task(app, monitor_due(app.feedback_tick, cadence_ms))
}

/// One immediate hardware refresh on (re)connect, outside the tick cadence.
fn refresh_task(app: &mut App, read_monitor: bool) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };
    if feedback_cadence(app).is_none() {
        return Task::none();
    }
    app.feedback_in_flight = true;
    let location = device.location.clone();
    Task::perform(
        async move {
            FeedbackOutcome {
                location,
                read_monitor,
                result: read_feedback(device, read_monitor).await,
            }
        },
        Message::FeedbackFinished,
    )
}

/// Adopts a feedback poll result: hardware values win over local state.
///
/// Monitor fields the device answered become confirmed and move their
/// controls; controls with a send in flight keep their pending value until
/// the send completes. Meter levels replace the previous block only when
/// their length still matches the current strips, so a completion from a
/// replaced device cannot resize or shift live meters. A failed poll blanks
/// the meters — a frozen meter presented as live would be a lie — but keeps
/// the last confirmed monitor values underneath a retry notice.
fn finish_feedback(app: &mut App, outcome: FeedbackOutcome) -> Task<Message> {
    let current = selected_device(app).map(|device| device.location.clone());
    if current.as_ref() != Some(&outcome.location) {
        // A completion from a replaced attachment: it must neither adopt
        // state nor clear a newer poll's in-flight flag. With no device at
        // all the flag cannot belong to a running task, so reset it.
        if current.is_none() {
            app.feedback_in_flight = false;
        }
        return Task::none();
    }
    app.feedback_in_flight = false;
    match outcome.result {
        Ok(snapshot) => {
            app.feedback_notice = None;
            if outcome.read_monitor
                && let Some(monitor) = snapshot.monitor
            {
                adopt_monitor_snapshot(app, &monitor);
            }
            adopt_meter_levels(app, snapshot.meters);
        }
        Err(error) => {
            if app.feedback_notice.is_none() {
                tracing::warn!(%error, "Device feedback is unavailable");
            } else {
                tracing::debug!(%error, "Device feedback poll failed");
            }
            app.meters.fill(None);
            app.feedback_notice = Some(error);
        }
    }
    Task::none()
}

fn adopt_monitor_snapshot(app: &mut App, monitor: &crate::device::MonitorSnapshot) {
    if let Some(level) = monitor.speaker_level
        && app.speaker.apply_confirmed(level)
    {
        tracing::debug!(level, "Speaker level confirmed by hardware");
    }
    for (toggle, on) in &monitor.toggles {
        if app.toggles[toggle_index(*toggle)].apply_confirmed(*on) {
            tracing::debug!(?toggle, on, "Monitor toggle confirmed by hardware");
        }
    }
    if let Some(mode) = monitor.digital_output_mode
        && app.digital_output_mode.apply_confirmed(mode.as_toggle())
    {
        tracing::debug!(
            mode = mode.label(),
            "Digital output mode confirmed by hardware"
        );
    }
}

fn adopt_meter_levels(app: &mut App, meters: Option<Vec<u8>>) {
    match meters {
        Some(levels) if levels.len() == app.meters.len() => {
            for (slot, level) in app.meters.iter_mut().zip(levels) {
                *slot = Some(level);
            }
        }
        Some(_) => {
            tracing::debug!("Ignoring meter block from a replaced device");
        }
        None => {
            app.meters.fill(None);
        }
    }
}

/// Starts a background speaker-volume send, queuing when one is in flight.
///
/// The USB work runs in the returned task, away from Iced's UI thread.
fn request_speaker_volume(app: &mut App, level: f32) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };

    match app.speaker.request(level) {
        Some(send) => volume_task(device, send),
        None => Task::none(),
    }
}

/// Records a background send result and starts the queued level, if any.
fn finish_speaker_volume(app: &mut App, outcome: SpeakerVolumeOutcome) -> Task<Message> {
    let SpeakerVolumeOutcome { level, result } = outcome;
    if !app.speaker.is_in_flight(level) {
        return Task::none();
    }

    match &result {
        Ok(()) => tracing::info!(level, "Speaker volume sent"),
        Err(error) => tracing::warn!(level, %error, "Speaker volume send failed"),
    }

    match app.speaker.finish(level, result) {
        Some(next) => {
            if let Some(device) = selected_device(app) {
                volume_task(device, next)
            } else {
                app.speaker.drop_pending();
                Task::none()
            }
        }
        None => Task::none(),
    }
}

/// Starts a background headphone-volume send, queuing when one is in flight.
///
/// The USB work runs in the returned task, away from Iced's UI thread.
fn request_headphone_volume(app: &mut App, level: f32) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };

    match app.headphone.request(level) {
        Some(send) => headphone_task(device, send),
        None => Task::none(),
    }
}

/// Records a background send result and starts the queued level, if any.
fn finish_headphone_volume(app: &mut App, outcome: HeadphoneVolumeOutcome) -> Task<Message> {
    let HeadphoneVolumeOutcome { level, result } = outcome;
    if !app.headphone.is_in_flight(level) {
        return Task::none();
    }

    match &result {
        Ok(()) => tracing::info!(level, "Headphone volume sent"),
        Err(error) => tracing::warn!(level, %error, "Headphone volume send failed"),
    }

    match app.headphone.finish(level, result) {
        Some(next) => {
            if let Some(device) = selected_device(app) {
                headphone_task(device, next)
            } else {
                app.headphone.drop_pending();
                Task::none()
            }
        }
        None => Task::none(),
    }
}

fn route_control(
    app: &mut App,
    destination: RoutingDestination,
) -> Option<&mut OutputRouteControl> {
    app.routes
        .iter_mut()
        .find(|output| output.destination == destination)
}

/// Starts a named output route; ignored while that output has one in flight.
///
/// The USB work runs in the returned task, away from Iced's UI thread.
fn request_routing(app: &mut App, route: Route) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };
    let Some(output) = route_control(app, route.destination) else {
        return Task::none();
    };

    match output.control.request(route.source) {
        Some(_) => routing_task(device, route),
        None => Task::none(),
    }
}

fn request_route_reset(app: &mut App, destination: RoutingDestination) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };
    let Ok(route) = reset_route(device.model, destination) else {
        return Task::none();
    };
    request_routing(app, route)
}

/// Records a routing send result; stale completions after a rescan are ignored.
fn finish_routing(app: &mut App, outcome: RoutingOutcome) -> Task<Message> {
    let RoutingOutcome { route, result } = outcome;
    let Some(output) = route_control(app, route.destination) else {
        return Task::none();
    };
    if !output.control.is_in_flight(route.source) {
        return Task::none();
    }

    match &result {
        Ok(()) => tracing::info!(
            destination = %route.destination.label(),
            source = route.source.label(),
            "Output route sent"
        ),
        Err(error) => tracing::warn!(
            destination = %route.destination.label(),
            source = route.source.label(),
            %error,
            "Output route send failed"
        ),
    }

    let _ = output.control.finish(route.source, result);
    Task::none()
}

fn request_digital_output_mode(app: &mut App, mode: DigitalOutputMode) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };
    if !digital_output_mode_available(&device) {
        return Task::none();
    }
    match app.digital_output_mode.request(mode.as_toggle()) {
        Some(_) => digital_output_mode_task(device, mode),
        None => Task::none(),
    }
}

fn finish_digital_output_mode(app: &mut App, outcome: DigitalOutputModeOutcome) -> Task<Message> {
    let DigitalOutputModeOutcome { mode, result } = outcome;
    let value = mode.as_toggle();
    if !app.digital_output_mode.is_in_flight(value) {
        return Task::none();
    }
    match &result {
        Ok(()) => tracing::info!(mode = mode.label(), "Digital output mode sent"),
        Err(error) => tracing::warn!(mode = mode.label(), %error, "Digital output mode failed"),
    }
    let _ = app.digital_output_mode.finish(value, result);
    Task::none()
}

/// Starts a monitor-toggle send; ignored while that toggle has one in flight.
///
/// The USB work runs in the returned task, away from Iced's UI thread.
fn request_monitor_toggle(app: &mut App, toggle: MonitorToggle, on: bool) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };

    match app.toggles[toggle_index(toggle)].request(on) {
        Some(send) => monitor_toggle_task(device, toggle, send),
        None => Task::none(),
    }
}

/// Records a toggle send result; stale completions after a rescan are ignored.
fn finish_monitor_toggle(app: &mut App, outcome: MonitorToggleOutcome) -> Task<Message> {
    let MonitorToggleOutcome { toggle, on, result } = outcome;
    let control = &mut app.toggles[toggle_index(toggle)];
    if !control.is_in_flight(on) {
        return Task::none();
    }

    match &result {
        Ok(()) => tracing::info!(?toggle, on, "Monitor toggle sent"),
        Err(error) => tracing::warn!(?toggle, on, %error, "Monitor toggle send failed"),
    }

    let _ = control.finish(on, result);
    Task::none()
}

/// Looks up the strip for a channel, ignoring completions for strips that
/// no longer exist after a rescan or model change.
fn channel_strip(app: &mut App, channel: u8) -> Option<&mut ChannelStrip> {
    app.channels.get_mut(channel as usize)
}

/// Starts a background channel-level send, queuing when one is in flight.
///
/// The USB work runs in the returned task, away from Iced's UI thread.
fn request_channel_level(app: &mut App, channel: u8, level: f32) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };
    let Some(strip) = channel_strip(app, channel) else {
        return Task::none();
    };

    match strip.level.request(level) {
        Some(send) => channel_level_task(device, channel, send),
        None => Task::none(),
    }
}

/// Records a channel-level result and starts the queued level, if any.
fn finish_channel_level(app: &mut App, outcome: ChannelLevelOutcome) -> Task<Message> {
    let ChannelLevelOutcome {
        channel,
        level,
        result,
    } = outcome;
    let Some(strip) = channel_strip(app, channel) else {
        return Task::none();
    };
    if !strip.level.is_in_flight(level) {
        return Task::none();
    }

    match &result {
        Ok(()) => tracing::info!(channel, level, "Channel level sent"),
        Err(error) => tracing::warn!(channel, level, %error, "Channel level send failed"),
    }

    let next = strip.level.finish(level, result);
    match next {
        Some(next_level) => {
            if let Some(device) = selected_device(app) {
                channel_level_task(device, channel, next_level)
            } else if let Some(strip) = channel_strip(app, channel) {
                strip.level.drop_pending();
                Task::none()
            } else {
                Task::none()
            }
        }
        None => Task::none(),
    }
}

/// Starts a channel-polarity send; ignored while one is in flight.
///
/// The USB work runs in the returned task, away from Iced's UI thread.
fn request_channel_polarity(app: &mut App, channel: u8, flipped: bool) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };
    let Some(strip) = channel_strip(app, channel) else {
        return Task::none();
    };

    match strip.polarity.request(flipped) {
        Some(send) => channel_polarity_task(device, channel, send),
        None => Task::none(),
    }
}

/// Records a channel-polarity result; stale completions are ignored.
fn finish_channel_polarity(app: &mut App, outcome: ChannelPolarityOutcome) -> Task<Message> {
    let ChannelPolarityOutcome {
        channel,
        flipped,
        result,
    } = outcome;
    let Some(strip) = channel_strip(app, channel) else {
        return Task::none();
    };
    if !strip.polarity.is_in_flight(flipped) {
        return Task::none();
    }

    match &result {
        Ok(()) => tracing::info!(channel, flipped, "Channel polarity sent"),
        Err(error) => tracing::warn!(channel, flipped, %error, "Channel polarity send failed"),
    }

    let _ = strip.polarity.finish(flipped, result);
    Task::none()
}

/// The explicitly picked device, else the first supported one.
fn selected_device(app: &App) -> Option<DetectedDevice> {
    match &app.status {
        DeviceStatus::Ready(report) => {
            if let Some(wanted) = app.selected.as_ref()
                && let Some(hit) = report.supported.iter().find(|d| &d.location == wanted)
            {
                return Some(hit.clone());
            }
            report.supported.first().cloned()
        }
        _ => None,
    }
}

fn volume_task(device: DetectedDevice, level: f32) -> Task<Message> {
    Task::perform(
        async move {
            SpeakerVolumeOutcome {
                level,
                result: send_speaker_level(device, level).await,
            }
        },
        Message::SpeakerVolumeFinished,
    )
}

fn headphone_task(device: DetectedDevice, level: f32) -> Task<Message> {
    Task::perform(
        async move {
            HeadphoneVolumeOutcome {
                level,
                result: send_headphone_level(device, level).await,
            }
        },
        Message::HeadphoneVolumeFinished,
    )
}

fn routing_task(device: DetectedDevice, route: Route) -> Task<Message> {
    Task::perform(
        async move {
            RoutingOutcome {
                route,
                result: send_output_route(device, route).await,
            }
        },
        Message::RoutingFinished,
    )
}

fn digital_output_mode_task(device: DetectedDevice, mode: DigitalOutputMode) -> Task<Message> {
    Task::perform(
        async move {
            DigitalOutputModeOutcome {
                mode,
                result: send_digital_output_mode(device, mode).await,
            }
        },
        Message::DigitalOutputModeFinished,
    )
}

fn monitor_toggle_task(device: DetectedDevice, toggle: MonitorToggle, on: bool) -> Task<Message> {
    Task::perform(
        async move {
            MonitorToggleOutcome {
                toggle,
                on,
                result: send_monitor_toggle(device, toggle, on).await,
            }
        },
        Message::MonitorToggleFinished,
    )
}

fn channel_level_task(device: DetectedDevice, channel: u8, level: f32) -> Task<Message> {
    Task::perform(
        async move {
            ChannelLevelOutcome {
                channel,
                level,
                result: send_channel_level(device, channel, level).await,
            }
        },
        Message::ChannelLevelFinished,
    )
}

fn channel_polarity_task(device: DetectedDevice, channel: u8, flipped: bool) -> Task<Message> {
    Task::perform(
        async move {
            ChannelPolarityOutcome {
                channel,
                flipped,
                result: send_channel_polarity(device, channel, flipped).await,
            }
        },
        Message::ChannelPolarityFinished,
    )
}

fn request_scan(app: &mut App) -> Task<Message> {
    if app.scan_in_flight {
        app.rescan_requested = true;
        return Task::none();
    }

    app.scan_in_flight = true;
    app.status = DeviceStatus::Scanning;
    discovery_task()
}

fn finish_scan(app: &mut App) -> Task<Message> {
    app.scan_in_flight = false;

    if std::mem::take(&mut app.rescan_requested) {
        request_scan(app)
    } else {
        Task::none()
    }
}

fn discovery_task() -> Task<Message> {
    Task::perform(discover(), Message::DiscoveryFinished)
}

fn subscription(app: &App) -> Subscription<Message> {
    let watch = Subscription::run(watch_events).map(Message::DeviceWatch);
    match feedback_cadence(app) {
        // The timer exists only while a supported device is present, so
        // metering suspends on disconnect instead of polling blindly.
        Some(cadence) => Subscription::batch([
            watch,
            iced::time::every(cadence).map(|_| Message::FeedbackTick),
        ]),
        None => watch,
    }
}

fn theme(_app: &App) -> Theme {
    Theme::Dark
}

fn view(app: &App) -> Element<'_, Message> {
    let scanning = app.scan_in_flight;
    let refresh = button(text(if scanning {
        "Scanning…"
    } else {
        "Scan again"
    }))
    .padding([10, 16])
    .on_press_maybe((!scanning).then_some(Message::Refresh));

    let header = row![
        text("Selah").size(24),
        space().width(Length::Fill),
        container(text(feedback_summary(app)).size(13).style(text::secondary))
            .padding([6, 10])
            .style(container::secondary),
    ]
    .align_y(Alignment::Center);

    let content = column![
        header,
        column![
            text("Find your interface").size(36),
            text("Discovery only reads USB descriptors. Each send briefly claims a safe control interface, then releases it.")
                .size(16)
                .style(text::secondary),
        ]
        .spacing(8),
        container(status_view(app))
            .width(Fill)
            .padding(28)
            .style(container::rounded_box),
        row![
            text(watch_status(app)).size(13).style(text::secondary),
            space().width(Length::Fill),
            refresh,
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(32)
    .width(Fill)
    .max_width(680);

    container(scrollable(content).width(Fill))
        .center_x(Fill)
        .padding(40)
        .into()
}

/// One-line honesty summary for the header: what this attachment actually
/// reports back, so write-only controls are never mistaken for live state.
fn feedback_summary(app: &App) -> &'static str {
    let Some(device) = selected_device(app) else {
        return "Write-only · no readback";
    };
    if device.control_interface.is_none() {
        return "Write-only · no readback";
    }
    if meter_feedback_available(&device) {
        "Hardware feedback · monitor + meters"
    } else if speaker_feedback_available(&device)
        || toggle_feedback_available(&device)
        || digital_output_mode_available(&device)
    {
        "Hardware feedback · monitor readback"
    } else {
        "Write-only · no readback"
    }
}

fn watch_status(app: &App) -> &'static str {
    match app.watch_status {
        WatchStatus::Starting => "Starting automatic USB detection",
        WatchStatus::Active => "Watching for USB connection changes",
        WatchStatus::Failed => "Automatic detection unavailable — use Scan again",
    }
}

fn status_view(app: &App) -> Element<'_, Message> {
    match &app.status {
        DeviceStatus::Scanning => column![
            status_label("Scanning", container::secondary),
            text("Looking for Audient interfaces…").size(24),
            text("This usually takes less than a second.")
                .size(15)
                .style(text::secondary),
        ]
        .spacing(14)
        .into(),
        DeviceStatus::Empty => column![
            status_label("No device", container::secondary),
            text("No Audient interface detected").size(24),
            text("Connect an interface, wait for Linux to recognize it, then scan again.")
                .size(15)
                .style(text::secondary),
        ]
        .spacing(14)
        .into(),
        DeviceStatus::Ready(report) => supported_view(app, report),
        DeviceStatus::Unsupported(report) => column![
            unknown_view(&report.unsupported[0]),
            diagnostics_footer(report),
        ]
        .spacing(14)
        .into(),
        DeviceStatus::Failed(error) => column![
            status_label("Scan failed", container::danger),
            text("Couldn’t scan USB devices").size(24),
            text(error.to_string()).size(15),
            text(error.recovery_hint()).size(15).style(text::secondary),
        ]
        .spacing(14)
        .into(),
    }
}

/// One button per supported attachment; the current pick has no action.
/// Two identical models stay distinguishable through their USB location.
fn device_picker<'a>(
    report: &'a DiscoveryReport,
    current: &DeviceLocation,
) -> Element<'a, Message> {
    let mut picker = row![text("Interface:").size(13)]
        .spacing(6)
        .align_y(Alignment::Center);
    for candidate in &report.supported {
        let label = format!(
            "{} @ {}:{}",
            candidate.model.name, candidate.location.bus, candidate.location.address
        );
        let is_current = candidate.location == *current;
        picker = picker.push(button(text(label).size(13)).on_press_maybe(
            (!is_current).then_some(Message::DeviceSelected(candidate.location.clone())),
        ));
    }
    picker.into()
}

/// One line on whether Selah may claim a safe control interface for sends.
fn control_readiness(device: &DetectedDevice) -> String {
    match device.control_interface {
        Some(control) => format!(
            "Safe control interface {} available ({})",
            control.number, control.kind
        ),
        None => {
            "No safe control interface found; Selah will not claim the audio interface.".to_owned()
        }
    }
}

fn supported_view<'a>(app: &'a App, report: &'a DiscoveryReport) -> Element<'a, Message> {
    // The explicitly picked attachment, else the first one. Resolved as a
    // borrow from the report so the picker below can reference it.
    let picked = selected_device(app).map(|selected| selected.location);
    let device: &DetectedDevice = picked
        .as_ref()
        .and_then(|wanted| {
            report
                .supported
                .iter()
                .find(|candidate| &candidate.location == wanted)
        })
        .unwrap_or(&report.supported[0]);
    let model = device.model;
    let session_readiness = control_readiness(device);

    let mut content = column![
        status_label("Detected", container::success),
        text(model.name).size(28),
        text("Recognized from its USB descriptor. The control interface is claimed only for each send, then released.")
            .size(15)
            .style(text::secondary),
        text(session_readiness).size(14),
    ]
    .spacing(14);

    if report.supported.len() > 1 {
        content = content.push(device_picker(report, &device.location));
    }

    let support = support_label(support_level(model));
    content = content.push(text(support).size(13).style(text::secondary));

    if let Some(notice) = app.feedback_notice.as_deref() {
        content = content.push(
            text(format!(
                "Hardware read failed: {notice} Scan again to retry."
            ))
            .size(13)
            .style(text::warning),
        );
    }

    content = content.push(
        column![
            detail_row("Microphone inputs", model.mic_inputs),
            detail_row("Digital inputs", model.digital_inputs),
            detail_row("Analog outputs", model.analog_outputs),
            detail_row("Digital outputs", model.digital_outputs),
            detail_row("Inserts", model.inserts),
        ]
        .spacing(9),
    );

    // Capability-driven: the volume controls only exist when a safe control
    // interface is available. Without one there is nothing to send through.
    if monitor_controls_available(device) {
        content = content.push(volume_slider(
            "Speaker volume",
            &app.speaker,
            Message::SpeakerVolumeChanged,
        ));
        content = content.push(volume_slider(
            "Headphone volume",
            &app.headphone,
            Message::HeadphoneVolumeChanged,
        ));
    }

    if routing_available(device) {
        content = content.push(routing_view(model, &app.routes));
    } else if model.routing.is_some() {
        content = content.push(
            column![
                text("Output routing").size(16),
                text("Unavailable until Selah can use a safe control interface.")
                    .size(13)
                    .style(text::secondary),
            ]
            .spacing(6),
        );
    } else if model.analog_outputs > 2 || model.digital_outputs > 0 {
        content = content.push(
            column![
                text("Output routing").size(16),
                text("Unavailable on this model: its routing codes are not verified, so Selah will not guess.")
                    .size(13)
                    .style(text::secondary),
            ]
            .spacing(6),
        );
    }

    if digital_output_mode_available(device) {
        content = content.push(digital_output_mode_view(&app.digital_output_mode));
    }

    if model.inserts > 0 {
        content = content.push(
            text("Insert and send/return routing is unavailable: no verified USB mapping exists yet.")
                .size(13)
                .style(text::secondary),
        );
    }

    // Monitor toggles share the same gate: the `0x36` toggle table is
    // verified against the iD14 MKII layout so far.
    if monitor_toggles_available(device) {
        content = content.push(monitor_toggles_view(&app.toggles));
    }

    // Input mixer strips, model-driven: microphones then digital inputs.
    // Channel mute, solo, and stereo linking have no known mapping, so no
    // control for them is shown.
    if mixer_available(device) {
        content = content.push(mixer_view(device.model, &app.channels, &app.meters));
    }

    content = content.push(diagnostics_footer(report));

    content.into()
}

/// Copy-safe diagnostics block shared by the Ready and Unsupported views.
/// No new message or button: users copy via text selection, so no
/// clipboard dependency is needed.
fn diagnostics_footer(report: &DiscoveryReport) -> Element<'_, Message> {
    text(crate::device::diagnostics_text(report))
        .size(12)
        .style(text::secondary)
        .into()
}

/// One monitor volume slider.
///
/// The speaker slider adopts hardware-confirmed levels; the headphone
/// slider has no trusted readback, so its position stays the last requested
/// level and its status line reports what was sent — never confirmed state.
fn volume_slider<'a>(
    title: &'static str,
    control: &'a VolumeControl,
    on_change: fn(f32) -> Message,
) -> Element<'a, Message> {
    column![
        text(title).size(16),
        slider(0.0..=1.0, control.position(), on_change).step(0.01_f32),
        text(control.status_text()).size(13).style(text::secondary),
    ]
    .spacing(8)
    .into()
}

/// Compact output-oriented routing. Rows name physical destinations and only
/// offer sources present in the connected model's capability data.
fn routing_view<'a>(
    model: &'a crate::device::DeviceModel,
    routes: &'a [OutputRouteControl],
) -> Element<'a, Message> {
    let Some(capabilities) = model.routing else {
        return column![].into();
    };
    let mut section = column![
        text("Output routing").size(16),
        text("Choose what each output pair plays. Reset sends that output’s documented default; routes are not read back.")
            .size(13)
            .style(text::secondary),
    ]
    .spacing(10);

    for output in routes {
        let sending = matches!(output.control.status(), RouteStatus::Sending { .. });
        let requested = output.control.requested();
        let mut choices = row![].spacing(6).align_y(Alignment::Center);
        for &source in capabilities.sources {
            let label = if requested == Some(source) {
                format!("{} · requested", source.label())
            } else {
                source.label().to_owned()
            };
            choices = choices.push(button(text(label).size(13)).on_press_maybe(
                (!sending).then_some(Message::RouteSelected(Route {
                    destination: output.destination,
                    source,
                })),
            ));
        }
        choices = choices.push(
            button(text("Reset").size(13))
                .on_press_maybe((!sending).then_some(Message::RouteReset(output.destination))),
        );
        section = section.push(
            column![
                text(output.destination.label()).size(14),
                scrollable(choices).direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::default(),
                )),
                text(output.control.status_text())
                    .size(12)
                    .style(text::secondary),
            ]
            .spacing(6),
        );
    }
    section.into()
}

fn digital_output_mode_view(control: &ToggleControl) -> Element<'_, Message> {
    use crate::monitor::ToggleStatus;

    let sending = matches!(control.status(), ToggleStatus::Sending { .. });
    let confirmed = match control.status() {
        ToggleStatus::Confirmed { on } => Some(on),
        _ => None,
    };
    let known_request = !matches!(control.status(), ToggleStatus::Unknown);
    let requested_adat = known_request && control.position();
    let requested_spdif = known_request && !control.position();
    let status = match control.status() {
        ToggleStatus::Unknown => "Hardware format unknown — choose a format to send it.".to_owned(),
        ToggleStatus::Sending { sending } => {
            format!("Sending {}…", if sending { "ADAT" } else { "S/PDIF" })
        }
        ToggleStatus::Confirmed { on } => format!(
            "Hardware reports {} — follows the device.",
            if on { "ADAT" } else { "S/PDIF" }
        ),
        ToggleStatus::Sent { on } => format!(
            "Last sent {} — accepted, not read back from the device.",
            if on { "ADAT" } else { "S/PDIF" }
        ),
        ToggleStatus::Failed { error, .. } => {
            format!("Format send failed: {error} Choose a format to retry.")
        }
    };
    column![
        text("Digital output format").size(16),
        text("ADAT carries eight optical channels; S/PDIF carries one stereo pair.")
            .size(13)
            .style(text::secondary),
        row![
            button(text(if confirmed == Some(true) {
                "ADAT · hardware"
            } else if requested_adat {
                "ADAT · requested"
            } else {
                "ADAT"
            }))
            .on_press_maybe(
                (!sending).then_some(Message::DigitalOutputModeSelected(DigitalOutputMode::Adat,))
            ),
            button(text(if confirmed == Some(false) {
                "S/PDIF · hardware"
            } else if requested_spdif {
                "S/PDIF · requested"
            } else {
                "S/PDIF"
            }))
            .on_press_maybe(
                (!sending).then_some(Message::DigitalOutputModeSelected(DigitalOutputMode::Spdif,))
            ),
        ]
        .spacing(6),
        text(status).size(12).style(text::secondary),
    ]
    .spacing(6)
    .into()
}

/// Monitor toggles: confirmed values follow the hardware.
///
/// Each button shows the last requested on/off value until readback
/// confirms what the device holds; the status line names which one it is.
/// Buttons are real Iced buttons, so they stay keyboard-focusable.
fn monitor_toggles_view(toggles: &[ToggleControl; 5]) -> Element<'_, Message> {
    let mut section = column![text("Monitor").size(16)].spacing(8);
    for toggle in MonitorToggle::ALL {
        section = section.push(monitor_toggle_button(
            toggle,
            &toggles[toggle_index(toggle)],
        ));
    }
    section.into()
}

fn monitor_toggle_button(toggle: MonitorToggle, control: &ToggleControl) -> Element<'_, Message> {
    use crate::monitor::ToggleStatus;

    let sending = matches!(control.status(), ToggleStatus::Sending { .. });
    let pressed_label = if control.position() { "ON" } else { "OFF" };
    column![
        row![
            text(toggle.label()).size(14),
            space().width(Length::Fill),
            button(text(pressed_label))
                .padding([4, 14])
                .on_press_maybe((!sending).then_some(Message::MonitorToggleChanged {
                    toggle,
                    on: !control.position(),
                })),
        ]
        .align_y(Alignment::Center),
        text(control.status_text()).size(13).style(text::secondary),
    ]
    .spacing(4)
    .into()
}

/// Input mixer section: one strip per input, scrolling horizontally on
/// high-channel-count models instead of hiding channels. Each strip carries
/// a bounded VU meter fed by the device's meter block; meters with no data
/// show an unknown placeholder, never a fake zero.
#[allow(
    clippy::cast_possible_truncation,
    reason = "strip count comes from a u8 model count, so enumerate indexes always fit"
)]
fn mixer_view<'a>(
    model: &'a crate::device::DeviceModel,
    channels: &'a [ChannelStrip],
    meters: &'a [Option<u8>],
) -> Element<'a, Message> {
    let mut strips = row![].spacing(12);
    for (index, strip) in channels.iter().enumerate() {
        strips = strips.push(channel_strip_view(
            model,
            index as u8,
            strip,
            meters.get(index).copied().flatten(),
        ));
    }
    column![
        text("Input mix").size(16),
        text("Mute, solo, and stereo linking have no known mapping and are not shown.")
            .size(13)
            .style(text::secondary),
        scrollable(strips).width(Fill),
    ]
    .spacing(8)
    .into()
}

/// One channel strip: last-requested fader and polarity, plus live meter.
///
/// The fader position and polarity value are the last requested values
/// until monitor-style readback exists for the matrix — the matrix does not
/// answer reads, so their status lines report what was sent. The meter is
/// the opposite: purely device-reported, unknown until a block arrives.
fn channel_strip_view<'a>(
    model: &'a crate::device::DeviceModel,
    channel: u8,
    strip: &'a ChannelStrip,
    meter: Option<u8>,
) -> Element<'a, Message> {
    use crate::monitor::ToggleStatus;

    let polarity_sending = matches!(strip.polarity.status(), ToggleStatus::Sending { .. });
    let polarity_label = if strip.polarity.position() {
        "Ø ON"
    } else {
        "Ø OFF"
    };
    column![
        text(channel_name(model, channel)).size(14),
        slider(0.0..=1.0, strip.level.position(), move |level| {
            Message::ChannelLevelChanged { channel, level }
        })
        .step(0.01_f32),
        text(strip.level.status_text())
            .size(12)
            .style(text::secondary),
        meter_view(meter),
        button(text(polarity_label)).on_press_maybe((!polarity_sending).then_some(
            Message::ChannelPolarityChanged {
                channel,
                flipped: !strip.polarity.position(),
            }
        )),
        text(strip.polarity.status_text())
            .size(12)
            .style(text::secondary),
    ]
    .spacing(6)
    .width(Length::Fixed(150.0))
    .into()
}

/// One bounded VU meter: a fixed-width bar for a device-reported level, or
/// an unknown placeholder that is visually distinct from silence.
fn meter_view(meter: Option<u8>) -> Element<'static, Message> {
    match meter {
        Some(level) => {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a 0..=255 level byte converts to f32 exactly"
            )]
            let percent = f32::from(level) / 255.0 * 100.0;
            progress_bar(0.0..=100.0, percent).into()
        }
        None => text("meter —").size(11).style(text::secondary).into(),
    }
}

fn unknown_view(device: &UnknownAudientDevice) -> Element<'_, Message> {
    let name = device
        .reported_name
        .as_deref()
        .unwrap_or("Audient interface");

    column![
        status_label("Unsupported", container::danger),
        text(name).size(24),
        text(format!("Unknown product ID {:04x}", device.product_id)).size(15),
        text("This interface is made by Audient but is not in Selah’s device catalog yet.")
            .size(15)
            .style(text::secondary),
    ]
    .spacing(14)
    .into()
}

fn status_label(
    label: &'static str,
    style: fn(&Theme) -> container::Style,
) -> Element<'static, Message> {
    container(text(label).size(13))
        .padding([6, 10])
        .style(style)
        .into()
}

fn detail_row(label: &'static str, value: u8) -> Element<'static, Message> {
    row![
        text(label).size(14).style(text::secondary),
        space().width(Length::Fill),
        text(value).size(14),
    ]
    .width(Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::{
        App, ChannelLevelOutcome, ChannelPolarityOutcome, DeviceStatus, DigitalOutputModeOutcome,
        FeedbackOutcome, METER_CADENCE_MS, MONITOR_CADENCE_MS, Message, MonitorToggleOutcome,
        RoutingOutcome, SpeakerVolumeOutcome, feedback_cadence_ms, finish_scan, fresh_meters_for,
        fresh_routes_for, monitor_due, request_scan, route_control, selected_device, toggle_index,
        update,
    };
    use crate::device::{
        ControlInterface, ControlInterfaceKind, DetectedDevice, DeviceLocation, DiscoveryReport,
        FeedbackSnapshot, MonitorSnapshot, MonitorToggle,
    };
    use crate::monitor::{ToggleStatus, VolumeStatus};
    use crate::routing::{
        DigitalOutputMode, Route, RouteStatus, RoutingDestination, RoutingSource,
    };

    #[test]
    fn scan_requests_are_coalesced_while_a_scan_is_running() {
        let (mut app, _startup) = App::new();

        let _first_scan = request_scan(&mut app);
        let _queued_scan = request_scan(&mut app);
        let _duplicate_scan = request_scan(&mut app);

        assert!(app.scan_in_flight);
        assert!(app.rescan_requested);

        let _restart = finish_scan(&mut app);
        assert!(app.scan_in_flight);
        assert!(!app.rescan_requested);

        let _finished = finish_scan(&mut app);
        assert!(!app.scan_in_flight);
        assert!(!app.rescan_requested);
    }

    #[test]
    fn volume_change_without_a_device_is_ignored() {
        let (mut app, _startup) = App::new();

        let _ignored = update(&mut app, Message::SpeakerVolumeChanged(0.5));
        let _also_ignored = update(&mut app, Message::HeadphoneVolumeChanged(0.5));

        assert_level_eq(app.speaker.position(), 0.0);
        assert_eq!(app.speaker.status(), VolumeStatus::Unknown);
        assert_level_eq(app.headphone.position(), 0.0);
        assert_eq!(app.headphone.status(), VolumeStatus::Unknown);
    }

    #[test]
    fn stale_completion_after_a_rescan_is_ignored() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _send = update(&mut app, Message::SpeakerVolumeChanged(0.2));
        let _rescan = update(
            &mut app,
            Message::DiscoveryFinished(Ok(DiscoveryReport::default())),
        );
        let _stale = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );

        // The rescan reset the control; the late completion must not invent
        // an acknowledged send for the new (empty) device set.
        assert!(matches!(app.status, DeviceStatus::Empty));
        assert_eq!(app.speaker.status(), VolumeStatus::Unknown);
    }

    #[test]
    fn speaker_volume_moves_from_pending_to_applied() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _send = update(&mut app, Message::SpeakerVolumeChanged(0.2));
        assert_level_eq(app.speaker.position(), 0.2);
        assert_eq!(
            app.speaker.status(),
            VolumeStatus::Sending {
                sending: 0.2,
                queued: None,
            }
        );

        let _done = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );
        assert_eq!(app.speaker.status(), VolumeStatus::Sent { level: 0.2 });
    }

    #[test]
    fn speaker_volume_failure_keeps_last_sent_and_reports_the_error() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _first = update(&mut app, Message::SpeakerVolumeChanged(0.2));
        let _applied = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );

        let _retry = update(&mut app, Message::SpeakerVolumeChanged(0.3));
        assert_eq!(
            app.speaker.status(),
            VolumeStatus::Sending {
                sending: 0.3,
                queued: None,
            }
        );
        let _failed = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.3,
                result: Err("no device".to_owned()),
            }),
        );

        // The failed level is not presented as confirmed: the previous
        // acknowledged send is preserved and the error stays visible.
        assert_level_eq(app.speaker.position(), 0.3);
        assert_eq!(
            app.speaker.status(),
            VolumeStatus::Failed {
                error: "no device".to_owned(),
                last_sent: Some(0.2),
            }
        );
    }

    #[test]
    fn routing_request_without_a_device_is_ignored() {
        let (mut app, _startup) = App::new();
        let route = headphone_route(RoutingSource::MainMix);
        let _ignored = update(&mut app, Message::RouteSelected(route));
        assert!(app.routes.is_empty());
    }

    #[test]
    fn routing_moves_from_unknown_through_sending_to_sent() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.routes = fresh_routes_for(&selected_device(&app).unwrap());
        let route = headphone_route(RoutingSource::MainMix);

        let _send = update(&mut app, Message::RouteSelected(route));
        assert_eq!(
            route_control(&mut app, route.destination)
                .unwrap()
                .control
                .status(),
            RouteStatus::Sending {
                source: route.source
            }
        );

        let _done = update(
            &mut app,
            Message::RoutingFinished(RoutingOutcome {
                route,
                result: Ok(()),
            }),
        );
        assert_eq!(
            route_control(&mut app, route.destination)
                .unwrap()
                .control
                .status(),
            RouteStatus::Sent {
                source: route.source
            }
        );
    }

    #[test]
    fn routing_failure_reports_the_error() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.routes = fresh_routes_for(&selected_device(&app).unwrap());
        let route = headphone_route(RoutingSource::CueB);

        let _send = update(&mut app, Message::RouteSelected(route));
        let _failed = update(
            &mut app,
            Message::RoutingFinished(RoutingOutcome {
                route,
                result: Err("no device".to_owned()),
            }),
        );
        assert_eq!(
            route_control(&mut app, route.destination)
                .unwrap()
                .control
                .status(),
            RouteStatus::Failed {
                error: "no device".to_owned(),
                last_sent: None,
            }
        );
    }

    #[test]
    fn reset_sends_the_documented_default() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.routes = fresh_routes_for(&selected_device(&app).unwrap());
        let destination = RoutingDestination::Outputs3And4;

        let _send = update(&mut app, Message::RouteReset(destination));
        assert_eq!(
            route_control(&mut app, destination)
                .unwrap()
                .control
                .status(),
            RouteStatus::Sending {
                source: RoutingSource::DawMix
            }
        );
    }

    #[test]
    fn digital_output_mode_is_pending_then_sent_without_claiming_readback() {
        let (mut app, _startup) = App::new();
        let mut report = report_with_control();
        report.supported[0].model = crate::device::supported_device(0x000d).unwrap();
        app.status = DeviceStatus::Ready(report);

        let _send = update(
            &mut app,
            Message::DigitalOutputModeSelected(DigitalOutputMode::Adat),
        );
        assert_eq!(
            app.digital_output_mode.status(),
            ToggleStatus::Sending { sending: true }
        );

        let _done = update(
            &mut app,
            Message::DigitalOutputModeFinished(DigitalOutputModeOutcome {
                mode: DigitalOutputMode::Adat,
                result: Ok(()),
            }),
        );
        assert_eq!(
            app.digital_output_mode.status(),
            ToggleStatus::Sent { on: true }
        );
    }

    #[test]
    fn monitor_toggle_moves_from_unknown_through_sending_to_sent() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _send = update(
            &mut app,
            Message::MonitorToggleChanged {
                toggle: MonitorToggle::Dim,
                on: true,
            },
        );
        assert_eq!(
            app.toggles[toggle_index(MonitorToggle::Dim)].status(),
            ToggleStatus::Sending { sending: true }
        );

        let _done = update(
            &mut app,
            Message::MonitorToggleFinished(MonitorToggleOutcome {
                toggle: MonitorToggle::Dim,
                on: true,
                result: Ok(()),
            }),
        );
        assert_eq!(
            app.toggles[toggle_index(MonitorToggle::Dim)].status(),
            ToggleStatus::Sent { on: true }
        );
    }

    fn assert_level_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < f32::EPSILON,
            "expected slider level {expected}, got {actual}"
        );
    }

    #[test]
    fn channels_are_rebuilt_from_the_discovered_model() {
        use crate::monitor::{ToggleStatus, VolumeStatus};

        let (mut app, _startup) = App::new();
        assert!(app.channels.is_empty());

        let _ready = update(
            &mut app,
            Message::DiscoveryFinished(Ok(report_with_control())),
        );
        // iD14 MKII: 2 microphones + 8 digital inputs.
        assert_eq!(app.channels.len(), 10);
        assert_eq!(app.channels[0].level.status(), VolumeStatus::Unknown);
        assert_eq!(app.channels[0].polarity.status(), ToggleStatus::Unknown);
    }

    #[test]
    fn channel_level_moves_from_pending_to_applied() {
        use crate::monitor::VolumeStatus;

        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.channels = fresh_ready_channels();

        let _send = update(
            &mut app,
            Message::ChannelLevelChanged {
                channel: 2,
                level: 0.6,
            },
        );
        assert_level_eq(app.channels[2].level.position(), 0.6);
        assert_eq!(
            app.channels[2].level.status(),
            VolumeStatus::Sending {
                sending: 0.6,
                queued: None,
            }
        );

        let _done = update(
            &mut app,
            Message::ChannelLevelFinished(ChannelLevelOutcome {
                channel: 2,
                level: 0.6,
                result: Ok(()),
            }),
        );
        assert_eq!(
            app.channels[2].level.status(),
            VolumeStatus::Sent { level: 0.6 }
        );
    }

    #[test]
    fn channel_polarity_moves_from_pending_to_applied() {
        use crate::monitor::ToggleStatus;

        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.channels = fresh_ready_channels();

        let _send = update(
            &mut app,
            Message::ChannelPolarityChanged {
                channel: 1,
                flipped: true,
            },
        );
        assert_eq!(
            app.channels[1].polarity.status(),
            ToggleStatus::Sending { sending: true }
        );

        let _done = update(
            &mut app,
            Message::ChannelPolarityFinished(ChannelPolarityOutcome {
                channel: 1,
                flipped: true,
                result: Ok(()),
            }),
        );
        assert_eq!(
            app.channels[1].polarity.status(),
            ToggleStatus::Sent { on: true }
        );
    }

    fn fresh_ready_channels() -> Vec<crate::mixer::ChannelStrip> {
        std::iter::repeat_with(crate::mixer::ChannelStrip::default)
            .take(10)
            .collect()
    }

    #[test]
    fn feedback_cadence_needs_a_supported_device_with_a_control_interface() {
        let (mut app, _startup) = App::new();
        assert_eq!(feedback_cadence_ms(&app), None);

        app.status = DeviceStatus::Empty;
        assert_eq!(feedback_cadence_ms(&app), None);

        // Ready but no safe control interface: nothing may be polled.
        let mut report = report_with_control();
        report.supported[0].control_interface = None;
        app.status = DeviceStatus::Ready(report);
        assert_eq!(feedback_cadence_ms(&app), None);
    }

    #[test]
    fn feedback_cadence_prefers_meters_then_monitor_readback() {
        let (mut app, _startup) = App::new();

        // iD14 MKII: strips and meters share one gate.
        app.status = DeviceStatus::Ready(report_with_control());
        assert_eq!(feedback_cadence_ms(&app), Some(METER_CADENCE_MS));

        // iD4: monitor volumes read back, but no mixer means no meters.
        let mut report = report_with_control();
        report.supported[0].model = crate::device::supported_device(0x0003).unwrap();
        app.status = DeviceStatus::Ready(report);
        assert_eq!(feedback_cadence_ms(&app), Some(MONITOR_CADENCE_MS));
    }

    #[test]
    fn monitor_refresh_folds_into_the_meter_cadence() {
        assert!(monitor_due(10, METER_CADENCE_MS));
        assert!(monitor_due(20, METER_CADENCE_MS));
        assert!(!monitor_due(5, METER_CADENCE_MS));
        assert!(!monitor_due(11, METER_CADENCE_MS));
        // On the slow cadence every tick carries the monitor snapshot.
        assert!(monitor_due(1, MONITOR_CADENCE_MS));
        assert!(monitor_due(7, MONITOR_CADENCE_MS));
    }

    #[test]
    fn feedback_tick_is_dropped_while_a_poll_owns_the_session() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.feedback_in_flight = true;

        let _dropped = update(&mut app, Message::FeedbackTick);
        assert!(app.feedback_in_flight);
        assert_eq!(app.feedback_tick, 0);
    }

    #[test]
    fn connect_refresh_adopts_hardware_state_as_confirmed() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.channels = fresh_ready_channels();
        app.meters = fresh_meters_for(&selected_device(&app).unwrap());
        assert_eq!(app.speaker.status(), VolumeStatus::Unknown);

        let _done = update(
            &mut app,
            Message::FeedbackFinished(feedback_ok(
                MonitorSnapshot {
                    speaker_level: Some(0.7),
                    toggles: vec![(MonitorToggle::Dim, true)],
                    digital_output_mode: None,
                },
                Some(vec![0x40; 10]),
            )),
        );

        // Hardware wins: the slider moves to the device value and the
        // status names it confirmed, not sent.
        assert_level_eq(app.speaker.position(), 0.7);
        assert_eq!(app.speaker.status(), VolumeStatus::Confirmed { level: 0.7 });
        assert_eq!(
            app.toggles[toggle_index(MonitorToggle::Dim)].status(),
            ToggleStatus::Confirmed { on: true }
        );
        assert_eq!(app.meters, vec![Some(0x40); 10]);
        assert!(app.feedback_notice.is_none());
        assert!(!app.feedback_in_flight);
    }

    #[test]
    fn pending_send_is_not_overwritten_by_readback() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _send = update(&mut app, Message::SpeakerVolumeChanged(0.2));
        let _refresh = update(
            &mut app,
            Message::FeedbackFinished(feedback_ok(
                MonitorSnapshot {
                    speaker_level: Some(0.9),
                    toggles: Vec::new(),
                    digital_output_mode: None,
                },
                None,
            )),
        );

        // The local operation wins transiently; the hardware value is not
        // adopted until the send completes and the next poll confirms it.
        assert_level_eq(app.speaker.position(), 0.2);
        assert_eq!(
            app.speaker.status(),
            VolumeStatus::Sending {
                sending: 0.2,
                queued: None,
            }
        );

        let _done = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );
        assert_eq!(app.speaker.status(), VolumeStatus::Sent { level: 0.2 });
    }

    #[test]
    fn failed_poll_blanks_meters_but_keeps_confirmed_monitor() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.channels = fresh_ready_channels();
        app.meters = fresh_meters_for(&selected_device(&app).unwrap());

        let _confirmed = update(
            &mut app,
            Message::FeedbackFinished(feedback_ok(
                MonitorSnapshot {
                    speaker_level: Some(0.5),
                    toggles: Vec::new(),
                    digital_output_mode: None,
                },
                Some(vec![0x20; 10]),
            )),
        );
        assert_eq!(app.speaker.status(), VolumeStatus::Confirmed { level: 0.5 });

        // A frozen meter presented as live would be a lie: meters go back
        // to unknown while the last confirmed monitor value stands.
        let _failed = update(
            &mut app,
            Message::FeedbackFinished(FeedbackOutcome {
                location: report_location(),
                read_monitor: false,
                result: Err("stalled".to_owned()),
            }),
        );
        assert!(app.meters.iter().all(Option::is_none));
        assert_eq!(app.speaker.status(), VolumeStatus::Confirmed { level: 0.5 });
        assert_eq!(app.feedback_notice.as_deref(), Some("stalled"));
    }

    #[test]
    fn stale_feedback_from_a_replaced_device_is_ignored() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.feedback_in_flight = true;

        let _stale = update(
            &mut app,
            Message::FeedbackFinished(FeedbackOutcome {
                location: DeviceLocation {
                    bus: "9".to_owned(),
                    address: 9,
                },
                read_monitor: true,
                result: Ok(FeedbackSnapshot {
                    monitor: Some(MonitorSnapshot {
                        speaker_level: Some(0.1),
                        toggles: Vec::new(),
                        digital_output_mode: None,
                    }),
                    meters: None,
                }),
            }),
        );

        // Neither adopted nor cleared: the newer poll still owns the flag.
        assert_eq!(app.speaker.status(), VolumeStatus::Unknown);
        assert!(app.feedback_in_flight);
    }

    #[test]
    fn reconnect_resets_confirmed_state_and_meters_to_unknown() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());
        app.channels = fresh_ready_channels();
        app.meters = fresh_meters_for(&selected_device(&app).unwrap());

        let _confirmed = update(
            &mut app,
            Message::FeedbackFinished(feedback_ok(
                MonitorSnapshot {
                    speaker_level: Some(0.6),
                    toggles: Vec::new(),
                    digital_output_mode: None,
                },
                Some(vec![0x10; 10]),
            )),
        );
        assert_eq!(app.speaker.status(), VolumeStatus::Confirmed { level: 0.6 });

        // Disconnect drops everything: old values are never restored as
        // confirmed before readback succeeds again.
        let _gone = update(
            &mut app,
            Message::DiscoveryFinished(Ok(DiscoveryReport::default())),
        );
        assert!(matches!(app.status, DeviceStatus::Empty));
        assert_eq!(app.speaker.status(), VolumeStatus::Unknown);
        assert!(app.meters.is_empty());
        assert!(app.feedback_notice.is_none());
        assert!(!app.feedback_in_flight);
    }

    #[test]
    fn second_device_can_be_selected_explicitly() {
        let (mut app, _startup) = App::new();
        let mk = |addr: u8| DetectedDevice {
            location: DeviceLocation {
                bus: "3".to_owned(),
                address: addr,
            },
            model: crate::device::supported_device(0x0008).unwrap(),
            reported_name: None,
            control_interface: Some(ControlInterface {
                number: 4,
                kind: ControlInterfaceKind::ApplicationSpecific,
            }),
        };
        app.status = DeviceStatus::Ready(DiscoveryReport {
            supported: vec![mk(16), mk(17)],
            unsupported: vec![],
        });
        app.selected = None;
        let first = selected_device(&app).unwrap().location.address;
        assert_eq!(first, 16);
        let _pick = update(
            &mut app,
            Message::DeviceSelected(DeviceLocation {
                bus: "3".to_owned(),
                address: 17,
            }),
        );
        assert_eq!(selected_device(&app).unwrap().location.address, 17);
    }

    #[test]
    fn picked_model_drives_channel_and_meter_counts() {
        let (mut app, _startup) = App::new();
        let mk = |product_id: u16, address: u8| DetectedDevice {
            location: DeviceLocation {
                bus: "3".to_owned(),
                address,
            },
            model: crate::device::supported_device(product_id).unwrap(),
            reported_name: None,
            control_interface: Some(ControlInterface {
                number: 4,
                kind: ControlInterfaceKind::ApplicationSpecific,
            }),
        };
        let report = || DiscoveryReport {
            supported: vec![mk(0x0008, 16), mk(0x0003, 17)],
            unsupported: vec![],
        };
        let first_model = crate::device::supported_device(0x0008).unwrap();
        let second_model = crate::device::supported_device(0x0003).unwrap();
        let first_strips = crate::mixer::mixer_channel_count(first_model) as usize;
        let first_meters = crate::mixer::meter_channel_count(first_model) as usize;
        let second_strips = crate::mixer::mixer_channel_count(second_model) as usize;
        let second_meters = crate::mixer::meter_channel_count(second_model) as usize;
        assert_ne!(first_strips, second_strips);

        let _found = update(&mut app, Message::DiscoveryFinished(Ok(report())));
        assert_eq!(selected_device(&app).unwrap().location.address, 16);
        assert_eq!(app.channels.len(), first_strips);
        assert_eq!(app.meters.len(), first_meters);

        let _pick = update(
            &mut app,
            Message::DeviceSelected(DeviceLocation {
                bus: "3".to_owned(),
                address: 17,
            }),
        );
        assert_eq!(selected_device(&app).unwrap().location.address, 17);
        assert_eq!(app.channels.len(), second_strips);
        assert_eq!(app.meters.len(), second_meters);
    }

    fn feedback_ok(monitor: MonitorSnapshot, meters: Option<Vec<u8>>) -> FeedbackOutcome {
        FeedbackOutcome {
            location: report_location(),
            read_monitor: true,
            result: Ok(FeedbackSnapshot {
                monitor: Some(monitor),
                meters,
            }),
        }
    }

    fn report_location() -> DeviceLocation {
        DeviceLocation {
            bus: "1".to_owned(),
            address: 2,
        }
    }

    fn headphone_route(source: RoutingSource) -> Route {
        Route {
            destination: RoutingDestination::Headphones,
            source,
        }
    }

    fn report_with_control() -> DiscoveryReport {
        DiscoveryReport {
            supported: vec![DetectedDevice {
                location: DeviceLocation {
                    bus: "1".to_owned(),
                    address: 2,
                },
                model: crate::device::supported_device(0x0008).unwrap(),
                reported_name: None,
                control_interface: Some(ControlInterface {
                    number: 4,
                    kind: ControlInterfaceKind::ApplicationSpecific,
                }),
            }],
            unsupported: vec![],
        }
    }
}
