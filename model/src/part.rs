//! The words for a part: what its kind is called, the order a bill of
//! materials lists them in, what it announced about itself, the listing it is
//! logged in, and — where the hardware's own words are not a name a person
//! would recognise — the name Framework sells it under.

use std::fmt::Write;

use frameguin_wire::{self as wire, Detail, Identity, PartKind, VENDOR};

use crate::control::battery::reading::{capacity, volts};
use crate::date;

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
        // Memory is deliberately absent: the listing's capacity variants
        // carry codes nothing on a module maps to, so a listing names a
        // module no better than it links one.
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
    match registered(part.kind, &part.vendor) {
        Some(curated) => curated,
        None if !part.vendor_name.is_empty() => &part.vendor_name,
        None => &part.vendor,
    }
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

/// A registry spells a maker its own way: `Sandisk Corp` where the drive's
/// own label says `SanDisk`.
fn registered(kind: PartKind, id: &str) -> Option<&'static str> {
    match (kind, id) {
        (PartKind::Display, "CSW") => Some("CSOT"),
        (PartKind::Storage, "15b7") => Some("SanDisk"),
        (PartKind::Touchpad, "093a") => Some("PixArt"),
        _ => None,
    }
}

/// Binary units are spelled GB and TB too, as a module is labelled.
#[expect(
    clippy::cast_precision_loss,
    reason = "three significant figures ask far less than an f64 carries"
)]
fn scaled(bytes: u64, base: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= base && unit < UNITS.len() - 1 {
        value /= base;
        unit += 1;
    }
    let decimals = match value {
        v if v >= 100.0 => 0,
        v if v >= 10.0 => 1,
        _ => 2,
    };
    format!("{} {}", trimmed(format!("{value:.decimals$}")), UNITS[unit])
}

/// Only a decimal is trimmed: a whole number's trailing zeros are its value.
fn trimmed(mut spelled: String) -> String {
    if spelled.contains('.') {
        spelled.truncate(spelled.trim_end_matches('0').trim_end_matches('.').len());
    }
    spelled
}

#[must_use]
pub fn detail_row(detail: &Detail) -> (&'static str, String) {
    match detail {
        Detail::MemoryCapacity(bytes) => ("Capacity", scaled(*bytes, 1024.0)),
        Detail::StorageCapacity(bytes) => ("Capacity", scaled(*bytes, 1000.0)),
        Detail::DesignCapacity {
            milliamp_hours,
            millivolts,
        } => ("Design capacity", capacity(*milliamp_hours, *millivolts)),
        Detail::NominalVoltage(millivolts) => ("Nominal voltage", volts(*millivolts)),
        Detail::ManufactureDate(date) => ("Manufactured", date::spelled(date)),
        Detail::Resolution { across, down } => (
            "Resolution",
            format!("{across} × {down} ({})", aspect(*across, *down)),
        ),
        Detail::PanelSize { across, down } => (
            "Size",
            format!("{} inches ({across} × {down} mm)", diagonal(*across, *down)),
        ),
        Detail::ColourDepth(bits) => ("Colour depth", format!("{bits} bits per colour")),
        Detail::RefreshRate { slowest, fastest } if slowest == fastest => {
            ("Refresh rate", format!("{fastest} Hz"))
        }
        Detail::RefreshRate { slowest, fastest } => {
            ("Refresh rate", format!("{slowest}–{fastest} Hz"))
        }
        Detail::ManufactureYear(year) => ("Manufactured", year.to_string()),
        Detail::ModelYear(year) => ("Model year", year.to_string()),
        Detail::MemoryType(name) => ("Type", name.clone()),
        Detail::FormFactor(name) => ("Form factor", name.clone()),
        Detail::Speed(rate) => ("Speed", format!("{rate} MT/s")),
        Detail::ConfiguredSpeed(rate) => ("Configured speed", format!("{rate} MT/s")),
    }
}

/// A panel is sold as a ratio it need not exactly have.
fn aspect(across: u16, down: u16) -> String {
    const NAMED: [((u16, u16), &str); 6] = [
        ((3, 2), "3:2"),
        ((4, 3), "4:3"),
        ((5, 4), "5:4"),
        ((16, 9), "16:9"),
        ((32, 9), "32:9"),
        ((8, 5), "16:10"),
    ];
    let divisor = gcd(across, down);
    let reduced = (across / divisor, down / divisor);
    NAMED.iter().find(|(pair, _)| *pair == reduced).map_or_else(
        || format!("{:.2}:1", f64::from(across) / f64::from(down)),
        |(_, name)| (*name).to_owned(),
    )
}

fn gcd(mut a: u16, mut b: u16) -> u16 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// A panel is sold by the inches of its diagonal.
fn diagonal(width: u16, height: u16) -> String {
    const MILLIMETRES_PER_INCH: f64 = 25.4;
    let (across, down) = (f64::from(width), f64::from(height));
    let inches = across.hypot(down) / MILLIMETRES_PER_INCH;
    trimmed(format!("{inches:.1}"))
}

