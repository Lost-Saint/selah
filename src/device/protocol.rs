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
// Phones-to-Main-Mix routing lives on mixer entity `0x33`: `MixiD`
// `set_routing_value` (driver.h) writes one byte per channel with
// `wValue = chanVals[chan]`. Channels 4 and 5 are HP L/R; position 0 is
// Main Mix (`0x1b` left, `0x1c` right). Both transfers must succeed for
// the pair to stay matched.
const ROUTING_ENTITY: u16 = 0x3300;
const PHONES_ROUTE_CONTROLS: [u16; 2] = [0x0604, 0x0605];
const MAIN_MIX_ROUTES: [u8; 2] = [0x1b, 0x1c];
// Monitor toggles live on the monitor entity `0x36`: `MixiD`
// `set_bool_state` (driver.h) writes a one-byte bool with
// `wValue = masterVals[mode]`. `MixiD` keeps the on/off state in a local
// dummy array, so Selah must present the result as sent, never confirmed.
const MONITOR_TOGGLE_ENTITY: u16 = 0x3600;

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

/// A monitor toggle on the `0x36` entity, after `MixiD` `masterVals`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MonitorToggle {
    Dim,
    AltSpeaker,
    Talkback,
    Mono,
    SpeakerMute,
}

impl MonitorToggle {
    /// All toggles in stable UI order.
    pub const ALL: [Self; 5] = [
        Self::Dim,
        Self::AltSpeaker,
        Self::Talkback,
        Self::Mono,
        Self::SpeakerMute,
    ];

    /// Short label for buttons, matching the reference panel.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Dim => "DIM",
            Self::AltSpeaker => "ALT",
            Self::Talkback => "TB",
            Self::Mono => "MONO",
            Self::SpeakerMute => "MUTE",
        }
    }

    fn control(self) -> u16 {
        match self {
            Self::Dim => 0x0500,
            Self::AltSpeaker => 0x0c00,
            Self::Talkback => 0x0700,
            Self::Mono => 0x0000,
            Self::SpeakerMute => 0x0400,
        }
    }
}

/// Encodes one monitor toggle as the one-byte bool request `MixiD` sends.
///
/// This is one-way: `MixiD` flips a local dummy bool with no readback, so
/// callers must present the result as sent, never confirmed.
pub(crate) fn monitor_toggle(
    toggle: MonitorToggle,
    on: bool,
    interface_number: u8,
) -> ControlRequest {
    ControlRequest {
        request: SET_CURRENT,
        value: toggle.control(),
        index: MONITOR_TOGGLE_ENTITY | u16::from(interface_number),
        payload: vec![u8::from(on)],
    }
}

/// Encodes routing the headphone pair to Main Mix as two channel requests.
///
/// `MixiD` `routeToggle` rows 4 and 5 (HP L/R) select Main Mix with values
/// `0x1b` and `0x1c`. This is one-way: Selah cannot read the current route
/// back, so callers must present the result as sent, never confirmed.
pub(crate) fn phones_to_main_mix(interface_number: u8) -> [ControlRequest; 2] {
    std::array::from_fn(|i| ControlRequest {
        request: SET_CURRENT,
        value: PHONES_ROUTE_CONTROLS[i],
        index: ROUTING_ENTITY | u16::from(interface_number),
        payload: vec![MAIN_MIX_ROUTES[i]],
    })
}

#[cfg(test)]
mod tests {
    use super::{
        ControlRequest, MonitorToggle, NormalizedLevel, headphone_volume, monitor_toggle,
        phones_to_main_mix, speaker_volume,
    };

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

    #[test]
    fn encodes_phones_to_main_mix_requests() {
        assert_eq!(
            phones_to_main_mix(4),
            [
                ControlRequest {
                    request: 0x01,
                    value: 0x0604,
                    index: 0x3304,
                    payload: vec![0x1b],
                },
                ControlRequest {
                    request: 0x01,
                    value: 0x0605,
                    index: 0x3304,
                    payload: vec![0x1c],
                },
            ]
        );
    }

    #[test]
    fn encodes_reference_monitor_toggles() {
        let cases = [
            (MonitorToggle::Dim, 0x0500),
            (MonitorToggle::AltSpeaker, 0x0c00),
            (MonitorToggle::Talkback, 0x0700),
            (MonitorToggle::Mono, 0x0000),
            (MonitorToggle::SpeakerMute, 0x0400),
        ];
        for (toggle, value) in cases {
            assert_eq!(
                monitor_toggle(toggle, true, 4),
                ControlRequest {
                    request: 0x01,
                    value,
                    index: 0x3604,
                    payload: vec![0x01],
                }
            );
            assert_eq!(monitor_toggle(toggle, false, 4).payload, vec![0x00]);
        }
    }

    #[test]
    fn monitor_toggle_labels_match_the_reference_panel() {
        let labels = MonitorToggle::ALL.map(MonitorToggle::label);
        assert_eq!(labels, ["DIM", "ALT", "TB", "MONO", "MUTE"]);
    }
}
