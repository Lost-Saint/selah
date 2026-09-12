use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::routing::{
    DigitalOutputMode, InvalidRoute, Route, RoutingScheme, RoutingSource, validate_route,
};

const SET_CURRENT: u8 = 0x01;
// `GET_CUR` shares `bRequest 0x01` with `SET_CUR`; the IN direction lives in
// `bmRequestType`, which `nusb` selects through its `control_in` call.
const GET_CURRENT: u8 = 0x01;
// Meters are a different request family: `GET_MEM`, answered as one block
// for every input node rather than per channel (BiD `get_meters`).
const GET_MEMORY: u8 = 0x03;
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
// Output routing lives on entity `0x33`, selector `0x06`. The selected
// model capability supplies actual output indexes; source bytes use either
// MixiD's iD14 table or BiD's externally verified iD24 mapping.
const ROUTING_ENTITY: u16 = 0x3300;
const ROUTING_SELECTOR: u16 = 0x0600;
// Official-app decoding in Monix/BiD identifies optical-output mode as a
// four-byte little-endian value: 0 = ADAT, 1 = S/PDIF.
const DIGITAL_OUTPUT_MODE_ENTITY: u16 = 0x1400;
const DIGITAL_OUTPUT_MODE_CONTROL: u16 = 0x0100;
// Monitor toggles live on the monitor entity `0x36`: `MixiD`
// `set_bool_state` (driver.h) writes a one-byte bool with
// `wValue = masterVals[mode]`. `MixiD` keeps the on/off state in a local
// dummy array, so Selah must present the result as sent, never confirmed.
const MONITOR_TOGGLE_ENTITY: u16 = 0x3600;
// Mixer matrix lives on entity `0x3c`: `MixiD` `set_channel_volume`
// (driver.h) writes one input's Main-send pair as two cells,
// `wValue = 0x0100 + channel * 6` (Main L) and `+ 1` (Main R). The two
// transfers must succeed for the pair to stay matched. Per BiD's
// measurements the pair is the input's stereo image (ratio = pan), so a
// single level to both sums the input to the centre.
const MIXER_MATRIX_ENTITY: u16 = 0x3c00;
const MIXER_MATRIX_SELECTOR_BASE: u16 = 0x0100;
const MIXER_MATRIX_CELL_STRIDE: u16 = 6;
// Input-node meter block: sixteen nodes of two bytes, the first byte of
// each pair carrying the level (BiD `get_meters`).
const METER_BLOCK_LENGTH: u16 = 32;
/// How many input nodes one meter block can describe.
pub const MAX_METER_INPUTS: u8 = 16;
// Input polarity lives on entity `0x0b`: `MixiD` `set_phase_state`
// writes a one-byte bool with `wValue = 0x0d01 + channel`.
const POLARITY_ENTITY: u16 = 0x0b00;
const POLARITY_SELECTOR_BASE: u16 = 0x0d01;

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

/// Encodes one input channel's level as its Main-send cell pair.
///
/// `channel` is the running input index across microphone then digital
/// inputs (not the per-group position: `MixiD`'s digital loop reuses its
/// loop counter, aliasing digital channels onto microphone mappings, which
/// Selah deliberately does not reproduce).
///
/// Both transfers must succeed for the pair to stay matched. This is
/// one-way: the matrix does not read back, so callers must present the
/// result as sent, never confirmed.
pub(crate) fn channel_volume(
    level: NormalizedLevel,
    channel: u8,
    interface_number: u8,
) -> [ControlRequest; 2] {
    let base = MIXER_MATRIX_SELECTOR_BASE + u16::from(channel) * MIXER_MATRIX_CELL_STRIDE;
    [base, base + 1].map(|control| ControlRequest {
        request: SET_CURRENT,
        value: control,
        index: MIXER_MATRIX_ENTITY | u16::from(interface_number),
        payload: level.audient_value().to_le_bytes().to_vec(),
    })
}

/// Encodes one input channel's polarity as the one-byte bool `MixiD` sends.
///
/// This is one-way: polarity does not read back, so callers must present
/// the result as sent, never confirmed.
pub(crate) fn channel_polarity(channel: u8, flipped: bool, interface_number: u8) -> ControlRequest {
    ControlRequest {
        request: SET_CURRENT,
        value: POLARITY_SELECTOR_BASE + u16::from(channel),
        index: POLARITY_ENTITY | u16::from(interface_number),
        payload: vec![u8::from(flipped)],
    }
}

