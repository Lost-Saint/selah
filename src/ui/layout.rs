use iced::widget::{column, container, scrollable};
use iced::{Element, Fill};

use crate::app::{App, Message};

use super::header::header;
use super::status::status_view;
use super::style;

pub(crate) fn view(app: &App) -> Element<'_, Message> {
    container(column![
        header(app),
        scrollable(status_view(app)).width(Fill).height(Fill),
    ])
    .width(Fill)
    .height(Fill)
    .style(style::page)
    .into()
}
