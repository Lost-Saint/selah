//! Monitor volume state for one output.
//!
//! A [`VolumeControl`] tracks what the user last requested and what the
//! device last acknowledged. Selah cannot read levels back from the
//! hardware, so the slider position is never presented as confirmed device
//! state; [`VolumeStatus`] names the honest states instead.

use crate::device::DetectedDevice;

/// Slider position and send state for one output volume.
///
/// `position` is only what the user last requested. Selah has no state
/// readback, so it is never presented as the device's confirmed level.
/// `last_sent` records the most recent level the device acknowledged.
#[derive(Clone, Debug, Default)]
pub struct VolumeControl {
    position: f32,
    in_flight: Option<f32>,
    queued: Option<f32>,
    last_sent: Option<f32>,
    last_error: Option<String>,
}

/// The honest, nameable state of one volume control.
///
/// There is no confirmed-device state: without readback, the best Selah can
/// say is what was sent, what is sending, what failed, or that nothing is
/// known yet.
#[derive(Clone, Debug, PartialEq)]
pub enum VolumeStatus {
    /// Nothing has been sent yet; the device level is unknown.
    Unknown,
    /// A send is running, with the newest waiting level when one arrived
    /// while sending.
    Sending { sending: f32, queued: Option<f32> },
    /// The last send failed; the error stays visible until the next request.
    /// `last_sent` is the most recent acknowledged level, preserved so a
    /// failure is never presented as the device's level.
    Failed {
        error: String,
        last_sent: Option<f32>,
    },
    /// The last send was acknowledged; still not read back from the device.
    Sent { level: f32 },
}

impl VolumeControl {
    /// Returns the slider position: the last requested level, not device state.
    #[must_use]
    pub fn position(&self) -> f32 {
        self.position
    }

    /// Names the current honest state of this control.
    #[must_use]
    pub fn status(&self) -> VolumeStatus {
        match (self.in_flight, &self.last_error, self.last_sent) {
            (Some(sending), _, _) => VolumeStatus::Sending {
                sending,
                queued: self.queued,
            },
            (None, Some(error), _) => VolumeStatus::Failed {
                error: error.clone(),
                last_sent: self.last_sent,
            },
            (None, None, Some(level)) => VolumeStatus::Sent { level },
            (None, None, None) => VolumeStatus::Unknown,
        }
    }

    /// Describes the current state without implying confirmed device state.
    ///
    /// Levels here are compared exactly because they are copied slider values
    /// used as request identities, not computed arithmetic.
    #[allow(clippy::float_cmp, reason = "copied slider levels identify requests")]
    #[must_use]
    pub fn status_text(&self) -> String {
        match self.status() {
            VolumeStatus::Sending {
                sending,
                queued: Some(requested),
            } if requested != sending => format!(
                "Sending {:.0}%… (latest request {:.0}%)",
                sending * 100.0,
                requested * 100.0
            ),
            VolumeStatus::Sending { sending, .. } => {
                format!("Sending {:.0}%…", sending * 100.0)
            }
            VolumeStatus::Failed { error, .. } => {
                format!("Send failed: {error} Move the slider to retry.")
            }
            VolumeStatus::Sent { level } => {
                format!(
                    "Last sent {:.0}% — not read back from the device.",
                    level * 100.0
                )
            }
            VolumeStatus::Unknown => {
                "No value read from the device — moving the slider sends a new level.".to_owned()
            }
        }
    }

    /// Records a user-requested level.
    ///
    /// Returns the level to send immediately, or `None` when the request was
    /// invalid (non-finite) or was queued behind a send already in flight.
    /// Only one send runs at a time: a change made while sending is
    /// remembered as the queued level and started when the in-flight send
    /// completes.
    #[must_use]
    pub fn request(&mut self, level: f32) -> Option<f32> {
        if !level.is_finite() {
            return None;
        }
        let level = level.clamp(0.0, 1.0);
        self.position = level;
        self.last_error = None;

        if self.in_flight.is_some() {
            self.queued = Some(level);
            return None;
        }

        self.in_flight = Some(level);
        self.queued = None;
        Some(level)
    }

    /// Whether this completion belongs to the current send.
    ///
    /// Completions from before a rescan are stale: a rescan resets the
    /// control, so a completion whose level is not in flight belongs to a
    /// previous device set and must be ignored without logging.
    ///
    /// Levels here are compared exactly because they are copied slider values
    /// used as request identities, not computed arithmetic.
    #[allow(clippy::float_cmp, reason = "copied slider levels identify requests")]
    #[must_use]
    pub fn is_in_flight(&self, level: f32) -> bool {
        self.in_flight == Some(level)
    }

