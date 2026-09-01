//! The mainboard, as the firmware's DMI fields describe it: a part the
//! daemon reads and never sets, carrying the firmware it runs.

use crate::dmi;
use crate::ec::Ec;
use crate::part::{Firmware, Identity, Part, PartKind};
use crate::pd;

pub struct Mainboard {
    identity: Identity,
}

impl Part for Mainboard {
    fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl Mainboard {
    /// The board, named by its own part number, on any machine that
    /// publishes one. `ec` is None where there is no Framework EC, which
    /// costs the board its EC version and nothing else.
    pub fn detect(ec: Option<&Ec>) -> Option<Self> {
        let firmware = [
            dmi::field("bios_version").map(|v| Firmware::new("BIOS", &v)),
            // A version is never worth a failed detection, so a silent EC
            // costs the board that field alone.
            ec.and_then(|ec| ec.version().ok())
                .map(|v| Firmware::new("EC", &v)),
        ]
        .into_iter()
        .flatten()
        .chain(pd_firmware(&ec.map(Ec::pd_versions).unwrap_or_default()))
        .collect();
        Some(Self::new(
            &dmi::field("board_vendor").unwrap_or_default(),
            &dmi::product().unwrap_or_default(),
            &dmi::field("board_name")?,
            &dmi::field("board_serial").unwrap_or_default(),
            firmware,
        ))
    }

    /// `product` is the machine the board is sold for, which is how the
    /// board's generation is spoken of; `board` is its own part number.
    pub fn new(
        vendor: &str,
        product: &str,
        board: &str,
        serial: &str,
        firmware: Vec<Firmware>,
    ) -> Self {
        Self {
            identity: Identity {
                kind: PartKind::Mainboard,
                vendor: vendor.to_owned(),
                model: product.to_owned(),
                serial: serial.to_owned(),
                id: format!("dmi-board:{board}"),
                firmware,
            },
        }
    }
}

/// The USB-C power delivery controllers, named by the EC's controller
/// number — the same number the port index divides by, two ports to a
/// controller. They are soldered to the board like the EC itself, so they
/// are firmware it runs rather than parts of their own; the Laptop 16's
/// third rides on whichever module fills the expansion bay, and is the one
/// this misplaces.
///
/// A controller keeps its number when an earlier one has no version, so a
/// gap in what the EC answers is a gap in the names rather than a renaming
/// of the controllers after it.
fn pd_firmware(versions: &[[u8; pd::VERSION_LEN]]) -> Vec<Firmware> {
    versions
        .iter()
        .enumerate()
        .filter_map(|(index, blob)| {
            Some(Firmware::new(&format!("PD {index}"), &pd::version(*blob)?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::Mainboard;
    use crate::part::{Firmware, Part, PartKind};
    use crate::testing::PD_VERSION;

    #[test]
    fn a_board_is_identified_by_its_part_number_and_named_for_its_machine() {
        let board = Mainboard::new(
            "Framework",
            "Laptop 13 Pro (Intel Core Ultra Series 3)",
            "FRANMJCP07",
            "",
            vec![Firmware::new("BIOS", "03.02")],
        );
        let identity = board.identity();
        assert_eq!(identity.kind, PartKind::Mainboard);
        assert_eq!(identity.id, "dmi-board:FRANMJCP07");
        assert_eq!(identity.model, "Laptop 13 Pro (Intel Core Ultra Series 3)");
        assert_eq!(identity.firmware[0].name, "BIOS");
    }

    #[test]
    fn each_pd_controller_is_firmware_named_by_its_number() {
        let firmware = super::pd_firmware(&[PD_VERSION, PD_VERSION]);
        let named: Vec<(&str, &str)> = firmware
            .iter()
            .map(|f| (f.name.as_str(), f.version.as_str()))
            .collect();
        assert_eq!(named, [("PD 0", "1.0.0A"), ("PD 1", "1.0.0A")]);
    }

    #[test]
    fn a_controller_the_ec_never_saw_leaves_the_rest_their_numbers() {
        let firmware = super::pd_firmware(&[[0; 8], PD_VERSION]);
        assert_eq!(firmware.len(), 1);
        assert_eq!(firmware[0].name, "PD 1");
    }
}
