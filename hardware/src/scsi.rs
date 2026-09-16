//! The kernel's SCSI class: which devices are disks, and how much each holds.

use std::fs;
use std::path::Path;

use crate::block;

/// The SCSI peripheral type of a direct-access device.
const DISK: &str = "0";

/// In bytes; None for a device that is not a disk or holds nothing.
pub(crate) fn capacity(device: &Path) -> Option<u64> {
    let kind = fs::read_to_string(device.join("type")).ok()?;
    let disk = fs::read_dir(device.join("block")).ok()?.flatten().next()?;
    held(kind.trim(), block::bytes(&disk.path())?)
}

/// A bridge can present a CD emulation with a size of its own, and an image
/// slot as a disk holding nothing.
fn held(kind: &str, bytes: u64) -> Option<u64> {
    (kind == DISK && bytes != 0).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::{DISK, held};

    const CD_ROM: &str = "5";

    #[test]
    fn a_disk_holds_its_bytes() {
        assert_eq!(held(DISK, 2_000_398_934_016), Some(2_000_398_934_016));
    }

    #[test]
    fn a_cd_emulation_is_not_a_disk() {
        assert_eq!(held(CD_ROM, 1_073_741_312), None);
    }

    #[test]
    fn an_empty_image_slot_holds_nothing() {
        assert_eq!(held(DISK, 0), None);
    }
}
