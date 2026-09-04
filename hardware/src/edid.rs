//! What a panel's EDID says it is, apart from the DRM class so the decoding
//! is testable without a connector.

const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];

const BLOCK: usize = 128;
const DESCRIPTORS: usize = 54;
const DESCRIPTOR_LENGTH: usize = 18;
const TEXT: usize = 5;
const PRODUCT_NAME: u8 = 0xfc;
const SERIAL_NUMBER: u8 = 0xff;

/// What a panel announces about itself, in its own spelling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edid {
    /// The maker's three-letter PNP id, which names nobody without the
    /// register that assigned it.
    pub manufacturer: String,
    pub product: u16,
    /// Empty where the EDID carries no product-name descriptor, which is
    /// optional and a panel is free to omit.
    pub name: String,
    /// Empty where the EDID carries no serial-number descriptor.
    pub serial: String,
}

/// None for anything that is not a whole, well-formed first block.
pub fn parse(edid: &[u8]) -> Option<Edid> {
    let block = edid.get(..BLOCK)?;
    (block[..HEADER.len()] == HEADER).then_some(())?;
    let sum = block.iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte));
    (sum == 0).then_some(())?;
    Some(Edid {
        manufacturer: pnp(u16::from_be_bytes([block[8], block[9]]))?,
        product: u16::from_le_bytes([block[10], block[11]]),
        name: descriptor(block, PRODUCT_NAME).unwrap_or_default(),
        serial: descriptor(block, SERIAL_NUMBER).unwrap_or_default(),
    })
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

/// The text of the descriptor carrying `tag`, less the terminator and the
/// spaces padding it out. None where no descriptor carries the tag, since a
/// panel need not offer either of the ones read here.
fn descriptor(block: &[u8], tag: u8) -> Option<String> {
    let descriptor = block
        .get(DESCRIPTORS..)?
        .chunks_exact(DESCRIPTOR_LENGTH)
        .find(|descriptor| descriptor[..3] == [0, 0, 0] && descriptor[3] == tag)?;
    let text = std::str::from_utf8(&descriptor[TEXT..]).ok()?;
    let text = text.split('\n').next()?.trim_end();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{BLOCK, Edid, parse};

    const HEADER_BYTE: usize = 0;
    const CHECKSUM_BYTE: usize = 127;

    /// This machine's panel, block 0 as the kernel hands it over.
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

    #[test]
    fn a_panel_is_read_off_its_edid() {
        assert_eq!(
            parse(&PANEL).unwrap(),
            Edid {
                manufacturer: "CSW".to_owned(),
                product: 4898,
                name: "MND508ZB1-1".to_owned(),
                serial: String::new(),
            }
        );
    }

    #[test]
    fn a_block_that_is_short_or_corrupt_reads_as_no_panel() {
        assert!(parse(&PANEL[..BLOCK - 1]).is_none());
        assert!(parse(&altered(HEADER_BYTE, 0xff)).is_none());
        assert!(parse(&altered(CHECKSUM_BYTE, 0x00)).is_none());
    }
}