/// The machine's parts as the daemon's journal and the app's debug report
/// both print them, so a bug report and the log it is read against spell a
/// part the same. The serial is left out, a report being pasted into a public
/// issue.
#[must_use]
pub fn listing(parts: &[Identity]) -> String {
    if parts.is_empty() {
        return "parts: none\n".to_owned();
    }
    let mut out = String::from("parts:\n");
    for part in parts {
        // Whole, so a field added to the identity cannot reach the listing
        // without a decision about it.
        let Identity {
            kind,
            vendor,
            vendor_name,
            model,
            part_number,
            serial: _,
            id,
            firmware,
            details,
        } = part;
        let _ = writeln!(out, "  {}  {id}", kind_label(*kind));
        let vendor = if vendor_name.is_empty() {
            vendor.clone()
        } else {
            format!("{vendor} ({vendor_name})")
        };
        let rows: Vec<_> = [
            ("vendor", vendor),
            ("model", model.clone()),
            ("part number", part_number.clone()),
        ]
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .collect();
        let details: Vec<_> = details.iter().map(detail_row).collect();
        let firmware: Vec<_> = firmware
            .iter()
            .map(|firmware| {
                let stamped: Vec<_> = [
                    firmware.version.clone(),
                    date::spelled(&firmware.built),
                    firmware.builder.clone(),
                ]
                .into_iter()
                .filter(|field| !field.is_empty())
                .collect();
                (firmware.name.as_str(), stamped.join(" "))
            })
            .collect();
        write_rows(&mut out, 4, &rows);
        for (title, rows) in [("details", details), ("firmware", firmware)] {
            if rows.is_empty() {
                continue;
            }
            let _ = writeln!(out, "    {title}:");
            write_rows(&mut out, 6, &rows);
        }
    }
    out
}

fn write_rows(out: &mut String, indent: usize, rows: &[(&str, String)]) {
    let width = rows
        .iter()
        .map(|(title, _)| title.chars().count())
        .max()
        .unwrap_or(0);
    for (title, value) in rows {
        let _ = writeln!(
            out,
            "{:indent$}{:<pad$}{value}",
            "",
            format!("{title}:"),
            pad = width + 3
        );
    }
}

#[cfg(test)]
mod tests {
    use frameguin_wire::{self as wire, Detail, Firmware, Identity, PartKind, VENDOR};

    use super::{
        aspect, catalogue, detail_row, inventory, listing, maker, name, ordered, part_number,
    };

