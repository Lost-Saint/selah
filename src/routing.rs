//! Capability-driven output routing and honest write-only operation state.
//!
//! Audient routing assigns one named mix source to each physical stereo
//! output pair. The device has no route-off value: resetting a route means
//! sending that output pair's documented default source.

use std::error::Error;
use std::fmt::{self, Display, Formatter};

use crate::device::{DetectedDevice, DeviceModel};

/// A source that a physical output pair can play.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingSource {
    MainMix,
    AltSpeaker,
    CueA,
    CueB,
    DawMix,
}

impl RoutingSource {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::MainMix => "Main Mix",
            Self::AltSpeaker => "Alt Speaker",
            Self::CueA => "Cue A",
            Self::CueB => "Cue B",
            Self::DawMix => "DAW Mix",
        }
    }
}

impl Display for RoutingSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One channel in an eight-channel ADAT stream, numbered as users see it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AdatChannel(u8);

impl AdatChannel {
    /// Creates an ADAT channel in the user-facing range `1..=8`.
    pub const fn new(channel: u8) -> Option<Self> {
        if channel >= 1 && channel <= 8 {
            Some(Self(channel))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn number(self) -> u8 {
        self.0
    }
}

/// A model's digital output relationship, independent of USB route indexes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DigitalOutput {
    /// The first stereo pair carried by an ADAT optical output.
    AdatPair {
        left: AdatChannel,
        right: AdatChannel,
    },
}

/// Optical output framing supported by the evidenced iD24 request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DigitalOutputMode {
    Adat,
    Spdif,
}

impl DigitalOutputMode {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Adat => "ADAT",
            Self::Spdif => "S/PDIF",
        }
    }

    #[must_use]
    pub const fn as_toggle(self) -> bool {
        matches!(self, Self::Adat)
    }
}

/// A physical stereo output pair that receives a routing source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingDestination {
    MainSpeakers,
    Outputs3And4,
    Headphones,
    Digital(DigitalOutput),
}

impl RoutingDestination {
    #[must_use]
    pub fn label(self) -> String {
        match self {
            Self::MainSpeakers => "Main speakers · Outputs 1/2".to_owned(),
            Self::Outputs3And4 => "Line outputs · Outputs 3/4".to_owned(),
            Self::Headphones => "Headphones".to_owned(),
            Self::Digital(DigitalOutput::AdatPair { left, right }) => {
                format!("ADAT outputs {}/{}", left.number(), right.number())
            }
        }
    }
}

/// Known wire-code family for a model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RoutingScheme {
    /// `MixiD`'s six-output table, verified externally on an iD14 MKII.
    Id14Table,
    /// The iD24 formula decoded from the official app and verified externally.
    Id24Formula,
}

/// One routable output pair and its safe reset source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoutingOutput {
    pub destination: RoutingDestination,
    /// Actual routing-unit output channels. These are protocol identity, not UI
    /// row numbers; the optical pair is deliberately non-contiguous with the
    /// analog pairs.
    pub channels: [u8; 2],
    pub default_source: RoutingSource,
}

/// Static, per-model routing facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RoutingCapabilities {
    pub scheme: RoutingScheme,
    pub sources: &'static [RoutingSource],
    pub outputs: &'static [RoutingOutput],
    /// Whether the optical output's ADAT/S/PDIF mode request is evidenced.
    pub digital_output_mode: bool,
}

const ID14_SOURCES: &[RoutingSource] = &[
    RoutingSource::MainMix,
    RoutingSource::CueA,
    RoutingSource::CueB,
    RoutingSource::DawMix,
];

const ID14_OUTPUTS: &[RoutingOutput] = &[
    RoutingOutput {
        destination: RoutingDestination::MainSpeakers,
        channels: [0, 1],
        default_source: RoutingSource::MainMix,
    },
    RoutingOutput {
        destination: RoutingDestination::Outputs3And4,
        channels: [2, 3],
        default_source: RoutingSource::DawMix,
    },
    RoutingOutput {
        destination: RoutingDestination::Headphones,
        channels: [4, 5],
        default_source: RoutingSource::CueA,
    },
];

const ID24_SOURCES: &[RoutingSource] = &[
    RoutingSource::MainMix,
    RoutingSource::AltSpeaker,
    RoutingSource::CueA,
    RoutingSource::CueB,
    RoutingSource::DawMix,
];

const ID24_OUTPUTS: &[RoutingOutput] = &[
    RoutingOutput {
        destination: RoutingDestination::MainSpeakers,
        channels: [0, 1],
        default_source: RoutingSource::MainMix,
    },
    RoutingOutput {
        destination: RoutingDestination::Outputs3And4,
        channels: [2, 3],
        default_source: RoutingSource::AltSpeaker,
    },
    RoutingOutput {
        destination: RoutingDestination::Headphones,
        channels: [4, 5],
        default_source: RoutingSource::CueA,
    },
];

