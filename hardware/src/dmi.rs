//! The firmware's SMBIOS table, two ways in: the handful of fields the
//! kernel publishes as world-readable files under `/sys/class/dmi/id`, and
//! the raw structures under `/sys/firmware/dmi/entries`, which it keeps
//! root-only — so those are the daemon's to read, not the app's.

use std::path::PathBuf;

const ID: &str = "/sys/class/dmi/id";
const ENTRIES: &str = "/sys/firmware/dmi/entries";

/// Trimmed of the newline sysfs ends every one of these files with. None
/// where the kernel keeps the field from this process — the serials are
/// root-only — as well as where the firmware left it out.
pub(crate) fn field(name: &str) -> Option<String> {
    std::fs::read_to_string(format!("{ID}/{name}"))
        .ok()
        .map(|value| value.trim().to_owned())
}

/// When the BIOS was built, as an ISO date where the firmware stamped the
/// `mm/dd/yyyy` the specification asks of it and as it stands where it
/// stamped something else.
///
/// The American ordering is the specification's, and so this table's to
/// know about.
pub(crate) fn bios_date() -> Option<String> {
    let stamped = field("bios_date")?;
    Some(iso(&stamped).unwrap_or(stamped))
}

/// None for a two-digit year as much as for a stamp of another shape: which
/// century it meant is a guess, and the stamp as written says more than a
/// wrong date.
fn iso(stamped: &str) -> Option<String> {
    let (month, rest) = stamped.split_once('/')?;
    let (day, year) = rest.split_once('/')?;
    let month: u8 = month
        .parse()
        .ok()
        .filter(|month| (1..=12).contains(month))?;
    let day: u8 = day.parse().ok().filter(|day| (1..=31).contains(day))?;
    (year.len() == 4 && year.parse::<u16>().is_ok()).then_some(())?;
    Some(format!("{year}-{month:02}-{day:02}"))
}

/// Without `/dev/cros_ec`, `framework_lib` falls back to raw port I/O; on a
/// non-Framework EC every command spin-waits to a timeout, stalling the
/// daemon's start for tens of seconds. Don't touch the EC unless the
/// firmware says this is the hardware it belongs to.
pub(crate) fn is_framework() -> bool {
    field("sys_vendor").as_deref() == Some(frameguin_wire::VENDOR)
}

/// The mainboard as its firmware names it, and never anything plugged into
/// it.
///
/// Read here rather than taken from `framework_lib`, whose `get_platform`
/// answers with a type its crate keeps private and so unnameable from
/// outside. The string this matches is the one that library maps too.
pub(crate) fn product() -> Option<String> {
    field("product_name")
}

/// The formatted area the spec lays out by offset, and the string table
/// that follows it, which the formatted area refers into by one-based
/// index.
pub(crate) struct Structure {
    formatted: Vec<u8>,
    strings: Vec<String>,
}

impl Structure {
    /// None where the bytes are shorter than the header says the formatted
    /// area is.
    pub(crate) fn parse(raw: &[u8]) -> Option<Self> {
        let length = usize::from(*raw.get(1)?);
        let formatted = raw.get(..length)?.to_vec();
        let strings = raw[length..]
            .split(|&b| b == 0)
            .take_while(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).trim().to_owned())
            .collect();
        Some(Self { formatted, strings })
    }

    pub(crate) fn byte(&self, offset: usize) -> Option<u8> {
        self.formatted.get(offset).copied()
    }

    pub(crate) fn u16(&self, offset: usize) -> Option<u16> {
        let bytes = self.formatted.get(offset..offset + 2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub(crate) fn u32(&self, offset: usize) -> Option<u32> {
        let bytes = self.formatted.get(offset..offset + 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// The string the byte at `offset` indexes, and None both for an index
    /// of zero — the spec's "no string" — and for one past the table.
    pub(crate) fn string(&self, offset: usize) -> Option<&str> {
        let index = usize::from(self.byte(offset)?).checked_sub(1)?;
        self.strings
            .get(index)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }
}

/// Every structure of one type, in the order the table lists them. Empty
/// where the entries cannot be read, which is what an unprivileged process
/// sees.
pub(crate) fn entries(kind: u8) -> Vec<Structure> {
    (0..)
        .map(|instance| PathBuf::from(ENTRIES).join(format!("{kind}-{instance}/raw")))
        .map_while(|path| std::fs::read(path).ok())
        .filter_map(|raw| Structure::parse(&raw))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Structure, iso};

    #[test]
    fn a_stamp_of_the_shape_the_specification_asks_for_reads_as_an_iso_date() {
        assert_eq!(iso("05/26/2026").as_deref(), Some("2026-05-26"));
        assert_eq!(iso("12/01/1999").as_deref(), Some("1999-12-01"));
    }

    #[test]
    fn a_stamp_of_another_shape_is_left_to_be_shown_as_it_stands() {
        for stamped in ["05/26/26", "2026-05-26", "26 May 2026", "05/26", ""] {
            assert!(iso(stamped).is_none());
        }
    }

    #[test]
    fn a_stamp_naming_no_month_or_day_of_the_year_is_not_a_date() {
        assert!(iso("13/01/2026").is_none());
        assert!(iso("05/32/2026").is_none());
        assert!(iso("00/26/2026").is_none());
    }

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
