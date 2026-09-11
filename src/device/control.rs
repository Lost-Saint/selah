//! Monitor volume sends over a short-lived safe device session.
//!
//! Each send opens a session, transfers one bounded volume request (two for
//! headphones, in order on the same session), and always closes the session.
//! These run inside Iced background tasks, away from the UI thread.
//! Validation happens before any USB I/O. Errors are formatted with a
//! recovery hint at this boundary; typed [`SessionError`] distinctions stay
//! inside the transport.

use super::{DetectedDevice, DeviceSession, NormalizedLevel, SessionError};

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

async fn open_validated_session(
    device: &DetectedDevice,
    level: f32,
) -> Result<(DeviceSession, NormalizedLevel), String> {
    let valid = NormalizedLevel::new(level).map_err(|error| error.to_string())?;
    let session = DeviceSession::open(device)
        .await
        .map_err(|error| format!("{error} — {}", error.recovery_hint()))?;
    Ok((session, valid))
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
