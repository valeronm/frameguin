//! The firmware's SMBIOS table, two ways in: the handful of fields the
//! kernel publishes as world-readable files under `/sys/class/dmi/id`, and
//! the raw structures under `/sys/firmware/dmi/entries`, which it keeps
//! root-only — so those are the daemon's to read, not the app's.

use std::path::PathBuf;

const ID: &str = "/sys/class/dmi/id";
const ENTRIES: &str = "/sys/firmware/dmi/entries";

/// One published field, trimmed of the newline sysfs ends every one of them
/// with.
fn field(name: &str) -> Option<String> {
    std::fs::read_to_string(format!("{ID}/{name}"))
        .ok()
        .map(|value| value.trim().to_owned())
}

/// Without `/dev/cros_ec`, `framework_lib` falls back to raw port I/O; on a
/// non-Framework EC every command spin-waits to a timeout, stalling the
/// first `GetCapabilities` for tens of seconds. Don't touch the EC unless the
/// firmware says this is the hardware it belongs to.
pub fn is_framework() -> bool {
    field("sys_vendor").as_deref() == Some(frameguin_wire::VENDOR)
}

/// The mainboard, under the name its firmware gives it — and the mainboard
/// only, which is what makes this the right question to ask about a
/// processor pad and the wrong one to ask about anything plugged into it.
///
/// Read here rather than taken from `framework_lib`, whose `get_platform`
/// answers with a type its crate keeps private and so unnameable from
/// outside. The string this matches is the one that library maps too.
pub fn product() -> Option<String> {
    field("product_name")
}

/// One structure: the formatted area the spec lays out by offset, and the
/// string table that follows it, which the formatted area refers into by
/// one-based index.
pub struct Structure {
    formatted: Vec<u8>,
    strings: Vec<String>,
}

impl Structure {
    /// None where the bytes are shorter than the header says the formatted
    /// area is.
    pub fn parse(raw: &[u8]) -> Option<Self> {
        let length = usize::from(*raw.get(1)?);
        let formatted = raw.get(..length)?.to_vec();
        let strings = raw[length..]
            .split(|&b| b == 0)
            .take_while(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).trim().to_owned())
            .collect();
        Some(Self { formatted, strings })
    }

    pub fn u16(&self, offset: usize) -> Option<u16> {
        let bytes = self.formatted.get(offset..offset + 2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub fn u32(&self, offset: usize) -> Option<u32> {
        let bytes = self.formatted.get(offset..offset + 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// The string the byte at `offset` indexes, and None both for an index
    /// of zero — the spec's "no string" — and for one past the table.
    pub fn string(&self, offset: usize) -> Option<&str> {
        let index = usize::from(*self.formatted.get(offset)?).checked_sub(1)?;
        self.strings
            .get(index)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }
}

/// Every structure of one type, in the order the table lists them. Empty
/// where the entries cannot be read, which is what an unprivileged process
/// sees.
pub fn entries(kind: u8) -> Vec<Structure> {
    (0..)
        .map(|instance| PathBuf::from(ENTRIES).join(format!("{kind}-{instance}/raw")))
        .map_while(|path| std::fs::read(path).ok())
        .filter_map(|raw| Structure::parse(&raw))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::Structure;

    #[test]
    fn strings_are_indexed_from_one_and_zero_is_none() {
        let raw = [
            0x11, 0x06, 0x00, 0x00, 0x01, 0x00, b'a', b'b', 0, b'c', 0, 0,
        ];
        let s = Structure::parse(&raw).unwrap();
        assert_eq!(s.string(4), Some("ab"));
        assert_eq!(s.string(5), None);
        assert_eq!(s.u16(2), Some(0));
    }

    #[test]
    fn a_header_longer_than_the_bytes_is_not_a_structure() {
        assert!(Structure::parse(&[0x11, 0x20, 0x00]).is_none());
    }
}
