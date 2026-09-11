//! Selah application and device support.

mod app;
pub mod device;

/// Starts the Selah desktop application.
///
/// # Errors
///
/// Returns an error when Iced cannot initialize or run the application.
pub fn run() -> iced::Result {
    app::run()
}
