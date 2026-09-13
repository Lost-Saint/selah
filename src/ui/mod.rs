pub(crate) mod header;
pub(crate) mod layout;
pub(crate) mod mixer;
pub(crate) mod monitor;
pub(crate) mod routing;
pub(crate) mod status;

use iced::Theme;

use crate::app::App;

pub(crate) use layout::view;

pub(crate) fn theme(_app: &App) -> Theme {
    Theme::Dark
}
