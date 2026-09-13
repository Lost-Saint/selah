use std::fmt::{self, Display, Formatter};

use iced::font::{Font, Weight};
use iced::widget::{button, column, container, pick_list, responsive, row, space, text};
use iced::{Alignment, Element, Fill, Length};

use crate::app::{App, DeviceStatus, Message, WatchStatus, selected_device};
use crate::device::{
    DetectedDevice, DeviceLocation, DiscoveryReport, support_label, support_level,
};
use crate::mixer::mixer_available;
use crate::monitor::{monitor_controls_available, monitor_toggles_available};
use crate::routing::{digital_output_mode_available, routing_available};

use super::mixer::mixer_view;
use super::monitor::monitor_panel;
use super::routing::{digital_output_mode_view, routing_view};
use super::style;

pub(crate) fn status_view(app: &App) -> Element<'_, Message> {
    match &app.status {
        DeviceStatus::Scanning => state_view(
            app,
            "Scanning for Audient interfaces…".to_owned(),
            "Selah is reading USB descriptors. No audio interface is claimed during discovery."
                .to_owned(),
            false,
            false,
        ),
        DeviceStatus::Empty => state_view(
            app,
            "No Audient interface detected".to_owned(),
            "Connect and power on an interface. Selah watches for USB changes automatically."
                .to_owned(),
            true,
            false,
        ),
        DeviceStatus::Ready(report) => supported_view(app, report),
        DeviceStatus::Unsupported(report) => {
            let device = &report.unsupported[0];
            let name = device
                .reported_name
                .as_deref()
                .unwrap_or("Audient interface");
            column![
                state_view(
                    app,
                    format!("{name} is not supported yet"),
                    format!(
                        "Product ID {:04x} is not in Selah’s catalog. No control requests will be sent.",
                        device.product_id
                    ),
                    true,
                    true,
                ),
                diagnostics(report),
            ]
            .spacing(12)
            .padding(18)
            .into()
        }
        DeviceStatus::Failed(error) => state_view(
            app,
            "USB scan failed".to_owned(),
            format!("{} {}", error, error.recovery_hint()),
            true,
            true,
        ),
    }
}

fn state_view(
    app: &App,
    title: String,
    description: String,
    retry: bool,
    danger: bool,
) -> Element<'static, Message> {
    let watch = match app.watch_status {
        WatchStatus::Starting => "Starting automatic USB detection",
        WatchStatus::Active => "Watching for USB connection changes",
        WatchStatus::Failed => "Automatic detection unavailable — use Scan",
    };
    let scanning = app.scan_in_flight;
    let content = column![
        text(title).size(27).font(Font {
            weight: Weight::Semibold,
            ..Font::DEFAULT
        }),
        text(description).size(14).color(style::MUTED).width(Fill),
        button(
            text(if scanning {
                "Scanning…"
            } else {
                "Scan again"
            })
            .size(13)
        )
        .padding([9, 15])
        .style(style::primary_button)
        .on_press_maybe((retry && !scanning).then_some(Message::Refresh)),
        text(watch).size(11).color(style::SUBTLE),
    ]
    .spacing(14)
    .width(Fill)
    .max_width(560.0)
    .align_x(Alignment::Center);

    container(content)
        .center_x(Fill)
        .padding([72, 24])
        .style(if danger {
            style::danger_panel
        } else {
            style::mixer_panel
        })
        .into()
}

fn supported_view<'a>(app: &'a App, report: &'a DiscoveryReport) -> Element<'a, Message> {
    let picked = selected_device(app).map(|selected| selected.location);
    let device = picked
        .as_ref()
        .and_then(|wanted| {
            report
                .supported
                .iter()
                .find(|candidate| &candidate.location == wanted)
        })
        .unwrap_or(&report.supported[0]);

    let mut content = column![device_strip(report, device)].spacing(0);
    if let Some(notice) = app.feedback_notice.as_deref() {
        content = content.push(
            container(
                text(format!("Hardware feedback paused: {notice} Scan to retry."))
                    .size(11)
                    .color(style::WARNING),
            )
            .width(Fill)
            .padding([8, 12])
            .style(style::warning_panel),
        );
    }

    content = content.push(
        responsive(move |size| console_layout(app, device, size.width)).height(Length::Shrink),
    );
    content = content.push(device_footer(report, device));
    content.into()
}

