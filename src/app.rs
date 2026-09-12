use iced::widget::{button, column, container, row, slider, space, text};
use iced::{Alignment, Element, Fill, Length, Subscription, Task, Theme};

use crate::device::{
    DetectedDevice, DeviceWatchEvent, DiscoveryError, DiscoveryReport, UnknownAudientDevice,
    discover, send_headphone_level, send_phones_to_main_mix, send_speaker_level, watch_events,
};
use crate::monitor::{VolumeControl, monitor_controls_available};
use crate::routing::{RouteControl, phones_main_mix_available};

struct App {
    status: DeviceStatus,
    scan_in_flight: bool,
    rescan_requested: bool,
    watch_status: WatchStatus,
    speaker: VolumeControl,
    headphone: VolumeControl,
    routing: RouteControl,
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

#[derive(Clone, Debug)]
enum Message {
    Refresh,
    DeviceWatch(DeviceWatchEvent),
    DiscoveryFinished(Result<DiscoveryReport, DiscoveryError>),
    SpeakerVolumeChanged(f32),
    SpeakerVolumeFinished(SpeakerVolumeOutcome),
    HeadphoneVolumeChanged(f32),
    HeadphoneVolumeFinished(HeadphoneVolumeOutcome),
    RoutePhonesToMainMix,
    RoutingFinished(Result<(), String>),
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
                routing: RouteControl::default(),
            },
            Task::none(),
        )
    }
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
            app.routing = RouteControl::default();
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
            app.routing = RouteControl::default();
            finish_scan(app)
        }
        Message::DiscoveryFinished(Ok(_)) => {
            tracing::info!("No Audient interface detected");
            app.status = DeviceStatus::Empty;
            app.speaker = VolumeControl::default();
            app.headphone = VolumeControl::default();
            app.routing = RouteControl::default();
            finish_scan(app)
        }
        Message::DiscoveryFinished(Err(error)) => {
            tracing::error!(%error, "Audient device scan failed");
            app.status = DeviceStatus::Failed(error);
            app.speaker = VolumeControl::default();
            app.headphone = VolumeControl::default();
            app.routing = RouteControl::default();
            finish_scan(app)
        }
        Message::SpeakerVolumeChanged(level) => request_speaker_volume(app, level),
        Message::SpeakerVolumeFinished(outcome) => finish_speaker_volume(app, outcome),
        Message::HeadphoneVolumeChanged(level) => request_headphone_volume(app, level),
        Message::HeadphoneVolumeFinished(outcome) => finish_headphone_volume(app, outcome),
        Message::RoutePhonesToMainMix => request_routing(app),
        Message::RoutingFinished(result) => finish_routing(app, result),
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

/// Starts a one-way phones-to-Main-Mix send; ignored while one is in flight.
///
/// The USB work runs in the returned task, away from Iced's UI thread.
fn request_routing(app: &mut App) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };

    match app.routing.request() {
        Some(()) => routing_task(device),
        None => Task::none(),
    }
}

