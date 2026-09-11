//! Audient device identification and, eventually, USB communication.

mod catalog;
mod discovery;
mod error;
mod session;
mod watch;

pub use catalog::{AUDIENT_VENDOR_ID, DeviceModel, SUPPORTED_DEVICES, supported_device};
pub use discovery::{
    DetectedDevice, DeviceLocation, DiscoveryReport, UnknownAudientDevice, discover,
};
pub use error::DiscoveryError;
pub use session::{ControlInterface, ControlInterfaceKind, DeviceSession, SessionError};
pub use watch::{DeviceWatchEvent, watch_events};
