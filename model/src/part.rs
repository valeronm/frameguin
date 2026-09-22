//! The words for a part: what its kind is called, the order a bill of
//! materials lists them in, what it announced about itself, the listing it is
//! logged in, and — where the hardware's own words are not a name a person
//! would recognise — the name Framework sells it under.

use std::fmt::Write;

use frameguin_wire::{Detail, FirmwareKind, Identity, PartKind, Platform, Series, VENDOR};

use crate::control::battery::reading::{capacity, volts};
use crate::date;

#[must_use]
pub fn kind_label(kind: PartKind) -> &'static str {
    match kind {
        PartKind::Mainboard => "Mainboard",
        PartKind::Battery => "Battery",
        PartKind::Memory => "Memory",
        PartKind::Storage => "Storage",
        PartKind::Wifi => "Wi-Fi",
        PartKind::Display => "Display",
        PartKind::Camera => "Camera",
        PartKind::Touchpad => "Touchpad",
        PartKind::Fingerprint => "Fingerprint Reader",
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
        PartKind::Wifi => 3,
        PartKind::Battery => 4,
        PartKind::Display => 5,
        PartKind::Camera => 6,
        PartKind::Touchpad => 7,
        PartKind::Fingerprint => 8,
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
    /// leading it and less the `variant`.
    pub model: &'static str,
    /// What narrows a product name several parts share to this one, whether
    /// the name ends on it or the page offers it as a choice; None where the
    /// name is the whole of it.
    pub variant: Option<&'static str>,
    /// None where no live page names this part: the name is still what a
    /// reader wants and a page about another part is not.
    pub url: Option<&'static str>,
}

/// A part is keyed by the words it announces for itself where those are
/// guaranteed and its own, and by its identifier where they are not: a
/// descriptor is free to carry no strings at all, an EDID may leave out its
/// product-name descriptor, and a radio's model is the database's reading of
/// its ids. A mainboard is keyed by neither: the platform settles its
/// listing.
fn key(part: &Identity) -> (PartKind, &str) {
    let key = match part.kind {
        PartKind::Battery | PartKind::Memory | PartKind::Storage => &part.model,
        PartKind::Mainboard => "",
        PartKind::Wifi
        | PartKind::Display
        | PartKind::Camera
        | PartKind::Touchpad
        | PartKind::Fingerprint => &part.id,
    };
    (part.kind, key)
}

/// The haptic touchpad ships in no other input cover.
const PRO_INPUT_COVER_TOUCHPAD: &str = "hid:093a:1343";

fn has_pro_input_cover(parts: &[Identity]) -> bool {
    parts
        .iter()
        .any(|part| key(part) == (PartKind::Touchpad, PRO_INPUT_COVER_TOUCHPAD))
}

#[must_use]
pub fn mainboard(platform: Platform) -> Option<Catalogue> {
    match platform {
        Platform::Laptop13Gen11 => Some(varied("Laptop 13 Mainboard", "11th Gen Intel Core", None)),
        Platform::Laptop13Gen12 => Some(varied("Laptop 13 Mainboard", "12th Gen Intel Core", None)),
        Platform::Laptop13Gen13 => Some(varied("Laptop 13 Mainboard", "13th Gen Intel Core", None)),
        // A page carries the processor as a variant code the product name
        // does not map to, so a board's link is to the page unvaried.
        Platform::Laptop13Ultra1 => Some(varied(
            "Laptop 13 Mainboard",
            "Intel Core Ultra Series 1",
            Some("https://frame.work/products/mainboard-ultra-1-intel-core"),
        )),
        Platform::Laptop13Amd7040 => Some(varied(
            "Laptop 13 Mainboard",
            "AMD Ryzen 7040 Series",
            Some("https://frame.work/products/mainboard-amd-ryzen-7040-series"),
        )),
        Platform::Laptop13AmdAi300 => Some(varied(
            "Laptop 13 Mainboard",
            "AMD Ryzen AI 300 Series",
            Some("https://frame.work/products/mainboard-amd-ai300"),
        )),
        Platform::Laptop13ProUltra3 => Some(varied(
            "Laptop 13 Pro Mainboard",
            "Intel Core Ultra Series 3",
            Some("https://frame.work/products/laptop13pro-mainboard-intel-ultra-3"),
        )),
        Platform::Laptop12Gen13 => Some(varied(
            "Laptop 12 Mainboard",
            "13th Gen Intel Core",
            Some("https://frame.work/products/laptop12-mainboard-13th-gen-intel-core"),
        )),
        Platform::Laptop12Core3 => Some(varied(
            "Laptop 12 Mainboard",
            "Intel Core Series 3",
            Some("https://frame.work/products/laptop12-mainboard-series3"),
        )),
        Platform::Laptop16Amd7040 => Some(varied(
            "Laptop 16 Mainboard",
            "AMD Ryzen 7040 Series",
            Some("https://frame.work/products/16-mainboard-amd-ryzen-7040-series"),
        )),
        Platform::Laptop16AmdAi300 => Some(varied(
            "Laptop 16 Mainboard",
            "AMD Ryzen AI 300 Series",
            Some("https://frame.work/products/laptop16-mainboard-amd-ai300"),
        )),
        Platform::DesktopAmdAiMax300 => Some(varied(
            "Desktop Mainboard",
            "AMD Ryzen AI Max 300 Series",
            Some(
                "https://frame.work/products/framework-desktop-mainboard-amd-ryzen-ai-max-300-series",
            ),
        )),
        Platform::Unknown => None,
    }
}