/// Records a routing send result; stale completions after a rescan are ignored.
fn finish_routing(app: &mut App, result: Result<(), String>) -> Task<Message> {
    if !app.routing.is_sending() {
        return Task::none();
    }

    match &result {
        Ok(()) => tracing::info!("Phones routed to Main Mix"),
        Err(error) => tracing::warn!(%error, "Phones routing send failed"),
    }

    app.routing.finish(result);
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

fn routing_task(device: DetectedDevice) -> Task<Message> {
    Task::perform(
        async move { send_phones_to_main_mix(device).await },
        Message::RoutingFinished,
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
            &app.routing
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

    container(content).center(Fill).padding(40).into()
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
    routing: &'a RouteControl,
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
        DeviceStatus::Ready(report) => supported_view(report, speaker, headphone, routing),
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
    routing: &'a RouteControl,
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

    // One-way and iD14 MKII-only: MixiD's six-channel table matches that
    // layout, and Selah cannot read routing back or restore the old route.
    if phones_main_mix_available(device) {
        content = content.push(routing_button(routing));
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

/// One-way phones-to-Main-Mix action without implying confirmed device state.
///
/// Selah cannot read routing back, so the button reports what was sent —
/// never what the device is confirmed to hold — and warns that it cannot be
/// undone from here.
fn routing_button(routing: &RouteControl) -> Element<'_, Message> {
    let sending = matches!(routing.status(), crate::routing::RouteStatus::Sending);
    column![
        text("Phones routing").size(16),
        button(text("Route phones to Main Mix"))
            .on_press_maybe((!sending).then_some(Message::RoutePhonesToMainMix)),
        text(routing.status_text()).size(13).style(text::secondary),
    ]
    .spacing(8)
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
        App, DeviceStatus, HeadphoneVolumeOutcome, Message, SpeakerVolumeOutcome, finish_scan,
        request_scan, update,
    };
    use crate::device::{
        ControlInterface, ControlInterfaceKind, DetectedDevice, DeviceLocation, DiscoveryReport,
    };
    use crate::monitor::VolumeStatus;

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
    fn speaker_volume_changes_while_sending_are_queued_behind_one_send() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _first = update(&mut app, Message::SpeakerVolumeChanged(0.2));
        let _second = update(&mut app, Message::SpeakerVolumeChanged(0.3));

        // Still a single send in flight; the latest request waits its turn.
        assert_eq!(
            app.speaker.status(),
            VolumeStatus::Sending {
                sending: 0.2,
                queued: Some(0.3),
            }
        );
        assert_level_eq(app.speaker.position(), 0.3);

        let _next = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );
        assert_eq!(
            app.speaker.status(),
            VolumeStatus::Sending {
                sending: 0.3,
                queued: None,
            }
        );
    }

    #[test]
    fn headphone_volume_moves_from_pending_to_applied() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _send = update(&mut app, Message::HeadphoneVolumeChanged(0.2));
        assert_level_eq(app.headphone.position(), 0.2);
        assert_eq!(
            app.headphone.status(),
            VolumeStatus::Sending {
                sending: 0.2,
                queued: None,
            }
        );

        let _done = update(
            &mut app,
            Message::HeadphoneVolumeFinished(HeadphoneVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );
        assert_eq!(app.headphone.status(), VolumeStatus::Sent { level: 0.2 });
    }

    #[test]
    fn headphone_volume_failure_keeps_last_sent_and_reports_the_error() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _first = update(&mut app, Message::HeadphoneVolumeChanged(0.2));
        let _applied = update(
            &mut app,
            Message::HeadphoneVolumeFinished(HeadphoneVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );

        let _retry = update(&mut app, Message::HeadphoneVolumeChanged(0.3));
        assert_eq!(
            app.headphone.status(),
            VolumeStatus::Sending {
                sending: 0.3,
                queued: None,
            }
        );
        let _failed = update(
            &mut app,
            Message::HeadphoneVolumeFinished(HeadphoneVolumeOutcome {
                level: 0.3,
                result: Err("no device".to_owned()),
            }),
        );

        // The failed level is not presented as confirmed: the previous
        // acknowledged send is preserved and the error stays visible.
        assert_level_eq(app.headphone.position(), 0.3);
        assert_eq!(
            app.headphone.status(),
            VolumeStatus::Failed {
                error: "no device".to_owned(),
                last_sent: Some(0.2),
            }
        );
    }

    #[test]
    fn headphone_volume_changes_while_sending_are_queued_behind_one_send() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _first = update(&mut app, Message::HeadphoneVolumeChanged(0.2));
        let _second = update(&mut app, Message::HeadphoneVolumeChanged(0.3));

        // Still a single send in flight; the latest request waits its turn.
        assert_eq!(
            app.headphone.status(),
            VolumeStatus::Sending {
                sending: 0.2,
                queued: Some(0.3),
            }
        );
        assert_level_eq(app.headphone.position(), 0.3);

        let _next = update(
            &mut app,
            Message::HeadphoneVolumeFinished(HeadphoneVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );
        assert_eq!(
            app.headphone.status(),
            VolumeStatus::Sending {
                sending: 0.3,
                queued: None,
            }
        );
    }

    #[test]
    fn speaker_and_headphone_sends_track_independently() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _speaker_send = update(&mut app, Message::SpeakerVolumeChanged(0.2));
        let _headphone_send = update(&mut app, Message::HeadphoneVolumeChanged(0.4));

        assert_eq!(
            app.speaker.status(),
            VolumeStatus::Sending {
                sending: 0.2,
                queued: None,
            }
        );
        assert_eq!(
            app.headphone.status(),
            VolumeStatus::Sending {
                sending: 0.4,
                queued: None,
            }
        );

        let _speaker_done = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );

        // Finishing one control leaves the other untouched.
        assert_eq!(app.speaker.status(), VolumeStatus::Sent { level: 0.2 });
        assert_eq!(
            app.headphone.status(),
            VolumeStatus::Sending {
                sending: 0.4,
                queued: None,
            }
        );
    }

    #[test]
    fn routing_request_without_a_device_is_ignored() {
        use crate::routing::RouteStatus;

        let (mut app, _startup) = App::new();

        let _ignored = update(&mut app, Message::RoutePhonesToMainMix);
        assert_eq!(app.routing.status(), RouteStatus::Unknown);
    }

    #[test]
    fn routing_moves_from_unknown_through_sending_to_sent() {
        use crate::routing::RouteStatus;

        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _send = update(&mut app, Message::RoutePhonesToMainMix);
        assert_eq!(app.routing.status(), RouteStatus::Sending);

        let _done = update(&mut app, Message::RoutingFinished(Ok(())));
        assert_eq!(app.routing.status(), RouteStatus::Sent);
    }

    #[test]
    fn routing_failure_reports_the_error() {
        use crate::routing::RouteStatus;

        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _send = update(&mut app, Message::RoutePhonesToMainMix);
        let _failed = update(
            &mut app,
            Message::RoutingFinished(Err("no device".to_owned())),
        );
        assert_eq!(
            app.routing.status(),
            RouteStatus::Failed {
                error: "no device".to_owned(),
                ever_sent: false,
            }
        );
    }

    #[test]
    fn second_routing_request_while_sending_is_ignored() {
        use crate::routing::RouteStatus;

        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _first = update(&mut app, Message::RoutePhonesToMainMix);
        let _second = update(&mut app, Message::RoutePhonesToMainMix);
        assert_eq!(app.routing.status(), RouteStatus::Sending);

        let _done = update(&mut app, Message::RoutingFinished(Ok(())));
        assert_eq!(app.routing.status(), RouteStatus::Sent);
    }

    #[test]
    fn stale_routing_completion_after_a_rescan_is_ignored() {
        use crate::routing::RouteStatus;

        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _send = update(&mut app, Message::RoutePhonesToMainMix);
        let _rescan = update(
            &mut app,
            Message::DiscoveryFinished(Ok(DiscoveryReport::default())),
        );
        let _stale = update(&mut app, Message::RoutingFinished(Ok(())));

        assert!(matches!(app.status, DeviceStatus::Empty));
        assert_eq!(app.routing.status(), RouteStatus::Unknown);
    }

    fn assert_level_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < f32::EPSILON,
            "expected slider level {expected}, got {actual}"
        );
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
