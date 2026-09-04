//! The kernel's DRM class: the connectors a panel is wired to, and the EDID
//! block each answers with.

use std::fs;
use std::path::{Path, PathBuf};

const CLASS: &str = "/sys/class/drm";

/// What a connector's name carries where the panel is wired to the board
/// rather than plugged into a port.
const INTERNAL: &str = "-eDP-";

/// The EDID of every panel wired to the board, in connector order. What is
/// plugged into a port is left out: the inventory is read once for the
/// daemon's run, and a monitor comes and goes within it.
pub fn panels() -> Vec<Vec<u8>> {
    let Ok(entries) = fs::read_dir(CLASS) else {
        return Vec::new();
    };
    let mut connectors: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| internal(path))
        .collect();
    connectors.sort();
    connectors.iter().filter_map(|path| read(path)).collect()
}

fn internal(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.contains(INTERNAL))
}

/// None for a connector with no panel on it, which `status` is the kernel's
/// own answer to — the `edid` attribute reads empty both there and where a
/// panel is present but was never read from.
fn read(connector: &Path) -> Option<Vec<u8>> {
    let status = fs::read_to_string(connector.join("status")).ok()?;
    (status.trim() == "connected").then_some(())?;
    fs::read(connector.join("edid")).ok()
}
