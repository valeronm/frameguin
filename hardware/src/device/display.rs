//! The panel the board drives, as its EDID describes it: a device that is a
//! part and no control, read for the bill of materials alone.

use crate::drm;
use crate::edid::{self, Edid, Year};
use crate::part::{self, Detail, Firmware, Identity, Part};
use crate::udev;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Display {
    identity: Identity,
}

impl Part for Display {
    fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl Display {
    /// Every panel the board drives, in connector order. The touch
    /// controller's version joins the one panel's firmware and is dropped
    /// where a machine has more, nothing saying which panel it sits in
    /// front of.
    pub(crate) fn detect(controller: Option<Firmware>) -> Vec<Self> {
        let mut panels: Vec<Self> = drm::panels()
            .iter()
            .filter_map(|block| edid::parse(block))
            .map(|edid| {
                let vendor = udev::acpi_vendor(&edid.manufacturer).unwrap_or_default();
                Self::of_edid(&edid, &vendor)
            })
            .collect();
        if let ([panel], Some(controller)) = (panels.as_mut_slice(), controller) {
            panel.identity.firmware.push(controller);
        }
        panels
    }

    /// The vendor is the EDID's PNP id, which names nobody in words.
    fn of_edid(edid: &Edid, vendor_name: &str) -> Self {
        Self {
            identity: Identity {
                details: details(edid),
                ..part::edid(
                    &edid.manufacturer,
                    vendor_name,
                    edid.product,
                    &edid.name,
                    &edid.serial,
                )
            },
        }
    }
}

fn details(edid: &Edid) -> Vec<Detail> {
    let (dated, year) = match edid.year {
        Some(Year::Model(year)) => ("Model year", Some(year)),
        Some(Year::Manufacture(year)) => ("Manufactured", Some(year)),
        None => ("Manufactured", None),
    };
    part::details([
        (
            "Resolution",
            edid.resolution
                .map(|(across, down)| format!("{across} × {down} ({})", aspect(across, down))),
        ),
        (
            "Size",
            edid.size.map(|(across, down)| {
                format!("{} inches ({across} × {down} mm)", diagonal(across, down))
            }),
        ),
        (
            "Colour depth",
            edid.depth.map(|bits| format!("{bits} bits per colour")),
        ),
        ("Refresh rate", edid.refresh.map(spelled_rate)),
        (dated, year.map(|year| year.to_string())),
    ])
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

/// The inches a panel is sold by, from the millimetres it announced.
fn diagonal(width: u16, height: u16) -> String {
    const MILLIMETRES_PER_INCH: f64 = 25.4;
    let (across, down) = (f64::from(width), f64::from(height));
    let inches = across.hypot(down) / MILLIMETRES_PER_INCH;
    let mut spelled = format!("{inches:.1}");
    if spelled.ends_with(".0") {
        spelled.truncate(spelled.len() - 2);
    }
    spelled
}

/// A panel with no variable refresh states its one rate as both ends of the
/// range.
fn spelled_rate((slowest, fastest): (u16, u16)) -> String {
    if slowest == fastest {
        format!("{fastest} Hz")
    } else {
        format!("{slowest}–{fastest} Hz")
    }
}

#[cfg(test)]
mod tests {
    use super::{Display, aspect, details};
    use crate::edid::{Edid, Year, tests::panel};
    use crate::part::{Detail, Part};
    use crate::testing::display_identity;

    #[test]
    fn a_panel_is_named_by_what_its_edid_announced() {
        let resolved = "China Star Optoelectronics Technology Co., Ltd";
        assert_eq!(
            Display::of_edid(&panel(), resolved).identity(),
            &display_identity()
        );
    }

    #[test]
    fn a_panel_stating_none_of_it_carries_no_rows() {
        let silent = Edid {
            size: None,
            depth: None,
            resolution: None,
            refresh: None,
            year: None,
            ..panel()
        };
        assert!(details(&silent).is_empty());
    }

    #[test]
    fn a_panel_dated_by_its_model_year_says_which_year_that_is() {
        let dated = Edid {
            year: Some(Year::Model(2025)),
            ..panel()
        };
        assert!(details(&dated).contains(&Detail::new("Model year", "2025")));
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
    fn a_panel_of_whole_inches_is_named_without_a_trailing_zero() {
        let sixteen = Edid {
            size: Some((345, 215)),
            ..panel()
        };
        let size = details(&sixteen).into_iter().find(|d| d.name == "Size");
        assert_eq!(size.unwrap().value, "16 inches (345 × 215 mm)");
    }

    #[test]
    fn a_panel_of_one_rate_names_it_once() {
        let fixed = Edid {
            refresh: Some((60, 60)),
            ..panel()
        };
        let rate = details(&fixed)
            .into_iter()
            .find(|d| d.name == "Refresh rate");
        assert_eq!(rate.unwrap().value, "60 Hz");
    }
}
