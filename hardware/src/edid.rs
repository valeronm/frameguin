//! What a panel's EDID says it is, apart from the DRM class so the decoding
//! is testable without a connector.

const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];

const BLOCK: usize = 128;
const WEEK: usize = 0x10;
const YEAR: usize = 0x11;
const MODEL_YEAR: u8 = 0xff;
const YEAR_BASE: u16 = 1990;
const INPUT: usize = 0x14;
const WIDTH: usize = 0x15;
const HEIGHT: usize = 0x16;
const DESCRIPTORS: usize = 54;
const DESCRIPTOR_LENGTH: usize = 18;
const TEXT: usize = 5;
const PRODUCT_NAME: u8 = 0xfc;
const SERIAL_NUMBER: u8 = 0xff;
const RANGE_LIMITS: u8 = 0xfd;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Year {
    Manufacture(u16),
    Model(u16),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Edid {
    /// The maker's three-letter PNP id, which names nobody without the
    /// register that assigned it.
    pub(crate) manufacturer: String,
    pub(crate) product: u16,
    /// Empty where the EDID carries no product-name descriptor.
    pub(crate) name: String,
    /// Empty where the EDID carries no serial-number descriptor.
    pub(crate) serial: String,
    /// Millimetres from the preferred timing, ten times the precision of
    /// the header's whole centimetres.
    pub(crate) size: Option<(u16, u16)>,
    /// Bits per colour.
    pub(crate) depth: Option<u8>,
    /// The active pixels of the timing the panel prefers, which on a fixed
    /// panel is the only one it really has.
    pub(crate) resolution: Option<(u16, u16)>,
    /// The vertical rates it accepts, in whole Hz.
    pub(crate) refresh: Option<(u16, u16)>,
    pub(crate) year: Option<Year>,
}

/// None for anything that is not a whole, well-formed first block.
pub(crate) fn parse(edid: &[u8]) -> Option<Edid> {
    let block = edid.get(..BLOCK)?;
    (block[..HEADER.len()] == HEADER).then_some(())?;
    let sum = block.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    (sum == 0).then_some(())?;
    Some(Edid {
        manufacturer: pnp(u16::from_be_bytes([block[8], block[9]]))?,
        product: u16::from_le_bytes([block[10], block[11]]),
        name: text(block, PRODUCT_NAME).unwrap_or_default(),
        serial: text(block, SERIAL_NUMBER).unwrap_or_default(),
        size: size(block),
        depth: depth(block[INPUT]),
        resolution: resolution(block),
        refresh: refresh(block),
        year: year(block),
    })
}

/// A zero year byte is an unfilled field rather than the year 1990.
fn year(block: &[u8]) -> Option<Year> {
    let offset = block[YEAR];
    (offset > 0).then_some(())?;
    let year = YEAR_BASE + u16::from(offset);
    Some(if block[WEEK] == MODEL_YEAR {
        Year::Model(year)
    } else {
        Year::Manufacture(year)
    })
}

fn size(block: &[u8]) -> Option<(u16, u16)> {
    let stated = |(across, down): (u16, u16)| (across > 0 && down > 0).then_some((across, down));
    preferred(block)
        .and_then(|timing| {
            stated((
                u16::from(timing[12]) | (u16::from(timing[14] & 0xf0) << 4),
                u16::from(timing[13]) | (u16::from(timing[14] & 0x0f) << 8),
            ))
        })
        .or_else(|| stated((u16::from(block[WIDTH]) * 10, u16::from(block[HEIGHT]) * 10)))
}

/// An analogue panel's byte means something else entirely, and the codes
/// for an undefined and a reserved depth name none.
fn depth(input: u8) -> Option<u8> {
    (input & 0x80 != 0).then_some(())?;
    match (input >> 4) & 0x7 {
        0 | 7 => None,
        code => Some(4 + code * 2),
    }
}

/// The first detailed timing, which the specification reserves for the one
/// the panel prefers. A descriptor is one unless its first bytes are zero.
fn preferred(block: &[u8]) -> Option<&[u8]> {
    descriptors(block).find(|timing| timing[..2] != [0, 0])
}

/// Active pixels are split across a byte and the high nibble of another,
/// the blanking taking the low one.
fn resolution(block: &[u8]) -> Option<(u16, u16)> {
    let timing = preferred(block)?;
    let active =
        |low: usize, high: usize| u16::from(timing[low]) | (u16::from(timing[high] & 0xf0) << 4);
    let pixels = (active(2, 4), active(5, 7));
    (pixels.0 > 0 && pixels.1 > 0).then_some(pixels)
}

/// Either rate may be offset by 255, which is how a panel states one the
/// byte cannot hold.
fn refresh(block: &[u8]) -> Option<(u16, u16)> {
    let limits = tagged(block, RANGE_LIMITS)?;
    let offset = |flag: u8| if limits[4] & flag == 0 { 0 } else { 255 };
    Some((
        u16::from(limits[5]) + offset(0x01),
        u16::from(limits[6]) + offset(0x02),
    ))
}

/// The three letters a manufacturer id packs into five bits each, 1 for A.
/// None where a letter falls outside that, which no assigned id does.
fn pnp(id: u16) -> Option<String> {
    (0..3)
        .rev()
        .map(|letter| {
            let value = u8::try_from((id >> (letter * 5)) & 0x1f).ok()?;
            (1..=26)
                .contains(&value)
                .then(|| char::from(b'A' + value - 1))
        })
        .collect()
}

fn descriptors(block: &[u8]) -> impl Iterator<Item = &[u8]> {
    block
        .get(DESCRIPTORS..)
        .unwrap_or_default()
        .chunks_exact(DESCRIPTOR_LENGTH)
}

fn tagged(block: &[u8], tag: u8) -> Option<&[u8]> {
    descriptors(block).find(|descriptor| descriptor[..3] == [0, 0, 0] && descriptor[3] == tag)
}

fn text(block: &[u8], tag: u8) -> Option<String> {
    let descriptor = tagged(block, tag)?;
    let text = std::str::from_utf8(&descriptor[TEXT..]).ok()?;
    let text = text.split('\n').next()?.trim_end();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{BLOCK, Edid, Year, parse};

    const HEADER_BYTE: usize = 0;
    const CHECKSUM_BYTE: usize = 127;
    const WEEK_BYTE: usize = 0x10;
    const YEAR_BYTE: usize = 0x11;

    const PANEL: [u8; 128] = [
        0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00, 0x0e, 0x77, 0x22, 0x13, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x23, 0x01, 0x04, 0xb5, 0x1c, 0x13, 0x78, 0x03, 0xcc, 0x85, 0xa4, 0x55, 0x4c,
        0x9c, 0x24, 0x0d, 0x50, 0x54, 0x00, 0x00, 0x00, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01,
        0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0xda, 0x90, 0x40, 0xa0, 0xb0, 0x80,
        0x71, 0x70, 0x30, 0x20, 0x66, 0x00, 0x1d, 0xbe, 0x10, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00,
        0xfd, 0x00, 0x1e, 0x78, 0xf4, 0xf4, 0x4b, 0x01, 0x0a, 0x20, 0x20, 0x20, 0x20, 0x20, 0x20,
        0x00, 0x00, 0x00, 0xfe, 0x00, 0x43, 0x53, 0x4f, 0x54, 0x20, 0x54, 0x33, 0x20, 0x20, 0x20,
        0x20, 0x20, 0x20, 0x00, 0x00, 0x00, 0xfc, 0x00, 0x4d, 0x4e, 0x44, 0x35, 0x30, 0x38, 0x5a,
        0x42, 0x31, 0x2d, 0x31, 0x0a, 0x20, 0x02, 0x9c,
    ];

    fn altered(at: usize, to: u8) -> [u8; 128] {
        let mut edid = PANEL;
        edid[at] = to;
        edid
    }

    fn resummed(at: usize, to: u8) -> [u8; 128] {
        let mut edid = altered(at, to);
        edid[CHECKSUM_BYTE] = 0;
        let sum = edid.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte));
        edid[CHECKSUM_BYTE] = sum.wrapping_neg();
        edid
    }

    pub(crate) fn panel() -> Edid {
        parse(&PANEL).unwrap()
    }

    #[test]
    fn a_panel_is_read_off_its_edid() {
        assert_eq!(
            parse(&PANEL).unwrap(),
            Edid {
                manufacturer: "CSW".to_owned(),
                product: 4898,
                name: "MND508ZB1-1".to_owned(),
                serial: String::new(),
                size: Some((285, 190)),
                depth: Some(10),
                resolution: Some((2880, 1920)),
                refresh: Some((30, 120)),
                year: Some(Year::Manufacture(2025)),
            }
        );
    }

    #[test]
    fn a_panel_whose_week_byte_is_the_marker_states_a_model_year() {
        assert_eq!(
            parse(&resummed(WEEK_BYTE, 0xff)).unwrap().year,
            Some(Year::Model(2025))
        );
    }

    #[test]
    fn a_panel_counting_no_years_from_1990_is_undated() {
        assert!(parse(&resummed(YEAR_BYTE, 0)).unwrap().year.is_none());
    }

    #[test]
    fn a_block_that_is_short_or_corrupt_reads_as_no_panel() {
        assert!(parse(&PANEL[..BLOCK - 1]).is_none());
        assert!(parse(&altered(HEADER_BYTE, 0xff)).is_none());
        assert!(parse(&altered(CHECKSUM_BYTE, 0x00)).is_none());
    }
}
