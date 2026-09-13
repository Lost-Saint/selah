use iced::font::{Font, Weight};
use iced::widget::{button, column, container, row, slider, space, text};
use iced::{Alignment, Element, Fill};

use crate::app::state::{InputFilter, InputGroup};
use crate::app::{Message, toggle_index};
use crate::device::MonitorToggle;
use crate::monitor::{ToggleControl, ToggleStatus, VolumeControl, VolumeStatus};

use super::style;

/// The monitor panel beside the mixer, following `MixiD`'s grouping on one
/// dark surface: the input-group selector on top, one speaker volume
/// control, then the Dim/Alt/Talkback/Mono/Mute toggles.
///
/// Headphone volume is intentionally absent: the official application
/// exposes no in-app headphone gain, and the reference mapping stays
/// write-only and inaudible on cue-fed phones outputs, so a slider would be
/// a control that silently fails. The transport keeps
/// `send_headphone_level` for future evidence; the UI offers nothing.
pub(crate) fn monitor_panel<'a>(
    speaker: &'a VolumeControl,
    toggles: Option<&'a [ToggleControl; 5]>,
    filter: &'a InputFilter,
    mic_inputs: u8,
    digital_inputs: u8,
) -> Element<'a, Message> {
    let mut panel = column![
        row![
            text("MONITOR").size(13).font(Font {
                weight: Weight::Bold,
                ..Font::DEFAULT
            }),
            space().width(Fill),
            text("CONTROL ROOM").size(9).color(style::MUTED),
        ]
        .align_y(Alignment::Center),
        input_group_selector(filter, mic_inputs, digital_inputs),
        volume_control("SPEAKERS", speaker, Message::SpeakerVolumeChanged),
    ]
    .spacing(20);

    if let Some(toggles) = toggles {
        panel = panel.push(toggle_bank(toggles));
    }

    container(panel)
        .padding([18, 16])
        .width(Fill)
        .height(Fill)
        .style(style::master_panel)
        .into()
}

/// The `MIC / OPT / DAW` selector from Audient's application. `MIC` and
/// `OPT` filter the mixer bank; `DAW` returns have no mapped strips, so the
/// segment stays a disabled indicator rather than a control that pretends
/// otherwise. View-only: it never sends USB.
fn input_group_selector(
    filter: &InputFilter,
    mic_inputs: u8,
    digital_inputs: u8,
) -> Element<'_, Message> {
    let mic_button = button(text("MIC").size(11))
        .width(Fill)
        .padding([8, 10])
        .style(style::dark_choice(filter.mic))
        .on_press_maybe((mic_inputs > 0).then_some(Message::InputFilterChanged(InputGroup::Mic)));
    let optical_button = button(text("OPT").size(11))
        .width(Fill)
        .padding([8, 10])
        .style(style::dark_choice(filter.optical))
        .on_press_maybe(
            (digital_inputs > 0).then_some(Message::InputFilterChanged(InputGroup::Optical)),
        );
    // DAW returns exist on the matrix but Selah maps no DAW strips yet, so
    // this segment is an honest disabled indicator, matching Audient's
    // three-segment shape without inventing a control.
    let daw_button: Element<'_, Message> = button(text("DAW").size(11))
        .width(Fill)
        .padding([8, 10])
        .style(style::dark_choice(false))
        .into();

    column![
        row![mic_button, optical_button, daw_button].spacing(5),
        text("DAW returns are not mapped yet")
            .size(9)
            .color(style::SUBTLE),
    ]
    .spacing(6)
    .into()
}

fn volume_control<'a>(
    label: &'static str,
    control: &'a VolumeControl,
    on_change: fn(f32) -> Message,
) -> Element<'a, Message> {
    let status_color = match control.status() {
        VolumeStatus::Failed { .. } => style::DANGER,
        VolumeStatus::Sending { .. } => style::WARNING,
        VolumeStatus::Confirmed { .. } => style::SUCCESS,
        VolumeStatus::Sent { .. } | VolumeStatus::Unknown => style::MUTED,
    };
    let status_text = match control.status() {
        VolumeStatus::Unknown => "LEVEL UNKNOWN".to_owned(),
        VolumeStatus::Sending { .. } => "SENDING…".to_owned(),
        VolumeStatus::Failed { .. } => control.status_text(),
        VolumeStatus::Confirmed { .. } => "HARDWARE CONFIRMED".to_owned(),
        VolumeStatus::Sent { .. } => "LAST REQUEST SENT".to_owned(),
    };

    column![
        row![
            text(label).size(11).font(Font {
                weight: Weight::Semibold,
                ..Font::DEFAULT
            }),
            space().width(Fill),
            text(format!("{:.0}%", control.position() * 100.0))
                .size(18)
                .font(Font {
                    weight: Weight::Semibold,
                    ..Font::DEFAULT
                }),
        ]
        .align_y(Alignment::Center),
        slider(0.0..=1.0, control.position(), on_change)
            .step(0.01_f32)
            .style(style::dark_slider),
        text(status_text).size(9).color(status_color),
    ]
    .spacing(8)
    .into()
}

fn toggle_bank(toggles: &[ToggleControl; 5]) -> Element<'_, Message> {
    let mut bank = column![
        row![
            toggle_button(MonitorToggle::Dim, toggles),
            toggle_button(MonitorToggle::AltSpeaker, toggles),
        ]
        .spacing(5),
        row![
            toggle_button(MonitorToggle::Talkback, toggles),
            toggle_button(MonitorToggle::Mono, toggles),
        ]
        .spacing(5),
        toggle_button(MonitorToggle::SpeakerMute, toggles),
    ]
    .spacing(5);

    // MixiD shows no per-toggle status lines, but a failed send must not
    // fail silently: one shared error line names the first failure and its
    // retry path.
    if let Some(failed) = toggles
        .iter()
        .find(|control| matches!(control.status(), ToggleStatus::Failed { .. }))
    {
        bank = bank.push(text(failed.status_text()).size(9).color(style::DANGER));
    }

    bank.into()
}

fn toggle_button(toggle: MonitorToggle, toggles: &[ToggleControl; 5]) -> Element<'_, Message> {
    let control = &toggles[toggle_index(toggle)];
    let sending = matches!(control.status(), ToggleStatus::Sending { .. });
    let active = control.position();
    let label = format!("{}  {}", toggle.label(), if active { "ON" } else { "OFF" });

    button(text(label).size(11))
        .width(Fill)
        .padding([9, 10])
        .style(style::dark_choice(active))
        .on_press_maybe((!sending).then_some(Message::MonitorToggleChanged {
            toggle,
            on: !active,
        }))
        .into()
}
