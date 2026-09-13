//! Construction of routing state from the selected model's capabilities.

use crate::device::DetectedDevice;
use crate::routing::{OutputRouteControl, route_controls};

pub(super) fn fresh_routes_for(device: &DetectedDevice) -> Vec<OutputRouteControl> {
    route_controls(device.model)
}
