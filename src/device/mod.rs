//! Audient device identification and, eventually, USB communication.

mod catalog;
mod control;
mod discovery;
mod error;
mod protocol;
mod session;
mod watch;

pub use catalog::{AUDIENT_VENDOR_ID, DeviceModel, SUPPORTED_DEVICES, supported_device};
pub use control::{
    send_channel_level, send_channel_polarity, send_headphone_level, send_monitor_toggle,
    send_phones_to_main_mix, send_speaker_level,
};
pub use discovery::{
    DetectedDevice, DeviceLocation, DiscoveryReport, UnknownAudientDevice, discover,
};
pub use error::DiscoveryError;
pub use protocol::{InvalidLevel, MonitorToggle, NormalizedLevel};
pub use session::{
    ControlInterface, ControlInterfaceKind, DeviceSession, SessionError, SessionErrorKind,
};
pub use watch::{DeviceWatchEvent, watch_events};
