//! Control sends over short-lived safe sessions.
//!
//! Each send opens a session, transfers its bounded request sequence in order,
//! and always closes the session. These run inside Iced background tasks, away
//! from the UI thread. Validation happens before any USB I/O. Errors are
//! formatted with a recovery hint at this boundary; typed [`SessionError`]
//! distinctions stay inside the transport.

use super::{DetectedDevice, DeviceSession, MonitorToggle, NormalizedLevel, SessionError};
use crate::mixer::{meter_channel_count, meter_feedback_available};
use crate::monitor::{speaker_feedback_available, toggle_feedback_available};
use crate::routing::{DigitalOutputMode, Route, digital_output_mode_available, validate_route};

/// Sends one bounded speaker volume request and always closes the session.
///
/// # Errors
///
/// Returns a message with a recovery hint when validation, opening, sending,
/// or closing fails.
pub async fn send_speaker_level(device: DetectedDevice, level: f32) -> Result<(), String> {
    let (mut session, valid) = open_validated_session(&device, level).await?;
    let send = session.set_speaker_level(valid).await;
    close_after_send(session, send).await
}

/// Sends the bounded headphone volume requests and always closes the session.
///
/// Retained for protocol evidence and opt-in hardware checks; the UI
/// intentionally exposes no headphone slider (the official application has
/// none, and the mapping is write-only and inaudible on cue-fed outputs).
///
/// # Errors
///
/// Returns a message with a recovery hint when validation, opening, sending,
/// or closing fails.
pub async fn send_headphone_level(device: DetectedDevice, level: f32) -> Result<(), String> {
    let (mut session, valid) = open_validated_session(&device, level).await?;
    let send = session.set_headphone_level(valid).await;
    close_after_send(session, send).await
}

/// Sends one validated output route and always closes the session.
///
/// # Errors
///
/// Returns a message with a recovery hint when opening, sending, or closing
/// fails.
pub async fn send_output_route(device: DetectedDevice, route: Route) -> Result<(), String> {
    validate_route(device.model, route).map_err(|error| error.to_string())?;
    let mut session = open_session(&device).await?;
    let send = session.set_output_route(route).await;
    close_after_send(session, send).await
}

/// Sends one evidenced optical-output mode request and always closes the session.
///
/// # Errors
///
/// Returns an error before USB I/O when the model does not expose this
/// capability, or when opening, sending, or closing fails.
pub async fn send_digital_output_mode(
    device: DetectedDevice,
    mode: DigitalOutputMode,
) -> Result<(), String> {
    if !digital_output_mode_available(&device) {
        return Err("digital output mode is unavailable for this model".to_owned());
    }
    let mut session = open_session(&device).await?;
    let send = session.set_digital_output_mode(mode).await;
    close_after_send(session, send).await
}

/// Sends one monitor toggle request and always closes the session.
///
/// This is one-way with no readback; callers must present the result as
/// sent, never confirmed.
///
/// # Errors
///
/// Returns a message with a recovery hint when opening, sending, or closing
/// fails.
pub async fn send_monitor_toggle(
    device: DetectedDevice,
    toggle: MonitorToggle,
    on: bool,
) -> Result<(), String> {
    let mut session = open_session(&device).await?;
    let send = session.set_monitor_toggle(toggle, on).await;
    close_after_send(session, send).await
}

/// Sends one input channel's level and always closes the session.
///
/// `channel` is the running input index across microphone then digital
/// inputs. This is one-way with no readback; callers must present the
/// result as sent, never confirmed.
///
/// # Errors
///
/// Returns a message with a recovery hint when the channel is outside the
/// model's input count, or when validation, opening, sending, or closing
/// fails.
pub async fn send_channel_level(
    device: DetectedDevice,
    channel: u8,
    level: f32,
) -> Result<(), String> {
    let valid_channel = validated_channel(&device, channel)?;
    let (mut session, valid) = open_validated_session(&device, level).await?;
    let send = session.set_channel_level(valid, valid_channel).await;
    close_after_send(session, send).await
}

/// Sends one input channel's polarity and always closes the session.
///
/// This is one-way with no readback; callers must present the result as
/// sent, never confirmed.
///
/// # Errors
///
/// Returns a message with a recovery hint when the channel is outside the
/// model's input count, or when opening, sending, or closing fails.
pub async fn send_channel_polarity(
    device: DetectedDevice,
    channel: u8,
    flipped: bool,
) -> Result<(), String> {
    let valid_channel = validated_channel(&device, channel)?;
    let mut session = open_session(&device).await?;
    let send = session.set_channel_polarity(valid_channel, flipped).await;
    close_after_send(session, send).await
}

/// Monitor state read back from the device. Every field is optional: `None`
/// means the control is unsupported on this model or the device did not
/// answer, and callers must keep showing unknown — never zero, never the
/// last locally requested value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MonitorSnapshot {
    pub speaker_level: Option<f32>,
    pub toggles: Vec<(MonitorToggle, bool)>,
    pub digital_output_mode: Option<DigitalOutputMode>,
}

