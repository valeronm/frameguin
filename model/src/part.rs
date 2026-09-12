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
        PartKind::Storage => "Storage",
        PartKind::Display => "Display",
        PartKind::Touchpad => "Touchpad",
    }
}

/// Where a kind's parts sit in the list: the board first, then what plugs
/// into it. A match rather than the vocabulary's own order, so a variant
/// added there lands where this says and not wherever it was declared.
fn rank(kind: PartKind) -> u8 {
    match kind {
        PartKind::Mainboard => 0,
        PartKind::Memory => 1,
        PartKind::Storage => 2,
        PartKind::Battery => 3,
        PartKind::Display => 4,
        PartKind::Touchpad => 5,
    }
}

/// Parts in list order, those of a kind by their identifier — so two memory
/// modules list by slot whatever order the table gave them in.
fn ordered(parts: &[Identity]) -> Vec<&Identity> {
    let mut parts: Vec<&Identity> = parts.iter().collect();
    parts.sort_by_key(|part| (rank(part.kind), part.id.as_str()));
    parts
}

/// The machine's parts in list order, each with the words it is listed
/// under: its kind, numbered where the machine holds more than one of that
/// kind, two memory modules being the same word twice otherwise. The
/// numbering counts in the order returned, so the two cannot be taken apart.
#[must_use]
pub fn inventory(parts: &[Identity]) -> Vec<(&Identity, String)> {
    let ordered = ordered(parts);
    ordered
        .iter()
        .enumerate()
        .map(|(index, part)| {
            let kind = kind_label(part.kind);
            let alike = |other: &&&Identity| other.kind == part.kind;
            let listed = if ordered.iter().filter(alike).count() == 1 {
                kind.to_owned()
            } else {
                let ordinal = ordered[..index].iter().filter(alike).count() + 1;
                format!("{kind} #{ordinal}")
            };
            (*part, listed)
        })
        .collect()
}

/// A part as Framework's marketplace names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Catalogue {
    /// The product's name as its page spells it, less the vendor's own name
    /// leading it and less the `variant` it ends on.
    pub model: &'static str,
    /// The processor line the product's name ends on; None where the name
    /// is one piece.
    pub variant: Option<&'static str>,
    /// None where no live page names this part: the name is still what a
    /// reader wants and a page about another part is not.
    pub url: Option<&'static str>,
}

