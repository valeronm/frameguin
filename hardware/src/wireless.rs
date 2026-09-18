//! The kernel's `ieee80211` class: the PCI function each radio is driven
//! through, and the address it answers to.

use std::fs;
use std::path::{Path, PathBuf};

use crate::pci::{self, Function};

const CLASS: &str = "/sys/class/ieee80211";

pub(crate) struct Radio {
    pub(crate) function: Function,
    pub(crate) mac: Option<String>,
}

/// Every radio the board carries, in name order. One reached over anything
/// but PCI is an adapter plugged into the machine rather than a part of it.
pub(crate) fn radios() -> Vec<Radio> {
    let Ok(entries) = fs::read_dir(CLASS) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort();
    paths.iter().filter_map(|path| read(path)).collect()
}

fn read(phy: &Path) -> Option<Radio> {
    Some(Radio {
        function: pci::function(phy)?,
        mac: fs::read_to_string(phy.join("macaddress"))
            .ok()
            .map(|text| text.trim().to_owned()),
    })
}
