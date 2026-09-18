//! The kernel's `NVMe` class: the controllers of the drives the board carries,
//! and what each announces about itself.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{block, pci};

const CLASS: &str = "/sys/class/nvme";

/// What a controller announces, trimmed of the padding its fixed-width
/// fields carry.
pub(crate) struct Controller {
    /// The PCI ids of the function the controller sits on.
    pub(crate) vendor: u16,
    pub(crate) device: u16,
    pub(crate) address: String,
    pub(crate) model: String,
    pub(crate) serial: String,
    pub(crate) firmware: String,
    /// Bytes across every namespace the controller holds, and None where it
    /// holds none or sysfs would not list them.
    pub(crate) capacity: Option<u64>,
}

/// Every controller the board carries, in controller order. A drive behind
/// an external port is left out: the inventory is read once for the daemon's
/// run, and an enclosure comes and goes within it.
pub(crate) fn controllers() -> Vec<Controller> {
    let Ok(entries) = fs::read_dir(CLASS) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    paths.iter().filter_map(|path| read(path)).collect()
}

fn read(controller: &Path) -> Option<Controller> {
    // A fabrics controller is a drive on another machine.
    if attribute(controller, "transport").as_deref() != Some("pcie") {
        return None;
    }
    let function = controller.join("device");
    // The kernel marks a device below an external-facing port removable.
    if attribute(&function, "removable").as_deref() == Some("removable") {
        return None;
    }
    let pci = pci::function(controller)?;
    Some(Controller {
        vendor: pci.vendor,
        device: pci.device,
        address: pci.address,
        model: attribute(controller, "model").unwrap_or_default(),
        serial: attribute(controller, "serial").unwrap_or_default(),
        firmware: attribute(controller, "firmware_rev").unwrap_or_default(),
        capacity: capacity(controller),
    })
}

fn capacity(controller: &Path) -> Option<u64> {
    let bytes: u64 = fs::read_dir(controller)
        .ok()?
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("nvme"))
        })
        .filter_map(|entry| block::bytes(&entry.path()))
        .sum();
    (bytes != 0).then_some(bytes)
}

fn attribute(dir: &Path, name: &str) -> Option<String> {
    fs::read_to_string(dir.join(name))
        .ok()
        .map(|value| value.trim().to_owned())
}
