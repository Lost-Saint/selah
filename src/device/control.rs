//! Monitor volume and one-way routing sends over short-lived safe sessions.
//!
//! Each send opens a session, transfers bounded requests (two for headphones
//! and for phones-to-Main-Mix, in order on the same session), and always
//! closes the session. These run inside Iced background tasks, away from the
//! UI thread. Validation happens before any USB I/O. Errors are formatted
//! with a recovery hint at this boundary; typed [`SessionError`] distinctions
//! stay inside the transport.

use super::{DetectedDevice, DeviceSession, MonitorToggle, NormalizedLevel, SessionError};

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
/// # Errors
///
/// Returns a message with a recovery hint when validation, opening, sending,
/// or closing fails.
pub async fn send_headphone_level(device: DetectedDevice, level: f32) -> Result<(), String> {
    let (mut session, valid) = open_validated_session(&device, level).await?;
    let send = session.set_headphone_level(valid).await;
    close_after_send(session, send).await
}

/// Routes the headphone pair to Main Mix and always closes the session.
///
/// This is one-way with no readback and no restore; callers must present the
/// result as sent, never confirmed.
///
/// # Errors
///
/// Returns a message with a recovery hint when opening, sending, or closing
/// fails.
pub async fn send_phones_to_main_mix(device: DetectedDevice) -> Result<(), String> {
    let mut session = open_session(&device).await?;
    let send = session.set_phones_to_main_mix().await;
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
