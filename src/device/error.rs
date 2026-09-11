use std::error::Error;
use std::fmt::{self, Display, Formatter};

/// Failure to enumerate or monitor USB devices.
#[derive(Clone, Debug)]
pub struct DiscoveryError {
    operation: DiscoveryOperation,
    source: nusb::Error,
}

impl DiscoveryError {
    pub(crate) fn enumerate(source: nusb::Error) -> Self {
        Self {
            operation: DiscoveryOperation::Enumerate,
            source,
        }
    }

    pub(crate) fn watch(source: nusb::Error) -> Self {
        Self {
            operation: DiscoveryOperation::Watch,
            source,
        }
    }

    /// Gives the user a useful next step for this class of error.
    #[must_use]
    pub fn recovery_hint(&self) -> &'static str {
        match self.source.kind() {
            nusb::ErrorKind::PermissionDenied => {
                "Check the Selah udev rule, then reconnect the interface and try again."
            }
            nusb::ErrorKind::Unsupported => {
                "USB discovery is not supported on this system. Selah currently targets Linux."
            }
            _ => "Reconnect the interface and try the scan again.",
        }
    }
}

impl Display for DiscoveryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let operation = match self.operation {
            DiscoveryOperation::Enumerate => "scan",
            DiscoveryOperation::Watch => "monitor",
        };

        write!(formatter, "USB device {operation} failed: {}", self.source)
    }
}

#[derive(Clone, Copy, Debug)]
enum DiscoveryOperation {
    Enumerate,
    Watch,
}

impl Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}
