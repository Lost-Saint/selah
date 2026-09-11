use iced::widget::{button, column, container, row, slider, space, text};
use iced::{Alignment, Element, Fill, Length, Subscription, Task, Theme};

use crate::device::{
    DetectedDevice, DeviceSession, DeviceWatchEvent, DiscoveryError, DiscoveryReport,
    NormalizedLevel, UnknownAudientDevice, discover, watch_events,
};

struct App {
    status: DeviceStatus,
    scan_in_flight: bool,
    rescan_requested: bool,
    watch_status: WatchStatus,
    speaker: SpeakerControl,
}

/// Slider position and send state for main speaker volume.
///
/// `position` is only what the user last requested. Selah has no state
/// readback, so it is never presented as the device's confirmed level.
/// `last_sent` records the most recent level the device acknowledged.
#[derive(Clone, Debug, Default)]
struct SpeakerControl {
    position: f32,
    in_flight: Option<f32>,
    queued: Option<f32>,
    last_sent: Option<f32>,
    last_error: Option<String>,
}

/// Outcome of one background speaker-volume send, paired with the level it
/// attempted so stale completions can be ignored after a rescan.
#[derive(Clone, Debug)]
struct SpeakerVolumeOutcome {
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
                speaker: SpeakerControl::default(),
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
            app.speaker = SpeakerControl::default();
            finish_scan(app)
        }
        Message::DiscoveryFinished(Ok(report)) if !report.unsupported.is_empty() => {
            tracing::warn!(
                unsupported = report.unsupported.len(),
                "Found an unrecognized Audient interface"
            );
            app.status = DeviceStatus::Unsupported(report);
            app.speaker = SpeakerControl::default();
            finish_scan(app)
        }
        Message::DiscoveryFinished(Ok(_)) => {
            tracing::info!("No Audient interface detected");
            app.status = DeviceStatus::Empty;
            app.speaker = SpeakerControl::default();
            finish_scan(app)
        }
        Message::DiscoveryFinished(Err(error)) => {
            tracing::error!(%error, "Audient device scan failed");
            app.status = DeviceStatus::Failed(error);
            app.speaker = SpeakerControl::default();
            finish_scan(app)
        }
        Message::SpeakerVolumeChanged(level) => request_speaker_volume(app, level),
        Message::SpeakerVolumeFinished(outcome) => finish_speaker_volume(app, outcome),
    }
}

/// Starts a background speaker-volume send, queuing when one is in flight.
///
/// Only one send runs at a time: a change made while sending is remembered
/// as `queued` and started when the in-flight send completes. The USB work
/// runs in the returned task, away from Iced's UI thread.
fn request_speaker_volume(app: &mut App, level: f32) -> Task<Message> {
    if !level.is_finite() {
        return Task::none();
    }
    let level = level.clamp(0.0, 1.0);
    let Some(device) = selected_device(app) else {
        return Task::none();
    };

    app.speaker.position = level;
    app.speaker.last_error = None;

    if app.speaker.in_flight.is_some() {
        app.speaker.queued = Some(level);
        return Task::none();
    }

    app.speaker.in_flight = Some(level);
    app.speaker.queued = None;
    volume_task(device, level)
}

/// Records a background send result and starts the queued level, if any.
///
/// Stale completions from before a rescan are ignored: a rescan resets the
/// speaker state, so a completion whose level is not in flight belongs to a
/// previous device set.
///
/// Levels here are compared exactly because they are copied slider values
/// used as request identities, not computed arithmetic.
#[allow(clippy::float_cmp, reason = "copied slider levels identify requests")]
fn finish_speaker_volume(app: &mut App, outcome: SpeakerVolumeOutcome) -> Task<Message> {
    let SpeakerVolumeOutcome { level, result } = outcome;
    if app.speaker.in_flight != Some(level) {
        return Task::none();
    }

    match result {
        Ok(()) => {
            tracing::info!(level, "Speaker volume sent");
            app.speaker.last_sent = Some(level);
            app.speaker.last_error = None;
        }
        Err(error) => {
            tracing::warn!(level, %error, "Speaker volume send failed");
            app.speaker.last_error = Some(error);
        }
    }

    let next = app.speaker.queued.take();
    match next {
        Some(next) if next != level => {
            app.speaker.in_flight = Some(next);
            if let Some(device) = selected_device(app) {
                volume_task(device, next)
            } else {
                app.speaker.in_flight = None;
                Task::none()
            }
        }
        _ => {
            app.speaker.in_flight = None;
            Task::none()
        }
    }
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
        apply_speaker_volume(device, level),
        Message::SpeakerVolumeFinished,
    )
}

