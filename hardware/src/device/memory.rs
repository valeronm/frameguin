//! The memory modules, as the firmware's SMBIOS table describes them: a
//! device that is a part and no control, read for the bill of materials
//! alone.

use crate::dmi::{self, Structure};
use crate::part::{Detail, Identity, Part, PartKind};

/// SMBIOS type 17, "Memory Device".
const MEMORY_DEVICE: u8 = 17;

/// Offsets into the formatted area, per the SMBIOS 3 specification.
const SIZE: usize = 0x0c;
const FORM_FACTOR: usize = 0x0e;
const LOCATOR: usize = 0x10;
const MEMORY_TYPE: usize = 0x12;
const SPEED: usize = 0x15;
const MANUFACTURER: usize = 0x17;
const SERIAL: usize = 0x18;
const PART_NUMBER: usize = 0x1a;
const EXTENDED_SIZE: usize = 0x1c;
const CONFIGURED_SPEED: usize = 0x20;
const EXTENDED_SPEED: usize = 0x54;
const EXTENDED_CONFIGURED_SPEED: usize = 0x58;

/// The value a 16-bit speed field carries where the module runs faster than
/// it can hold, the rate itself being in the 32-bit extension.
const SPEED_EXTENDED: u16 = 0xffff;

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

    /// None for a structure with no fitted module in it.
    fn parse(entry: &Structure) -> Option<Self> {
        let capacity = size(entry)?;
        Some(Self {
            identity: Identity {
                kind: PartKind::Memory,
                vendor: entry.string(MANUFACTURER).unwrap_or_default().to_owned(),
                vendor_name: String::new(),
                model: entry.string(PART_NUMBER).unwrap_or_default().to_owned(),
                part_number: String::new(),
                serial: entry.string(SERIAL).unwrap_or_default().to_owned(),
                id: format!("dmi-slot:{}", entry.string(LOCATOR).unwrap_or_default()),
                firmware: Vec::new(),
                details: details(entry, capacity),
            },
        })
    }
}

