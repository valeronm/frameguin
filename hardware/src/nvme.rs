//! The kernel's `NVMe` class: the controllers of the drives the board carries,
//! and what each announces about itself.

use std::fs;
use std::path::{Path, PathBuf};

const CLASS: &str = "/sys/class/nvme";

/// The unit the kernel counts a block device's `size` in, whatever the
/// drive's own block size.
const SECTOR: u64 = 512;

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
    /// Bytes across every namespace the controller holds.
    pub(crate) capacity: u64,
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
    let pci = controller.join("device");
    // The kernel marks a device below an external-facing port removable.
    if attribute(&pci, "removable").as_deref() == Some("removable") {
        return None;
    }
    Some(Controller {
        vendor: pci_id(&pci, "vendor")?,
        device: pci_id(&pci, "device")?,
        address: attribute(controller, "address").unwrap_or_default(),
        model: attribute(controller, "model").unwrap_or_default(),
        serial: attribute(controller, "serial").unwrap_or_default(),
        firmware: attribute(controller, "firmware_rev").unwrap_or_default(),
        capacity: capacity(controller),
    })
}

fn capacity(controller: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(controller) else {
        return 0;
    };
    let sectors: u64 = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("nvme"))
        })
        .filter_map(|entry| attribute(&entry.path(), "size")?.parse::<u64>().ok())
        .sum();
    sectors * SECTOR
}

fn attribute(dir: &Path, name: &str) -> Option<String> {
    fs::read_to_string(dir.join(name))
        .ok()
        .map(|value| value.trim().to_owned())
}

fn pci_id(dir: &Path, name: &str) -> Option<u16> {
    let value = attribute(dir, name)?;
    u16::from_str_radix(value.strip_prefix("0x")?, 16).ok()
}
