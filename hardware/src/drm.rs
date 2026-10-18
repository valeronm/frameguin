//! The kernel's DRM class: the connectors a panel is wired to, the EDID
//! block each answers with, and which connectors the compositor has lit.

use std::fs;
use std::path::{Path, PathBuf};

const CLASS: &str = "/sys/class/drm";

/// What a connector's name carries where the panel is wired to the board
/// rather than plugged into a port.
const INTERNAL: &str = "-eDP-";

/// The EDID of every panel wired to the board, in connector order. What is
/// plugged into a port is left out: the inventory is read once for the
/// daemon's run, and a monitor comes and goes within it.
pub(crate) fn panels() -> Vec<Vec<u8>> {
    let mut connectors: Vec<PathBuf> = connectors()
        .unwrap_or_default()
        .into_iter()
        .filter(|path| internal(path))
        .collect();
    connectors.sort();
    connectors.iter().filter_map(|path| read(path)).collect()
}

/// Every connector's directory, and None where the class did not read.
fn connectors() -> Option<Vec<PathBuf>> {
    Some(
        fs::read_dir(CLASS)
            .ok()?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| connector(path))
            .collect(),
    )
}

fn internal(path: &Path) -> bool {
    name(path).is_some_and(|name| name.contains(INTERNAL))
}

fn name(path: &Path) -> Option<&str> {
    path.file_name()?.to_str()
}

/// None for a connector with no panel on it, which `status` is the kernel's
/// own answer to — the `edid` attribute reads empty both there and where a
/// panel is present but was never read from.
fn read(connector: &Path) -> Option<Vec<u8>> {
    let status = fs::read_to_string(connector.join("status")).ok()?;
    (status.trim() == "connected").then_some(())?;
    fs::read(connector.join("edid")).ok()
}

/// The machine's display outputs the compositor has lit.
#[derive(Clone, Copy, Default)]
pub struct Displays {
    /// Whether a panel wired to the board is lit.
    pub internal: bool,
    /// How many other outputs are lit, a monitor on a port's alternate mode,
    /// a dock's and a socket's alike.
    pub external: u8,
}

/// What the ports' device needs of the DRM class.
pub trait Outputs: Send + Sync {
    /// None where the class or a connector's `enabled` did not read.
    fn displays(&self) -> Option<Displays>;
}

pub(crate) struct Sysfs;

impl Outputs for Sysfs {
    /// A connector's `enabled` is whether the compositor drives it, where
    /// `status` is only whether something is attached.
    fn displays(&self) -> Option<Displays> {
        let mut lit = Displays::default();
        for path in connectors()? {
            let enabled = fs::read_to_string(path.join("enabled")).ok()?;
            if enabled.trim() != "enabled" {
                continue;
            }
            if internal(&path) {
                lit.internal = true;
            } else {
                lit.external = lit.external.saturating_add(1);
            }
        }
        Some(lit)
    }
}

/// A connector's directory is named for its card, `card0-DP-1`, beside the
/// card's own and the render nodes.
fn connector(path: &Path) -> bool {
    name(path)
        .and_then(|name| name.strip_prefix("card"))
        .is_some_and(|rest| rest.contains('-'))
}
