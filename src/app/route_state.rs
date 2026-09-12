//! Construction of routing state from the selected model's capabilities.

use super::{DeviceStatus, OutputRouteControl};
use crate::routing::route_controls;

pub(super) fn fresh_routes(status: &DeviceStatus) -> Vec<OutputRouteControl> {
    match status {
        DeviceStatus::Ready(report) => route_controls(report.supported[0].model),
        _ => Vec::new(),
    }
}
