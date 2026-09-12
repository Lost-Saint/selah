//! One-way phones-to-Main-Mix routing state.
//!
//! Selah cannot read routing back from the hardware, so a [`RouteControl`]
//! tracks only what was sent, what is sending, what failed, or that nothing
//! is known yet. The first version is intentionally one-way with no restore.

use crate::device::DetectedDevice;

/// Send state for the one-way phones-to-Main-Mix action.
#[derive(Clone, Debug, Default)]
pub struct RouteControl {
    sending: bool,
    ever_sent: bool,
    last_error: Option<String>,
}

/// The honest, nameable state of the routing action.
#[derive(Clone, Debug, PartialEq)]
pub enum RouteStatus {
    /// Nothing has been sent yet; the device route is unknown.
    Unknown,
    /// A send is running.
    Sending,
    /// The last send was acknowledged; still not read back from the device.
    Sent,
    /// The last send failed; the error stays visible until the next request.
    Failed { error: String, ever_sent: bool },
}

impl RouteControl {
    /// Names the current honest state of this control.
    #[must_use]
    pub fn status(&self) -> RouteStatus {
        if self.sending {
            RouteStatus::Sending
        } else if let Some(error) = &self.last_error {
            RouteStatus::Failed {
                error: error.clone(),
                ever_sent: self.ever_sent,
            }
        } else if self.ever_sent {
            RouteStatus::Sent
        } else {
            RouteStatus::Unknown
        }
    }

    /// Describes the current state without implying confirmed device state.
    #[must_use]
    pub fn status_text(&self) -> String {
        match self.status() {
            RouteStatus::Sending => "Sending phones to Main Mix…".to_owned(),
            RouteStatus::Failed { error, .. } => {
                format!("Route send failed: {error} Try again.")
            }
            RouteStatus::Sent => "Last route sent — not read back from the device.".to_owned(),
            RouteStatus::Unknown => {
                "No route read from the device — this sends once and cannot be undone.".to_owned()
            }
        }
    }

    /// Records a user request to route phones to Main Mix.
    ///
    /// Returns `Some(())` when a send should start, or `None` when one is
    /// already in flight. The button is disabled while sending, so unlike
    /// volume sliders there is no queued level.
    #[must_use]
    pub fn request(&mut self) -> Option<()> {
        if self.sending {
            return None;
        }
        self.sending = true;
        self.last_error = None;
        Some(())
    }

    /// Whether a completion belongs to the current send.
    #[must_use]
    pub fn is_sending(&self) -> bool {
        self.sending
    }

    /// Records a background send result. Stale completions (not sending,
    /// e.g. after a rescan reset the control) leave state untouched.
    pub fn finish(&mut self, result: Result<(), String>) {
        if !self.sending {
            return;
        }
        self.sending = false;
        match result {
            Ok(()) => {
                self.ever_sent = true;
                self.last_error = None;
            }
            Err(error) => {
                self.last_error = Some(error);
            }
        }
    }
}

/// Whether the phones-to-Main-Mix action can be offered for this attachment.
///
/// Gated to the iD14 MKII (`0x0008`): `MixiD`'s six-channel route table matches
/// that layout, and larger ADAT models likely differ. Do not widen without
/// per-model hardware evidence.
#[must_use]
pub fn phones_main_mix_available(device: &DetectedDevice) -> bool {
    device.control_interface.is_some() && device.model.product_id == 0x0008
}

#[cfg(test)]
mod tests {
    use super::{RouteControl, RouteStatus, phones_main_mix_available};
    use crate::device::{ControlInterface, ControlInterfaceKind, DetectedDevice, DeviceLocation};

    #[test]
    fn starts_unknown() {
        let control = RouteControl::default();
        assert_eq!(control.status(), RouteStatus::Unknown);
        assert!(!control.is_sending());
    }

    #[test]
    fn moves_from_unknown_through_sending_to_sent() {
        let mut control = RouteControl::default();
        assert_eq!(control.request(), Some(()));
        assert_eq!(control.status(), RouteStatus::Sending);
        assert!(control.is_sending());

        control.finish(Ok(()));
        assert_eq!(control.status(), RouteStatus::Sent);
        assert!(
            control
                .status_text()
                .contains("not read back from the device")
        );
    }

    #[test]
    fn second_request_while_sending_is_ignored() {
        let mut control = RouteControl::default();
        assert_eq!(control.request(), Some(()));
        assert_eq!(control.request(), None);
        assert_eq!(control.status(), RouteStatus::Sending);

        control.finish(Ok(()));
        assert_eq!(control.status(), RouteStatus::Sent);
    }

    #[test]
    fn failure_reports_the_error_and_preserves_ever_sent() {
        let mut control = RouteControl::default();
        assert_eq!(control.request(), Some(()));
        control.finish(Ok(()));
        assert_eq!(control.status(), RouteStatus::Sent);

        assert_eq!(control.request(), Some(()));
        control.finish(Err("no device".to_owned()));
        assert_eq!(
            control.status(),
            RouteStatus::Failed {
                error: "no device".to_owned(),
                ever_sent: true,
            }
        );
        assert!(control.status_text().contains("no device"));
    }

    #[test]
    fn a_new_request_clears_the_visible_error() {
        let mut control = RouteControl::default();
        assert_eq!(control.request(), Some(()));
        control.finish(Err("busy".to_owned()));
        assert!(matches!(control.status(), RouteStatus::Failed { .. }));

        assert_eq!(control.request(), Some(()));
        assert_eq!(control.status(), RouteStatus::Sending);
    }

    #[test]
    fn stale_completions_leave_state_untouched() {
        let mut control = RouteControl::default();
        control.finish(Ok(()));
        assert_eq!(control.status(), RouteStatus::Unknown);

        assert_eq!(control.request(), Some(()));
        control.finish(Ok(()));
        // Not sending anymore; a late duplicate must not change Sent.
        control.finish(Ok(()));
        assert_eq!(control.status(), RouteStatus::Sent);
    }

    #[test]
    fn availability_requires_id14_mkii_with_a_control_interface() {
        assert!(phones_main_mix_available(&device_with(0x0008, true)));
        assert!(!phones_main_mix_available(&device_with(0x0008, false)));
        assert!(!phones_main_mix_available(&device_with(0x0002, true)));
        assert!(!phones_main_mix_available(&device_with(0x000d, true)));
    }

    fn device_with(product_id: u16, has_control: bool) -> DetectedDevice {
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
}
