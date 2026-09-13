use iced::font::{Font, Weight};
use iced::widget::{button, container, responsive, row, space, text};
use iced::{Alignment, Element, Fill, Length};

use crate::app::subscription::{FeedbackKind, feedback_kind};
use crate::app::{App, Message, selected_device};
use crate::device::DetectedDevice;

use super::style;

pub(crate) fn header(app: &App) -> Element<'_, Message> {
    responsive(move |size| {
        let compact = size.width < 680.0;
        let scanning = app.scan_in_flight;
        let device = selected_device(app);

        let mut bar = row![
            container(text("S").size(13).font(Font {
                weight: Weight::Bold,
                ..Font::DEFAULT
            }))
            .center(Length::Fixed(26.0))
            .style(style::brand_mark),
            text("Selah").size(17).font(Font {
                weight: Weight::Semibold,
                ..Font::DEFAULT
            }),
            space().width(Fill),
        ]
        .spacing(9)
        .align_y(Alignment::Center);

        if !compact {
            bar = bar.push(
                container(text(feedback_summary(app)).size(11))
                    .padding([5, 9])
                    .style(if device.is_some() {
                        style::status_pill
                    } else {
                        style::neutral_pill
                    }),
            );
        }

        bar = bar.push(
            text(
                device
                    .as_ref()
                    .map_or("No interface", |item| item.model.name),
            )
            .size(12)
            .color(if device.is_some() {
                style::TEXT
            } else {
                style::MUTED
            }),
        );
        bar = bar.push(
            button(text(if scanning { "Scanning…" } else { "Scan" }).size(12))
                .padding([7, 12])
                .style(style::secondary_button)
                .on_press_maybe((!scanning).then_some(Message::Refresh)),
        );

        container(bar)
            .width(Fill)
            .padding([9, 12])
            .style(style::header)
            .into()
    })
    .height(Length::Shrink)
    .into()
}

pub(crate) fn feedback_summary(app: &App) -> &'static str {
    let Some(device) = selected_device(app) else {
        return "No hardware feedback";
    };
    feedback_summary_for(&device)
}

fn feedback_summary_for(device: &DetectedDevice) -> &'static str {
    match feedback_kind(device) {
        FeedbackKind::MetersAndMonitor => "Monitor + meter readback",
        FeedbackKind::MonitorOnly => "Monitor readback",
        FeedbackKind::WriteOnly => "Write-only controls",
    }
}
