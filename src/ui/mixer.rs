use iced::widget::{button, column, progress_bar, row, scrollable, slider, text};
use iced::{Element, Fill, Length};

use crate::app::Message;
use crate::device::DeviceModel;
use crate::mixer::{ChannelStrip, channel_name};
use crate::monitor::ToggleStatus;

/// Input mixer section: one strip per input, scrolling horizontally on
/// high-channel-count models instead of hiding channels. Each strip carries
/// a bounded VU meter fed by the device's meter block; meters with no data
/// show an unknown placeholder, never a fake zero.
#[allow(
    clippy::cast_possible_truncation,
    reason = "strip count comes from a u8 model count, so enumerate indexes always fit"
)]
pub(crate) fn mixer_view<'a>(
    model: &'a DeviceModel,
    channels: &'a [ChannelStrip],
    meters: &'a [Option<u8>],
) -> Element<'a, Message> {
    let mut strips = row![].spacing(12);
    for (index, strip) in channels.iter().enumerate() {
        strips = strips.push(channel_strip_view(
            model,
            index as u8,
            strip,
            meters.get(index).copied().flatten(),
        ));
    }
    column![
        text("Input mix").size(16),
        text("Mute, solo, and stereo linking have no known mapping and are not shown.")
            .size(13)
            .style(text::secondary),
        scrollable(strips).width(Fill),
    ]
    .spacing(8)
    .into()
}

/// One channel strip: last-requested fader and polarity, plus live meter.
///
/// The fader position and polarity value are the last requested values
/// until monitor-style readback exists for the matrix — the matrix does not
/// answer reads, so their status lines report what was sent. The meter is
/// the opposite: purely device-reported, unknown until a block arrives.
fn channel_strip_view<'a>(
    model: &'a DeviceModel,
    channel: u8,
    strip: &'a ChannelStrip,
    meter: Option<u8>,
) -> Element<'a, Message> {
    let polarity_sending = matches!(strip.polarity.status(), ToggleStatus::Sending { .. });
    let polarity_label = if strip.polarity.position() {
        "Ø ON"
    } else {
        "Ø OFF"
    };
    column![
        text(channel_name(model, channel)).size(14),
        slider(0.0..=1.0, strip.level.position(), move |level| {
            Message::ChannelLevelChanged { channel, level }
        })
        .step(0.01_f32),
        text(strip.level.status_text())
            .size(12)
            .style(text::secondary),
        meter_view(meter),
        button(text(polarity_label)).on_press_maybe((!polarity_sending).then_some(
            Message::ChannelPolarityChanged {
                channel,
                flipped: !strip.polarity.position(),
            }
        )),
        text(strip.polarity.status_text())
            .size(12)
            .style(text::secondary),
    ]
    .spacing(6)
    .width(Length::Fixed(150.0))
    .into()
}

/// One bounded VU meter: a fixed-width bar for a device-reported level, or
/// an unknown placeholder that is visually distinct from silence.
fn meter_view(meter: Option<u8>) -> Element<'static, Message> {
    match meter {
        Some(level) => {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a 0..=255 level byte converts to f32 exactly"
            )]
            let percent = f32::from(level) / 255.0 * 100.0;
            progress_bar(0.0..=100.0, percent).into()
        }
        None => text("meter —").size(11).style(text::secondary).into(),
    }
}
