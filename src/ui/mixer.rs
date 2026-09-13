use iced::font::{Font, Weight};
use iced::widget::{
    button, column, container, progress_bar, row, scrollable, space, text, vertical_slider,
};
use iced::{Alignment, Element, Fill, Length};

use crate::app::Message;
use crate::app::state::InputFilter;
use crate::device::DeviceModel;
use crate::mixer::{ChannelStrip, channel_name};
use crate::monitor::{ToggleStatus, VolumeStatus};

use super::style;

/// One persistent mixer bank in the familiar console layout. Channel width is
/// fixed and additional inputs scroll horizontally instead of being regrouped.
/// The `MIC / OPT` view filter hides whole input groups; meters stay zipped
/// to their strip by running channel index so a filtered bank cannot shift a
/// meter onto the wrong channel.
#[allow(
    clippy::cast_possible_truncation,
    reason = "strip count comes from a u8 model count, so enumerate indexes always fit"
)]
pub(crate) fn mixer_view<'a>(
    model: &'a DeviceModel,
    channels: &'a [ChannelStrip],
    meters: &'a [Option<u8>],
    filter: &'a InputFilter,
) -> Element<'a, Message> {
    let mut strips = row![].spacing(0);
    let mut visible = 0_usize;
    for (index, strip) in channels.iter().enumerate() {
        let channel = index as u8;
        if !filter.shows(channel, model.mic_inputs) {
            continue;
        }
        visible += 1;
        strips = strips.push(channel_strip_view(
            model,
            channel,
            strip,
            meters.get(index).copied().flatten(),
        ));
    }

    let bank: Element<'a, Message> = if visible == 0 {
        container(
            text("No inputs in this view — enable MIC or OPT in the monitor panel.")
                .size(11)
                .color(style::SUBTLE),
        )
        .padding(12)
        .width(Fill)
        .style(style::dark_inset)
        .into()
    } else {
        scrollable(strips)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default(),
            ))
            .width(Fill)
            .into()
    };

    container(
        column![
            row![
                text("INPUT MIX").size(15).font(Font {
                    weight: Weight::Semibold,
                    ..Font::DEFAULT
                }),
                space().width(Fill),
                text(format!("{visible} / {} CHANNELS", channels.len()))
                    .size(9)
                    .color(style::SUBTLE),
            ]
            .align_y(Alignment::Center),
            bank,
            text("Levels and polarity are write-only · meters are hardware feedback")
                .size(9)
                .color(style::SUBTLE),
        ]
        .spacing(10),
    )
    .padding([12, 10])
    .width(Fill)
    .style(style::mixer_panel)
    .into()
}

fn channel_strip_view<'a>(
    model: &'a DeviceModel,
    channel: u8,
    strip: &'a ChannelStrip,
    meter: Option<u8>,
) -> Element<'a, Message> {
    let polarity_sending = matches!(strip.polarity.status(), ToggleStatus::Sending { .. });
    let polarity_active = strip.polarity.position();
    let level_state = match strip.level.status() {
        VolumeStatus::Unknown => "UNKNOWN",
        VolumeStatus::Sending { .. } => "SENDING",
        VolumeStatus::Failed { .. } => "FAILED",
        VolumeStatus::Confirmed { .. } => "CONFIRMED",
        VolumeStatus::Sent { .. } => "SENT",
    };
    let state_color = match strip.level.status() {
        VolumeStatus::Failed { .. } => style::DANGER,
        VolumeStatus::Sending { .. } => style::WARNING,
        VolumeStatus::Confirmed { .. } => style::SUCCESS,
        VolumeStatus::Unknown | VolumeStatus::Sent { .. } => style::SUBTLE,
    };

    container(
        column![
            column![
                text(channel_name(model, channel)).size(13).font(Font {
                    weight: Weight::Semibold,
                    ..Font::DEFAULT
                }),
                text(if channel < model.mic_inputs {
                    "ANALOG"
                } else {
                    "DIGITAL"
                })
                .size(8)
                .color(style::SUBTLE),
            ]
            .spacing(2)
            .align_x(Alignment::Center),
            row![
                vertical_slider(0.0..=1.0, strip.level.position(), move |level| {
                    Message::ChannelLevelChanged { channel, level }
                })
                .step(0.01_f32)
                .width(34.0)
                .height(Length::Fixed(285.0))
                .style(style::fader_slider),
                meter_view(meter),
            ]
            .spacing(15)
            .align_y(Alignment::Center),
            text(format!(
                "{:.0}%  {level_state}",
                strip.level.position() * 100.0
            ))
            .size(9)
            .color(state_color),
            button(text(if polarity_active { "Ø  ON" } else { "Ø  OFF" }).size(10))
                .width(Fill)
                .padding([7, 9])
                .style(style::dark_choice(polarity_active))
                .on_press_maybe(
                    (!polarity_sending).then_some(Message::ChannelPolarityChanged {
                        channel,
                        flipped: !polarity_active,
                    })
                ),
        ]
        .spacing(10)
        .align_x(Alignment::Center),
    )
    .padding([10, 8])
    .width(Length::Fixed(132.0))
    .style(style::strip)
    .into()
}

fn meter_view(meter: Option<u8>) -> Element<'static, Message> {
    match meter {
        Some(level) => {
            #[allow(
                clippy::cast_precision_loss,
                reason = "a 0..=255 level byte converts to f32 exactly"
            )]
            let percent = f32::from(level) / 255.0 * 100.0;
            progress_bar(0.0..=100.0, percent)
                .vertical()
                .length(Length::Fixed(285.0))
                .girth(12.0)
                .style(style::meter)
                .into()
        }
        None => container(text("—").size(9).color(style::SUBTLE))
            .center_x(Length::Fixed(12.0))
            .center_y(Length::Fixed(285.0))
            .style(style::dark_inset)
            .into(),
    }
}
