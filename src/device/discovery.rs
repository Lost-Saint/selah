use super::{AUDIENT_VENDOR_ID, DeviceModel, DiscoveryError, supported_device};

/// A recognized Audient interface found during discovery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DetectedDevice {
    pub model: &'static DeviceModel,
    pub reported_name: Option<String>,
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
            vendor_id: device.vendor_id(),
            product_id: device.product_id(),
            reported_name: device.product_string().map(str::to_owned),
        }
    })))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DeviceIdentity {
    vendor_id: u16,
    product_id: u16,
    reported_name: Option<String>,
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
                    model,
                    reported_name: device.reported_name,
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
    use super::{AUDIENT_VENDOR_ID, DeviceIdentity, DiscoveryReport};

    #[test]
    fn ignores_devices_from_other_vendors() {
        let report = DiscoveryReport::from_identities([identity(0x1234, 0x000d, None)]);

        assert_eq!(report, DiscoveryReport::default());
    }

    #[test]
    fn separates_supported_and_unknown_audient_devices() {
        let report = DiscoveryReport::from_identities([
            identity(AUDIENT_VENDOR_ID, 0x000d, Some("Audient iD24")),
            identity(AUDIENT_VENDOR_ID, 0xbeef, Some("Future iD")),
        ]);

        assert_eq!(report.supported.len(), 1);
        assert_eq!(report.supported[0].model.name, "iD24");
        assert_eq!(
            report.supported[0].reported_name.as_deref(),
            Some("Audient iD24")
        );

        assert_eq!(report.unsupported.len(), 1);
        assert_eq!(report.unsupported[0].product_id, 0xbeef);
        assert_eq!(
            report.unsupported[0].reported_name.as_deref(),
            Some("Future iD")
        );
    }

    fn identity(vendor_id: u16, product_id: u16, reported_name: Option<&str>) -> DeviceIdentity {
        DeviceIdentity {
            vendor_id,
            product_id,
            reported_name: reported_name.map(str::to_owned),
        }
    }
}
