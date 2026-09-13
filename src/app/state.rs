use crate::device::{DetectedDevice, DeviceLocation, DiscoveryReport, MonitorToggle};
use crate::mixer::{ChannelStrip, mixer_channel_count};
use crate::monitor::{ToggleControl, VolumeControl};
use crate::routing::{OutputRouteControl, route_controls};

pub(crate) struct App {
    pub(crate) status: DeviceStatus,
    pub(crate) selected: Option<DeviceLocation>,
    pub(crate) scan_in_flight: bool,
    pub(crate) rescan_requested: bool,
    pub(crate) watch_status: WatchStatus,
    pub(crate) speaker: VolumeControl,
    pub(crate) routes: Vec<OutputRouteControl>,
    pub(crate) digital_output_mode: ToggleControl,
    pub(crate) toggles: [ToggleControl; 5],
    pub(crate) channels: Vec<ChannelStrip>,
    /// Which input groups the mixer bank shows. `MIC` covers microphone
    /// inputs, `OPT` covers digital/optical inputs. `DAW` returns have no
    /// mapped strips, so the DAW segment stays an honest disabled indicator.
    /// View-only: it filters strips, never sends USB.
    pub(crate) input_filter: InputFilter,
    /// One level byte per mixer input while meters are supported, `None`
    /// per input while the level is unknown. Unknown is never shown as zero.
    pub(crate) meters: Vec<Option<u8>>,
    /// Whether a background feedback poll currently owns a device session.
    /// Ticks arriving while this is set are skipped, so polls can never
    /// overlap or queue behind each other.
    pub(crate) feedback_in_flight: bool,
    /// Counts feedback ticks to fold the ~1 Hz monitor refresh into the
    /// faster meter cadence on one session per tick.
    pub(crate) feedback_tick: u64,
    /// The last feedback failure, shown once until a poll succeeds or the
    /// device set changes. Last confirmed values stay visible underneath.
    pub(crate) feedback_notice: Option<String>,
}

pub(crate) enum DeviceStatus {
    Scanning,
    Ready(DiscoveryReport),
    Unsupported(DiscoveryReport),
    Empty,
    Failed(crate::device::DiscoveryError),
}

pub(crate) enum WatchStatus {
    Starting,
    Active,
    Failed,
}

impl App {
    pub(crate) fn new() -> (Self, iced::Task<super::message::Message>) {
        (
            Self {
                status: DeviceStatus::Scanning,
                selected: None,
                scan_in_flight: false,
                rescan_requested: false,
                watch_status: WatchStatus::Starting,
                speaker: VolumeControl::default(),
                routes: Vec::new(),
                digital_output_mode: ToggleControl::default(),
                toggles: Default::default(),
                input_filter: InputFilter::default(),
                channels: Vec::new(),
                meters: Vec::new(),
                feedback_in_flight: false,
                feedback_tick: 0,
                feedback_notice: None,
            },
            iced::Task::none(),
        )
    }
}

/// Which input group a mixer strip belongs to, matching the Audient
/// application's `MIC / OPT / DAW` selector grouping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InputGroup {
    Mic,
    Optical,
}

/// View-only filter for the mixer bank. Both groups default to visible;
/// `DAW` has no mapped strips so it is not a toggleable group.
#[derive(Clone, Copy, Debug)]
pub(crate) struct InputFilter {
    pub(crate) mic: bool,
    pub(crate) optical: bool,
}

impl Default for InputFilter {
    fn default() -> Self {
        Self {
            mic: true,
            optical: true,
        }
    }
}

impl InputFilter {
    /// Whether the strip at a running channel index stays visible.
    /// `mic_inputs` is the count of microphone inputs on the model; every
    /// higher index is a digital/optical input.
    #[must_use]
    pub(crate) fn shows(self, channel: u8, mic_inputs: u8) -> bool {
        if channel < mic_inputs {
            self.mic
        } else {
            self.optical
        }
    }

    /// Flips one group's visibility. The last visible group stays on so the
    /// mixer cannot be filtered into a mode that hides every strip.
    pub(crate) fn toggle(&mut self, group: InputGroup) {
        match group {
            InputGroup::Mic => {
                if self.mic && !self.optical {
                    return;
                }
                self.mic = !self.mic;
            }
            InputGroup::Optical => {
                if self.optical && !self.mic {
                    return;
                }
                self.optical = !self.optical;
            }
        }
    }
}

/// Index of a toggle in `App::toggles`, matching `MonitorToggle::ALL` order.
pub(crate) fn toggle_index(toggle: MonitorToggle) -> usize {
    MonitorToggle::ALL
        .iter()
        .position(|candidate| *candidate == toggle)
        .expect("every monitor toggle is listed in ALL")
}

/// The explicitly picked device, else the first supported one.
pub(crate) fn selected_device(app: &App) -> Option<DetectedDevice> {
    match &app.status {
        DeviceStatus::Ready(report) => {
            if let Some(wanted) = app.selected.as_ref()
                && let Some(hit) = report.supported.iter().find(|d| &d.location == wanted)
            {
                return Some(hit.clone());
            }
            report.supported.first().cloned()
        }
        _ => None,
    }
}

/// Builds a fresh strip per input of one explicitly selected device.
pub(crate) fn fresh_channels_for(device: &DetectedDevice) -> Vec<ChannelStrip> {
    let count = mixer_channel_count(device.model) as usize;
    std::iter::repeat_with(ChannelStrip::default)
        .take(count)
        .collect()
}

/// Builds one unknown meter slot per mixer input of one explicitly
/// selected device.
pub(crate) fn fresh_meters_for(device: &DetectedDevice) -> Vec<Option<u8>> {
    let count = mixer_channel_count(device.model) as usize;
    std::iter::repeat_with(|| None).take(count).collect()
}

/// Construction of routing state from the selected model's capabilities.
pub(crate) fn fresh_routes_for(device: &DetectedDevice) -> Vec<OutputRouteControl> {
    route_controls(device.model)
}

#[cfg(test)]
mod tests {
    use super::{InputFilter, InputGroup};

    #[test]
    fn both_groups_are_visible_by_default() {
        let filter = InputFilter::default();
        // iD14 MKII shape: 2 microphones, rest digital.
        assert!(filter.shows(0, 2));
        assert!(filter.shows(1, 2));
        assert!(filter.shows(2, 2));
        assert!(filter.shows(9, 2));
    }

    #[test]
    fn toggling_hides_one_group_at_a_time() {
        let mut filter = InputFilter::default();
        filter.toggle(InputGroup::Mic);
        assert!(!filter.shows(0, 2));
        assert!(!filter.shows(1, 2));
        assert!(filter.shows(2, 2));

        filter.toggle(InputGroup::Optical);
        // Optical was on while mic was off: turning it off would hide every
        // strip, so the toggle refuses and the bank never goes fully blank
        // from the filter alone.
        assert!(filter.shows(2, 2));

        filter.toggle(InputGroup::Mic);
        filter.toggle(InputGroup::Optical);
        assert!(filter.shows(0, 2));
        assert!(!filter.shows(2, 2));
    }
}
