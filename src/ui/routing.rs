use iced::widget::{button, column, row, scrollable, text};
use iced::{Alignment, Element};

use crate::app::Message;
use crate::device::DeviceModel;
use crate::monitor::{ToggleControl, ToggleStatus};
use crate::routing::{DigitalOutputMode, OutputRouteControl, Route, RouteStatus};

/// Compact output-oriented routing. Rows name physical destinations and only
/// offer sources present in the connected model's capability data.
pub(crate) fn routing_view<'a>(
    model: &'a DeviceModel,
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

pub(crate) fn digital_output_mode_view(control: &ToggleControl) -> Element<'_, Message> {
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
