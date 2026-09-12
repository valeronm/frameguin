//! The memory modules, as the firmware's SMBIOS table describes them: a
//! device that is a part and no control, read for the bill of materials
//! alone.

use crate::dmi::{self, Structure};
use crate::part::{Identity, Part, PartKind};

/// SMBIOS type 17, "Memory Device".
const MEMORY_DEVICE: u8 = 17;

/// Offsets into the formatted area, per the SMBIOS 3 specification.
const SIZE: usize = 0x0c;
const LOCATOR: usize = 0x10;
const MANUFACTURER: usize = 0x17;
const SERIAL: usize = 0x18;
const PART_NUMBER: usize = 0x1a;
const EXTENDED_SIZE: usize = 0x1c;

const SIZE_EXTENDED: u16 = 0x7fff;
const SIZE_UNKNOWN: u16 = 0xffff;
/// Set where the 16-bit size counts kilobytes rather than megabytes.
const SIZE_IN_KILOBYTES: u16 = 0x8000;
/// The extension's size field, its top bit being reserved.
const EXTENDED_SIZE_MASK: u32 = 0x7fff_ffff;

const KILOBYTE: u64 = 1 << 10;
const MEGABYTE: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Module {
    identity: Identity,
}

impl Part for Module {
    fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl Module {
    /// Every fitted module, in slot order. Empty slots are listed by the
    /// table too, with no size, and are left out here: a slot is a fact
    /// about the board, not a part.
    pub(crate) fn detect() -> Vec<Self> {
        dmi::entries(MEMORY_DEVICE)
            .iter()
            .filter_map(Self::parse)
            .collect()
    }

    /// None for a structure that names no fitted module.
    fn parse(entry: &Structure) -> Option<Self> {
        let size_bytes = size(entry)?;
        Some(Self {
            identity: Identity {
                kind: PartKind::Memory,
                vendor: entry.string(MANUFACTURER).unwrap_or_default().to_owned(),
                vendor_name: String::new(),
                model: entry.string(PART_NUMBER).unwrap_or_default().to_owned(),
                part_number: String::new(),
                serial: entry.string(SERIAL).unwrap_or_default().to_owned(),
                size_bytes,
                id: format!("dmi-slot:{}", entry.string(LOCATOR).unwrap_or_default()),
                firmware: Vec::new(),
            },
        })
    }
}

/// An empty slot says so as none, as unknown, or — for a module the 16-bit
/// field cannot hold — as a zero in the 32-bit extension that carries its
/// megabytes.
fn size(entry: &Structure) -> Option<u64> {
    match entry.u16(SIZE)? {
        0 | SIZE_UNKNOWN => None,
        SIZE_EXTENDED => match entry.u32(EXTENDED_SIZE)? & EXTENDED_SIZE_MASK {
            0 => None,
            megabytes => Some(u64::from(megabytes) * MEGABYTE),
        },
        size if size & SIZE_IN_KILOBYTES != 0 => {
            Some(u64::from(size & !SIZE_IN_KILOBYTES) * KILOBYTE)
        }
        size => Some(u64::from(size) * MEGABYTE),
    }
}

#[cfg(test)]
mod tests {
    use super::Module;
    use crate::dmi::Structure;
    use crate::part::{Part, PartKind};

    const FORMATTED_LENGTH: u8 = 0x64;

    /// A type-17 structure with only the fields the parser reads filled in,
    /// the rest zero, and the string table after it.
    fn entry(size: u16, extended_size: u32, strings: &[&str]) -> Vec<u8> {
        let mut raw = vec![0; usize::from(FORMATTED_LENGTH)];
        raw[0] = 17;
        raw[1] = FORMATTED_LENGTH;
        raw[0x0c..0x0e].copy_from_slice(&size.to_le_bytes());
        raw[0x10] = 1;
        raw[0x17] = 2;
        raw[0x18] = 3;
        raw[0x1a] = 4;
        raw[0x1c..0x20].copy_from_slice(&extended_size.to_le_bytes());
        for s in strings {
            raw.extend_from_slice(s.as_bytes());
            raw.push(0);
        }
        raw.push(0);
        raw
    }

    /// A real module's strings, with the padding firmware puts after them;
    /// only the serial is invented, the part number being the catalogue's.
    const STRINGS: [&str; 4] = [
        "LPCAMM2_0",
        "Micron Technology",
        "01234567",
        "MTD16C20325N4FN023F1 YF       ",
    ];

    #[test]
    fn a_fitted_module_is_read_off_the_table() {
        let entry = Structure::parse(&entry(0x7fff, 32 * 1024, &STRINGS)).unwrap();
        let identity = Module::parse(&entry).unwrap().identity().clone();
        assert_eq!(identity.kind, PartKind::Memory);
        assert_eq!(identity.vendor, "Micron Technology");
        assert_eq!(identity.model, "MTD16C20325N4FN023F1 YF");
        assert_eq!(identity.serial, "01234567");
        assert_eq!(identity.size_bytes, 32 << 30);
        assert_eq!(identity.id, "dmi-slot:LPCAMM2_0");
    }

    #[test]
    fn a_module_the_short_field_can_size_is_counted_in_megabytes() {
        let entry = Structure::parse(&entry(0x2000, 0, &STRINGS)).unwrap();
        assert_eq!(
            Module::parse(&entry).unwrap().identity().size_bytes,
            8 << 30
        );
    }

    #[test]
    fn a_short_field_with_its_top_bit_set_counts_kilobytes() {
        let entry = Structure::parse(&entry(0x8000 | 512, 0, &STRINGS)).unwrap();
        assert_eq!(
            Module::parse(&entry).unwrap().identity().size_bytes,
            512 << 10
        );
    }

    #[test]
    fn an_empty_slot_is_not_a_module() {
        for (size, extended) in [(0, 0), (0xffff, 0), (0x7fff, 0)] {
            let entry = Structure::parse(&entry(size, extended, &STRINGS)).unwrap();
            assert!(Module::parse(&entry).is_none());
        }
    }
}
