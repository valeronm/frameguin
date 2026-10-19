//! What an expansion bay module's descriptor declares, apart from `ec` so
//! the decoding is testable without a module.
//!
//! Laid out as `framework_lib`'s `GpuCfgDescriptor` and the EC's
//! `gpu_configuration.h` declare it: a header, then blocks of a type byte, a
//! length byte and that many bytes of data. It is read and rejected as the
//! EC reads and rejects it.

use std::mem::{offset_of, size_of};

use framework_lib::chromium_ec::GpuCfgDescriptor;

use crate::ec::PdPorts;

/// The EC reads the descriptor from the first of these that answers.
const EEPROMS: [u16; 2] = [0x50, 0x52];
pub(crate) const HEADER_LEN: usize = size_of::<GpuCfgDescriptor>();
pub(crate) const DESCRIPTOR_LENGTH: usize = offset_of!(GpuCfgDescriptor, descriptor_length);
pub(crate) const HEADER_CRC: usize = offset_of!(GpuCfgDescriptor, crc32);
pub(crate) const PD_BLOCK: u8 = 11;
pub(crate) const CCG8S: u8 = 2;

/// The address of the CCG8S the module on `bus` declares, and None where it
/// declares another PD chip or none, where its descriptor does not read, and
/// where its header does not check.
pub(crate) fn ccg8s(ec: &dyn PdPorts, bus: u8) -> Option<u8> {
    let (eeprom, header) = EEPROMS
        .iter()
        .find_map(|&eeprom| Some((eeprom, ec.eeprom(bus, eeprom, 0, HEADER_LEN).ok()?)))?;
    let blocks = ec
        .eeprom(bus, eeprom, HEADER_LEN, blocks_len(&header)?)
        .ok()?;
    pd_ccg8s(&blocks)
}

/// The length of the blocks after `header`, and None for a header the EC
/// rejects: a magic byte of `0xFF`, which a blank EEPROM reads as, or a CRC
/// that does not match.
fn blocks_len(header: &[u8]) -> Option<usize> {
    let header = header.get(..HEADER_LEN)?;
    if header[..4].contains(&0xFF) || crc32(&header[..HEADER_CRC]) != word(header, HEADER_CRC)? {
        return None;
    }
    usize::try_from(word(header, DESCRIPTOR_LENGTH)?).ok()
}

fn pd_ccg8s(mut blocks: &[u8]) -> Option<u8> {
    while let [kind, len, rest @ ..] = blocks {
        let data = rest.get(..usize::from(*len))?;
        if *kind == PD_BLOCK {
            return match data {
                [CCG8S, address, ..] => Some(*address),
                _ => None,
            };
        }
        blocks = &rest[data.len()..];
    }
    None
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// CRC-32 as the EC's own table computes it, the IEEE polynomial reflected.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    !bytes.iter().fold(u32::MAX, |crc, &byte| {
        (0..8).fold(crc ^ u32::from(byte), |crc, _| {
            (crc >> 1) ^ (0xEDB8_8320 & (crc & 1).wrapping_neg())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{CCG8S, PD_BLOCK, ccg8s, crc32};
    use crate::testing::{Connectors, ccg8s_module, ej889i_module, module};

    const BUS: u8 = 5;

    fn at(eeprom: u16, descriptor: Vec<u8>) -> Connectors {
        Connectors {
            bay: Some((eeprom, descriptor)),
            ..Connectors::default()
        }
    }

    #[test]
    fn the_crc_is_the_ieee_one() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn a_ccg8s_module_gives_its_controller_address() {
        assert_eq!(ccg8s(&at(0x50, ccg8s_module(0x42)), BUS), Some(0x42));
    }

    #[test]
    fn the_second_eeprom_is_read_where_the_first_does_not_answer() {
        assert_eq!(ccg8s(&at(0x52, ccg8s_module(0x42)), BUS), Some(0x42));
    }

    #[test]
    fn another_pd_chip_gives_no_address() {
        assert_eq!(ccg8s(&at(0x50, ej889i_module()), BUS), None);
    }

    #[test]
    fn a_module_with_no_pd_block_gives_no_address() {
        let serial_block = 9;
        let descriptor = module(&[(serial_block, &[1; 21])]);
        assert_eq!(ccg8s(&at(0x50, descriptor), BUS), None);
    }

    #[test]
    fn the_pd_block_is_found_past_others() {
        let thermal_block = 2;
        let thermal_sensor = [1, 0x4D, 0, 0, 0, 0, 0, 0, 0, 0];
        let descriptor = module(&[(thermal_block, &thermal_sensor), (PD_BLOCK, &[CCG8S, 0x08])]);
        assert_eq!(ccg8s(&at(0x50, descriptor), BUS), Some(0x08));
    }

    #[test]
    fn a_blank_eeprom_has_no_descriptor() {
        assert_eq!(ccg8s(&at(0x50, vec![0xFF; 64]), BUS), None);
    }

    #[test]
    fn a_header_whose_crc_does_not_match_is_rejected() {
        let mut descriptor = ccg8s_module(0x42);
        descriptor[16] ^= 1;
        assert_eq!(ccg8s(&at(0x50, descriptor), BUS), None);
    }

    #[test]
    fn a_truncated_descriptor_gives_no_address() {
        let descriptor = ccg8s_module(0x42);
        let short = descriptor[..descriptor.len() - 1].to_vec();
        assert_eq!(ccg8s(&at(0x50, short), BUS), None);
    }

    #[test]
    fn a_bay_with_no_eeprom_gives_no_address() {
        assert_eq!(ccg8s(&Connectors::default(), BUS), None);
    }
}
