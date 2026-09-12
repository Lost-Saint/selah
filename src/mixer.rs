//! Input mixer channel strips.
//!
//! Each strip pairs a level control with a polarity control, reusing the
//! honest-state types from [`crate::monitor`]. Channel mute, solo, and
//! stereo linking have no known USB mapping in `MixiD`, `BiD`, or Monix, so
//! Selah sends nothing for them and shows no control that pretends
//! otherwise.

use crate::device::{DetectedDevice, DeviceModel};
use crate::monitor::{ToggleControl, VolumeControl};
use crate::routing::{AdatChannel, adat_input_mixer_index};

/// Live send state for one input channel strip.
#[derive(Clone, Debug, Default)]
pub struct ChannelStrip {
    /// Fader level: the last requested value, never confirmed device state.
    pub level: VolumeControl,
    /// Polarity invert: the last requested value, never confirmed state.
    pub polarity: ToggleControl,
}

/// How many strips the mixer shows for a model: microphones then digital.
#[must_use]
pub fn mixer_channel_count(model: &DeviceModel) -> u8 {
    model.mic_inputs.saturating_add(model.digital_inputs)
}

/// Display name for a strip: microphones first, then digital inputs.
///
/// `index` is the running channel across both groups, matching the USB
/// channel numbering.
#[must_use]
pub fn channel_name(model: &DeviceModel, index: u8) -> String {
    if index < model.mic_inputs {
        format!("Mic {}", index + 1)
    } else {
        let digital = index - model.mic_inputs + 1;
        if let Some(adat) = AdatChannel::new(digital)
            && adat_input_mixer_index(model, adat) == Ok(index)
        {
            format!("ADAT {digital}")
        } else {
            format!("Digital {digital}")
        }
    }
}

/// Whether the input mixer can be offered for this attachment.
///
/// Gated to the iD14 MKII (`0x0008`): the matrix cell formula matches the
/// family, but strip counts and audibility are verified nowhere else yet.
/// Do not widen without per-model hardware evidence.
#[must_use]
pub fn mixer_available(device: &DetectedDevice) -> bool {
    device.control_interface.is_some() && device.model.product_id == 0x0008
}

#[cfg(test)]
mod tests {
    use super::{channel_name, mixer_available, mixer_channel_count};
    use crate::device::{ControlInterface, ControlInterfaceKind, DetectedDevice, DeviceLocation};

    #[test]
    fn channel_count_covers_microphone_plus_digital_inputs() {
        assert_eq!(
            mixer_channel_count(crate::device::supported_device(0x0008).unwrap()),
            10
        );
        assert_eq!(
            mixer_channel_count(crate::device::supported_device(0x0003).unwrap()),
            1
        );
        assert_eq!(
            mixer_channel_count(crate::device::supported_device(0x000b).unwrap()),
            20
        );
    }

    #[test]
    fn channel_names_number_microphones_then_digital_inputs() {
        let model = crate::device::supported_device(0x0008).unwrap();
        assert_eq!(channel_name(model, 0), "Mic 1");
        assert_eq!(channel_name(model, 1), "Mic 2");
        assert_eq!(channel_name(model, 2), "ADAT 1");
        assert_eq!(channel_name(model, 9), "ADAT 8");
    }

    #[test]
    fn availability_requires_id14_mkii_with_a_control_interface() {
        assert!(mixer_available(&device_with(0x0008, true)));
        assert!(!mixer_available(&device_with(0x0008, false)));
        assert!(!mixer_available(&device_with(0x000d, true)));
    }

    fn device_with(product_id: u16, has_control: bool) -> DetectedDevice {
        DetectedDevice {
            location: DeviceLocation {
                bus: "1".to_owned(),
                address: 2,
            },
            model: crate::device::supported_device(product_id).unwrap(),
            reported_name: None,
            control_interface: has_control.then_some(ControlInterface {
                number: 4,
                kind: ControlInterfaceKind::ApplicationSpecific,
            }),
        }
    }
}
