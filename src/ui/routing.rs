use iced::font::{Font, Weight};
use iced::widget::{button, column, container, pick_list, row, text};
use iced::{Element, Fill};

use crate::app::Message;
use crate::device::DeviceModel;
use crate::monitor::{ToggleControl, ToggleStatus};
use crate::routing::{DigitalOutputMode, OutputRouteControl, Route, RouteStatus};

use super::style;

/// One dropdown per physical output pair, like `MixiD`'s per-channel source
/// choice: the menu lists only the sources in the connected model's
/// capability map, the current last-requested source stays selected, and
/// Reset sends that output's documented default (there is no route-off
/// wire value). Sends in flight are ignored by the update path, so the
/// menu stays usable while a send runs.
pub(crate) fn routing_view<'a>(
    model: &'a DeviceModel,
    routes: &'a [OutputRouteControl],
) -> Element<'a, Message> {
    let Some(capabilities) = model.routing else {
        return column![].into();
    };
    let mut section = column![].spacing(12);

    for output in routes {
        let sending = matches!(output.control.status(), RouteStatus::Sending { .. });
        let destination = output.destination;
        let source_menu = pick_list(
            capabilities.sources,
            output.control.requested(),
            move |source| {
                Message::RouteSelected(Route {
                    destination,
                    source,
                })
            },
        )
        .placeholder("Choose source…")
        .width(Fill);

        let status_color = match output.control.status() {
            RouteStatus::Failed { .. } => style::DANGER,
            RouteStatus::Sending { .. } => style::WARNING,
            RouteStatus::Sent { .. } => style::SUCCESS,
            RouteStatus::Unknown => style::SUBTLE,
        };
        section = section.push(
            container(
                column![
                    text(output.destination.label()).size(12).font(Font {
                        weight: Weight::Semibold,
                        ..Font::DEFAULT
                    }),
                    row![
                        source_menu,
                        button(text("RESET").size(9))
                            .padding([7, 9])
                            .style(style::secondary_button)
                            .on_press_maybe((!sending).then_some(Message::RouteReset(destination))),
                    ]
                    .spacing(6),
                    text(output.control.status_text())
                        .size(9)
                        .color(status_color),
                ]
                .spacing(6),
            )
            .padding(9)
            .width(Fill)
            .style(style::dark_inset),
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
    let adat_active = confirmed == Some(true) || (known_request && control.position());
    let spdif_active = confirmed == Some(false) || (known_request && !control.position());
    let status = match control.status() {
        ToggleStatus::Unknown => "FORMAT UNKNOWN".to_owned(),
        ToggleStatus::Sending { .. } => "SENDING…".to_owned(),
        ToggleStatus::Confirmed { .. } => "HARDWARE CONFIRMED".to_owned(),
        ToggleStatus::Sent { .. } => "LAST REQUEST SENT".to_owned(),
        ToggleStatus::Failed { ref error, .. } => format!("SEND FAILED: {error}"),
    };

    container(
        column![
            text("DIGITAL FORMAT").size(9).color(style::SUBTLE),
            row![
                button(text("ADAT").size(10))
                    .width(Fill)
                    .padding([8, 9])
                    .style(style::dark_choice(adat_active))
                    .on_press_maybe(
                        (!sending && !adat_active)
                            .then_some(Message::DigitalOutputModeSelected(DigitalOutputMode::Adat))
                    ),
                button(text("S/PDIF").size(10))
                    .width(Fill)
                    .padding([8, 9])
                    .style(style::dark_choice(spdif_active))
                    .on_press_maybe(
                        (!sending && !spdif_active).then_some(Message::DigitalOutputModeSelected(
                            DigitalOutputMode::Spdif
                        ))
                    ),
            ]
            .spacing(4),
            text(status).size(9).color(style::MUTED),
        ]
        .spacing(6),
    )
    .padding(9)
    .width(Fill)
    .style(style::dark_inset)
    .into()
}