/// Encodes a validated named route for both halves of a physical output pair.
///
/// Validation happens before any request is returned, so callers cannot send
/// a source, destination, or channel index outside the selected model's
/// capability table.
pub(crate) fn output_route(
    model: &crate::device::DeviceModel,
    route: Route,
    interface_number: u8,
) -> Result<[ControlRequest; 2], InvalidRoute> {
    let (capabilities, output) = validate_route(model, route)?;
    Ok(std::array::from_fn(|side| {
        let channel = output.channels[side];
        ControlRequest {
            request: SET_CURRENT,
            value: ROUTING_SELECTOR | u16::from(channel),
            index: ROUTING_ENTITY | u16::from(interface_number),
            payload: vec![route_code(capabilities.scheme, route.source, channel)],
        }
    }))
}

fn route_code(scheme: RoutingScheme, source: RoutingSource, output_channel: u8) -> u8 {
    let side = output_channel & 1;
    match (scheme, source) {
        (RoutingScheme::Id14Table, RoutingSource::MainMix) => 0x1b + side,
        (RoutingScheme::Id14Table, RoutingSource::AltSpeaker) => unreachable!("validated out"),
        (RoutingScheme::Id14Table, RoutingSource::CueA) => 0x19,
        (RoutingScheme::Id14Table, RoutingSource::CueB) => 0x1a,
        (RoutingScheme::Id14Table | RoutingScheme::Id24Formula, RoutingSource::DawMix) => {
            output_channel
        }
        (RoutingScheme::Id24Formula, RoutingSource::MainMix) => 0x25 + side,
        (RoutingScheme::Id24Formula, RoutingSource::AltSpeaker) => 0x27 + side,
        (RoutingScheme::Id24Formula, RoutingSource::CueA) => 0x1e + side,
        (RoutingScheme::Id24Formula, RoutingSource::CueB) => 0x20 + side,
    }
}

/// Encodes the evidenced iD24 optical-output mode request.
pub(crate) fn digital_output_mode(mode: DigitalOutputMode, interface_number: u8) -> ControlRequest {
    let value = match mode {
        DigitalOutputMode::Adat => 0_u32,
        DigitalOutputMode::Spdif => 1_u32,
    };
    ControlRequest {
        request: SET_CURRENT,
        value: DIGITAL_OUTPUT_MODE_CONTROL,
        index: DIGITAL_OUTPUT_MODE_ENTITY | u16::from(interface_number),
        payload: value.to_le_bytes().to_vec(),
    }
}

/// A device-to-host control read: the `GET_CUR` or `GET_MEM` counterpart of
/// a [`ControlRequest`]. Only entities with reference evidence that reads
/// answer truthfully have constructors here. The mixer matrix, routing
/// table, channel polarity, and headphone volume read back aliased values
/// or stall, so they stay write-only (see `docs/protocol.md`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ControlReadRequest {
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

/// Reads the monitor volume node back: entity `0x36`, selector `0x12`, two
/// bytes little-endian (`BiD` `get_monitor_volume`).
pub(crate) fn speaker_volume_read(interface_number: u8) -> ControlReadRequest {
    ControlReadRequest {
        request: GET_CURRENT,
        value: SPEAKER_VOLUME_CONTROL,
        index: SPEAKER_OUTPUT_ENTITY | u16::from(interface_number),
        length: 2,
    }
}

/// Reads one monitor toggle back as the one-byte bool the device holds
/// (`BiD` `get_bool_state`). The `wValue` matches the corresponding write.
pub(crate) fn monitor_toggle_read(
    toggle: MonitorToggle,
    interface_number: u8,
) -> ControlReadRequest {
    ControlReadRequest {
        request: GET_CURRENT,
        value: toggle.control(),
        index: MONITOR_TOGGLE_ENTITY | u16::from(interface_number),
        length: 1,
    }
}

/// Reads the evidenced iD24 optical-output mode back: entity `0x14`,
/// selector `0x01`, four bytes little-endian (`BiD` `get_optical_mode`).
pub(crate) fn digital_output_mode_read(interface_number: u8) -> ControlReadRequest {
    ControlReadRequest {
        request: GET_CURRENT,
        value: DIGITAL_OUTPUT_MODE_CONTROL,
        index: DIGITAL_OUTPUT_MODE_ENTITY | u16::from(interface_number),
        length: 4,
    }
}

