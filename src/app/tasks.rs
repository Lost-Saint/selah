use iced::Task;

use crate::device::{
    DetectedDevice, MonitorToggle, discover, read_feedback, send_channel_level,
    send_channel_polarity, send_digital_output_mode, send_monitor_toggle, send_output_route,
    send_speaker_level,
};
use crate::monitor::{ToggleControl, VolumeControl};
use crate::routing::{DigitalOutputMode, Route};

use super::message::{
    ChannelLevelOutcome, ChannelPolarityOutcome, DigitalOutputModeOutcome, FeedbackOutcome,
    Message, MonitorToggleOutcome, RoutingOutcome, SpeakerVolumeOutcome,
};
use super::state::{
    App, DeviceStatus, fresh_channels_for, fresh_meters_for, fresh_routes_for, selected_device,
};
use super::subscription::feedback_cadence;

/// Drops per-device state for a new or rescanned selection, then schedules
/// one hardware refresh. Shared by explicit picks and discovery completions
/// so both rebuild routes, channels, and meters for the same device.
pub(crate) fn adopt_selection(app: &mut App) -> Task<Message> {
    let selected = selected_device(app);
    app.speaker = VolumeControl::default();
    app.toggles = Default::default();
    app.routes = selected.as_ref().map(fresh_routes_for).unwrap_or_default();
    app.digital_output_mode = ToggleControl::default();
    app.channels = selected
        .as_ref()
        .map(fresh_channels_for)
        .unwrap_or_default();
    // A (re)connected device starts unknown: old local values are
    // never restored as confirmed, and the refresh below adopts
    // whatever the hardware actually reports.
    app.meters = selected.as_ref().map(fresh_meters_for).unwrap_or_default();
    app.feedback_notice = None;
    app.feedback_in_flight = false;
    app.feedback_tick = 0;
    let scan = finish_scan(app);
    Task::batch([scan, refresh_task(app, true)])
}

pub(crate) fn request_scan(app: &mut App) -> Task<Message> {
    if app.scan_in_flight {
        app.rescan_requested = true;
        return Task::none();
    }

    app.scan_in_flight = true;
    app.status = DeviceStatus::Scanning;
    discovery_task()
}

pub(crate) fn finish_scan(app: &mut App) -> Task<Message> {
    app.scan_in_flight = false;

    if std::mem::take(&mut app.rescan_requested) {
        request_scan(app)
    } else {
        Task::none()
    }
}

pub(crate) fn discovery_task() -> Task<Message> {
    Task::perform(discover(), Message::DiscoveryFinished)
}

/// One immediate hardware refresh on (re)connect, outside the tick cadence.
pub(crate) fn refresh_task(app: &mut App, read_monitor: bool) -> Task<Message> {
    let Some(device) = selected_device(app) else {
        return Task::none();
    };
    if feedback_cadence(app).is_none() {
        return Task::none();
    }
    app.feedback_in_flight = true;
    let location = device.location.clone();
    Task::perform(
        async move {
            FeedbackOutcome {
                location,
                read_monitor,
                result: read_feedback(device, read_monitor).await,
            }
        },
        Message::FeedbackFinished,
    )
}

pub(crate) fn volume_task(device: DetectedDevice, level: f32) -> Task<Message> {
    Task::perform(
        async move {
            SpeakerVolumeOutcome {
                level,
                result: send_speaker_level(device, level).await,
            }
        },
        Message::SpeakerVolumeFinished,
    )
}

pub(crate) fn routing_task(device: DetectedDevice, route: Route) -> Task<Message> {
    Task::perform(
        async move {
            RoutingOutcome {
                route,
                result: send_output_route(device, route).await,
            }
        },
        Message::RoutingFinished,
    )
}

pub(crate) fn digital_output_mode_task(
    device: DetectedDevice,
    mode: DigitalOutputMode,
) -> Task<Message> {
    Task::perform(
        async move {
            DigitalOutputModeOutcome {
                mode,
                result: send_digital_output_mode(device, mode).await,
            }
        },
        Message::DigitalOutputModeFinished,
    )
}

pub(crate) fn monitor_toggle_task(
    device: DetectedDevice,
    toggle: MonitorToggle,
    on: bool,
) -> Task<Message> {
    Task::perform(
        async move {
            MonitorToggleOutcome {
                toggle,
                on,
                result: send_monitor_toggle(device, toggle, on).await,
            }
        },
        Message::MonitorToggleFinished,
    )
}

pub(crate) fn channel_level_task(device: DetectedDevice, channel: u8, level: f32) -> Task<Message> {
    Task::perform(
        async move {
            ChannelLevelOutcome {
                channel,
                level,
                result: send_channel_level(device, channel, level).await,
            }
        },
        Message::ChannelLevelFinished,
    )
}

pub(crate) fn channel_polarity_task(
    device: DetectedDevice,
    channel: u8,
    flipped: bool,
) -> Task<Message> {
    Task::perform(
        async move {
            ChannelPolarityOutcome {
                channel,
                flipped,
                result: send_channel_polarity(device, channel, flipped).await,
            }
        },
        Message::ChannelPolarityFinished,
    )
}
