/// Audient's USB vendor identifier.
pub const AUDIENT_VENDOR_ID: u16 = 0x2708;

/// Static capabilities needed to construct controls for an interface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeviceModel {
    pub name: &'static str,
    pub product_id: u16,
    pub mic_inputs: u8,
    pub digital_inputs: u8,
    pub analog_outputs: u8,
    pub digital_outputs: u8,
    pub inserts: u8,
}

/// Models reported as supported by the `MixiD` reference implementation.
pub const SUPPORTED_DEVICES: &[DeviceModel] = &[
    DeviceModel::new("iD4", 0x0003, 1, 0, 2, 0, 0),
    DeviceModel::new("iD4 MKII", 0x0009, 1, 0, 2, 0, 0),
    DeviceModel::new("iD14", 0x0002, 2, 8, 4, 0, 0),
    DeviceModel::new("iD14 MKII", 0x0008, 2, 8, 4, 0, 0),
    DeviceModel::new("iD22", 0x0001, 2, 8, 4, 8, 2),
    DeviceModel::new("iD24", 0x000d, 2, 10, 4, 14, 2),
    DeviceModel::new("iD44", 0x0005, 4, 16, 4, 16, 2),
    DeviceModel::new("iD44 MKII", 0x000b, 4, 16, 4, 16, 2),
    DeviceModel::new("iD48", 0x0012, 8, 16, 4, 16, 8),
];

impl DeviceModel {
    const fn new(
        name: &'static str,
        product_id: u16,
        mic_inputs: u8,
        digital_inputs: u8,
        analog_outputs: u8,
        digital_outputs: u8,
        inserts: u8,
    ) -> Self {
        Self {
            name,
            product_id,
            mic_inputs,
            digital_inputs,
            analog_outputs,
            digital_outputs,
            inserts,
        }
    }
}

/// Returns the known model for an Audient product ID.
#[must_use]
pub fn supported_device(product_id: u16) -> Option<&'static DeviceModel> {
    SUPPORTED_DEVICES
        .iter()
        .find(|device| device.product_id == product_id)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{SUPPORTED_DEVICES, supported_device};

    #[test]
    fn every_product_id_is_unique() {
        let unique_ids = SUPPORTED_DEVICES
            .iter()
            .map(|device| device.product_id)
            .collect::<HashSet<_>>();

        assert_eq!(unique_ids.len(), SUPPORTED_DEVICES.len());
    }

    #[test]
    fn finds_a_supported_model_by_product_id() {
        assert_eq!(
            supported_device(0x000d).map(|device| device.name),
            Some("iD24")
        );
        assert_eq!(supported_device(0xffff), None);
    }
}
