//! The kernel's PCI bus: the function a class device is registered under.

use std::fs;
use std::path::Path;

/// What the bus answers for a function, which is ids and never words.
pub(crate) struct Function {
    pub(crate) vendor: u16,
    pub(crate) device: u16,
    /// The domain, bus, device and function, which is what udev keys its
    /// record on.
    pub(crate) address: String,
}

/// The function the class device at `dir` is registered under, and None
/// where it sits on some other bus.
pub(crate) fn function(dir: &Path) -> Option<Function> {
    let link = dir.join("device");
    Some(Function {
        vendor: id(&link, "vendor")?,
        device: id(&link, "device")?,
        address: fs::read_link(&link).ok()?.file_name()?.to_str()?.to_owned(),
    })
}

fn id(dir: &Path, name: &str) -> Option<u16> {
    let value = fs::read_to_string(dir.join(name)).ok()?;
    u16::from_str_radix(value.trim().strip_prefix("0x")?, 16).ok()
}
