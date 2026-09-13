use iced::widget::{container, row, space, text};
use iced::{Alignment, Element, Length};

use crate::app::subscription::{FeedbackKind, feedback_kind};
use crate::app::{App, Message, selected_device};
use crate::device::DetectedDevice;

pub(crate) fn header(app: &App) -> Element<'_, Message> {
    row![
        text("Selah").size(24),
        space().width(Length::Fill),
        container(text(feedback_summary(app)).size(13).style(text::secondary))
            .padding([6, 10])
            .style(container::secondary),
    ]
    .align_y(Alignment::Center)
    .into()
}

/// One-line honesty summary for the header: what this attachment actually
/// reports back, so write-only controls are never mistaken for live state.
pub(crate) fn feedback_summary(app: &App) -> &'static str {
    let Some(device) = selected_device(app) else {
        return "Write-only · no readback";
    };
    feedback_summary_for(&device)
}

fn feedback_summary_for(device: &DetectedDevice) -> &'static str {
    match feedback_kind(device) {
        FeedbackKind::MetersAndMonitor => "Hardware feedback · monitor + meters",
        FeedbackKind::MonitorOnly => "Hardware feedback · monitor readback",
        FeedbackKind::WriteOnly => "Write-only · no readback",
    }
}
