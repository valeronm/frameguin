//! The words for a part: what its kind is called, the order a bill of
//! materials lists them in, and — where the hardware's own words are not a
//! name a person would recognise — the name Framework sells it under.

use frameguin_wire::{self as wire, Identity, PartKind, VENDOR};

#[must_use]
pub fn kind_label(kind: PartKind) -> &'static str {
    match kind {
        PartKind::Mainboard => "Mainboard",
        PartKind::Battery => "Battery",
        PartKind::Memory => "Memory",
        PartKind::Touchpad => "Touchpad",
        PartKind::Touchscreen => "Touchscreen",
    }
}

/// Where a kind's parts sit in the list: the board first, then what plugs
/// into it. A match rather than the vocabulary's own order, so a variant
/// added there lands where this says and not wherever it was declared.
fn rank(kind: PartKind) -> u8 {
    match kind {
        PartKind::Mainboard => 0,
        PartKind::Memory => 1,
        PartKind::Battery => 2,
        PartKind::Touchpad => 3,
        PartKind::Touchscreen => 4,
    }
}

/// Parts in list order, those of a kind by their identifier — so two memory
/// modules list by slot whatever order the table gave them in.
#[must_use]
pub fn ordered(parts: &[Identity]) -> Vec<&Identity> {
    let mut parts: Vec<&Identity> = parts.iter().collect();
    parts.sort_by_key(|part| (rank(part.kind), part.id.as_str()));
    parts
}

/// A part as Framework's marketplace names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Catalogue {
    /// The product's name as its page spells it, less the vendor's own name
    /// leading it.
    pub model: &'static str,
    /// None for a part Framework has stopped selling, its page taken down
    /// with it: the name is still what a reader wants and a dead link is
    /// not.
    pub url: Option<&'static str>,
}

/// What names a part to the catalogue: the identifier it announces itself
/// by, except a mainboard, whose part number is confirmed for one board
/// only, and a memory module, whose identifier is the board's slot rather
/// than the module's — both keyed on their model string instead.
fn key(part: &Identity) -> (PartKind, &str) {
    let key = match part.kind {
        PartKind::Mainboard | PartKind::Memory => &part.model,
        _ => &part.id,
    };
    (part.kind, key)
}

/// The marketplace entry for a part. Curated from Framework's own listings,
/// trademark marks left off; a part with no entry is shown under its own
/// words, and an entry is never guessed from a resemblance, since a wrong
/// name reads exactly like a right one.
#[must_use]
pub fn catalogue(part: &Identity) -> Option<Catalogue> {
    match key(part) {
        (PartKind::Mainboard, wire::BOARD_LAPTOP13_11TH_GEN) => Some(Catalogue {
            model: "Laptop 13 Mainboard (11th Gen Intel Core)",
            url: None,
        }),
        (PartKind::Mainboard, wire::BOARD_LAPTOP13_12TH_GEN) => Some(Catalogue {
            model: "Laptop 13 Mainboard (12th Gen Intel Core)",
            url: None,
        }),
        (PartKind::Mainboard, wire::BOARD_LAPTOP13_13TH_GEN) => Some(Catalogue {
            model: "Laptop 13 Mainboard (13th Gen Intel Core)",
            url: None,
        }),
        // A page carries the processor as a variant code the product name
        // does not map to, so a board's link is to the page unvaried.
        (PartKind::Mainboard, wire::BOARD_LAPTOP13_ULTRA_1) => Some(Catalogue {
            model: "Laptop 13 Mainboard (Intel Core Ultra Series 1)",
            url: Some("https://frame.work/products/mainboard-ultra-1-intel-core"),
        }),
        (
            PartKind::Mainboard,
            wire::BOARD_LAPTOP13_AMD_7040 | wire::BOARD_LAPTOP13_AMD_7040_UNSPACED,
        ) => Some(Catalogue {
            model: "Laptop 13 Mainboard (AMD Ryzen 7040 Series)",
            url: Some("https://frame.work/products/mainboard-amd-ryzen-7040-series"),
        }),
        (PartKind::Mainboard, wire::BOARD_LAPTOP13_AMD_AI_300) => Some(Catalogue {
            model: "Laptop 13 Mainboard (AMD Ryzen AI 300 Series)",
            url: Some("https://frame.work/products/mainboard-amd-ai300"),
        }),
        (PartKind::Mainboard, wire::BOARD_LAPTOP13_PRO_ULTRA_3) => Some(Catalogue {
            model: "Laptop 13 Pro Mainboard (Intel Core Ultra Series 3)",
            url: Some("https://frame.work/products/laptop13pro-mainboard-intel-ultra-3"),
        }),
        (PartKind::Mainboard, wire::BOARD_LAPTOP12_13TH_GEN) => Some(Catalogue {
            model: "Laptop 12 Mainboard (13th Gen Intel Core)",
            url: Some("https://frame.work/products/laptop12-mainboard-13th-gen-intel-core"),
        }),
        (PartKind::Mainboard, wire::BOARD_LAPTOP16_AMD_7040) => Some(Catalogue {
            model: "Laptop 16 Mainboard (AMD Ryzen 7040 Series)",
            url: Some("https://frame.work/products/16-mainboard-amd-ryzen-7040-series"),
        }),
        (PartKind::Mainboard, wire::BOARD_LAPTOP16_AMD_AI_300) => Some(Catalogue {
            model: "Laptop 16 Mainboard (AMD Ryzen AI 300 Series)",
            url: Some("https://frame.work/products/laptop16-mainboard-amd-ai300"),
        }),
        (PartKind::Mainboard, wire::BOARD_DESKTOP_AMD_AI_MAX_300) => Some(Catalogue {
            model: "Desktop Mainboard (AMD Ryzen AI Max 300 Series)",
            url: Some(
                "https://frame.work/products/framework-desktop-mainboard-amd-ryzen-ai-max-300-series",
            ),
        }),
        (PartKind::Battery, "sbs:FRANEDA") => Some(Catalogue {
            model: "Laptop 13 Pro Battery - 74Wh",
            url: Some("https://frame.work/products/pro-battery-74wh"),
        }),
        // The haptic touchpad is sold only fitted to the input cover frame.
        (PartKind::Touchpad, "hid:093a:1343") => Some(Catalogue {
            model: "Laptop 13 Pro Input Cover Frame",
            url: Some("https://frame.work/products/laptop13pro-input-cover-frame"),
        }),
        (PartKind::Touchscreen, "hid:3558:14fd") => Some(Catalogue {
            model: "Laptop 13 Pro Touchscreen Display Kit - 2.8K",
            url: Some("https://frame.work/products/laptop13pro-display-kit"),
        }),
        // The page's capacity variants carry codes nothing on the module
        // maps to, so the link is to the page unvaried.
        (PartKind::Memory, "MTD16C20325N4FN023F1 YF") => Some(Catalogue {
            model: "LPCAMM2 - LPDDR5X 8533 Memory",
            url: Some("https://frame.work/products/lpcamm2-lpddr5x"),
        }),
        _ => None,
    }
}

