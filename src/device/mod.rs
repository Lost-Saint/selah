//! Audient device identification and, eventually, USB communication.

mod catalog;

pub use catalog::{AUDIENT_VENDOR_ID, DeviceModel, SUPPORTED_DEVICES, supported_device};
