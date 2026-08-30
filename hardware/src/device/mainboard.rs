//! The mainboard, as the firmware's DMI fields describe it: a part the
//! daemon reads and never sets, carrying the firmware it runs.

use crate::dmi;
use crate::ec::Ec;
use crate::part::{Firmware, Identity, Part, PartKind};

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
            ec.and_then(Ec::version).map(|v| Firmware::new("EC", &v)),
        ]
        .into_iter()
        .flatten()
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

#[cfg(test)]
mod tests {
    use super::Mainboard;
    use crate::part::{Firmware, Part, PartKind};

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
}