/// The marketplace entry for a part. Curated from Framework's own listings,
/// trademark marks left off; a part with no entry is shown under its own
/// words, and an entry is never guessed from a resemblance, since a wrong
/// name reads exactly like a right one. The platform, or the `parts` it is
/// fitted beside, settles a part whose own identifier does not distinguish
/// what it is listed for.
#[must_use]
pub fn catalogue(part: &Identity, parts: &[Identity], platform: Platform) -> Option<Catalogue> {
    match key(part) {
        (PartKind::Mainboard, _) => mainboard(platform),
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
        (PartKind::Touchpad, PRO_INPUT_COVER_TOUCHPAD) => Some(listed(
            "Laptop 13 Pro Input Cover Frame",
            Some("https://frame.work/products/laptop13pro-input-cover-frame"),
        )),
        // One listing per generation, each a variant of the same page.
        (PartKind::Camera, "usb:32ac:001c") => Some(listed(
            "Webcam Module (2nd Gen)",
            Some("https://frame.work/products/webcam-module?v=FRANJB0001"),
        )),
        (PartKind::Camera, "usb:32ac:001d") => Some(listed(
            "Laptop 12 Webcam Module",
            Some("https://frame.work/products/webcam-module?v=FRAPAB0001"),
        )),
        // The sensor is Goodix's part, carrying one id in every kit. The
        // Laptop 13 Pro's input cover takes no standalone kit and fits any
        // Laptop 13 with any mainboard.
        (PartKind::Fingerprint, "usb:27c6:609c") if has_pro_input_cover(parts) => Some(listed(
            "Laptop 13 Pro Input Cover Kit",
            Some("https://frame.work/products/laptop13pro-input-cover-kit"),
        )),
        (PartKind::Fingerprint, "usb:27c6:609c") if platform.series() == Some(Series::Laptop13) => {
            Some(listed(
                "Laptop 13 Fingerprint Reader Kit",
                Some("https://frame.work/products/fingerprint-reader-kit?v=FRANTD0001"),
            ))
        }
        (PartKind::Fingerprint, "usb:27c6:609c") if platform.series() == Some(Series::Laptop16) => {
            Some(listed(
                "Laptop 16 Fingerprint Reader Kit",
                Some("https://frame.work/products/16-fingerprint-reader-kit"),
            ))
        }
        // A chipset that carries the Wi-Fi MAC itself is deliberately
        // absent: its ids name the platform, so every machine of a
        // generation would key alike whichever module is in the slot.
        (PartKind::Wifi, "pci:14c3:0616") => Some(listed(
            "AMD RZ616 Wi-Fi 6E",
            Some("https://frame.work/products/amd-rz616-wi-fi-6e"),
        )),
        (PartKind::Wifi, "pci:14c3:0717") => Some(listed(
            "AMD RZ717 Wi-Fi 7",
            Some("https://frame.work/products/amd-rz717-wi-fi-7"),
        )),
        (PartKind::Wifi, "pci:8086:2725") => Some(listed(
            "Wi-Fi 6E AX210",
            Some("https://frame.work/products/intel-wi-fi-6e-ax210"),
        )),
        (PartKind::Display, "edid:BOE:095f") => Some(varied(
            "Laptop 13 Display Kit",
            "2.2K",
            Some("https://frame.work/products/display-kit?v=FRANGX0001"),
        )),
        (PartKind::Display, "edid:BOE:0cb4") => Some(varied(
            "Laptop 13 Display Kit",
            "2.8K",
            Some("https://frame.work/products/display-kit?v=FRANJF0001"),
        )),
        // The Pro's kit is the panel and the touch controller together, and
        // the panel is the half every machine with a screen has.
        (PartKind::Display, "edid:CSW:1322") => Some(listed(
            "Laptop 13 Pro Touchscreen Display Kit - 2.8K",
            Some("https://frame.work/products/laptop13pro-display-kit"),
        )),
        (PartKind::Display, "edid:BOE:0d56") => Some(listed(
            "Laptop 12 Display Kit",
            Some("https://frame.work/products/laptop12-display-kit"),
        )),
        // Both revisions are the one listing.
        (PartKind::Display, "edid:BOE:0bc9" | "edid:BOE:0d79") => Some(listed(
            "Laptop 16 Display Kit",
            Some("https://frame.work/products/16-display-kit"),
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

/// Which build of one product a part is, in the listing's own words, where
/// the listing sells the builds under a single name and the identifier tells
/// them apart. Empty where a listing sells one build and where no listing
/// names the part at all.
#[must_use]
pub fn generation(part: &Identity, sold: Option<Catalogue>) -> &'static str {
    if sold.is_none() {
        return "";
    }
    match key(part) {
        // Framework sells the later firmware as the 2nd Gen and says what
        // it adds is G-Sync, which is the adaptive-sync data block one of
        // the two panels carries and the other does not.
        (PartKind::Display, "edid:BOE:0bc9") => "1st Gen",
        (PartKind::Display, "edid:BOE:0d79") => "2nd Gen",
        _ => "",
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
/// screen as itself. Whose words a model is decides this and not which kinds
/// [`key`] takes an identifier for.
#[must_use]
pub fn part_number(part: &Identity, sold: Option<Catalogue>) -> &str {
    match (part.part_number.as_str(), sold) {
        // A radio's model is the database's reading of its ids, so a listing
        // taking the model's place displaces nothing the part announced.
        ("", Some(_)) if part.kind == PartKind::Wifi => "",
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
        (PartKind::Wifi, "14c3") => Some("MediaTek"),
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

/// A drive is sold in decimal units.
pub(crate) fn storage_capacity(bytes: u64) -> String {
    scaled(bytes, 1000.0)
}

/// Only a decimal is trimmed: a whole number's trailing zeros are its value.
pub(crate) fn trimmed(mut spelled: String) -> String {
    if spelled.contains('.') {
        spelled.truncate(spelled.trim_end_matches('0').trim_end_matches('.').len());
    }
    spelled
}

/// Carries a pixel density row no detail holds, derived where a panel stated
/// both its resolution and its size. The rest are the details in the order the
/// part stated them.
#[must_use]
pub fn detail_rows(details: &[Detail]) -> Vec<(&'static str, String)> {
    let mut rows = Vec::with_capacity(details.len());
    for detail in details {
        rows.push(detail_row(detail));
        if let Detail::PanelSize { .. } = detail
            && let Some(density) = density(details)
        {
            rows.push(("Pixel density", density));
        }
    }
    rows
}

fn detail_row(detail: &Detail) -> (&'static str, String) {
    match detail {
        Detail::MemoryCapacity(bytes) => ("Capacity", scaled(*bytes, 1024.0)),
        Detail::StorageCapacity(bytes) => ("Capacity", storage_capacity(*bytes)),
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
        Detail::MacAddress(address) => ("MAC address", address.clone()),
        Detail::Sku(sku) => ("SKU", sku.clone()),
    }
}

/// Named for what carries the firmware, as the part's user would.
#[must_use]
pub fn firmware_name(kind: FirmwareKind) -> String {
    match kind {
        FirmwareKind::Bios => "BIOS".to_owned(),
        FirmwareKind::Ec => "EC".to_owned(),
        FirmwareKind::PowerDelivery(controller) => format!("PD {controller}"),
        FirmwareKind::Own => "Firmware".to_owned(),
        FirmwareKind::TouchController => "Controller".to_owned(),
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
    let (across, down) = (f64::from(width), f64::from(height));
    trimmed(format!("{:.1}", inches(across.hypot(down))))
}

fn density(details: &[Detail]) -> Option<String> {
    let (across, down) = details.iter().find_map(|detail| match detail {
        Detail::Resolution { across, down } => Some((*across, *down)),
        _ => None,
    })?;
    let (width, height) = details.iter().find_map(|detail| match detail {
        Detail::PanelSize { across, down } => Some((*across, *down)),
        _ => None,
    })?;
    let pixels = f64::from(across).hypot(f64::from(down));
    let diagonal = inches(f64::from(width).hypot(f64::from(height)));
    Some(format!("{:.0} ppi", pixels / diagonal))
}

fn inches(millimetres: f64) -> f64 {
    const MILLIMETRES_PER_INCH: f64 = 25.4;
    millimetres / MILLIMETRES_PER_INCH
}

/// A detail naming this one machine rather than its model.
fn names_the_machine(detail: &Detail) -> bool {
    // No catch-all, so a detail added cannot reach the listing without a
    // decision about it.
    match detail {
        Detail::MacAddress(_) => true,
        Detail::MemoryCapacity(_)
        | Detail::StorageCapacity(_)
        | Detail::DesignCapacity { .. }
        | Detail::NominalVoltage(_)
        | Detail::ManufactureDate(_)
        | Detail::Resolution { .. }
        | Detail::PanelSize { .. }
        | Detail::ColourDepth(_)
        | Detail::RefreshRate { .. }
        | Detail::ManufactureYear(_)
        | Detail::ModelYear(_)
        | Detail::MemoryType(_)
        | Detail::FormFactor(_)
        | Detail::Speed(_)
        | Detail::ConfiguredSpeed(_)
        | Detail::Sku(_) => false,
    }
}

/// The machine's parts as the daemon's journal and the app's debug report
/// both print them, so a bug report and the log it is read against spell a
/// part the same. The serial and any detail naming the machine are left out,
/// a report being pasted into a public issue.
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
        let shareable: Vec<_> = details
            .iter()
            .filter(|detail| !names_the_machine(detail))
            .cloned()
            .collect();
        let details = detail_rows(&shareable);
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
                (firmware_name(firmware.kind), stamped.join(" "))
            })
            .collect();
        write_rows(&mut out, 4, &rows);
        write_section(&mut out, "details", &details);
        write_section(&mut out, "firmware", &firmware);
    }
    out
}

fn write_section(out: &mut String, title: &str, rows: &[(impl AsRef<str>, String)]) {
    if rows.is_empty() {
        return;
    }
    let _ = writeln!(out, "    {title}:");
    write_rows(out, 6, rows);
}

fn write_rows(out: &mut String, indent: usize, rows: &[(impl AsRef<str>, String)]) {
    let width = rows
        .iter()
        .map(|(title, _)| title.as_ref().chars().count())
        .max()
        .unwrap_or(0);
    for (title, value) in rows {
        let _ = writeln!(
            out,
            "{:indent$}{:<pad$}{value}",
            "",
            format!("{}:", title.as_ref()),
            pad = width + 3
        );
    }
}

#[cfg(test)]
mod tests {
    use frameguin_wire::{Detail, Firmware, FirmwareKind, Identity, PartKind, Platform, VENDOR};

    use super::{
        aspect, catalogue, detail_row, detail_rows, firmware_name, generation, inventory, listing,
        maker, name, ordered, part_number,
    };

    /// The machine the part fixtures are read from.
    const fn here() -> Platform {
        Platform::Laptop13ProUltra3
    }

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

    fn board() -> Identity {
        part(PartKind::Mainboard, "dmi-board:FRANMJCP07")
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
        assert!(catalogue(&part(PartKind::Memory, "dmi-slot:LPCAMM2_0"), &[], here()).is_none());
        assert!(catalogue(&part(PartKind::Touchpad, "hid:093a:1343"), &[], here()).is_some());
    }

    #[test]
    fn each_webcam_generation_is_its_own_listing() {
        let second = catalogue(&part(PartKind::Camera, "usb:32ac:001c"), &[], here()).unwrap();
        let twelve = catalogue(&part(PartKind::Camera, "usb:32ac:001d"), &[], here()).unwrap();
        assert_eq!(second.model, "Webcam Module (2nd Gen)");
        assert_eq!(twelve.model, "Laptop 12 Webcam Module");
        assert_ne!(second.url, twelve.url);
    }

    #[test]
    fn a_panel_is_catalogued_by_its_ids_and_not_by_words_an_edid_may_omit() {
        let pro = catalogue(&part(PartKind::Display, "edid:CSW:1322"), &[], here()).unwrap();
        assert_eq!(pro.model, "Laptop 13 Pro Touchscreen Display Kit - 2.8K");
        let twelve = catalogue(&part(PartKind::Display, "edid:BOE:0d56"), &[], here()).unwrap();
        assert_eq!(twelve.model, "Laptop 12 Display Kit");
    }

    #[test]
    fn each_resolution_of_the_laptop_13_kit_is_its_own_variant() {
        let lesser = catalogue(&part(PartKind::Display, "edid:BOE:095f"), &[], here()).unwrap();
        let greater = catalogue(&part(PartKind::Display, "edid:BOE:0cb4"), &[], here()).unwrap();
        assert_eq!(lesser.model, "Laptop 13 Display Kit");
        assert_eq!(lesser.model, greater.model);
        assert_eq!(lesser.variant, Some("2.2K"));
        assert_eq!(greater.variant, Some("2.8K"));
        assert_ne!(lesser.url, greater.url);
    }

    #[test]
    fn the_laptop_16_panel_s_two_revisions_are_the_one_kit() {
        let first = catalogue(&part(PartKind::Display, "edid:BOE:0bc9"), &[], here()).unwrap();
        let second = catalogue(&part(PartKind::Display, "edid:BOE:0d79"), &[], here()).unwrap();
        assert_eq!(first.model, "Laptop 16 Display Kit");
        assert_eq!(first, second);
    }

    #[test]
    fn a_revision_the_listing_does_not_name_is_a_generation_of_its_own() {
        let named = |id| {
            let panel = part(PartKind::Display, id);
            generation(&panel, catalogue(&panel, &[], here()))
        };
        assert_eq!(named("edid:BOE:0bc9"), "1st Gen");
        assert_eq!(named("edid:BOE:0d79"), "2nd Gen");
        assert_eq!(named("edid:BOE:0d56"), "");
    }

    #[test]
    fn a_part_no_listing_names_has_no_generation_of_one() {
        let panel = part(PartKind::Display, "edid:BOE:0bc9");
        assert_eq!(generation(&panel, None), "");
    }

    #[test]
    fn each_discrete_radio_is_its_own_listing_and_a_chipset_s_is_none() {
        let six = catalogue(&part(PartKind::Wifi, "pci:14c3:0616"), &[], here()).unwrap();
        let seven = catalogue(&part(PartKind::Wifi, "pci:14c3:0717"), &[], here()).unwrap();
        let intel = catalogue(&part(PartKind::Wifi, "pci:8086:2725"), &[], here()).unwrap();
        assert_eq!(six.model, "AMD RZ616 Wi-Fi 6E");
        assert_eq!(seven.model, "AMD RZ717 Wi-Fi 7");
        assert_eq!(intel.model, "Wi-Fi 6E AX210");
        assert_ne!(six.url, seven.url);
        assert_ne!(seven.url, intel.url);
        assert_ne!(intel.url, six.url);
        assert!(catalogue(&part(PartKind::Wifi, "pci:8086:e440"), &[], here()).is_none());
    }

    #[test]
    fn one_reader_is_two_kits_and_the_board_says_which() {
        let reader = part(PartKind::Fingerprint, "usb:27c6:609c");
        let thirteen = catalogue(&reader, &[], Platform::Laptop13Gen13).unwrap();
        let sixteen = catalogue(&reader, &[], Platform::Laptop16AmdAi300).unwrap();
        assert_eq!(thirteen.model, "Laptop 13 Fingerprint Reader Kit");
        assert_eq!(sixteen.model, "Laptop 16 Fingerprint Reader Kit");
        assert_ne!(thirteen.url, sixteen.url);
    }

    #[test]
    fn a_reader_beside_the_pro_touchpad_is_the_pro_input_cover_kit_on_any_laptop_13() {
        let reader = part(PartKind::Fingerprint, "usb:27c6:609c");
        let pro_cover = [part(PartKind::Touchpad, "hid:093a:1343"), reader.clone()];
        for platform in [here(), Platform::Laptop13Gen13] {
            let sold = catalogue(&reader, &pro_cover, platform).unwrap();
            assert_eq!(sold.model, "Laptop 13 Pro Input Cover Kit");
        }
    }

    #[test]
    fn the_same_reader_in_a_machine_of_no_known_kit_is_no_listing() {
        let reader = part(PartKind::Fingerprint, "usb:27c6:609c");
        assert!(catalogue(&reader, &[], Platform::Laptop12Core3).is_none());
        assert!(catalogue(&reader, &[], Platform::Unknown).is_none());
    }

    #[test]
    fn a_pack_is_catalogued_by_its_model_number_not_the_identifier_carrying_it() {
        let pack = Identity {
            model: "FRANEDA".to_owned(),
            ..part(PartKind::Battery, "sbs:FRANEDA")
        };
        assert!(catalogue(&pack, &[], here()).is_some());
    }

    #[test]
    fn a_board_is_catalogued_by_the_platform_and_not_by_its_own_identity() {
        let sold = catalogue(&board(), &[], Platform::Laptop13Amd7040).unwrap();
        assert_eq!(sold.model, "Laptop 13 Mainboard");
        assert_eq!(sold.variant, Some("AMD Ryzen 7040 Series"));
    }

    #[test]
    fn a_board_framework_no_longer_sells_is_named_without_a_link() {
        let sold = catalogue(&board(), &[], Platform::Laptop13Gen12).unwrap();
        assert_eq!(sold.model, "Laptop 13 Mainboard");
        assert!(sold.url.is_none());
    }

    #[test]
    fn a_board_of_no_known_machine_keeps_its_own_words() {
        assert!(catalogue(&board(), &[], Platform::Unknown).is_none());
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
        let board = Identity {
            model: "Laptop 13 Pro (Intel Core Ultra Series 3)".to_owned(),
            ..board()
        };
        let sold = catalogue(&board, &[], here());
        assert_eq!(name(&board, sold), "Laptop 13 Pro Mainboard");
        assert_eq!(
            part_number(&board, sold),
            "Laptop 13 Pro (Intel Core Ultra Series 3)"
        );
    }

    #[test]
    fn a_part_that_named_nothing_falls_back_to_its_kind() {
        assert_eq!(name(&part(PartKind::Display, "drm:eDP-1"), None), "Display");
    }

    #[test]
    fn a_board_shows_its_own_number_where_every_other_part_shows_its_model() {
        let board = Identity {
            part_number: "FRANMJCP07".to_owned(),
            ..board()
        };
        let sold = catalogue(&board, &[], here());
        assert_eq!(part_number(&board, sold), "FRANMJCP07");
        let pack = Identity {
            model: "FRANEDA".to_owned(),
            ..part(PartKind::Battery, "sbs:FRANEDA")
        };
        assert_eq!(part_number(&pack, catalogue(&pack, &[], here())), "FRANEDA");
        assert_eq!(part_number(&pack, None), "");
    }

    #[test]
    fn a_listed_radio_shows_no_number_where_its_model_is_the_database_s() {
        let radio = Identity {
            model: "MT7925 (RZ717) Wi-Fi 7 160MHz".to_owned(),
            ..part(PartKind::Wifi, "pci:14c3:0717")
        };
        assert_eq!(part_number(&radio, catalogue(&radio, &[], here())), "");
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
    fn a_panel_stating_pixels_and_millimetres_is_measured_in_both() {
        let panel = [
            Detail::Resolution {
                across: 2880,
                down: 1920,
            },
            Detail::PanelSize {
                across: 285,
                down: 190,
            },
            Detail::ColourDepth(10),
        ];
        let rows = detail_rows(&panel);
        assert_eq!(rows[2], ("Pixel density", "257 ppi".to_owned()));
        assert_eq!(rows[3].0, "Colour depth");
    }

    #[test]
    fn a_panel_stating_only_its_millimetres_carries_no_density() {
        let unresolved = [Detail::PanelSize {
            across: 285,
            down: 190,
        }];
        assert_eq!(detail_rows(&unresolved).len(), 1);
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
                ..Firmware::new(FirmwareKind::Own, "7612M000")
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
    fn a_listing_leaves_out_a_mac_address() {
        let radio = Identity {
            details: vec![
                Detail::MacAddress("e0:c9:32:00:00:00".to_owned()),
                Detail::ModelYear(2025),
            ],
            ..part(PartKind::Wifi, "pci:8086:e440")
        };
        assert_eq!(
            listing(&[radio]),
            "parts:\n\
             \x20 Wi-Fi  pci:8086:e440\n\
             \x20   details:\n\
             \x20     Model year:  2025\n"
        );
    }

    #[test]
    fn a_pd_controller_is_named_by_its_number() {
        assert_eq!(firmware_name(FirmwareKind::PowerDelivery(2)), "PD 2");
    }

    #[test]
    fn a_machine_with_no_parts_lists_none() {
        assert_eq!(listing(&[]), "parts: none\n");
    }
}