/// One bounded feedback poll: monitor state plus meter levels.
///
/// `meters` holds one level byte per mixer input, in running-input order,
/// or is `None` when meters are unsupported or the block did not arrive
/// whole. `monitor` is `None` when the caller did not ask for it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeedbackSnapshot {
    pub monitor: Option<MonitorSnapshot>,
    pub meters: Option<Vec<u8>>,
}

/// Reads supported device state and meter levels on one short-lived session.
///
/// Only controls with reference evidence for truthful reads are queried
/// (monitor volume, monitor toggles, optical-output mode, meter block).
/// Mixer levels, polarity, routing, and headphone level have no trusted
/// readback, so they are never read and stay unknown-or-sent in the UI.
/// Each field that fails to answer becomes `None` rather than failing the
/// whole snapshot; only opening or closing the session fails the poll.
///
/// # Errors
///
/// Returns a message with a recovery hint when opening or closing fails.
pub async fn read_feedback(
    device: DetectedDevice,
    read_monitor: bool,
) -> Result<FeedbackSnapshot, String> {
    let mut session = open_session(&device).await?;
    let snapshot = FeedbackSnapshot {
        monitor: if read_monitor {
            Some(read_monitor_snapshot(&mut session, &device).await)
        } else {
            None
        },
        meters: read_meter_levels(&mut session, &device).await,
    };
    close_after_read(session, snapshot).await
}

async fn read_monitor_snapshot(
    session: &mut DeviceSession,
    device: &DetectedDevice,
) -> MonitorSnapshot {
    let speaker_level = if speaker_feedback_available(device) {
        session.speaker_level().await.ok()
    } else {
        None
    };
    let mut toggles = Vec::new();
    if toggle_feedback_available(device) {
        for toggle in MonitorToggle::ALL {
            if let Ok(on) = session.monitor_toggle_state(toggle).await {
                toggles.push((toggle, on));
            }
        }
    }
    let digital_output_mode = if digital_output_mode_available(device) {
        session.digital_output_mode().await.ok()
    } else {
        None
    };
    MonitorSnapshot {
        speaker_level,
        toggles,
        digital_output_mode,
    }
}

async fn read_meter_levels(
    session: &mut DeviceSession,
    device: &DetectedDevice,
) -> Option<Vec<u8>> {
    if !meter_feedback_available(device) {
        return None;
    }
    session
        .meter_levels(meter_channel_count(device.model))
        .await
        .ok()
}

/// Rejects a channel index outside the model's microphone plus digital
/// inputs before any USB I/O happens.
fn validated_channel(device: &DetectedDevice, channel: u8) -> Result<u8, String> {
    let inputs = device
        .model
        .mic_inputs
        .saturating_add(device.model.digital_inputs);
    if channel < inputs {
        Ok(channel)
    } else {
        Err(format!(
            "input channel {channel} is outside this model's {inputs} inputs"
        ))
    }
}

async fn open_session(device: &DetectedDevice) -> Result<DeviceSession, String> {
    DeviceSession::open(device)
        .await
        .map_err(|error| format!("{error} — {}", error.recovery_hint()))
}

async fn open_validated_session(
    device: &DetectedDevice,
    level: f32,
) -> Result<(DeviceSession, NormalizedLevel), String> {
    let valid = NormalizedLevel::new(level).map_err(|error| error.to_string())?;
    Ok((open_session(device).await?, valid))
}

async fn close_after_send(
    session: DeviceSession,
    send: Result<(), SessionError>,
) -> Result<(), String> {
    let close = session.close().await;
    match send {
        Ok(()) => close.map_err(|error| format!("{error} — {}", error.recovery_hint())),
        Err(error) => {
            if let Err(close_error) = close {
                tracing::warn!(%close_error, "Control interface release failed after send error");
            }
            Err(format!("{error} — {}", error.recovery_hint()))
        }
    }
}

async fn close_after_read(
    session: DeviceSession,
    snapshot: FeedbackSnapshot,
) -> Result<FeedbackSnapshot, String> {
    match session.close().await {
        Ok(()) => Ok(snapshot),
        Err(error) => Err(format!("{error} — {}", error.recovery_hint())),
    }
}

#[cfg(test)]
mod tests {
    use futures_lite::future::block_on;

    use super::send_output_route;
    use crate::device::{DetectedDevice, DeviceLocation};
    use crate::routing::{Route, RoutingDestination, RoutingSource};

    #[test]
    fn invalid_route_is_rejected_before_opening_usb() {
        let device = DetectedDevice {
            location: DeviceLocation {
                bus: "no-such-bus".to_owned(),
                address: u8::MAX,
            },
            model: crate::device::supported_device(0x0008).unwrap(),
            reported_name: None,
            control_interface: None,
        };
        let error = block_on(send_output_route(
            device,
            Route {
                destination: RoutingDestination::Headphones,
                source: RoutingSource::AltSpeaker,
            },
        ))
        .unwrap_err();
        assert_eq!(error, "Alt Speaker is unavailable for this model");
    }
}
