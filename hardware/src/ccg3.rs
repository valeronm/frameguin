//! What the firmware report of a CCG3 expansion card — the HDMI and
//! `DisplayPort` cards — says, apart from `usb` so the decoding is testable
//! without a card.
//!
//! Laid out as `framework_lib`'s `ccgx::hid` reads it. Only the report read
//! is taken from there: its `check_ccg_fw_version` unlocks the card first
//! and leaves it in flashing mode.

use framework_lib::ccgx::BaseVersion;

pub(crate) const REPORT: u8 = 0xE0;
pub(crate) const REPORT_LEN: usize = 0x40;

/// The report descriptor's usage page item for `0xFFEE`, the interface the
/// firmware report is on.
const USAGE_PAGE: [u8; 3] = [0x06, 0xEE, 0xFF];

const SIGNATURE: std::ops::Range<usize> = 2..4;
/// `AA` as the card answers unprompted, `CY` once something has unlocked it.
const SIGNATURES: [&[u8]; 2] = [b"AA", b"CY"];
const OPERATING_MODE: usize = 4;
/// `framework_lib`'s `FwMode::BackupFw`, the one mode that runs image 1.
const BACKUP: u8 = 1;
const IMAGE_1: usize = 20;
const IMAGE_2: usize = 28;

pub(crate) fn vendor_interface(descriptor: &[u8]) -> bool {
    descriptor
        .windows(USAGE_PAGE.len())
        .any(|item| item == USAGE_PAGE)
}

/// The running image's version, as `framework_tool` spells it; None for a
/// report that is not this layout.
pub(crate) fn active_version(report: &[u8; REPORT_LEN]) -> Option<String> {
    if report[0] != REPORT || !SIGNATURES.contains(&&report[SIGNATURE]) {
        return None;
    }
    let image = match report[OPERATING_MODE] {
        BACKUP => IMAGE_1,
        0 | 2 => IMAGE_2,
        _ => return None,
    };
    Some(BaseVersion::from(&report[image..image + 4]).to_string())
}

#[cfg(test)]
mod tests {
    use super::{REPORT_LEN, active_version, vendor_interface};

    const HDMI_CARD: &str = "e000414102000000001d000084030230616102006a001030616102006a00103061610200001800000000010037e9350337110b00000000000000000000000000";
    const HDMI_CARD_DESCRIPTOR: &str = "06eeff0901a10185e00902150026ff007508953fb10285e10902150026ff0075089507910285e20902150026ff0075089583910285e30902150026ff0075089583b10285e40902150026ff0075089507b102c0";

    fn bytes(hex: &str) -> Vec<u8> {
        (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
            .collect()
    }

    fn report(hex: &str) -> [u8; REPORT_LEN] {
        bytes(hex).try_into().unwrap()
    }

    #[test]
    fn the_main_image_is_the_active_one_in_main_mode() {
        let mut main = report(HDMI_CARD);
        main[20..24].copy_from_slice(&[0x01, 0x00, 0x09, 0x30]);
        assert_eq!(active_version(&main).as_deref(), Some("3.0.10.06A"));
    }

    #[test]
    fn the_backup_mode_reads_the_first_image() {
        let mut backup = report(HDMI_CARD);
        backup[4] = 1;
        backup[20..24].copy_from_slice(&[0x01, 0x00, 0x09, 0x30]);
        assert_eq!(active_version(&backup).as_deref(), Some("3.0.9.001"));
    }

    #[test]
    fn an_unlocked_card_answers_the_same_layout() {
        let mut unlocked = report(HDMI_CARD);
        unlocked[2..4].copy_from_slice(b"CY");
        assert_eq!(active_version(&unlocked).as_deref(), Some("3.0.10.06A"));
    }

    #[test]
    fn an_unknown_signature_has_no_version() {
        let mut other = report(HDMI_CARD);
        other[2..4].copy_from_slice(b"ZZ");
        assert_eq!(active_version(&other), None);
    }

    #[test]
    fn an_operating_mode_outside_the_three_defined_has_no_version() {
        let mut other = report(HDMI_CARD);
        other[4] = 3;
        assert_eq!(active_version(&other), None);
    }

    #[test]
    fn the_cards_vendor_interface_is_found_by_its_usage_page() {
        assert!(vendor_interface(&bytes(HDMI_CARD_DESCRIPTOR)));
        assert!(!vendor_interface(&[0x05, 0x01, 0x09, 0x06]));
    }
}
