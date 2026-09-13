use crate::device::{
    DeviceLocation, DeviceWatchEvent, DiscoveryError, DiscoveryReport, FeedbackSnapshot,
    MonitorToggle,
};
use crate::routing::{DigitalOutputMode, Route, RoutingDestination};

/// Outcome of one background speaker-volume send, paired with the level it
/// attempted so stale completions can be ignored after a rescan.
#[derive(Clone, Debug)]
pub(crate) struct SpeakerVolumeOutcome {
    pub(crate) level: f32,
    pub(crate) result: Result<(), String>,
}

/// Outcome of one background headphone-volume send, paired with the level
/// it attempted so stale completions can be ignored after a rescan.
#[derive(Clone, Debug)]
pub(crate) struct HeadphoneVolumeOutcome {
    pub(crate) level: f32,
    pub(crate) result: Result<(), String>,
}

/// Outcome of one background monitor-toggle send, paired with the toggle and
/// value it attempted so stale completions can be ignored after a rescan.
#[derive(Clone, Debug)]
pub(crate) struct MonitorToggleOutcome {
    pub(crate) toggle: MonitorToggle,
    pub(crate) on: bool,
    pub(crate) result: Result<(), String>,
}

/// Outcome of one background channel-level send, paired with the channel
/// and level it attempted so stale completions can be ignored.
#[derive(Clone, Debug)]
pub(crate) struct ChannelLevelOutcome {
    pub(crate) channel: u8,
    pub(crate) level: f32,
    pub(crate) result: Result<(), String>,
}

/// Outcome of one background channel-polarity send, paired with the channel
/// and value it attempted so stale completions can be ignored.
#[derive(Clone, Debug)]
pub(crate) struct ChannelPolarityOutcome {
    pub(crate) channel: u8,
    pub(crate) flipped: bool,
    pub(crate) result: Result<(), String>,
}

/// Outcome of one output-pair routing send. The complete typed route is its
/// identity, so a completion from an older attachment cannot update new state.
#[derive(Clone, Debug)]
pub(crate) struct RoutingOutcome {
    pub(crate) route: Route,
    pub(crate) result: Result<(), String>,
}

#[derive(Clone, Debug)]
pub(crate) struct DigitalOutputModeOutcome {
    pub(crate) mode: DigitalOutputMode,
    pub(crate) result: Result<(), String>,
}

/// Outcome of one background feedback poll, paired with the attachment it
/// read so completions from a replaced device are ignored.
#[derive(Clone, Debug)]
pub(crate) struct FeedbackOutcome {
    pub(crate) location: DeviceLocation,
    pub(crate) read_monitor: bool,
    pub(crate) result: Result<FeedbackSnapshot, String>,
}

#[derive(Clone, Debug)]
pub(crate) enum Message {
    Refresh,
    DeviceSelected(DeviceLocation),
    DeviceWatch(DeviceWatchEvent),
    DiscoveryFinished(Result<DiscoveryReport, DiscoveryError>),
    SpeakerVolumeChanged(f32),
    SpeakerVolumeFinished(SpeakerVolumeOutcome),
    HeadphoneVolumeChanged(f32),
    HeadphoneVolumeFinished(HeadphoneVolumeOutcome),
    RouteSelected(Route),
    RouteReset(RoutingDestination),
    RoutingFinished(RoutingOutcome),
    DigitalOutputModeSelected(DigitalOutputMode),
    DigitalOutputModeFinished(DigitalOutputModeOutcome),
    MonitorToggleChanged { toggle: MonitorToggle, on: bool },
    MonitorToggleFinished(MonitorToggleOutcome),
    ChannelLevelChanged { channel: u8, level: f32 },
    ChannelLevelFinished(ChannelLevelOutcome),
    ChannelPolarityChanged { channel: u8, flipped: bool },
    ChannelPolarityFinished(ChannelPolarityOutcome),
    FeedbackTick,
    FeedbackFinished(FeedbackOutcome),
}
