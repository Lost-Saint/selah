use iced::widget::{column, container, text};
use iced::{Element, Fill, Theme};

#[derive(Default)]
struct App;

pub(crate) fn run() -> iced::Result {
    iced::application(App::new, update, view)
        .title("Selah")
        .theme(theme)
        .run()
}

impl App {
    fn new() -> Self {
        Self
    }
}

fn update(_app: &mut App, _message: ()) {}

fn theme(_app: &App) -> Theme {
    Theme::Dark
}

fn view(_app: &App) -> Element<'_, ()> {
    let content = column![
        text("Selah").size(32),
        text("Connect an Audient iD interface to get started."),
    ]
    .spacing(12);

    container(content).center(Fill).into()
}