fn console_layout<'a>(
    app: &'a App,
    device: &'a DetectedDevice,
    width: f32,
) -> Element<'a, Message> {
    let mixer = mixer_surface(app, device);
    let master = master_surface(app, device);
    let monitor = monitor_surface(app, device);

    // Audient's own grouping: the channel bank takes the room while MASTER
    // MIX and the control-room panel keep a fixed, familiar width. Portions
    // (not fixed pixels) split the row so the layout shrinks toward the
    // mid/narrow stacks instead of clipping; max widths stop ultrawide
    // windows from stretching the side panels into fader deserts.
    if width >= 1050.0 {
        row![
            container(mixer).width(Length::FillPortion(5)),
            container(master)
                .width(Length::FillPortion(2))
                .max_width(320.0),
            container(monitor)
                .width(Length::FillPortion(2))
                .max_width(320.0),
        ]
        .spacing(8)
        .into()
    } else if width >= 540.0 {
        column![
            row![
                container(master).width(Fill),
                container(monitor).width(Fill),
            ]
            .spacing(8),
            mixer,
        ]
        .spacing(8)
        .into()
    } else {
        column![monitor, master, mixer].spacing(8).into()
    }
}

fn mixer_surface<'a>(app: &'a App, device: &'a DetectedDevice) -> Element<'a, Message> {
    if mixer_available(device) {
        mixer_view(device.model, &app.channels, &app.meters, &app.input_filter)
    } else {
        unavailable_surface(
            "INPUT MIXER",
            "This model’s input mixer map is not verified, so Selah will not guess channel indexes.",
            style::mixer_panel,
        )
    }
}

fn master_surface<'a>(app: &'a App, device: &'a DetectedDevice) -> Element<'a, Message> {
    // MASTER MIX always renders: routing rows when the model map is
    // verified, otherwise an honest fallback. It is never hidden behind the
    // input-group filter.
    let mut panel = column![
        text("MASTER MIX").size(21).font(Font {
            weight: Weight::Semibold,
            ..Font::DEFAULT
        }),
        text("OUTPUT ROUTING").size(9).color(style::SUBTLE),
    ]
    .spacing(10);

    if routing_available(device) {
        panel = panel.push(routing_view(device.model, &app.routes));
    } else {
        panel = panel.push(
            text(if device.model.routing.is_some() {
                "A safe control interface is required before routing can be changed."
            } else {
                "Routing codes are not verified for this model."
            })
            .size(11)
            .color(style::MUTED),
        );
    }

    if digital_output_mode_available(device) {
        panel = panel.push(digital_output_mode_view(&app.digital_output_mode));
    }
    if device.model.inserts > 0 {
        panel = panel.push(
            text("Insert routing is not mapped yet.")
                .size(10)
                .color(style::SUBTLE),
        );
    }

    container(panel)
        .padding([16, 14])
        .width(Fill)
        .height(Fill)
        .style(style::master_panel)
        .into()
}

fn monitor_surface<'a>(app: &'a App, device: &'a DetectedDevice) -> Element<'a, Message> {
    if monitor_controls_available(device) {
        monitor_panel(
            &app.speaker,
            monitor_toggles_available(device).then_some(&app.toggles),
            &app.input_filter,
            device.model.mic_inputs,
            device.model.digital_inputs,
        )
    } else {
        unavailable_surface(
            "MONITOR",
            "No safe USB control interface is available. Selah will not claim an audio interface.",
            style::master_panel,
        )
    }
}

fn unavailable_surface(
    title: &'static str,
    description: &'static str,
    surface: fn(&iced::Theme) -> container::Style,
) -> Element<'static, Message> {
    container(
        column![
            text(title).size(18).font(Font {
                weight: Weight::Semibold,
                ..Font::DEFAULT
            }),
            text(description).size(12),
        ]
        .spacing(8),
    )
    .padding(16)
    .width(Fill)
    .height(Length::Fixed(260.0))
    .style(surface)
    .into()
}

fn device_strip<'a>(
    report: &'a DiscoveryReport,
    device: &'a DetectedDevice,
) -> Element<'a, Message> {
    responsive(move |size| device_strip_content(report, device, size.width < 680.0))
        .height(Length::Shrink)
        .into()
}

