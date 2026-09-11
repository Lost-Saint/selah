use std::error::Error;
use std::fmt::{self, Display, Formatter};

const SET_CURRENT: u8 = 0x01;
const SPEAKER_VOLUME_CONTROL: u16 = 0x1200;
const SPEAKER_OUTPUT_ENTITY: u16 = 0x3600;
// Headphone volume lives on feature unit `0x0c`, which carries four output
// channels: 1 and 2 are the monitor pair, 3 and 4 the headphones. `MixiD`
// `set_hp_volume` (driver.h) addressed entity `0x0a` instead, but that
// entity declares no controls at all so those writes went nowhere; the BiD
// fork corrected the entity to `0x0c` and verified it against descriptors
// (see docs/protocol.md).
const HEADPHONE_VOLUME_CONTROLS: [u16; 2] = [0x0203, 0x0204];
const HEADPHONE_OUTPUT_ENTITY: u16 = 0x0c00;

/// A finite mixer level between silence (`0.0`) and full scale (`1.0`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalizedLevel(f32);

impl NormalizedLevel {
    /// Validates a normalized mixer level.
    ///
    /// # Errors
    ///
    /// Returns an error for non-finite values or values outside `0.0..=1.0`.
    pub fn new(value: f32) -> Result<Self, InvalidLevel> {
        if value.is_finite() && (0.0..=1.0).contains(&value) {
            Ok(Self(value))
        } else {
            Err(InvalidLevel(value))
        }
    }

    #[allow(
        clippy::cast_possible_truncation,
        reason = "validated input bounds the result to i16 and protocol parity requires truncation"
    )]
    fn audient_value(self) -> i16 {
        (-32_768.0 + 32_767.0 * self.0) as i16
    }
}

/// A mixer level outside the device protocol's accepted range.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InvalidLevel(f32);

impl Display for InvalidLevel {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "mixer level {} is outside 0.0..=1.0", self.0)
    }
}

impl Error for InvalidLevel {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ControlRequest {
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub payload: Vec<u8>,
}

pub(crate) fn speaker_volume(level: NormalizedLevel, interface_number: u8) -> ControlRequest {
    ControlRequest {
        request: SET_CURRENT,
        value: SPEAKER_VOLUME_CONTROL,
        index: SPEAKER_OUTPUT_ENTITY | u16::from(interface_number),
        payload: level.audient_value().to_le_bytes().to_vec(),
    }
}

/// Encodes headphone volume as the two channel requests `MixiD` sends.
///
/// `MixiD` `set_hp_volume` (driver.h) writes the same level to controls
/// `0x0203` and `0x0204` — selector `0x02`, channels 3 and 4, the headphone
/// pair of feature unit `0x0c`. Both transfers must succeed for the left
/// and right channels to stay matched.
pub(crate) fn headphone_volume(
    level: NormalizedLevel,
    interface_number: u8,
) -> [ControlRequest; 2] {
    HEADPHONE_VOLUME_CONTROLS.map(|control| ControlRequest {
        request: SET_CURRENT,
        value: control,
        index: HEADPHONE_OUTPUT_ENTITY | u16::from(interface_number),
        payload: level.audient_value().to_le_bytes().to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::{ControlRequest, NormalizedLevel, headphone_volume, speaker_volume};

    #[test]
    fn rejects_invalid_normalized_levels() {
        for value in [-0.01, 1.01, f32::NAN, f32::INFINITY] {
            assert!(NormalizedLevel::new(value).is_err());
        }
    }

    #[test]
    fn encodes_reference_speaker_volume_request() {
        assert_eq!(
            speaker_volume(NormalizedLevel::new(0.0).unwrap(), 4),
            ControlRequest {
                request: 0x01,
                value: 0x1200,
                index: 0x3604,
                payload: vec![0x00, 0x80],
            }
        );
        assert_eq!(
            speaker_volume(NormalizedLevel::new(0.5).unwrap(), 4).payload,
            vec![0x00, 0xc0]
        );
        assert_eq!(
            speaker_volume(NormalizedLevel::new(1.0).unwrap(), 4).payload,
            vec![0xff, 0xff]
        );
    }

    #[test]
    fn encodes_reference_headphone_volume_requests() {
        assert_eq!(
            headphone_volume(NormalizedLevel::new(0.0).unwrap(), 4),
            [
                ControlRequest {
                    request: 0x01,
                    value: 0x0203,
                    index: 0x0c04,
                    payload: vec![0x00, 0x80],
                },
                ControlRequest {
                    request: 0x01,
                    value: 0x0204,
                    index: 0x0c04,
                    payload: vec![0x00, 0x80],
                },
            ]
        );
        for request in headphone_volume(NormalizedLevel::new(0.5).unwrap(), 4) {
            assert_eq!(request.payload, vec![0x00, 0xc0]);
        }
        for request in headphone_volume(NormalizedLevel::new(1.0).unwrap(), 4) {
            assert_eq!(request.payload, vec![0xff, 0xff]);
        }
    }
}