/// Opens a session, sends one bounded volume request, and always closes.
///
/// Runs inside an Iced task, away from the UI thread. Validation happens
/// before any USB I/O; the session is closed on both success and failure.
async fn apply_speaker_volume(device: DetectedDevice, level: f32) -> SpeakerVolumeOutcome {
    let result = async {
        let valid = NormalizedLevel::new(level).map_err(|error| error.to_string())?;
        let mut session = DeviceSession::open(&device)
            .await
            .map_err(|error| format!("{error} — {}", error.recovery_hint()))?;
        let send = session.set_speaker_level(valid).await;
        let close = session.close().await;
        match send {
            Ok(()) => close.map_err(|error| format!("{error} — {}", error.recovery_hint())),
            Err(error) => {
                if let Err(close_error) = close {
                    tracing::warn!(%close_error, "Control interface release failed after send error");
                }
                Err(format!("{error} — {}", error.recovery_hint()))
            }
        }
    }
    .await;

    SpeakerVolumeOutcome { level, result }
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
        container(text("Read-only discovery").size(13).style(text::secondary))
            .padding([6, 10])
            .style(container::secondary),
    ]
    .align_y(Alignment::Center);

    let content = column![
        header,
        column![
            text("Find your interface").size(36),
            text("Selah checks USB descriptors without opening the device or interrupting audio.")
                .size(16)
                .style(text::secondary),
        ]
        .spacing(8),
        container(status_view(&app.status, &app.speaker))
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

fn status_view<'a>(status: &'a DeviceStatus, speaker: &'a SpeakerControl) -> Element<'a, Message> {
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
        DeviceStatus::Ready(report) => supported_view(report, speaker),
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
    speaker: &'a SpeakerControl,
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

    // Capability-driven: the volume control only exists when a safe control
    // interface is available. Without one there is nothing to send through.
    if device.control_interface.is_some() {
        content = content.push(speaker_view(speaker));
    }

    if let Some(extra) = extra {
        content = content.push(text(extra).size(13).style(text::secondary));
    }

    content.into()
}

/// Main speaker volume without implying confirmed device state.
///
/// Selah cannot read the current level back, so the slider position is the
/// last requested level and the status line reports what was sent — never
/// what the device is confirmed to hold.
///
/// Queued and in-flight levels are compared exactly because they are copied
/// slider values identifying requests, not computed arithmetic.
#[allow(clippy::float_cmp, reason = "copied slider levels identify requests")]
fn speaker_view(speaker: &SpeakerControl) -> Element<'_, Message> {
    let status = match (speaker.in_flight, &speaker.last_error, speaker.last_sent) {
        (Some(sending), _, _) => match speaker.queued {
            Some(requested) if requested != sending => format!(
                "Sending {:.0}%… (latest request {:.0}%)",
                sending * 100.0,
                requested * 100.0
            ),
            _ => format!("Sending {:.0}%…", sending * 100.0),
        },
        (None, Some(error), _) => format!("Send failed: {error} Move the slider to retry."),
        (None, None, Some(sent)) => {
            format!(
                "Last sent {:.0}% — not read back from the device.",
                sent * 100.0
            )
        }
        (None, None, None) => {
            "No value read from the device — moving the slider sends a new level.".to_owned()
        }
    };

    column![
        text("Speaker volume").size(16),
        slider(0.0..=1.0, speaker.position, Message::SpeakerVolumeChanged).step(0.01_f32),
        text(status).size(13).style(text::secondary),
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
        App, DeviceStatus, Message, SpeakerVolumeOutcome, finish_scan, request_scan, update,
    };
    use crate::device::{
        ControlInterface, ControlInterfaceKind, DetectedDevice, DeviceLocation, DiscoveryReport,
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
    fn speaker_volume_moves_from_pending_to_applied() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _send = update(&mut app, Message::SpeakerVolumeChanged(0.2));
        assert_level_eq(app.speaker.position, 0.2);
        assert_eq!(app.speaker.in_flight, Some(0.2));
        assert_eq!(app.speaker.last_sent, None);

        let _done = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );
        assert_eq!(app.speaker.in_flight, None);
        assert_eq!(app.speaker.last_sent, Some(0.2));
        assert_eq!(app.speaker.last_error, None);
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
        assert_eq!(app.speaker.in_flight, Some(0.3));
        let _failed = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.3,
                result: Err("no device".to_owned()),
            }),
        );

        assert_eq!(app.speaker.in_flight, None);
        // The failed level is not presented as confirmed: the previous
        // acknowledged send is preserved and the error stays visible.
        assert_level_eq(app.speaker.position, 0.3);
        assert_eq!(app.speaker.last_sent, Some(0.2));
        assert_eq!(app.speaker.last_error.as_deref(), Some("no device"));
    }

    #[test]
    fn speaker_volume_changes_while_sending_are_queued_behind_one_send() {
        let (mut app, _startup) = App::new();
        app.status = DeviceStatus::Ready(report_with_control());

        let _first = update(&mut app, Message::SpeakerVolumeChanged(0.2));
        let _second = update(&mut app, Message::SpeakerVolumeChanged(0.3));

        // Still a single send in flight; the latest request waits its turn.
        assert_eq!(app.speaker.in_flight, Some(0.2));
        assert_eq!(app.speaker.queued, Some(0.3));
        assert_level_eq(app.speaker.position, 0.3);

        let _next = update(
            &mut app,
            Message::SpeakerVolumeFinished(SpeakerVolumeOutcome {
                level: 0.2,
                result: Ok(()),
            }),
        );
        assert_eq!(app.speaker.in_flight, Some(0.3));
        assert_eq!(app.speaker.queued, None);
        assert_eq!(app.speaker.last_sent, Some(0.2));
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