fn device_strip_content<'a>(
    report: &'a DiscoveryReport,
    device: &'a DetectedDevice,
    compact: bool,
) -> Element<'a, Message> {
    let support = support_label(support_level(device.model));
    let control = device.control_interface.map_or_else(
        || "Read-only · no safe interface".to_owned(),
        |interface| {
            format!(
                "Control interface {} · {}",
                interface.number, interface.kind
            )
        },
    );
    let identity = row![
        text(device.model.name).size(15).font(Font {
            weight: Weight::Semibold,
            ..Font::DEFAULT
        }),
        container(text("CONNECTED").size(9))
            .padding([4, 7])
            .style(style::status_pill),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let location = text(format!(
        "USB {}:{}",
        device.location.bus, device.location.address
    ))
    .size(10)
    .color(style::SUBTLE);

    let content: Element<'_, Message> = if compact {
        let mut compact_bar = column![
            row![identity, space().width(Fill), location].align_y(Alignment::Center),
            text(control).size(9).color(style::MUTED),
        ]
        .spacing(6);
        if report.supported.len() > 1 {
            compact_bar = compact_bar.push(device_picker(report, &device.location));
        }
        compact_bar.into()
    } else {
        let mut bar = row![
            identity,
            text(support).size(10).color(style::MUTED),
            space().width(Fill),
            text(control).size(10).color(style::MUTED),
            location,
        ]
        .spacing(9)
        .align_y(Alignment::Center);
        if report.supported.len() > 1 {
            bar = bar.push(device_picker(report, &device.location));
        }
        bar.into()
    };

    container(content)
        .width(Fill)
        .padding([9, 12])
        .style(style::master_panel)
        .into()
}

/// One entry in the interface dropdown. The label carries the USB location
/// so two identical models stay distinguishable.
#[derive(Clone, Debug, Eq, PartialEq)]
struct InterfaceChoice {
    label: String,
    location: DeviceLocation,
}

impl Display for InterfaceChoice {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

fn device_picker<'a>(
    report: &'a DiscoveryReport,
    current: &DeviceLocation,
) -> Element<'a, Message> {
    // MixiD's "Interface type" combo: one dropdown instead of a button per
    // attachment. Choosing the current entry re-selects it harmlessly; the
    // update path rebuilds state for the same device.
    let choices: Vec<InterfaceChoice> = report
        .supported
        .iter()
        .map(|candidate| InterfaceChoice {
            label: format!(
                "{} @ {}:{}",
                candidate.model.name, candidate.location.bus, candidate.location.address
            ),
            location: candidate.location.clone(),
        })
        .collect();
    let selected = choices
        .iter()
        .find(|choice| &choice.location == current)
        .cloned();
    pick_list(choices, selected, |choice| {
        Message::DeviceSelected(choice.location)
    })
    .placeholder("Select interface…")
    .width(Length::Shrink)
    .into()
}

fn device_footer<'a>(
    report: &'a DiscoveryReport,
    device: &'a DetectedDevice,
) -> Element<'a, Message> {
    let model = device.model;
    container(
        column![
            row![
                capability("MIC", model.mic_inputs),
                capability("DIGITAL IN", model.digital_inputs),
                capability("ANALOG OUT", model.analog_outputs),
                capability("DIGITAL OUT", model.digital_outputs),
                capability("INSERTS", model.inserts),
            ]
            .spacing(18),
            diagnostics(report),
        ]
        .spacing(8),
    )
    .padding([10, 12])
    .width(Fill)
    .style(style::master_panel)
    .into()
}

fn capability(label: &'static str, value: u8) -> Element<'static, Message> {
    row![
        text(label).size(9).color(style::SUBTLE),
        text(value).size(10).font(Font {
            weight: Weight::Semibold,
            ..Font::DEFAULT
        }),
    ]
    .spacing(4)
    .align_y(Alignment::Center)
    .into()
}

fn diagnostics(report: &DiscoveryReport) -> Element<'_, Message> {
    text(crate::device::diagnostics_text(report))
        .size(9)
        .font(Font::MONOSPACE)
        .color(style::SUBTLE)
        .into()
}
