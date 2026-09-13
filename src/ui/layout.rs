use iced::widget::{button, column, container, row, scrollable, space, text};
use iced::{Alignment, Element, Fill, Length};

use crate::app::{App, Message, WatchStatus};

use super::header::header;
use super::status::status_view;

pub(crate) fn view(app: &App) -> Element<'_, Message> {
    let scanning = app.scan_in_flight;
    let refresh = button(text(if scanning {
        "Scanning…"
    } else {
        "Scan again"
    }))
    .padding([10, 16])
    .on_press_maybe((!scanning).then_some(Message::Refresh));

    let content = column![
        header(app),
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

fn watch_status(app: &App) -> &'static str {
    match app.watch_status {
        WatchStatus::Starting => "Starting automatic USB detection",
        WatchStatus::Active => "Watching for USB connection changes",
        WatchStatus::Failed => "Automatic detection unavailable — use Scan again",
    }
}