/// The maker's own words for a part made by someone other than Framework —
/// a memory module is Micron's part before it is Framework's listing — as
/// manufacturer and part number. None where Framework made it or the
/// hardware named no maker.
#[must_use]
pub fn maker(part: &Identity) -> Option<(&str, &str)> {
    (!part.vendor.is_empty() && part.vendor != VENDOR)
        .then_some((part.vendor.as_str(), part.model.as_str()))
}

#[cfg(test)]
mod tests {
    use frameguin_wire::{self as wire, Identity, PartKind, VENDOR};

    use super::{catalogue, maker, ordered};

    fn part(kind: PartKind, id: &str) -> Identity {
        Identity {
            kind,
            vendor: String::new(),
            model: String::new(),
            serial: String::new(),
            id: id.to_owned(),
            firmware: Vec::new(),
        }
    }

    fn board(product: &str) -> Identity {
        Identity {
            model: product.to_owned(),
            ..part(PartKind::Mainboard, "dmi-board:FRANMJCP07")
        }
    }

    fn module(slot: &str) -> Identity {
        Identity {
            vendor: "Micron Technology".to_owned(),
            model: "MTD16C20325N4FN023F1 YF".to_owned(),
            ..part(PartKind::Memory, slot)
        }
    }

    #[test]
    fn the_board_leads_and_modules_list_by_slot() {
        let parts = [
            part(PartKind::Memory, "dmi-slot:LPCAMM2_1"),
            part(PartKind::Touchpad, "hid:093a:1343"),
            part(PartKind::Memory, "dmi-slot:LPCAMM2_0"),
            part(PartKind::Mainboard, "dmi-board:FRANMJCP07"),
        ];
        let ids: Vec<&str> = ordered(&parts)
            .iter()
            .map(|part| part.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "dmi-board:FRANMJCP07",
                "dmi-slot:LPCAMM2_0",
                "dmi-slot:LPCAMM2_1",
                "hid:093a:1343"
            ]
        );
    }

    #[test]
    fn a_part_the_catalogue_does_not_name_keeps_its_own_words() {
        assert!(catalogue(&part(PartKind::Memory, "dmi-slot:LPCAMM2_0")).is_none());
        assert!(catalogue(&part(PartKind::Touchscreen, "hid:3558:14fd")).is_some());
    }

    /// A module is the same part in whichever slot it sits, and a slot holds
    /// whichever module was fitted, so the slot cannot be the key.
    #[test]
    fn a_memory_module_is_catalogued_by_its_part_number_not_its_slot() {
        assert!(catalogue(&module("dmi-slot:LPCAMM2_1")).is_some());
    }

    #[test]
    fn a_board_is_catalogued_by_the_machine_its_firmware_names() {
        let sold = catalogue(&board(wire::BOARD_LAPTOP13_AMD_7040_UNSPACED)).unwrap();
        assert_eq!(sold.model, "Laptop 13 Mainboard (AMD Ryzen 7040 Series)");
    }

    #[test]
    fn a_board_framework_no_longer_sells_is_named_without_a_link() {
        let sold = catalogue(&board(wire::BOARD_LAPTOP13_12TH_GEN)).unwrap();
        assert_eq!(sold.model, "Laptop 13 Mainboard (12th Gen Intel Core)");
        assert!(sold.url.is_none());
    }

    #[test]
    fn a_board_of_no_known_machine_keeps_its_own_words() {
        assert!(catalogue(&board("Precision 5560")).is_none());
    }

    #[test]
    fn only_a_part_of_another_make_keeps_its_makers_words() {
        assert_eq!(
            maker(&module("dmi-slot:LPCAMM2_0")),
            Some(("Micron Technology", "MTD16C20325N4FN023F1 YF"))
        );
        let board = Identity {
            vendor: VENDOR.to_owned(),
            ..part(PartKind::Mainboard, "dmi-board:FRANMJCP07")
        };
        assert_eq!(maker(&board), None);
        assert_eq!(maker(&part(PartKind::Touchpad, "hid:093a:1343")), None);
    }
}