    fn part(kind: PartKind, id: &str) -> Identity {
        Identity {
            kind,
            vendor: String::new(),
            vendor_name: String::new(),
            model: String::new(),
            part_number: String::new(),
            serial: String::new(),
            id: id.to_owned(),
            firmware: Vec::new(),
            details: Vec::new(),
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
        assert_eq!(maker(&part(PartKind::Display, "edid:CSW:1322")), "");
    }

    #[test]
    fn a_curated_id_outranks_the_name_the_hardware_resolved() {
        let drive = Identity {
            vendor: "15b7".to_owned(),
            vendor_name: "Sandisk Corp".to_owned(),
            ..part(PartKind::Storage, "pci:15b7:5045")
        };
        assert_eq!(maker(&drive), "SanDisk");
    }

    #[test]
    fn an_uncurated_id_takes_the_resolved_name_and_then_itself() {
        let drive = |vendor_name: &str| Identity {
            vendor: "1e0f".to_owned(),
            vendor_name: vendor_name.to_owned(),
            ..part(PartKind::Storage, "pci:1e0f:0001")
        };
        assert_eq!(maker(&drive("KIOXIA Corporation")), "KIOXIA Corporation");
        assert_eq!(maker(&drive("")), "1e0f");
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
        let pack = Identity {
            model: "FRANEDA".to_owned(),
            ..part(PartKind::Battery, "sbs:FRANEDA")
        };
        assert_eq!(part_number(&pack, catalogue(&pack)), "FRANEDA");
        assert_eq!(part_number(&pack, None), "");
    }

    #[test]
    fn each_capacity_is_spelled_in_the_units_it_is_sold_in() {
        let spelled = |detail| detail_row(&detail).1;
        assert_eq!(spelled(Detail::MemoryCapacity(32 << 30)), "32 GB");
        assert_eq!(
            spelled(Detail::StorageCapacity(1_024_209_543_168)),
            "1.02 TB"
        );
        assert_eq!(spelled(Detail::StorageCapacity(512_110_190_592)), "512 GB");
        assert_eq!(spelled(Detail::StorageCapacity(2_000_398_934_016)), "2 TB");
    }

    #[test]
    fn a_ratio_the_trade_names_is_named_and_one_it_does_not_is_a_decimal() {
        assert_eq!(aspect(2880, 1920), "3:2");
        assert_eq!(aspect(1920, 1080), "16:9");
        assert_eq!(aspect(1024, 768), "4:3");
        assert_eq!(aspect(1280, 1024), "5:4");
        assert_eq!(aspect(3840, 1080), "32:9");
        assert_eq!(aspect(2560, 1600), "16:10");
        assert_eq!(aspect(1920, 1200), "16:10");
        assert_eq!(aspect(1366, 768), "1.78:1");
        assert_eq!(aspect(2560, 1080), "2.37:1");
    }

    #[test]
    fn a_panel_is_spelled_with_its_ratio_and_its_inches() {
        let resolution = Detail::Resolution {
            across: 2880,
            down: 1920,
        };
        assert_eq!(
            detail_row(&resolution),
            ("Resolution", "2880 × 1920 (3:2)".to_owned())
        );
        let size = Detail::PanelSize {
            across: 285,
            down: 190,
        };
        assert_eq!(
            detail_row(&size),
            ("Size", "13.5 inches (285 × 190 mm)".to_owned())
        );
    }

    #[test]
    fn a_panel_of_whole_inches_is_named_without_a_trailing_zero() {
        let sixteen = Detail::PanelSize {
            across: 345,
            down: 215,
        };
        assert_eq!(detail_row(&sixteen).1, "16 inches (345 × 215 mm)");
    }

    #[test]
    fn a_panel_of_one_rate_names_it_once() {
        let fixed = Detail::RefreshRate {
            slowest: 60,
            fastest: 60,
        };
        assert_eq!(detail_row(&fixed).1, "60 Hz");
        let variable = Detail::RefreshRate {
            slowest: 30,
            fastest: 120,
        };
        assert_eq!(detail_row(&variable).1, "30–120 Hz");
    }

    #[test]
    fn a_year_is_labelled_by_which_year_it_is() {
        assert_eq!(
            detail_row(&Detail::ManufactureYear(2025)),
            ("Manufactured", "2025".to_owned())
        );
        assert_eq!(
            detail_row(&Detail::ModelYear(2025)),
            ("Model year", "2025".to_owned())
        );
    }

    #[test]
    fn a_pack_is_rated_in_energy_and_dated_as_a_reader_says_it() {
        let design = Detail::DesignCapacity {
            milliamp_hours: 4_800,
            millivolts: 15_400,
        };
        assert_eq!(
            detail_row(&design),
            ("Design capacity", "73.9 Wh (4800 mAh)".to_owned())
        );
        assert_eq!(
            detail_row(&Detail::NominalVoltage(15_400)),
            ("Nominal voltage", "15.40 V".to_owned())
        );
        assert_eq!(
            detail_row(&Detail::ManufactureDate("2025-03-14".to_owned())),
            ("Manufactured", "14 March 2025".to_owned())
        );
    }

    #[test]
    fn a_listing_nests_each_part_s_facts_under_it_in_aligned_columns() {
        let module = Identity {
            vendor: "Micron Technology".to_owned(),
            model: "MTD16C20325N4FN023F1 YF".to_owned(),
            serial: "01234567".to_owned(),
            details: vec![
                Detail::MemoryCapacity(32 << 30),
                Detail::ConfiguredSpeed(7467),
            ],
            ..part(PartKind::Memory, "dmi-slot:LPCAMM2_0")
        };
        let drive = Identity {
            vendor: "15b7".to_owned(),
            vendor_name: "Sandisk Corp".to_owned(),
            model: "WD_BLACK SN7100".to_owned(),
            part_number: "SD PC SN7100S SDFPNSL-1T00".to_owned(),
            firmware: vec![Firmware {
                built: "2025-01-02".to_owned(),
                ..Firmware::new("Firmware", "7612M000")
            }],
            ..part(PartKind::Storage, "pci:15b7:5045")
        };
        assert_eq!(
            listing(&[module, drive]),
            "parts:\n\
             \x20 Memory  dmi-slot:LPCAMM2_0\n\
             \x20   vendor:  Micron Technology\n\
             \x20   model:   MTD16C20325N4FN023F1 YF\n\
             \x20   details:\n\
             \x20     Capacity:          32 GB\n\
             \x20     Configured speed:  7467 MT/s\n\
             \x20 Storage  pci:15b7:5045\n\
             \x20   vendor:       15b7 (Sandisk Corp)\n\
             \x20   model:        WD_BLACK SN7100\n\
             \x20   part number:  SD PC SN7100S SDFPNSL-1T00\n\
             \x20   firmware:\n\
             \x20     Firmware:  7612M000 2 January 2025\n"
        );
    }

    #[test]
    fn a_machine_with_no_parts_lists_none() {
        assert_eq!(listing(&[]), "parts: none\n");
    }
}