/// Two rows of the same number are no more use than one.
fn details(entry: &Structure, capacity: u64) -> Vec<Detail> {
    let rated = speed(entry, SPEED, EXTENDED_SPEED);
    let configured = speed(entry, CONFIGURED_SPEED, EXTENDED_CONFIGURED_SPEED);
    [
        Some(Detail::MemoryCapacity(capacity)),
        entry
            .byte(MEMORY_TYPE)
            .map(|value| Detail::MemoryType(memory_type(value))),
        entry
            .byte(FORM_FACTOR)
            .map(|value| Detail::FormFactor(form_factor(value))),
        rated.map(Detail::Speed),
        configured
            .filter(|rate| Some(*rate) != rated)
            .map(Detail::ConfiguredSpeed),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// None where the rate is unavailable, and the extension's rate where the
/// 16-bit field defers to it.
fn speed(entry: &Structure, short: usize, extended: usize) -> Option<u32> {
    match entry.u16(short)? {
        0 => None,
        SPEED_EXTENDED => entry.u32(extended).filter(|rate| *rate != 0),
        rate => Some(u32::from(rate)),
    }
}

/// Firmware outruns the table it is written against, and a value missing from
/// it is more use as itself than as `Unknown`.
fn spelled(value: u8, names: &[(u8, &'static str)]) -> String {
    names
        .iter()
        .find(|(known, _)| *known == value)
        .map_or_else(|| format!("{value:#04x}"), |(_, name)| (*name).to_owned())
}

/// Table 77 of the SMBIOS specification, less the values it reserves.
fn memory_type(value: u8) -> String {
    spelled(
        value,
        &[
            (0x01, "Other"),
            (0x02, "Unknown"),
            (0x03, "DRAM"),
            (0x04, "EDRAM"),
            (0x05, "VRAM"),
            (0x06, "SRAM"),
            (0x07, "RAM"),
            (0x08, "ROM"),
            (0x09, "FLASH"),
            (0x0a, "EEPROM"),
            (0x0b, "FEPROM"),
            (0x0c, "EPROM"),
            (0x0d, "CDRAM"),
            (0x0e, "3DRAM"),
            (0x0f, "SDRAM"),
            (0x10, "SGRAM"),
            (0x11, "RDRAM"),
            (0x12, "DDR"),
            (0x13, "DDR2"),
            (0x14, "DDR2 FB-DIMM"),
            (0x18, "DDR3"),
            (0x19, "FBD2"),
            (0x1a, "DDR4"),
            (0x1b, "LPDDR"),
            (0x1c, "LPDDR2"),
            (0x1d, "LPDDR3"),
            (0x1e, "LPDDR4"),
            (0x1f, "Logical non-volatile device"),
            (0x20, "HBM"),
            (0x21, "HBM2"),
            (0x22, "DDR5"),
            (0x23, "LPDDR5"),
            (0x24, "HBM3"),
            (0x25, "MRDIMM"),
            (0x26, "LPDDR6"),
        ],
    )
}

/// Table 76 of the SMBIOS specification.
fn form_factor(value: u8) -> String {
    spelled(
        value,
        &[
            (0x01, "Other"),
            (0x02, "Unknown"),
            (0x03, "SIMM"),
            (0x04, "SIP"),
            (0x05, "Chip"),
            (0x06, "DIP"),
            (0x07, "ZIP"),
            (0x08, "Proprietary Card"),
            (0x09, "DIMM"),
            (0x0a, "TSOP"),
            (0x0b, "Row of chips"),
            (0x0c, "RIMM"),
            (0x0d, "SODIMM"),
            (0x0e, "SRIMM"),
            (0x0f, "FB-DIMM"),
            (0x10, "Die"),
            (0x11, "CAMM"),
            (0x12, "CUDIMM"),
            (0x13, "CSODIMM"),
        ],
    )
}

/// An empty slot reads as none, as unknown, or — for a module the 16-bit
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
    use crate::part::{Detail, Part, PartKind};

    const FORMATTED_LENGTH: u8 = 0x64;

    /// A type-17 structure with only the fields the parser reads filled in,
    /// the rest zero, and the string table after it.
    fn entry(size: u16, extended_size: u32, strings: &[&str]) -> Vec<u8> {
        let mut raw = vec![0; usize::from(FORMATTED_LENGTH)];
        raw[0] = 17;
        raw[1] = FORMATTED_LENGTH;
        raw[0x0c..0x0e].copy_from_slice(&size.to_le_bytes());
        raw[0x0e] = 0x11;
        raw[0x10] = 1;
        raw[0x12] = 0x23;
        raw[0x15..0x17].copy_from_slice(&RATED.to_le_bytes());
        raw[0x17] = 2;
        raw[0x18] = 3;
        raw[0x1a] = 4;
        raw[0x1c..0x20].copy_from_slice(&extended_size.to_le_bytes());
        raw[0x20..0x22].copy_from_slice(&CONFIGURED.to_le_bytes());
        for s in strings {
            raw.extend_from_slice(s.as_bytes());
            raw.push(0);
        }
        raw.push(0);
        raw
    }

    const RATED: u16 = 8533;
    const CONFIGURED: u16 = 7467;

    /// A real module's strings, with the padding firmware puts after them;
    /// only the serial is invented.
    const STRINGS: [&str; 4] = [
        "LPCAMM2_0",
        "Micron Technology",
        "01234567",
        "MTD16C20325N4FN023F1 YF       ",
    ];

    fn fitted() -> Vec<u8> {
        entry(0x7fff, 32 * 1024, &STRINGS)
    }

    fn details(raw: &[u8]) -> Vec<Detail> {
        let entry = Structure::parse(raw).unwrap();
        Module::parse(&entry).unwrap().identity().details.clone()
    }

    #[test]
    fn a_fitted_module_is_read_off_the_table() {
        let entry = Structure::parse(&fitted()).unwrap();
        let identity = Module::parse(&entry).unwrap().identity().clone();
        assert_eq!(identity.kind, PartKind::Memory);
        assert_eq!(identity.vendor, "Micron Technology");
        assert_eq!(identity.model, "MTD16C20325N4FN023F1 YF");
        assert_eq!(identity.serial, "01234567");
        assert_eq!(identity.id, "dmi-slot:LPCAMM2_0");
    }

    #[test]
    fn a_module_the_short_field_can_size_is_counted_in_megabytes() {
        assert_eq!(
            details(&entry(0x2000, 0, &STRINGS))[0],
            Detail::MemoryCapacity(8 << 30)
        );
    }

    #[test]
    fn a_short_field_with_its_top_bit_set_counts_kilobytes() {
        assert_eq!(
            details(&entry(0x8000 | 512, 0, &STRINGS))[0],
            Detail::MemoryCapacity(512 << 10)
        );
    }

    #[test]
    fn a_module_carries_its_capacity_type_form_factor_and_both_speeds() {
        assert_eq!(
            details(&fitted()),
            [
                Detail::MemoryCapacity(32 << 30),
                Detail::MemoryType("LPDDR5".to_owned()),
                Detail::FormFactor("CAMM".to_owned()),
                Detail::Speed(u32::from(RATED)),
                Detail::ConfiguredSpeed(u32::from(CONFIGURED)),
            ]
        );
    }

    #[test]
    fn a_speed_matching_the_one_it_was_rated_at_is_not_repeated() {
        let mut raw = fitted();
        raw[0x20..0x22].copy_from_slice(&RATED.to_le_bytes());
        assert!(
            !details(&raw)
                .iter()
                .any(|detail| matches!(detail, Detail::ConfiguredSpeed(_)))
        );
    }

    #[test]
    fn a_speed_the_short_field_cannot_hold_comes_from_the_extension() {
        let mut raw = fitted();
        raw[0x15..0x17].copy_from_slice(&0xffffu16.to_le_bytes());
        raw[0x54..0x58].copy_from_slice(&70_000u32.to_le_bytes());
        assert!(details(&raw).contains(&Detail::Speed(70_000)));
    }

    #[test]
    fn a_value_the_specification_does_not_name_is_shown_as_itself() {
        let mut raw = fitted();
        raw[0x0e] = 0xfe;
        raw[0x12] = 0xfe;
        let details = details(&raw);
        assert!(details.contains(&Detail::FormFactor("0xfe".to_owned())));
        assert!(details.contains(&Detail::MemoryType("0xfe".to_owned())));
    }

    #[test]
    fn an_empty_slot_is_not_a_module() {
        for (size, extended) in [(0, 0), (0xffff, 0), (0x7fff, 0)] {
            let entry = Structure::parse(&entry(size, extended, &STRINGS)).unwrap();
            assert!(Module::parse(&entry).is_none());
        }
    }
}