pub const ID14_ROUTING: RoutingCapabilities = RoutingCapabilities {
    scheme: RoutingScheme::Id14Table,
    sources: ID14_SOURCES,
    outputs: ID14_OUTPUTS,
    digital_output_mode: false,
};

pub const ID24_ROUTING: RoutingCapabilities = RoutingCapabilities {
    scheme: RoutingScheme::Id24Formula,
    sources: ID24_SOURCES,
    outputs: ID24_OUTPUTS,
    digital_output_mode: true,
};

/// A validated source/destination choice.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Route {
    pub destination: RoutingDestination,
    pub source: RoutingSource,
}

/// Why a route cannot be encoded for a model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidRoute {
    RoutingUnavailable,
    UnsupportedSource(RoutingSource),
    UnsupportedDestination(RoutingDestination),
}

impl Display for InvalidRoute {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::RoutingUnavailable => {
                formatter.write_str("routing is unavailable for this model")
            }
            Self::UnsupportedSource(source) => {
                write!(
                    formatter,
                    "{} is unavailable for this model",
                    source.label()
                )
            }
            Self::UnsupportedDestination(destination) => {
                write!(
                    formatter,
                    "{} is not routable on this model",
                    destination.label()
                )
            }
        }
    }
}

impl Error for InvalidRoute {}

/// Validates a route and returns its model-owned output mapping.
pub fn validate_route(
    model: &DeviceModel,
    route: Route,
) -> Result<(&'static RoutingCapabilities, &'static RoutingOutput), InvalidRoute> {
    let capabilities = model.routing.ok_or(InvalidRoute::RoutingUnavailable)?;
    if !capabilities.sources.contains(&route.source) {
        return Err(InvalidRoute::UnsupportedSource(route.source));
    }
    let output = capabilities
        .outputs
        .iter()
        .find(|output| output.destination == route.destination)
        .ok_or(InvalidRoute::UnsupportedDestination(route.destination))?;
    Ok((capabilities, output))
}

/// Returns the documented default route for an output pair.
pub fn reset_route(
    model: &DeviceModel,
    destination: RoutingDestination,
) -> Result<Route, InvalidRoute> {
    let capabilities = model.routing.ok_or(InvalidRoute::RoutingUnavailable)?;
    let output = capabilities
        .outputs
        .iter()
        .find(|output| output.destination == destination)
        .ok_or(InvalidRoute::UnsupportedDestination(destination))?;
    Ok(Route {
        destination,
        source: output.default_source,
    })
}

/// Maps a user-facing ADAT input to its mixer row after the model's onboard
/// microphone inputs. This fixes the iD22 expansion alias described in `MixiD`
/// issue #25 without assuming USB output-route channels are contiguous.
pub fn adat_input_mixer_index(
    model: &DeviceModel,
    channel: AdatChannel,
) -> Result<u8, InvalidRoute> {
    if channel.number() > model.digital_inputs.min(8) {
        return Err(InvalidRoute::RoutingUnavailable);
    }
    Ok(model.mic_inputs + channel.number() - 1)
}

/// Honest operation state for one output pair.
#[derive(Clone, Debug, Default)]
pub struct RouteControl {
    requested: Option<RoutingSource>,
    in_flight: Option<RoutingSource>,
    last_sent: Option<RoutingSource>,
    last_error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RouteStatus {
    Unknown,
    Sending {
        source: RoutingSource,
    },
    Sent {
        source: RoutingSource,
    },
    Failed {
        error: String,
        last_sent: Option<RoutingSource>,
    },
}

impl RouteControl {
    #[must_use]
    pub fn requested(&self) -> Option<RoutingSource> {
        self.requested
    }

    #[must_use]
    pub fn status(&self) -> RouteStatus {
        match (self.in_flight, &self.last_error, self.last_sent) {
            (Some(source), _, _) => RouteStatus::Sending { source },
            (None, Some(error), last_sent) => RouteStatus::Failed {
                error: error.clone(),
                last_sent,
            },
            (None, None, Some(source)) => RouteStatus::Sent { source },
            (None, None, None) => RouteStatus::Unknown,
        }
    }

    #[must_use]
    pub fn status_text(&self) -> String {
        match self.status() {
            RouteStatus::Unknown => {
                "Hardware route unknown — choose a source to send it.".to_owned()
            }
            RouteStatus::Sending { source } => format!("Sending {}…", source.label()),
            RouteStatus::Sent { source } => format!(
                "Last sent {} — accepted, not read back from the device.",
                source.label()
            ),
            RouteStatus::Failed { error, .. } => {
                format!("Route send failed: {error} Choose a source to retry.")
            }
        }
    }

    #[must_use]
    pub fn request(&mut self, source: RoutingSource) -> Option<RoutingSource> {
        if self.in_flight.is_some() {
            return None;
        }
        self.requested = Some(source);
        self.in_flight = Some(source);
        self.last_error = None;
        Some(source)
    }

    #[must_use]
    pub fn is_in_flight(&self, source: RoutingSource) -> bool {
        self.in_flight == Some(source)
    }

