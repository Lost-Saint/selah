pub(crate) mod header;
pub(crate) mod layout;
pub(crate) mod mixer;
pub(crate) mod monitor;
pub(crate) mod routing;
pub(crate) mod status;
pub(crate) mod style;

use iced::{Color, Theme};

use crate::app::App;

pub(crate) use layout::view;

pub(crate) fn theme(_app: &App) -> Theme {
    Theme::custom(
        "Selah Studio",
        iced::theme::Palette {
            background: Color::from_rgb8(10, 14, 19),
            text: Color::from_rgb8(237, 242, 247),
            primary: Color::from_rgb8(67, 211, 194),
            success: Color::from_rgb8(77, 208, 139),
            warning: Color::from_rgb8(245, 184, 65),
            danger: Color::from_rgb8(255, 104, 116),
        },
    )
}
