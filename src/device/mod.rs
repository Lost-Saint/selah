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
    FeedbackSnapshot, MonitorSnapshot, read_feedback, send_channel_level, send_channel_polarity,
    send_digital_output_mode, send_headphone_level, send_monitor_toggle, send_output_route,
    send_speaker_level,
};
pub use discovery::{
    DetectedDevice, DeviceLocation, DiscoveryReport, UnknownAudientDevice, discover,
};
pub use error::DiscoveryError;
pub use protocol::{DecodeError, InvalidLevel, MAX_METER_INPUTS, MonitorToggle, NormalizedLevel};
pub use session::{
    ControlInterface, ControlInterfaceKind, DeviceSession, SessionError, SessionErrorKind,
};
pub use watch::{DeviceWatchEvent, watch_events};