    pub fn finish(&mut self, source: RoutingSource, result: Result<(), String>) -> bool {
        if self.in_flight != Some(source) {
            return false;
        }
        self.in_flight = None;
        match result {
            Ok(()) => {
                self.last_sent = Some(source);
                self.last_error = None;
            }
            Err(error) => self.last_error = Some(error),
        }
        true
    }
}

#[derive(Clone, Debug)]
pub struct OutputRouteControl {
    pub destination: RoutingDestination,
    pub control: RouteControl,
}

#[must_use]
pub fn route_controls(model: &DeviceModel) -> Vec<OutputRouteControl> {
    model.routing.map_or_else(Vec::new, |capabilities| {
        capabilities
            .outputs
            .iter()
            .map(|output| OutputRouteControl {
                destination: output.destination,
                control: RouteControl::default(),
            })
            .collect()
    })
}

#[must_use]
pub fn routing_available(device: &DetectedDevice) -> bool {
    device.control_interface.is_some() && device.model.routing.is_some()
}

#[must_use]
pub fn digital_output_mode_available(device: &DetectedDevice) -> bool {
    device.control_interface.is_some()
        && device
            .model
            .routing
            .is_some_and(|capabilities| capabilities.digital_output_mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id14_capabilities_include_outputs_3_4_but_not_alt() {
        let model = crate::device::supported_device(0x0008).unwrap();
        let capabilities = model.routing.unwrap();
        assert_eq!(capabilities.outputs.len(), 3);
        assert!(capabilities.sources.contains(&RoutingSource::CueB));
        assert!(!capabilities.sources.contains(&RoutingSource::AltSpeaker));
        assert!(
            validate_route(
                model,
                Route {
                    destination: RoutingDestination::Outputs3And4,
                    source: RoutingSource::DawMix,
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn id24_exposes_digital_mode_without_inventing_an_optical_route_reset() {
        let model = crate::device::supported_device(0x000d).unwrap();
        assert!(model.routing.unwrap().digital_output_mode);
        assert_eq!(model.routing.unwrap().outputs.len(), 3);
    }

    #[test]
    fn id22_adat_inputs_follow_onboard_inputs_in_mixer_order() {
        let model = crate::device::supported_device(0x0001).unwrap();
        assert_eq!(
            adat_input_mixer_index(model, AdatChannel::new(1).unwrap()),
            Ok(2)
        );
        assert_eq!(
            adat_input_mixer_index(model, AdatChannel::new(8).unwrap()),
            Ok(9)
        );
        assert!(
            model.routing.is_none(),
            "unverified iD22 route codes stay unavailable"
        );
    }

    #[test]
    fn rejects_invalid_sources_destinations_and_adat_channels() {
        let id14 = crate::device::supported_device(0x0008).unwrap();
        assert!(matches!(
            validate_route(
                id14,
                Route {
                    destination: RoutingDestination::Headphones,
                    source: RoutingSource::AltSpeaker,
                }
            ),
            Err(InvalidRoute::UnsupportedSource(_))
        ));
        assert!(matches!(
            validate_route(
                id14,
                Route {
                    destination: RoutingDestination::Digital(DigitalOutput::AdatPair {
                        left: AdatChannel::new(1).unwrap(),
                        right: AdatChannel::new(2).unwrap(),
                    }),
                    source: RoutingSource::MainMix,
                }
            ),
            Err(InvalidRoute::UnsupportedDestination(_))
        ));
        assert!(AdatChannel::new(0).is_none());
        assert!(AdatChannel::new(9).is_none());
    }

    #[test]
    fn source_display_matches_the_dropdown_label() {
        assert_eq!(RoutingSource::MainMix.to_string(), "Main Mix");
        assert_eq!(RoutingSource::CueA.to_string(), "Cue A");
        assert_eq!(RoutingSource::DawMix.to_string(), "DAW Mix");
    }

    #[test]
    fn reset_returns_each_output_documented_default() {
        let model = crate::device::supported_device(0x0008).unwrap();
        assert_eq!(
            reset_route(model, RoutingDestination::Outputs3And4)
                .unwrap()
                .source,
            RoutingSource::DawMix
        );
        assert_eq!(
            reset_route(model, RoutingDestination::Headphones)
                .unwrap()
                .source,
            RoutingSource::CueA
        );
    }

    #[test]
    fn operation_state_never_claims_hardware_confirmation() {
        let mut control = RouteControl::default();
        assert_eq!(control.status(), RouteStatus::Unknown);
        assert_eq!(
            control.request(RoutingSource::CueA),
            Some(RoutingSource::CueA)
        );
        assert_eq!(
            control.status(),
            RouteStatus::Sending {
                source: RoutingSource::CueA
            }
        );
        assert!(control.finish(RoutingSource::CueA, Ok(())));
        assert_eq!(
            control.status(),
            RouteStatus::Sent {
                source: RoutingSource::CueA
            }
        );
        assert!(control.status_text().contains("not read back"));
    }
}
