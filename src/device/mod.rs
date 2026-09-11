//! Audient device identification and, eventually, USB communication.

mod catalog;
mod discovery;
mod error;
mod session;

pub use catalog::{AUDIENT_VENDOR_ID, DeviceModel, SUPPORTED_DEVICES, supported_device};
pub use discovery::{DetectedDevice, DiscoveryReport, UnknownAudientDevice, discover};
pub use error::DiscoveryError;
pub use session::{ControlInterface, ControlInterfaceKind, DeviceSession, SessionError};
