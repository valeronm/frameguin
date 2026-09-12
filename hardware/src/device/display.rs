//! The panel the board drives, as its EDID describes it: a device that is a
//! part and no control, read for the bill of materials alone.

use crate::drm;
use crate::edid::{self, Edid};
use crate::part::{self, Firmware, Identity, Part};
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
            identity: part::edid(
                &edid.manufacturer,
                vendor_name,
                edid.product,
                &edid.name,
                &edid.serial,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Display;
    use crate::edid::Edid;
    use crate::part::Part;
    use crate::testing::display_identity;

    #[test]
    fn a_panel_is_named_by_what_its_edid_announced() {
        let edid = Edid {
            manufacturer: "CSW".to_owned(),
            product: 4898,
            name: "MND508ZB1-1".to_owned(),
            serial: String::new(),
        };
        let resolved = "China Star Optoelectronics Technology Co., Ltd";
        assert_eq!(
            Display::of_edid(&edid, resolved).identity(),
            &display_identity()
        );
    }
}