    /// Records a background send result.
    ///
    /// Returns the queued level to send next, if one arrived while sending
    /// and differs from the completed level. Stale completions leave the
    /// state untouched and return `None`.
    ///
    /// Levels here are compared exactly because they are copied slider values
    /// used as request identities, not computed arithmetic.
    #[allow(clippy::float_cmp, reason = "copied slider levels identify requests")]
    #[must_use]
    pub fn finish(&mut self, level: f32, result: Result<(), String>) -> Option<f32> {
        if self.in_flight != Some(level) {
            return None;
        }

        match result {
            Ok(()) => {
                self.last_sent = Some(level);
                self.last_error = None;
            }
            Err(error) => {
                self.last_error = Some(error);
            }
        }

        let next = self.queued.take().filter(|next| *next != level);
        self.in_flight = next;
        next
    }

    /// Drops a pending send without touching the last acknowledged state.
    ///
    /// Used only when the device set vanished between a completion and its
    /// queued follow-up, so the UI cannot display "Sending" forever with no
    /// task running behind it.
    pub fn drop_pending(&mut self) {
        self.in_flight = None;
        self.queued = None;
    }
}

/// Whether monitor volume controls can be offered for this attachment.
///
/// Every cataloged model exposes speaker and headphone volume through the
/// same safe control interface, so availability is purely "a session can be
/// opened". Per-model control differences are unverified; do not add
/// per-model booleans without hardware evidence.
#[must_use]
pub fn monitor_controls_available(device: &DetectedDevice) -> bool {
    device.control_interface.is_some()
}

/// Send state for one monitor toggle (dim, mute, mono, alt, talkback).
///
/// Like volumes and routing, Selah cannot read toggles back: `position` is
/// only the last requested on/off value, never confirmed device state.
#[derive(Clone, Debug, Default)]
pub struct ToggleControl {
    position: bool,
    in_flight: Option<bool>,
    last_sent: Option<bool>,
    last_error: Option<String>,
}

/// The honest, nameable state of one monitor toggle.
#[derive(Clone, Debug, PartialEq)]
pub enum ToggleStatus {
    /// Nothing has been sent yet; the device state is unknown.
    Unknown,
    /// A send is running.
    Sending { sending: bool },
    /// The last send failed; the error stays visible until the next request.
    /// `last_sent` is the most recent acknowledged value, preserved so a
    /// failure is never presented as the device's state.
    Failed {
        error: String,
        last_sent: Option<bool>,
    },
    /// The last send was acknowledged; still not read back from the device.
    Sent { on: bool },
}

impl ToggleControl {
    /// Returns the last requested value, not device state.
    #[must_use]
    pub fn position(&self) -> bool {
        self.position
    }

    /// Names the current honest state of this control.
    #[must_use]
    pub fn status(&self) -> ToggleStatus {
        match (self.in_flight, &self.last_error, self.last_sent) {
            (Some(sending), _, _) => ToggleStatus::Sending { sending },
            (None, Some(error), _) => ToggleStatus::Failed {
                error: error.clone(),
                last_sent: self.last_sent,
            },
            (None, None, Some(on)) => ToggleStatus::Sent { on },
            (None, None, None) => ToggleStatus::Unknown,
        }
    }

    /// Describes the current state without implying confirmed device state.
    #[must_use]
    pub fn status_text(&self) -> String {
        match self.status() {
            ToggleStatus::Sending { sending } => {
                format!("Sending {}…", if sending { "on" } else { "off" })
            }
            ToggleStatus::Failed { error, .. } => {
                format!("Send failed: {error} Press again to retry.")
            }
            ToggleStatus::Sent { on } => {
                format!(
                    "Last sent {} — not read back from the device.",
                    if on { "on" } else { "off" }
                )
            }
            ToggleStatus::Unknown => {
                "No value read from the device — pressing sends a new value.".to_owned()
            }
        }
    }

    /// Records a user-requested value.
    ///
    /// Returns the value to send immediately, or `None` when a send is
    /// already in flight (the button is disabled while sending).
    #[must_use]
    pub fn request(&mut self, on: bool) -> Option<bool> {
        self.position = on;
        self.last_error = None;

        if self.in_flight.is_some() {
            return None;
        }

        self.in_flight = Some(on);
        Some(on)
    }

