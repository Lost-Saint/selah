use std::time::Duration;

use iced::Subscription;

use crate::device::{DetectedDevice, watch_events};
use crate::mixer::meter_feedback_available;
use crate::monitor::{speaker_feedback_available, toggle_feedback_available};
use crate::routing::digital_output_mode_available;

use super::message::Message;
use super::state::{App, selected_device};

/// Meter polls run at 10 Hz: the slowest rate that still reads as motion.
/// Monitor state rides the same timer at ~1 Hz (see [`monitor_due`]).
pub(crate) const METER_CADENCE_MS: u64 = 100;
pub(crate) const MONITOR_CADENCE_MS: u64 = 1_000;

/// How often the device may be polled, or `None` when no feedback is
/// supported. Meters need ~10 Hz to read as motion; monitor state follows
/// at ~1 Hz. Anything without a readable control or a meter source gets no
/// timer at all, so Selah stays quiet instead of polling blindly.
pub(crate) fn feedback_cadence(app: &App) -> Option<Duration> {
    feedback_cadence_ms(app).map(Duration::from_millis)
}

/// What one attachment reports back. Single decision point shared by the
/// poll cadence and the header honesty line, so a new readable control
/// cannot update one without the other.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FeedbackKind {
    MetersAndMonitor,
    MonitorOnly,
    WriteOnly,
}

pub(crate) fn feedback_kind(device: &DetectedDevice) -> FeedbackKind {
    if device.control_interface.is_none() {
        return FeedbackKind::WriteOnly;
    }
    if meter_feedback_available(device) {
        FeedbackKind::MetersAndMonitor
    } else if speaker_feedback_available(device)
        || toggle_feedback_available(device)
        || digital_output_mode_available(device)
    {
        FeedbackKind::MonitorOnly
    } else {
        FeedbackKind::WriteOnly
    }
}

pub(crate) fn feedback_cadence_ms(app: &App) -> Option<u64> {
    let device = selected_device(app)?;
    match feedback_kind(&device) {
        FeedbackKind::MetersAndMonitor => Some(METER_CADENCE_MS),
        FeedbackKind::MonitorOnly => Some(MONITOR_CADENCE_MS),
        FeedbackKind::WriteOnly => None,
    }
}

/// Whether this tick also refreshes monitor state. The monitor snapshot
/// rides every ~1 s on top of the meter cadence, on the same session.
pub(crate) fn monitor_due(tick: u64, cadence_ms: u64) -> bool {
    tick.is_multiple_of((MONITOR_CADENCE_MS / cadence_ms.max(1)).max(1))
}

pub(crate) fn subscription(app: &App) -> Subscription<Message> {
    let watch = Subscription::run(watch_events).map(Message::DeviceWatch);
    match feedback_cadence(app) {
        // The timer exists only while a supported device is present, so
        // metering suspends on disconnect instead of polling blindly.
        Some(cadence) => Subscription::batch([
            watch,
            iced::time::every(cadence).map(|_| Message::FeedbackTick),
        ]),
        None => watch,
    }
}
