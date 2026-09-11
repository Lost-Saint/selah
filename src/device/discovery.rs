use super::session::select_control_interface;
use super::{AUDIENT_VENDOR_ID, ControlInterface, DeviceModel, DiscoveryError, supported_device};

/// A recognized Audient interface found during discovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectedDevice {
    pub location: DeviceLocation,
    pub model: &'static DeviceModel,
    pub reported_name: Option<String>,
    pub control_interface: Option<ControlInterface>,
}

/// USB location for the current attachment, not a persistent device identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceLocation {
    pub bus: String,
    pub address: u8,
}

impl DeviceLocation {
    pub(crate) fn from_info(info: &nusb::DeviceInfo) -> Self {
        Self {
            bus: info.bus_id().to_owned(),
            address: info.device_address(),
        }
    }
}

/// An Audient interface whose product ID is not in Selah's catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnknownAudientDevice {
    pub product_id: u16,
    pub reported_name: Option<String>,
}

/// The Audient interfaces visible during a read-only USB scan.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DiscoveryReport {
    pub supported: Vec<DetectedDevice>,
    pub unsupported: Vec<UnknownAudientDevice>,
}

/// Scans USB descriptors for Audient interfaces without opening or claiming them.
///
/// # Errors
///
/// Returns an error when the operating system cannot enumerate USB devices.
pub async fn discover() -> Result<DiscoveryReport, DiscoveryError> {
    let devices = nusb::list_devices()
        .await
        .map_err(DiscoveryError::enumerate)?;

    Ok(DiscoveryReport::from_identities(devices.map(|device| {
        DeviceIdentity {
            location: DeviceLocation::from_info(&device),
            vendor_id: device.vendor_id(),
            product_id: device.product_id(),
            reported_name: device.product_string().map(str::to_owned),
            interfaces: device
                .interfaces()
                .map(|interface| (interface.interface_number(), interface.class()))
                .collect(),
        }
    })))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DeviceIdentity {
    location: DeviceLocation,
    vendor_id: u16,
    product_id: u16,
    reported_name: Option<String>,
    interfaces: Vec<(u8, u8)>,
}

impl DiscoveryReport {
    fn from_identities(devices: impl IntoIterator<Item = DeviceIdentity>) -> Self {
        let mut report = Self::default();

        for device in devices {
            if device.vendor_id != AUDIENT_VENDOR_ID {
                continue;
            }

            if let Some(model) = supported_device(device.product_id) {
                report.supported.push(DetectedDevice {
                    location: device.location,
                    model,
                    reported_name: device.reported_name,
                    control_interface: select_control_interface(device.interfaces),
                });
            } else {
                report.unsupported.push(UnknownAudientDevice {
                    product_id: device.product_id,
                    reported_name: device.reported_name,
                });
            }
        }

        report
    }
}

#[cfg(test)]
mod tests {
    use super::{AUDIENT_VENDOR_ID, DeviceIdentity, DeviceLocation, DiscoveryReport};

    #[test]
    fn ignores_devices_from_other_vendors() {
        let report = DiscoveryReport::from_identities([identity(0x1234, 0x000d, None, &[])]);

        assert_eq!(report, DiscoveryReport::default());
    }

    #[test]
    fn separates_supported_and_unknown_audient_devices() {
        let report = DiscoveryReport::from_identities([
            identity(
                AUDIENT_VENDOR_ID,
                0x000d,
                Some("Audient iD24"),
                &[(0, 0x01), (4, 0xfe)],
            ),
            identity(AUDIENT_VENDOR_ID, 0xbeef, Some("Future iD"), &[]),
        ]);

        assert_eq!(report.supported.len(), 1);
        assert_eq!(report.supported[0].model.name, "iD24");
        assert_eq!(
            report.supported[0].reported_name.as_deref(),
            Some("Audient iD24")
        );
        assert_eq!(
            report.supported[0]
                .control_interface
                .map(|interface| interface.number),
            Some(4)
        );

        assert_eq!(report.unsupported.len(), 1);
        assert_eq!(report.unsupported[0].product_id, 0xbeef);
        assert_eq!(
            report.unsupported[0].reported_name.as_deref(),
            Some("Future iD")
        );
    }

    fn identity(
        vendor_id: u16,
        product_id: u16,
        reported_name: Option<&str>,
        interfaces: &[(u8, u8)],
    ) -> DeviceIdentity {
        DeviceIdentity {
            location: DeviceLocation {
                bus: "1".to_owned(),
                address: 2,
            },
            vendor_id,
            product_id,
            reported_name: reported_name.map(str::to_owned),
            interfaces: interfaces.to_vec(),
        }
    }
}
