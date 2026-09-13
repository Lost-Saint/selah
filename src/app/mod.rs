pub(crate) mod message;
pub(crate) mod state;
pub(crate) mod subscription;
pub(crate) mod tasks;
pub(crate) mod update;

pub(crate) use message::Message;
pub(crate) use state::{App, DeviceStatus, WatchStatus, selected_device, toggle_index};

pub(crate) fn run() -> iced::Result {
    iced::application(App::new, update::update, crate::ui::view)
        .title("Selah")
        .theme(crate::ui::theme)
        .subscription(subscription::subscription)
        .window_size((760.0, 520.0))
        .centered()
        .run()
}