    /// Whether this completion belongs to the current send.
    #[must_use]
    pub fn is_in_flight(&self, on: bool) -> bool {
        self.in_flight == Some(on)
    }

    /// Records a background send result. Stale completions leave the state
    /// untouched.
    #[must_use]
    pub fn finish(&mut self, on: bool, result: Result<(), String>) -> bool {
        if self.in_flight != Some(on) {
            return false;
        }

        match result {
            Ok(()) => {
                self.last_sent = Some(on);
                self.last_error = None;
            }
            Err(error) => {
                self.last_error = Some(error);
            }
        }

        self.in_flight = None;
        true
    }
}

/// Whether the monitor toggles can be offered for this attachment.
///
/// Gated to the iD14 MKII (`0x0008`): `MixiD`'s toggle table is verified
/// against that layout in Selah so far, and per-model differences are
/// unverified. Do not widen without hardware evidence.
#[must_use]
pub fn monitor_toggles_available(device: &DetectedDevice) -> bool {
    device.control_interface.is_some() && device.model.product_id == 0x0008
}

#[cfg(test)]
mod tests {
    use super::{
        ToggleControl, ToggleStatus, VolumeControl, VolumeStatus, monitor_controls_available,
        monitor_toggles_available,
    };
    use crate::device::{ControlInterface, ControlInterfaceKind, DetectedDevice, DeviceLocation};

    #[test]
    fn non_finite_requests_are_ignored_without_touching_state() {
        let mut control = VolumeControl::default();
        let _send = control.request(0.4);
        let _done = control.finish(0.4, Ok(()));

        for level in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert_eq!(control.request(level), None);
        }