/// What names a part to the catalogue: the model string, the part's own
/// words for itself, and not the identifier beside it — a board's is a part
/// number confirmed for one machine only. A HID part is the exception, its
/// descriptor free to carry no strings at all.
fn key(part: &Identity) -> (PartKind, &str) {
    let key = match part.kind {
        PartKind::Mainboard
        | PartKind::Battery
        | PartKind::Memory
        | PartKind::Storage
        | PartKind::Display => &part.model,
        PartKind::Touchpad => &part.id,
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
        (PartKind::Mainboard, wire::BOARD_LAPTOP13_11TH_GEN) => {
            Some(varied("Laptop 13 Mainboard", "11th Gen Intel Core", None))
        }
        (PartKind::Mainboard, wire::BOARD_LAPTOP13_12TH_GEN) => {
            Some(varied("Laptop 13 Mainboard", "12th Gen Intel Core", None))
        }
        (PartKind::Mainboard, wire::BOARD_LAPTOP13_13TH_GEN) => {
            Some(varied("Laptop 13 Mainboard", "13th Gen Intel Core", None))
        }
        // A page carries the processor as a variant code the product name
        // does not map to, so a board's link is to the page unvaried.
        (PartKind::Mainboard, wire::BOARD_LAPTOP13_ULTRA_1) => Some(varied(
            "Laptop 13 Mainboard",
            "Intel Core Ultra Series 1",
            Some("https://frame.work/products/mainboard-ultra-1-intel-core"),
        )),
        (
            PartKind::Mainboard,
            wire::BOARD_LAPTOP13_AMD_7040 | wire::BOARD_LAPTOP13_AMD_7040_UNSPACED,
        ) => Some(varied(
            "Laptop 13 Mainboard",
            "AMD Ryzen 7040 Series",
            Some("https://frame.work/products/mainboard-amd-ryzen-7040-series"),
        )),
        (PartKind::Mainboard, wire::BOARD_LAPTOP13_AMD_AI_300) => Some(varied(
            "Laptop 13 Mainboard",
            "AMD Ryzen AI 300 Series",
            Some("https://frame.work/products/mainboard-amd-ai300"),
        )),
        (PartKind::Mainboard, wire::BOARD_LAPTOP13_PRO_ULTRA_3) => Some(varied(
            "Laptop 13 Pro Mainboard",
            "Intel Core Ultra Series 3",
            Some("https://frame.work/products/laptop13pro-mainboard-intel-ultra-3"),
        )),
        (PartKind::Mainboard, wire::BOARD_LAPTOP12_13TH_GEN) => Some(varied(
            "Laptop 12 Mainboard",
            "13th Gen Intel Core",
            Some("https://frame.work/products/laptop12-mainboard-13th-gen-intel-core"),
        )),
        (PartKind::Mainboard, wire::BOARD_LAPTOP12_CORE_3) => Some(varied(
            "Laptop 12 Mainboard",
            "Intel Core Series 3",
            Some("https://frame.work/products/laptop12-mainboard-series3"),
        )),
        (PartKind::Mainboard, wire::BOARD_LAPTOP16_AMD_7040) => Some(varied(
            "Laptop 16 Mainboard",
            "AMD Ryzen 7040 Series",
            Some("https://frame.work/products/16-mainboard-amd-ryzen-7040-series"),
        )),
        (PartKind::Mainboard, wire::BOARD_LAPTOP16_AMD_AI_300) => Some(varied(
            "Laptop 16 Mainboard",
            "AMD Ryzen AI 300 Series",
            Some("https://frame.work/products/laptop16-mainboard-amd-ai300"),
        )),
        (PartKind::Mainboard, wire::BOARD_DESKTOP_AMD_AI_MAX_300) => Some(varied(
            "Desktop Mainboard",
            "AMD Ryzen AI Max 300 Series",
            Some(
                "https://frame.work/products/framework-desktop-mainboard-amd-ryzen-ai-max-300-series",
            ),
        )),
        // The EC publishes the pack's name in an eight-byte field, so these
        // are the first seven characters of it.
        (PartKind::Battery, "Framewo") => Some(listed("Laptop 13 Battery - 55Wh", None)),
        (PartKind::Battery, "FRANGWA") => Some(listed(
            "Laptop 13 Battery - 61Wh",
            Some("https://frame.work/products/battery"),
        )),
        (PartKind::Battery, "FRANEDA") => Some(listed(
            "Laptop 13 Pro Battery - 74Wh",
            Some("https://frame.work/products/pro-battery-74wh"),
        )),
        (PartKind::Battery, "FRANDZG") => Some(listed(
            "Laptop 12 Battery - 50Wh",
            Some("https://frame.work/products/laptop12-battery-50wh"),
        )),
        (PartKind::Battery, "FRANDBA") => Some(listed(
            "Laptop 16 Battery - 85Wh",
            Some("https://frame.work/products/16-battery"),
        )),
        // The haptic touchpad is sold only fitted to the input cover frame.
        (PartKind::Touchpad, "hid:093a:1343") => Some(listed(
            "Laptop 13 Pro Input Cover Frame",
            Some("https://frame.work/products/laptop13pro-input-cover-frame"),
        )),
        // The kit is the panel and the touch controller together, and the
        // panel is the half every machine with a screen has.
        (PartKind::Display, "MND508ZB1-1") => Some(listed(
            "Laptop 13 Pro Touchscreen Display Kit - 2.8K",
            Some("https://frame.work/products/laptop13pro-display-kit"),
        )),
        // The page's capacity variants carry codes nothing on the module
        // maps to, so the link is to the page unvaried.
        (PartKind::Memory, "MTD16C20325N4FN023F1 YF") => Some(listed(
            "LPCAMM2 - LPDDR5X 8533 Memory",
            Some("https://frame.work/products/lpcamm2-lpddr5x"),
        )),
        _ => None,
    }
}

const fn listed(model: &'static str, url: Option<&'static str>) -> Catalogue {
    Catalogue {
        model,
        variant: None,
        url,
    }
}

const fn varied(
    model: &'static str,
    variant: &'static str,
    url: Option<&'static str>,
) -> Catalogue {
    Catalogue {
        model,
        variant: Some(variant),
        url,
    }
}

/// The maker of a part made by someone other than Framework — a memory
/// module is Micron's part before it is Framework's listing. Empty where
/// Framework made it or the hardware named no maker.
#[must_use]
pub fn maker(part: &Identity) -> &str {
    if part.vendor.is_empty() || part.vendor == VENDOR {
        return "";
    }
    registered(part.kind, &part.vendor)
}

/// What a part is called: the catalogue's words where a listing names it,
/// the hardware's own where none does, and its kind where the descriptor
/// named nothing at all.
#[must_use]
pub fn name(part: &Identity, sold: Option<Catalogue>) -> &str {
    match sold {
        Some(sold) => sold.model,
        None if !part.model.is_empty() => &part.model,
        None => kind_label(part.kind),
    }
}

/// The number a part announced for itself: the one it gives apart from its
/// model, and the model where `sold` means a listing's name is standing in
/// the model's place. Empty where the model is the number and is already on
/// screen as itself.
#[must_use]
pub fn part_number(part: &Identity, sold: Option<Catalogue>) -> &str {
    match (part.part_number.as_str(), sold) {
        ("", Some(_)) => &part.model,
        ("", None) => "",
        (number, _) => number,
    }
}

/// The maker behind an id a registry assigns — a display's three-letter PNP
/// id, a drive's PCI vendor id — curated from the ids seen on real hardware:
/// the registries are not something this can carry, so an id with no entry
/// is left as the part gave it. The other kinds name their maker outright.
fn registered(kind: PartKind, id: &str) -> &str {
    match (kind, id) {
        (PartKind::Display, "CSW") => "CSOT",
        (PartKind::Storage, "15b7") => "SanDisk",
        _ => id,
    }
}

