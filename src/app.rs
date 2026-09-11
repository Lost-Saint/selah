use iced::widget::{button, column, container, row, space, text};
use iced::{Alignment, Element, Fill, Length, Task, Theme};

use crate::device::{DiscoveryError, DiscoveryReport, UnknownAudientDevice, discover};

struct App {
    status: DeviceStatus,
}

#[derive(Clone, Debug)]
enum Message {
    Refresh,
    DiscoveryFinished(Result<DiscoveryReport, DiscoveryError>),
}

enum DeviceStatus {
    Scanning,
    Ready(DiscoveryReport),
    Unsupported(DiscoveryReport),
    Empty,
    Failed(DiscoveryError),
}

pub(crate) fn run() -> iced::Result {
    iced::application(App::new, update, view)
        .title("Selah")
        .theme(theme)
        .window_size((760.0, 520.0))
        .centered()
        .run()
}

impl App {
    fn new() -> (Self, Task<Message>) {
        (
            Self {
                status: DeviceStatus::Scanning,
            },
            discovery_task(),
        )
    }
}

fn update(app: &mut App, message: Message) -> Task<Message> {
    match message {
        Message::Refresh if !matches!(app.status, DeviceStatus::Scanning) => {
            app.status = DeviceStatus::Scanning;
            discovery_task()
        }
        Message::Refresh => Task::none(),
        Message::DiscoveryFinished(Ok(report)) if !report.supported.is_empty() => {
            tracing::info!(
                supported = report.supported.len(),
                unsupported = report.unsupported.len(),
                "Audient device scan completed"
            );
            app.status = DeviceStatus::Ready(report);
            Task::none()
        }
        Message::DiscoveryFinished(Ok(report)) if !report.unsupported.is_empty() => {
            tracing::warn!(
                unsupported = report.unsupported.len(),
                "Found an unrecognized Audient interface"
            );
            app.status = DeviceStatus::Unsupported(report);
            Task::none()
        }
        Message::DiscoveryFinished(Ok(_)) => {
            tracing::info!("No Audient interface detected");
            app.status = DeviceStatus::Empty;
            Task::none()
        }
        Message::DiscoveryFinished(Err(error)) => {
            tracing::error!(%error, "Audient device scan failed");
            app.status = DeviceStatus::Failed(error);
            Task::none()
        }
    }
}

fn discovery_task() -> Task<Message> {
    Task::perform(discover(), Message::DiscoveryFinished)
}

fn theme(_app: &App) -> Theme {
    Theme::Dark
}

fn view(app: &App) -> Element<'_, Message> {
    let scanning = matches!(app.status, DeviceStatus::Scanning);
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
        container(status_view(&app.status))
            .width(Fill)
            .padding(28)
            .style(container::rounded_box),
        row![
            text("Audient USB vendor 2708")
                .size(13)
                .style(text::secondary),
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

fn status_view(status: &DeviceStatus) -> Element<'_, Message> {
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
        DeviceStatus::Ready(report) => supported_view(report),
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

fn supported_view(report: &DiscoveryReport) -> Element<'_, Message> {
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
        text("Recognized from its USB descriptor. No interface has been claimed.")
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

    if let Some(extra) = extra {
        content = content.push(text(extra).size(13).style(text::secondary));
    }

    content.into()
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
