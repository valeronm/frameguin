//! The kernel's block layer: how much a disk holds.

use std::fs;
use std::path::Path;

/// The unit the kernel counts a block device's `size` in, whatever the
/// drive's own block size.
const SECTOR: u64 = 512;

pub(crate) fn bytes(disk: &Path) -> Option<u64> {
    let sectors: u64 = fs::read_to_string(disk.join("size"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(sectors * SECTOR)
}
