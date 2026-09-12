use iced::widget::{button, column, container, row, scrollable, slider, space, text};
use iced::{Alignment, Element, Fill, Length, Subscription, Task, Theme};

use crate::device::{
    DetectedDevice, DeviceWatchEvent, DiscoveryError, DiscoveryReport, MonitorToggle,
    UnknownAudientDevice, discover, send_channel_level, send_channel_polarity,
    send_digital_output_mode, send_headphone_level, send_monitor_toggle, send_output_route,
    send_speaker_level, watch_events,
};
use crate::mixer::{ChannelStrip, channel_name, mixer_available, mixer_channel_count};
use crate::monitor::{
    ToggleControl, VolumeControl, monitor_controls_available, monitor_toggles_available,
};
use crate::routing::{
    DigitalOutputMode, OutputRouteControl, Route, RouteStatus, RoutingDestination,
    digital_output_mode_available, reset_route, routing_available,
};

mod route_state;
use route_state::fresh_routes;

struct App {
    status: DeviceStatus,
    scan_in_flight: bool,
    rescan_requested: bool,
    watch_status: WatchStatus,
    speaker: VolumeControl,
    headphone: VolumeControl,
    routes: Vec<OutputRouteControl>,
    digital_output_mode: ToggleControl,
    toggles: [ToggleControl; 5],
    channels: Vec<ChannelStrip>,
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

#[derive(Clone, Debug)]
enum Message {
    Refresh,
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
                scan_in_flight: false,
                rescan_requested: false,
                watch_status: WatchStatus::Starting,
                speaker: VolumeControl::default(),
                headphone: VolumeControl::default(),
                routes: Vec::new(),
                digital_output_mode: ToggleControl::default(),
                toggles: Default::default(),
                channels: Vec::new(),
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

fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::DeviceWatch(DeviceWatchEvent::Started) => {
            tracing::info!("Watching for USB device changes");
            app.watch_status = WatchStatus::Active;
            request_scan(app)
        }
        Message::Refresh | Message::DeviceWatch(DeviceWatchEvent::DevicesChanged) => {
            request_scan(app)
        }
        Message::DeviceWatch(DeviceWatchEvent::Failed(error)) => {
            tracing::warn!(%error, "Automatic USB device detection is unavailable");
            app.watch_status = WatchStatus::Failed;
            request_scan(app)
        }
        Message::DiscoveryFinished(Ok(report)) if !report.supported.is_empty() => {
            tracing::info!(
                supported = report.supported.len(),
                unsupported = report.unsupported.len(),
                "Audient device scan completed"
            );
            app.status = DeviceStatus::Ready(report);
            app.speaker = VolumeControl::default();
            app.headphone = VolumeControl::default();
            app.toggles = Default::default();
            app.routes = fresh_routes(&app.status);
            app.digital_output_mode = ToggleControl::default();
            app.channels = fresh_channels(&app.status);
            finish_scan(app)
        }
        Message::DiscoveryFinished(Ok(report)) if !report.unsupported.is_empty() => {
            tracing::warn!(
                unsupported = report.unsupported.len(),
                "Found an unrecognized Audient interface"
            );
            app.status = DeviceStatus::Unsupported(report);
            app.speaker = VolumeControl::default();
            app.headphone = VolumeControl::default();
            app.toggles = Default::default();
            app.routes = Vec::new();
            app.digital_output_mode = ToggleControl::default();
            app.channels = Vec::new();
            finish_scan(app)
        }
        Message::DiscoveryFinished(Ok(_)) => {
            tracing::info!("No Audient interface detected");
            app.status = DeviceStatus::Empty;
            app.speaker = VolumeControl::default();
            app.headphone = VolumeControl::default();
            app.toggles = Default::default();
            app.routes = Vec::new();
            app.digital_output_mode = ToggleControl::default();
            app.channels = Vec::new();
            finish_scan(app)
        }
        Message::DiscoveryFinished(Err(error)) => {
            tracing::error!(%error, "Audient device scan failed");
            app.status = DeviceStatus::Failed(error);
            app.speaker = VolumeControl::default();
            app.headphone = VolumeControl::default();
            app.toggles = Default::default();
            app.routes = Vec::new();
            app.digital_output_mode = ToggleControl::default();
            app.channels = Vec::new();
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
    }
}

/// Builds a fresh strip per input of the newly discovered model.
fn fresh_channels(status: &DeviceStatus) -> Vec<ChannelStrip> {
    match status {
        DeviceStatus::Ready(report) => {
            let count = mixer_channel_count(report.supported[0].model) as usize;
            std::iter::repeat_with(ChannelStrip::default)
                .take(count)
                .collect()
        }
        _ => Vec::new(),
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

/// The first supported device, matching what `supported_view` displays.
fn selected_device(app: &App) -> Option<DetectedDevice> {
    match &app.status {
        DeviceStatus::Ready(report) => report.supported.first().cloned(),
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

fn subscription(_app: &App) -> Subscription<Message> {
    Subscription::run(watch_events).map(Message::DeviceWatch)
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
        container(
            text("Write-only · no readback")
                .size(13)
                .style(text::secondary)
        )
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
        container(status_view(
            &app.status,
            &app.speaker,
            &app.headphone,
            &app.routes,
            &app.digital_output_mode,
            &app.toggles,
            &app.channels
        ))
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

fn watch_status(app: &App) -> &'static str {
    match app.watch_status {
        WatchStatus::Starting => "Starting automatic USB detection",
        WatchStatus::Active => "Watching for USB connection changes",
        WatchStatus::Failed => "Automatic detection unavailable — use Scan again",
    }
}

fn status_view<'a>(
    status: &'a DeviceStatus,
    speaker: &'a VolumeControl,
    headphone: &'a VolumeControl,
    routes: &'a [OutputRouteControl],
    digital_output_mode: &'a ToggleControl,
    toggles: &'a [ToggleControl; 5],
    channels: &'a [ChannelStrip],
) -> Element<'a, Message> {
    match status {
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
        DeviceStatus::Ready(report) => supported_view(
            report,
            speaker,
            headphone,
            routes,
            digital_output_mode,
            toggles,
            channels,
        ),
        DeviceStatus::Unsupported(report) => unknown_view(&report.unsupported[0]),
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

fn supported_view<'a>(
    report: &'a DiscoveryReport,
    speaker: &'a VolumeControl,
    headphone: &'a VolumeControl,
    routes: &'a [OutputRouteControl],
    digital_output_mode: &'a ToggleControl,
    toggles: &'a [ToggleControl; 5],
    channels: &'a [ChannelStrip],
) -> Element<'a, Message> {
    let device = &report.supported[0];
    let model = device.model;
    let extra = additional_devices(report);
    let session_readiness = match device.control_interface {
        Some(control) => format!(
            "Safe control interface {} available ({})",
            control.number, control.kind
        ),
        None => {
            "No safe control interface found; Selah will not claim the audio interface.".to_owned()
        }
    };

    let mut content = column![
        status_label("Detected", container::success),
        text(model.name).size(28),
        text("Recognized from its USB descriptor. The control interface is claimed only for each send, then released.")
            .size(15)
            .style(text::secondary),
        text(session_readiness).size(14),
        column![
            detail_row("Microphone inputs", model.mic_inputs),
            detail_row("Digital inputs", model.digital_inputs),
            detail_row("Analog outputs", model.analog_outputs),
            detail_row("Digital outputs", model.digital_outputs),
            detail_row("Inserts", model.inserts),
        ]
        .spacing(9),
    ]
    .spacing(14);

    // Capability-driven: the volume controls only exist when a safe control
    // interface is available. Without one there is nothing to send through.
    if monitor_controls_available(device) {
        content = content.push(volume_slider(
            "Speaker volume",
            speaker,
            Message::SpeakerVolumeChanged,
        ));
        content = content.push(volume_slider(
            "Headphone volume",
            headphone,
            Message::HeadphoneVolumeChanged,
        ));
    }

    if routing_available(device) {
        content = content.push(routing_view(model, routes));
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
        content = content.push(digital_output_mode_view(digital_output_mode));
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
        content = content.push(monitor_toggles_view(toggles));
    }

    // Input mixer strips, model-driven: microphones then digital inputs.
    // Channel mute, solo, and stereo linking have no known mapping, so no
    // control for them is shown.
    if mixer_available(device) {
        content = content.push(mixer_view(device.model, channels));
    }

    if let Some(extra) = extra {
        content = content.push(text(extra).size(13).style(text::secondary));
    }

    content.into()
}

/// One monitor volume slider without implying confirmed device state.
///
/// Selah cannot read the current level back, so the slider position is the
/// last requested level and the status line reports what was sent — never
/// what the device is confirmed to hold.
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
    let known_request = !matches!(control.status(), ToggleStatus::Unknown);
    let requested_adat = known_request && control.position();
    let requested_spdif = known_request && !control.position();
    let status = match control.status() {
        ToggleStatus::Unknown => "Hardware format unknown — choose a format to send it.".to_owned(),
        ToggleStatus::Sending { sending } => {
            format!("Sending {}…", if sending { "ADAT" } else { "S/PDIF" })
        }
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
            button(text(if requested_adat {
                "ADAT · requested"
            } else {
                "ADAT"
            }))
            .on_press_maybe(
                (!sending).then_some(Message::DigitalOutputModeSelected(DigitalOutputMode::Adat,))
            ),
            button(text(if requested_spdif {
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

/// Monitor toggles without implying confirmed device state.
///
/// Each button shows the last requested on/off value; the status line
/// reports what was sent — never what the device is confirmed to hold.
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
/// high-channel-count models instead of hiding channels.
#[allow(
    clippy::cast_possible_truncation,
    reason = "strip count comes from a u8 model count, so enumerate indexes always fit"
)]
fn mixer_view<'a>(
    model: &'a crate::device::DeviceModel,
    channels: &'a [ChannelStrip],
) -> Element<'a, Message> {
    let mut strips = row![].spacing(12);
    for (index, strip) in channels.iter().enumerate() {
        strips = strips.push(channel_strip_view(model, index as u8, strip));
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

/// One channel strip without implying confirmed device state.
///
/// The fader position and polarity value are the last requested values;
/// the status lines report what was sent — never what the device is
/// confirmed to hold.
fn channel_strip_view<'a>(
    model: &'a crate::device::DeviceModel,
    channel: u8,
    strip: &'a ChannelStrip,
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

fn additional_devices(report: &DiscoveryReport) -> Option<String> {
    let additional = report.supported.len().saturating_sub(1) + report.unsupported.len();

    match additional {
        0 => None,
        1 => Some("1 additional Audient interface was found.".to_owned()),
        count => Some(format!("{count} additional Audient interfaces were found.")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        App, ChannelLevelOutcome, ChannelPolarityOutcome, DeviceStatus, DigitalOutputModeOutcome,
        Message, MonitorToggleOutcome, RoutingOutcome, SpeakerVolumeOutcome, finish_scan,
        fresh_routes, request_scan, route_control, toggle_index, update,
    };
    use crate::device::{
        ControlInterface, ControlInterfaceKind, DetectedDevice, DeviceLocation, DiscoveryReport,
        MonitorToggle,
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
        app.routes = fresh_routes(&app.status);
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
        app.routes = fresh_routes(&app.status);
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
        app.routes = fresh_routes(&app.status);
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