/// Reads the whole input-node meter block at once: `GET_MEM` on entity
/// `0x3c`, offset zero (`BiD` `get_meters`). Callers decode per-input levels
/// with [`decode_meter_block`].
pub(crate) fn meter_block_read(interface_number: u8) -> ControlReadRequest {
    ControlReadRequest {
        request: GET_MEMORY,
        value: 0x0000,
        index: MIXER_MATRIX_ENTITY | u16::from(interface_number),
        length: METER_BLOCK_LENGTH,
    }
}

/// A device response that cannot be trusted as the requested value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    /// The device answered with fewer (or more) bytes than the request asks
    /// for. A short block is rejected rather than zero-filled.
    ShortResponse { expected: u16, actual: usize },
    /// The bytes arrived intact but name no valid value.
    InvalidValue(&'static str),
}

impl Display for DecodeError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ShortResponse { expected, actual } => write!(
                formatter,
                "device answered {actual} bytes, expected {expected}"
            ),
            Self::InvalidValue(detail) => {
                write!(formatter, "device answered an invalid value: {detail}")
            }
        }
    }
}

impl Error for DecodeError {}

/// Decodes a two-byte little-endian level response into `0.0..=1.0`.
///
/// This inverts [`NormalizedLevel`]'s device mapping; values outside the
/// volume range clamp instead of failing, matching the `BiD` reference.
pub(crate) fn decode_level_response(bytes: &[u8]) -> Result<f32, DecodeError> {
    if bytes.len() != 2 {
        return Err(DecodeError::ShortResponse {
            expected: 2,
            actual: bytes.len(),
        });
    }
    let raw = i16::from_le_bytes([bytes[0], bytes[1]]);
    Ok(((f32::from(raw) + 32_768.0) / 32_767.0).clamp(0.0, 1.0))
}

/// Decodes a one-byte monitor-toggle response: any nonzero byte means on.
pub(crate) fn decode_toggle_response(bytes: &[u8]) -> Result<bool, DecodeError> {
    if bytes.len() != 1 {
        return Err(DecodeError::ShortResponse {
            expected: 1,
            actual: bytes.len(),
        });
    }
    Ok(bytes[0] != 0)
}

/// Decodes a four-byte optical-output mode response (`0` = ADAT,
/// `1` = S/PDIF). Any other first byte is rejected, not guessed.
pub(crate) fn decode_digital_output_mode_response(
    bytes: &[u8],
) -> Result<DigitalOutputMode, DecodeError> {
    if bytes.len() != 4 {
        return Err(DecodeError::ShortResponse {
            expected: 4,
            actual: bytes.len(),
        });
    }
    match bytes[0] {
        0 => Ok(DigitalOutputMode::Adat),
        1 => Ok(DigitalOutputMode::Spdif),
        _ => Err(DecodeError::InvalidValue(
            "optical output mode is neither ADAT (0) nor S/PDIF (1)",
        )),
    }
}

