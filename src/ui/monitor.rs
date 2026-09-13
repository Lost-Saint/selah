use iced::widget::{button, column, row, slider, space, text};
use iced::{Alignment, Element, Length};

use crate::app::{Message, toggle_index};
use crate::device::MonitorToggle;
use crate::monitor::{ToggleControl, ToggleStatus, VolumeControl};

/// One monitor volume slider.
///
/// The speaker slider adopts hardware-confirmed levels; the headphone
/// slider has no trusted readback, so its position stays the last requested
/// level and its status line reports what was sent — never confirmed state.
pub(crate) fn volume_slider<'a>(
    title: &'static str,
    control: &'a VolumeControl,
    on_change: fn(f32) -> Message,
) -> Element<'a, Message> {
    column![
        text(title).size(16),
        slider(0.0..=1.0, control.position(), on_change).step(0.01_f32),
        text(control.status_text()).size(13).style(text::secondary),
    ]
    .spacing(8)
    .into()
}

/// Monitor toggles: confirmed values follow the hardware.
///
/// Each button shows the last requested on/off value until readback
/// confirms what the device holds; the status line names which one it is.
/// Buttons are real Iced buttons, so they stay keyboard-focusable.
pub(crate) fn monitor_toggles_view(toggles: &[ToggleControl; 5]) -> Element<'_, Message> {
    let mut section = column![text("Monitor").size(16)].spacing(8);
    for toggle in MonitorToggle::ALL {
        section = section.push(monitor_toggle_button(
            toggle,
            &toggles[toggle_index(toggle)],
        ));
    }
    section.into()
}

fn monitor_toggle_button(toggle: MonitorToggle, control: &ToggleControl) -> Element<'_, Message> {
    let sending = matches!(control.status(), ToggleStatus::Sending { .. });
    let pressed_label = if control.position() { "ON" } else { "OFF" };
    column![
        row![
            text(toggle.label()).size(14),
            space().width(Length::Fill),
            button(text(pressed_label))
                .padding([4, 14])
                .on_press_maybe((!sending).then_some(Message::MonitorToggleChanged {
                    toggle,
                    on: !control.position(),
                })),
        ]
        .align_y(Alignment::Center),
        text(control.status_text()).size(13).style(text::secondary),
    ]
    .spacing(4)
    .into()
}
