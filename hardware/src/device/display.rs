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
    [
        edid.resolution
            .map(|(across, down)| Detail::Resolution { across, down }),
        edid.size
            .map(|(across, down)| Detail::PanelSize { across, down }),
        edid.depth.map(Detail::ColourDepth),
        edid.refresh
            .map(|(slowest, fastest)| Detail::RefreshRate { slowest, fastest }),
        edid.year.map(|year| match year {
            Year::Manufacture(year) => Detail::ManufactureYear(year),
            Year::Model(year) => Detail::ModelYear(year),
        }),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::{Display, details};
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
    fn a_panel_is_identified_by_its_pnp_id_and_product_code() {
        assert_eq!(
            Display::of_edid(&panel(), "").identity().id,
            "edid:CSW:1322"
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
        assert!(details(&dated).contains(&Detail::ModelYear(2025)));
    }
}