/// Decodes the meter block into one level byte per input node.
///
/// `inputs` is the model's running input count, capped at
/// [`MAX_METER_INPUTS`]. The block must arrive whole: a short block is
/// rejected rather than padded with fake silence.
pub(crate) fn decode_meter_block(bytes: &[u8], inputs: u8) -> Result<Vec<u8>, DecodeError> {
    if inputs > MAX_METER_INPUTS {
        return Err(DecodeError::InvalidValue(
            "meter input count exceeds one block",
        ));
    }
    if bytes.len() != usize::from(METER_BLOCK_LENGTH) {
        return Err(DecodeError::ShortResponse {
            expected: METER_BLOCK_LENGTH,
            actual: bytes.len(),
        });
    }
    Ok((0..inputs)
        .map(|input| bytes[usize::from(input) * 2])
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{
        ControlReadRequest, ControlRequest, DecodeError, MAX_METER_INPUTS, MonitorToggle,
        NormalizedLevel, channel_polarity, channel_volume, decode_digital_output_mode_response,
        decode_level_response, decode_meter_block, decode_toggle_response, digital_output_mode,
        digital_output_mode_read, headphone_volume, meter_block_read, monitor_toggle,
        monitor_toggle_read, output_route, speaker_volume, speaker_volume_read,
    };
    use crate::routing::{DigitalOutputMode, Route, RoutingDestination, RoutingSource};

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
    fn encodes_id14_output_routes_at_minimum_and_maximum_channels() {
        let model = crate::device::supported_device(0x0008).unwrap();
        assert_eq!(
            output_route(
                model,
                Route {
                    destination: RoutingDestination::MainSpeakers,
                    source: RoutingSource::MainMix,
                },
                4
            )
            .unwrap(),
            [
                ControlRequest {
                    request: 0x01,
                    value: 0x0600,
                    index: 0x3304,
                    payload: vec![0x1b],
                },
                ControlRequest {
                    request: 0x01,
                    value: 0x0601,
                    index: 0x3304,
                    payload: vec![0x1c],
                },
            ]
        );
        let phones = output_route(
            model,
            Route {
                destination: RoutingDestination::Headphones,
                source: RoutingSource::DawMix,
            },
            4,
        )
        .unwrap();
        assert_eq!([phones[0].value, phones[1].value], [0x0604, 0x0605]);
        assert_eq!([phones[0].payload[0], phones[1].payload[0]], [4, 5]);
    }

    #[test]
    fn encodes_id24_extended_outputs() {
        let model = crate::device::supported_device(0x000d).unwrap();
        let line = output_route(
            model,
            Route {
                destination: RoutingDestination::Outputs3And4,
                source: RoutingSource::AltSpeaker,
            },
            4,
        )
        .unwrap();
        assert_eq!([line[0].value, line[1].value], [0x0602, 0x0603]);
        assert_eq!([line[0].payload[0], line[1].payload[0]], [0x27, 0x28]);
    }

    #[test]
    fn encodes_bounded_id24_digital_output_modes() {
        assert_eq!(
            digital_output_mode(DigitalOutputMode::Adat, 4),
            ControlRequest {
                request: 0x01,
                value: 0x0100,
                index: 0x1404,
                payload: vec![0, 0, 0, 0],
            }
        );
        assert_eq!(
            digital_output_mode(DigitalOutputMode::Spdif, 4).payload,
            vec![1, 0, 0, 0]
        );
    }

    #[test]
    fn rejects_unsupported_route_before_encoding() {
        let model = crate::device::supported_device(0x0008).unwrap();
        assert!(
            output_route(
                model,
                Route {
                    destination: RoutingDestination::Headphones,
                    source: RoutingSource::AltSpeaker,
                },
                4
            )
            .is_err()
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
    fn encodes_channel_volume_cell_pair() {
        assert_eq!(
            channel_volume(NormalizedLevel::new(1.0).unwrap(), 0, 4),
            [
                ControlRequest {
                    request: 0x01,
                    value: 0x0100,
                    index: 0x3c04,
                    payload: vec![0xff, 0xff],
                },
                ControlRequest {
                    request: 0x01,
                    value: 0x0101,
                    index: 0x3c04,
                    payload: vec![0xff, 0xff],
                },
            ]
        );
        // Channel 9 (last input on the iD14 MKII) strides by six cells.
        for request in channel_volume(NormalizedLevel::new(0.0).unwrap(), 9, 4) {
            assert_eq!(request.request, 0x01);
            assert_eq!(request.index, 0x3c04);
            assert_eq!(request.payload, vec![0x00, 0x80]);
        }
        let pair = channel_volume(NormalizedLevel::new(0.5).unwrap(), 9, 4);
        assert_eq!([pair[0].value, pair[1].value], [0x0136, 0x0137]);
    }

    #[test]
    fn encodes_channel_polarity_request() {
        assert_eq!(
            channel_polarity(2, true, 4),
            ControlRequest {
                request: 0x01,
                value: 0x0d03,
                index: 0x0b04,
                payload: vec![0x01],
            }
        );
        assert_eq!(channel_polarity(0, false, 4).payload, vec![0x00]);
    }

    #[test]
    fn read_requests_mirror_their_write_addresses() {
        assert_eq!(
            speaker_volume_read(4),
            ControlReadRequest {
                request: 0x01,
                value: 0x1200,
                index: 0x3604,
                length: 2,
            }
        );
        assert_eq!(
            monitor_toggle_read(MonitorToggle::Dim, 4),
            ControlReadRequest {
                request: 0x01,
                value: 0x0500,
                index: 0x3604,
                length: 1,
            }
        );
        assert_eq!(
            digital_output_mode_read(4),
            ControlReadRequest {
                request: 0x01,
                value: 0x0100,
                index: 0x1404,
                length: 4,
            }
        );
        // Meters are the one block GET_MEM read, not a per-channel GET_CUR.
        assert_eq!(
            meter_block_read(4),
            ControlReadRequest {
                request: 0x03,
                value: 0x0000,
                index: 0x3c04,
                length: 32,
            }
        );
    }

    #[test]
    fn level_responses_invert_the_device_mapping() {
        // Endpoints of the write mapping decode back to themselves.
        for level in [0.0, 0.25, 0.5, 1.0] {
            let encoded = NormalizedLevel::new(level).unwrap();
            let bytes = encoded.audient_value().to_le_bytes();
            let decoded = decode_level_response(&bytes).unwrap();
            assert!(
                (decoded - level).abs() < 0.001,
                "level {level} decoded as {decoded}"
            );
        }
        // Raw silence and full scale land exactly on the endpoints.
        assert_eq!(decode_level_response(&[0x00, 0x80]), Ok(0.0));
        assert_eq!(decode_level_response(&[0xff, 0xff]), Ok(1.0));
    }

    #[test]
    fn short_level_responses_are_rejected_not_zero_filled() {
        for bytes in [&[][..], &[0x00][..], &[0x00, 0x80, 0x00][..]] {
            assert_eq!(
                decode_level_response(bytes),
                Err(DecodeError::ShortResponse {
                    expected: 2,
                    actual: bytes.len(),
                })
            );
        }
    }

    #[test]
    fn toggle_responses_decode_any_nonzero_byte_as_on() {
        assert_eq!(decode_toggle_response(&[0x00]), Ok(false));
        assert_eq!(decode_toggle_response(&[0x01]), Ok(true));
        assert_eq!(decode_toggle_response(&[0xff]), Ok(true));
        assert_eq!(
            decode_toggle_response(&[]),
            Err(DecodeError::ShortResponse {
                expected: 1,
                actual: 0,
            })
        );
        assert_eq!(
            decode_toggle_response(&[0x01, 0x00]),
            Err(DecodeError::ShortResponse {
                expected: 1,
                actual: 2,
            })
        );
    }

    #[test]
    fn digital_mode_responses_accept_only_known_modes() {
        assert_eq!(
            decode_digital_output_mode_response(&[0, 0, 0, 0]),
            Ok(DigitalOutputMode::Adat)
        );
        assert_eq!(
            decode_digital_output_mode_response(&[1, 0, 0, 0]),
            Ok(DigitalOutputMode::Spdif)
        );
        assert!(matches!(
            decode_digital_output_mode_response(&[2, 0, 0, 0]),
            Err(DecodeError::InvalidValue(_))
        ));
        assert_eq!(
            decode_digital_output_mode_response(&[0, 0, 0]),
            Err(DecodeError::ShortResponse {
                expected: 4,
                actual: 3,
            })
        );
    }

    #[test]
    fn meter_blocks_yield_the_first_byte_of_each_node() {
        let mut block = [0_u8; 32];
        let (pairs, _) = block.as_chunks_mut::<2>();
        let mut level = 0_u8;
        for pair in pairs {
            pair[0] = level;
            pair[1] = 0xaa;
            level = level.wrapping_add(0x10);
        }
        assert_eq!(decode_meter_block(&block, 3), Ok(vec![0x00, 0x10, 0x20]));
        assert_eq!(decode_meter_block(&block, 0), Ok(vec![]));
        assert_eq!(
            decode_meter_block(&block, MAX_METER_INPUTS).unwrap().len(),
            16
        );
    }

    #[test]
    fn short_or_oversized_meter_reads_are_rejected() {
        let block = [0_u8; 32];
        assert_eq!(
            decode_meter_block(&block[..31], 10),
            Err(DecodeError::ShortResponse {
                expected: 32,
                actual: 31,
            })
        );
        assert_eq!(
            decode_meter_block(&[], 10),
            Err(DecodeError::ShortResponse {
                expected: 32,
                actual: 0,
            })
        );
        assert!(matches!(
            decode_meter_block(&block, MAX_METER_INPUTS + 1),
            Err(DecodeError::InvalidValue(_))
        ));
    }
}