#[cfg(test)]
mod tests {
    use frameguin_wire::{self as wire, Identity, PartKind, VENDOR};

    use super::{catalogue, inventory, maker, name, ordered, part_number};

    fn part(kind: PartKind, id: &str) -> Identity {
        Identity {
            kind,
            vendor: String::new(),
            model: String::new(),
            part_number: String::new(),
            serial: String::new(),
            size_bytes: 0,
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
    fn a_kind_the_machine_holds_twice_is_numbered_and_a_lone_kind_is_not() {
        let parts = [
            part(PartKind::Memory, "dmi-slot:LPCAMM2_1"),
            part(PartKind::Mainboard, "dmi-board:FRANMJCP07"),
            part(PartKind::Memory, "dmi-slot:LPCAMM2_0"),
        ];
        let listed: Vec<String> = inventory(&parts)
            .into_iter()
            .map(|(_, title)| title)
            .collect();
        assert_eq!(listed, ["Mainboard", "Memory #1", "Memory #2"]);
    }

    #[test]
    fn a_part_the_catalogue_does_not_name_keeps_its_own_words() {
        assert!(catalogue(&part(PartKind::Memory, "dmi-slot:LPCAMM2_0")).is_none());
        assert!(catalogue(&part(PartKind::Touchpad, "hid:093a:1343")).is_some());
    }

    /// A module is the same part in whichever slot it sits, and a slot holds
    /// whichever module was fitted, so the slot cannot be the key.
    #[test]
    fn a_memory_module_is_catalogued_by_its_part_number_not_its_slot() {
        assert!(catalogue(&module("dmi-slot:LPCAMM2_1")).is_some());
    }

    #[test]
    fn a_pack_is_catalogued_by_its_model_number_not_the_identifier_carrying_it() {
        let pack = Identity {
            model: "FRANEDA".to_owned(),
            ..part(PartKind::Battery, "sbs:FRANEDA")
        };
        assert!(catalogue(&pack).is_some());
    }

    #[test]
    fn a_board_is_catalogued_by_the_machine_its_firmware_names() {
        let sold = catalogue(&board(wire::BOARD_LAPTOP13_AMD_7040_UNSPACED)).unwrap();
        assert_eq!(sold.model, "Laptop 13 Mainboard");
        assert_eq!(sold.variant, Some("AMD Ryzen 7040 Series"));
    }

    #[test]
    fn a_board_framework_no_longer_sells_is_named_without_a_link() {
        let sold = catalogue(&board(wire::BOARD_LAPTOP13_12TH_GEN)).unwrap();
        assert_eq!(sold.model, "Laptop 13 Mainboard");
        assert!(sold.url.is_none());
    }

    #[test]
    fn a_board_of_no_known_machine_keeps_its_own_words() {
        assert!(catalogue(&board("Precision 5560")).is_none());
    }

    #[test]
    fn only_a_part_of_another_make_keeps_its_makers_words() {
        assert_eq!(maker(&module("dmi-slot:LPCAMM2_0")), "Micron Technology");
        let board = Identity {
            vendor: VENDOR.to_owned(),
            ..part(PartKind::Mainboard, "dmi-board:FRANMJCP07")
        };
        assert_eq!(maker(&board), "");
        assert_eq!(maker(&part(PartKind::Touchpad, "hid:093a:1343")), "");
    }

    #[test]
    fn a_drive_is_made_by_whoever_holds_its_pci_vendor_id() {
        let drive = |vendor: &str| Identity {
            vendor: vendor.to_owned(),
            ..part(PartKind::Storage, "pci:15b7:5045")
        };
        assert_eq!(maker(&drive("15b7")), "SanDisk");
        assert_eq!(maker(&drive("1e0f")), "1e0f");
    }

    #[test]
    fn a_listed_part_is_named_by_its_listing_and_numbered_by_its_own_model() {
        let board = board(wire::BOARD_LAPTOP13_PRO_ULTRA_3);
        let sold = catalogue(&board);
        assert_eq!(name(&board, sold), "Laptop 13 Pro Mainboard");
        assert_eq!(part_number(&board, sold), wire::BOARD_LAPTOP13_PRO_ULTRA_3);
    }

    #[test]
    fn a_part_that_named_nothing_falls_back_to_its_kind() {
        assert_eq!(name(&part(PartKind::Display, "drm:eDP-1"), None), "Display");
    }

    #[test]
    fn a_board_shows_its_own_number_where_every_other_part_shows_its_model() {
        let board = Identity {
            part_number: "FRANMJCP07".to_owned(),
            ..board(wire::BOARD_LAPTOP13_PRO_ULTRA_3)
        };
        let sold = catalogue(&board);
        assert_eq!(part_number(&board, sold), "FRANMJCP07");
        let module = module("dmi-slot:LPCAMM2_0");
        assert_eq!(
            part_number(&module, catalogue(&module)),
            "MTD16C20325N4FN023F1 YF"
        );
        assert_eq!(part_number(&module, None), "");
    }
}
