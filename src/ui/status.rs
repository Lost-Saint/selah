use iced::widget::{button, column, container, row, space, text};
use iced::{Alignment, Element, Fill, Theme};

use crate::app::{App, DeviceStatus, Message, selected_device};
use crate::device::{
    DetectedDevice, DeviceLocation, DiscoveryReport, UnknownAudientDevice, support_label,
    support_level,
};
use crate::mixer::mixer_available;
use crate::monitor::{monitor_controls_available, monitor_toggles_available};

use super::mixer::mixer_view;
use super::monitor::{monitor_toggles_view, volume_slider};
use super::routing::{digital_output_mode_view, routing_view};
use crate::routing::{digital_output_mode_available, routing_available};

pub(crate) fn status_view(app: &App) -> Element<'_, Message> {
    match &app.status {
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
        DeviceStatus::Ready(report) => supported_view(app, report),
        DeviceStatus::Unsupported(report) => column![
            unknown_view(&report.unsupported[0]),
            diagnostics_footer(report),
        ]
        .spacing(14)
        .into(),
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

/// One button per supported attachment; the current pick has no action.
/// Two identical models stay distinguishable through their USB location.
fn device_picker<'a>(
    report: &'a DiscoveryReport,
    current: &DeviceLocation,
) -> Element<'a, Message> {
    let mut picker = row![text("Interface:").size(13)]
        .spacing(6)
        .align_y(Alignment::Center);
    for candidate in &report.supported {
        let label = format!(
            "{} @ {}:{}",
            candidate.model.name, candidate.location.bus, candidate.location.address
        );
        let is_current = candidate.location == *current;
        picker = picker.push(button(text(label).size(13)).on_press_maybe(
            (!is_current).then_some(Message::DeviceSelected(candidate.location.clone())),
        ));
    }
    picker.into()
}

/// One line on whether Selah may claim a safe control interface for sends.
fn control_readiness(device: &DetectedDevice) -> String {
    match device.control_interface {
        Some(control) => format!(
            "Safe control interface {} available ({})",
            control.number, control.kind
        ),
        None => {
            "No safe control interface found; Selah will not claim the audio interface.".to_owned()
        }
    }
}

fn supported_view<'a>(app: &'a App, report: &'a DiscoveryReport) -> Element<'a, Message> {
    // The explicitly picked attachment, else the first one. Resolved as a
    // borrow from the report so the picker below can reference it.
    let picked = selected_device(app).map(|selected| selected.location);
    let device: &DetectedDevice = picked
        .as_ref()
        .and_then(|wanted| {
            report
                .supported
                .iter()
                .find(|candidate| &candidate.location == wanted)
        })
        .unwrap_or(&report.supported[0]);
    let model = device.model;
    let session_readiness = control_readiness(device);

    let mut content = column![
        status_label("Detected", container::success),
        text(model.name).size(28),
        text("Recognized from its USB descriptor. The control interface is claimed only for each send, then released.")
            .size(15)
            .style(text::secondary),
        text(session_readiness).size(14),
    ]
    .spacing(14);

    if report.supported.len() > 1 {
        content = content.push(device_picker(report, &device.location));
    }

    let support = support_label(support_level(model));
    content = content.push(text(support).size(13).style(text::secondary));

    if let Some(notice) = app.feedback_notice.as_deref() {
        content = content.push(
            text(format!(
                "Hardware read failed: {notice} Scan again to retry."
            ))
            .size(13)
            .style(text::warning),
        );
    }

    content = content.push(
        column![
            detail_row("Microphone inputs", model.mic_inputs),
            detail_row("Digital inputs", model.digital_inputs),
            detail_row("Analog outputs", model.analog_outputs),
            detail_row("Digital outputs", model.digital_outputs),
            detail_row("Inserts", model.inserts),
        ]
        .spacing(9),
    );

    // Capability-driven: the volume controls only exist when a safe control
    // interface is available. Without one there is nothing to send through.
    if monitor_controls_available(device) {
        content = content.push(volume_slider(
            "Speaker volume",
            &app.speaker,
            Message::SpeakerVolumeChanged,
        ));
        content = content.push(volume_slider(
            "Headphone volume",
            &app.headphone,
            Message::HeadphoneVolumeChanged,
        ));
    }

    if routing_available(device) {
        content = content.push(routing_view(model, &app.routes));
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
        content = content.push(digital_output_mode_view(&app.digital_output_mode));
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
        content = content.push(monitor_toggles_view(&app.toggles));
    }

    // Input mixer strips, model-driven: microphones then digital inputs.
    // Channel mute, solo, and stereo linking have no known mapping, so no
    // control for them is shown.
    if mixer_available(device) {
        content = content.push(mixer_view(device.model, &app.channels, &app.meters));
    }

    content = content.push(diagnostics_footer(report));

    content.into()
}

/// Copy-safe diagnostics block shared by the Ready and Unsupported views.
/// No new message or button: users copy via text selection, so no
/// clipboard dependency is needed.
fn diagnostics_footer(report: &DiscoveryReport) -> Element<'_, Message> {
    text(crate::device::diagnostics_text(report))
        .size(12)
        .style(text::secondary)
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
        space().width(Fill),
        text(value).size(14),
    ]
    .width(Fill)
    .into()
}
