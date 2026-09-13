//! Selah application and device support.

mod app;
pub mod device;
mod mixer;
mod monitor;
mod routing;
mod ui;

/// Starts the Selah desktop application.
///
/// # Errors
///
/// Returns an error when Iced cannot initialize or run the application.
pub fn run() -> iced::Result {
    app::run()
}
