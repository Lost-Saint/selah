use std::error::Error;
use std::fmt::{self, Display, Formatter};

use super::{AUDIENT_VENDOR_ID, DeviceModel};

const USB_CLASS_APPLICATION_SPECIFIC: u8 = 0xfe;
const USB_CLASS_VENDOR_SPECIFIC: u8 = 0xff;

/// A non-audio USB interface suitable for Audient mixer control requests.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ControlInterface {
    pub number: u8,
    pub kind: ControlInterfaceKind,
}

/// The USB class used by a selected control interface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlInterfaceKind {
    ApplicationSpecific,
    VendorSpecific,
}

impl Display for ControlInterfaceKind {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ApplicationSpecific => formatter.write_str("application/DFU"),
            Self::VendorSpecific => formatter.write_str("vendor-specific"),
        }
    }
}

/// Exclusive ownership of a safe Audient control interface.
///
/// Dropping the session releases the interface. Prefer [`DeviceSession::close`]
/// when the caller can await explicit release and handle a cleanup error.
pub struct DeviceSession {
    model: &'static DeviceModel,
    control: ControlInterface,
    interface: Option<nusb::Interface>,
}

impl DeviceSession {
    /// Opens and claims a non-audio control interface for a supported model.
    ///
    /// This function never detaches a kernel driver and never falls back to an
    /// audio-class interface.
    ///
    /// # Errors
    ///
    /// Returns a typed error if enumeration, opening, or claiming fails, or if
    /// the device has no safe control interface.
    pub async fn open(model: &'static DeviceModel) -> Result<Self, SessionError> {
        let mut devices = nusb::list_devices()
            .await
            .map_err(SessionError::Enumerate)?;

        let info = devices
            .find(|device| {
                device.vendor_id() == AUDIENT_VENDOR_ID && device.product_id() == model.product_id
            })
            .ok_or(SessionError::NotFound {
                product_id: model.product_id,
            })?;

        let control = select_control_interface(
            info.interfaces()
                .map(|interface| (interface.interface_number(), interface.class())),
        )
        .ok_or(SessionError::NoSafeControlInterface {
            product_id: model.product_id,
        })?;

        let device = info.open().await.map_err(SessionError::Open)?;
        let interface = device
            .claim_interface(control.number)
            .await
            .map_err(|source| SessionError::Claim {
                interface: control.number,
                source,
            })?;

        Ok(Self {
            model,
            control,
            interface: Some(interface),
        })
    }

    /// Returns the model associated with this session.
    #[must_use]
    pub fn model(&self) -> &'static DeviceModel {
        self.model
    }

    /// Returns the claimed non-audio control interface.
    #[must_use]
    pub fn control_interface(&self) -> ControlInterface {
        self.control
    }

    /// Releases the control interface and closes the session.
    ///
    /// # Errors
    ///
    /// Returns an error when the interface cannot be released cleanly.
    pub async fn close(mut self) -> Result<(), SessionError> {
        let Some(interface) = self.interface.take() else {
            return Ok(());
        };

        interface
            .release()
            .await
            .map_err(|source| SessionError::Release {
                interface: self.control.number,
                source,
            })
    }
}

impl std::fmt::Debug for DeviceSession {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeviceSession")
            .field("model", &self.model.name)
            .field("control", &self.control)
            .finish_non_exhaustive()
    }
}

/// Failure to establish or close a safe device session.
#[derive(Clone, Debug)]
pub enum SessionError {
    Enumerate(nusb::Error),
    NotFound { product_id: u16 },
    NoSafeControlInterface { product_id: u16 },
    Open(nusb::Error),
    Claim { interface: u8, source: nusb::Error },
    Release { interface: u8, source: nusb::Error },
}

impl SessionError {
    /// Gives the user a useful next step for this class of error.
    #[must_use]
    pub fn recovery_hint(&self) -> &'static str {
        match self {
            Self::NotFound { .. } => "Reconnect the interface and scan again.",
            Self::NoSafeControlInterface { .. } => {
                "Selah will not claim an audio interface. This model needs hardware investigation."
            }
            Self::Open(source) | Self::Enumerate(source)
                if source.kind() == nusb::ErrorKind::PermissionDenied =>
            {
                "Install the Selah udev rule, reconnect the interface, and try again."
            }
            Self::Claim { source, .. } if source.kind() == nusb::ErrorKind::Busy => {
                "Another process owns the control interface. Close it and try again."
            }
            _ => "Reconnect the interface and try again.",
        }
    }
}

impl Display for SessionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Enumerate(source) => write!(formatter, "USB device scan failed: {source}"),
            Self::NotFound { product_id } => {
                write!(
                    formatter,
                    "Audient device {product_id:04x} is no longer present"
                )
            }
            Self::NoSafeControlInterface { product_id } => write!(
                formatter,
                "Audient device {product_id:04x} has no safe control interface"
            ),
            Self::Open(source) => write!(formatter, "could not open the Audient device: {source}"),
            Self::Claim { interface, source } => {
                write!(
                    formatter,
                    "could not claim control interface {interface}: {source}"
                )
            }
            Self::Release { interface, source } => {
                write!(
                    formatter,
                    "could not release control interface {interface}: {source}"
                )
            }
        }
    }
}

impl Error for SessionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Enumerate(source)
            | Self::Open(source)
            | Self::Claim { source, .. }
            | Self::Release { source, .. } => Some(source),
            Self::NotFound { .. } | Self::NoSafeControlInterface { .. } => None,
        }
    }
}

pub(crate) fn select_control_interface(
    interfaces: impl IntoIterator<Item = (u8, u8)>,
) -> Option<ControlInterface> {
    let mut vendor_specific = None;

    for (number, class) in interfaces {
        match class {
            USB_CLASS_APPLICATION_SPECIFIC => {
                return Some(ControlInterface {
                    number,
                    kind: ControlInterfaceKind::ApplicationSpecific,
                });
            }
            USB_CLASS_VENDOR_SPECIFIC if vendor_specific.is_none() => {
                vendor_specific = Some(ControlInterface {
                    number,
                    kind: ControlInterfaceKind::VendorSpecific,
                });
            }
            _ => {}
        }
    }

    vendor_specific
}

#[cfg(test)]
mod tests {
    use super::{ControlInterface, ControlInterfaceKind, select_control_interface};

    #[test]
    fn prefers_application_specific_interface() {
        let selected = select_control_interface([(0, 0x01), (3, 0xff), (4, 0xfe)]);

        assert_eq!(
            selected,
            Some(ControlInterface {
                number: 4,
                kind: ControlInterfaceKind::ApplicationSpecific,
            })
        );
    }

    #[test]
    fn uses_vendor_specific_interface_when_dfu_is_absent() {
        let selected = select_control_interface([(0, 0x01), (2, 0xff)]);

        assert_eq!(
            selected,
            Some(ControlInterface {
                number: 2,
                kind: ControlInterfaceKind::VendorSpecific,
            })
        );
    }

    #[test]
    fn refuses_to_select_an_audio_interface() {
        assert_eq!(select_control_interface([(0, 0x01), (1, 0x01)]), None);
    }
}
