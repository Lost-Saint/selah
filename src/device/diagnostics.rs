//! Copy-paste diagnostics without serials, captures, or paths.

use std::fmt::Write as _;

use super::{DiscoveryReport, support_label, support_level};

/// One paste-safe block describing what Selah sees.
///
/// Includes only: model name, `vid:pid`, bus/address, control-interface
/// number + kind, and the honesty label. `reported_name` strings are
/// deliberately omitted: they can carry machine-specific text.
#[must_use]
pub fn diagnostics_text(report: &DiscoveryReport) -> String {
    let mut out = String::from("Selah diagnostics (copy-safe)\n");
    for device in &report.supported {
        let control = device.control_interface.map_or_else(
            || "no safe control interface".to_owned(),
            |control| format!("control {} ({})", control.number, control.kind),
        );
        let _ = writeln!(
            out,
            "- {} 2708:{:04x} @ {}:{} · {control} · {}",
            device.model.name,
            device.model.product_id,
            device.location.bus,
            device.location.address,
            support_label(support_level(device.model)),
        );
    }
    for unknown in &report.unsupported {
        let _ = writeln!(
            out,
            "- unknown Audient 2708:{:04x} · unsupported",
            unknown.product_id
        );
    }
    if report.supported.is_empty() && report.unsupported.is_empty() {
        out.push_str("- no Audient interfaces detected\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{
        ControlInterface, ControlInterfaceKind, DetectedDevice, DeviceLocation,
        UnknownAudientDevice,
    };
    use super::DiscoveryReport;

    #[test]
    fn diagnostics_omits_names_and_paths() {
        let report = DiscoveryReport {
            supported: vec![DetectedDevice {
                location: DeviceLocation {
                    bus: "3".to_owned(),
                    address: 16,
                },
                model: crate::device::supported_device(0x0008).unwrap(),
                reported_name: Some("DO NOT LEAK".to_owned()),
                control_interface: Some(ControlInterface {
                    number: 4,
                    kind: ControlInterfaceKind::ApplicationSpecific,
                }),
            }],
            unsupported: vec![UnknownAudientDevice {
                product_id: 0xbeef,
                reported_name: Some("DO NOT LEAK".to_owned()),
            }],
        };
        let text = super::diagnostics_text(&report);
        assert!(text.contains("iD14 MKII"));
        assert!(text.contains("2708:0008"));
        assert!(!text.contains("DO NOT LEAK"));
        assert!(!text.contains("serial"));
    }
}