        assert_level_eq(control.position(), 0.4);
        assert_eq!(control.status(), VolumeStatus::Sent { level: 0.4 });
    }

    #[test]
    fn requests_clamp_to_the_device_range() {
        let mut control = VolumeControl::default();

        assert_eq!(control.request(1.5), Some(1.0));
        assert_level_eq(control.position(), 1.0);
        assert_eq!(control.finish(1.0, Ok(())), None);

        assert_eq!(control.request(-0.5), Some(0.0));
        assert_level_eq(control.position(), 0.0);
    }

    #[test]
    fn moves_from_unknown_through_sending_to_sent() {
        let mut control = VolumeControl::default();
        assert_eq!(control.status(), VolumeStatus::Unknown);

        assert_eq!(control.request(0.2), Some(0.2));
        assert_eq!(
            control.status(),
            VolumeStatus::Sending {
                sending: 0.2,
                queued: None,
            }
        );

        assert_eq!(control.finish(0.2, Ok(())), None);
        assert_eq!(control.status(), VolumeStatus::Sent { level: 0.2 });
        assert!(
            control
                .status_text()
                .contains("not read back from the device")
        );
    }

    #[test]
    fn failure_keeps_last_sent_and_reports_the_error() {
        let mut control = VolumeControl::default();
        let _send = control.request(0.2);
        let _done = control.finish(0.2, Ok(()));

        assert_eq!(control.request(0.3), Some(0.3));
        assert_eq!(control.finish(0.3, Err("no device".to_owned())), None);

        // The failed level is not presented as confirmed: the previous
        // acknowledged send is preserved and the error stays visible.
        assert_level_eq(control.position(), 0.3);
        assert_eq!(
            control.status(),
            VolumeStatus::Failed {
                error: "no device".to_owned(),
                last_sent: Some(0.2),
            }
        );
        assert!(control.status_text().contains("no device"));
    }

    #[test]
    fn changes_while_sending_queue_behind_one_send() {
        let mut control = VolumeControl::default();

        assert_eq!(control.request(0.2), Some(0.2));
        assert_eq!(control.request(0.3), None);
        assert_eq!(control.request(0.4), None);

        // Still a single send in flight; only the latest request waits.
        assert_eq!(
            control.status(),
            VolumeStatus::Sending {
                sending: 0.2,
                queued: Some(0.4),
            }
        );
        assert_level_eq(control.position(), 0.4);

        assert_eq!(control.finish(0.2, Ok(())), Some(0.4));
        assert_eq!(
            control.status(),
            VolumeStatus::Sending {
                sending: 0.4,
                queued: None,
            }
        );
        let _done = control.finish(0.4, Ok(()));
        assert_eq!(control.status(), VolumeStatus::Sent { level: 0.4 });
    }

    #[test]
    fn stale_completions_leave_state_untouched() {
        let mut control = VolumeControl::default();
        let _send = control.request(0.2);

        // A completion for a level that is not in flight belongs to a
        // previous device set (e.g. before a rescan reset the control).
        assert_eq!(control.finish(0.9, Ok(())), None);
        assert!(!control.is_in_flight(0.9));
        assert!(control.is_in_flight(0.2));
        assert_level_eq(control.position(), 0.2);
        assert_eq!(
            control.status(),
            VolumeStatus::Sending {
                sending: 0.2,
                queued: None,
            }
        );
    }

    #[test]
    fn drop_pending_returns_to_the_last_acknowledged_state() {
        let mut control = VolumeControl::default();
        let _first = control.request(0.2);
        let _done = control.finish(0.2, Ok(()));
        let _second = control.request(0.3);
        let _queued = control.request(0.4);

        control.drop_pending();

        assert_eq!(control.status(), VolumeStatus::Sent { level: 0.2 });
    }

    #[test]
    fn availability_follows_the_safe_control_interface() {
        assert!(monitor_controls_available(&device_with_control(true)));
        assert!(!monitor_controls_available(&device_with_control(false)));
    }

    #[test]
    fn toggle_moves_from_unknown_through_sending_to_sent() {
        let mut control = ToggleControl::default();
        assert_eq!(control.status(), ToggleStatus::Unknown);

        assert_eq!(control.request(true), Some(true));
        assert!(control.position());
        assert_eq!(control.status(), ToggleStatus::Sending { sending: true });

        assert!(control.finish(true, Ok(())));
        assert_eq!(control.status(), ToggleStatus::Sent { on: true });
        assert!(
            control
                .status_text()
                .contains("not read back from the device")
        );
    }

    #[test]
    fn toggle_failure_keeps_last_sent_and_reports_the_error() {
        let mut control = ToggleControl::default();
        let _send = control.request(true);
        assert!(control.finish(true, Ok(())));

        assert_eq!(control.request(false), Some(false));
        assert!(control.finish(false, Err("no device".to_owned())));
        assert!(!control.position());
        assert_eq!(
            control.status(),
            ToggleStatus::Failed {
                error: "no device".to_owned(),
                last_sent: Some(true),
            }
        );
        assert!(control.status_text().contains("no device"));
    }

    #[test]
    fn toggle_request_while_sending_is_ignored() {
        let mut control = ToggleControl::default();
        assert_eq!(control.request(true), Some(true));
        assert_eq!(control.request(false), None);
        assert_eq!(control.status(), ToggleStatus::Sending { sending: true });

        assert!(control.finish(true, Ok(())));
        assert_eq!(control.status(), ToggleStatus::Sent { on: true });
    }

    #[test]
    fn toggle_availability_requires_id14_mkii_with_a_control_interface() {
        assert!(monitor_toggles_available(&device_with_product(
            0x0008, true
        )));
        assert!(!monitor_toggles_available(&device_with_product(
            0x0008, false
        )));
        assert!(!monitor_toggles_available(&device_with_product(
            0x0002, true
        )));
        assert!(!monitor_toggles_available(&device_with_product(
            0x000d, true
        )));
    }

    fn device_with_product(product_id: u16, has_control: bool) -> DetectedDevice {
        DetectedDevice {
            location: DeviceLocation {
                bus: "1".to_owned(),
                address: 2,
            },
            model: crate::device::supported_device(product_id).unwrap(),
            reported_name: None,
            control_interface: has_control.then_some(ControlInterface {
                number: 4,
                kind: ControlInterfaceKind::ApplicationSpecific,
            }),
        }
    }

    fn assert_level_eq(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < f32::EPSILON,
            "expected slider level {expected}, got {actual}"
        );
    }

    fn device_with_control(has_control: bool) -> DetectedDevice {
        DetectedDevice {
            location: DeviceLocation {
                bus: "1".to_owned(),
                address: 2,
            },
            model: crate::device::supported_device(0x0008).unwrap(),
            reported_name: None,
            control_interface: has_control.then_some(ControlInterface {
                number: 4,
                kind: ControlInterfaceKind::ApplicationSpecific,
            }),
        }
    }
}
