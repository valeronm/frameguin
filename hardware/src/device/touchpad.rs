//! The haptic touchpad: two write-only settings and the mirror that answers
//! for them.

use std::sync::Arc;

use frameguin_contract::{
    self as contract, DeviceError, DeviceResult, HAPTIC_INTENSITY_LEVELS, TouchpadControl,
};

use crate::lifetime::Lifetime;
use crate::mirror::{Mirror, Mirrors};
use crate::part::{self, Firmware, FirmwareKind, Identity, Part, PartKind};
use crate::state::Stored;
use crate::touchpad::{self, HapticPad};
use crate::udev;

const DEFAULT_HAPTIC_INTENSITY: u8 = 75;

const KEY_HAPTIC_INTENSITY: &str = "haptic_intensity";
const KEY_CLICK_FORCE: &str = "click_force";

/// An intensity on one of the pad's steps, which is all the store may name.
#[derive(Clone, Copy)]
struct Intensity(u8);

impl Stored for Intensity {
    fn from_stored(value: &str) -> Option<Self> {
        let percent = value.parse().ok()?;
        Touchpad::check_haptic_intensity(percent)
            .ok()
            .map(|()| Self(percent))
    }

    fn stored(&self) -> String {
        self.0.to_string()
    }
}

/// Kept in the store as the pad's own code, which is what a reload has to
/// be able to name again.
impl Stored for contract::ClickForce {
    fn from_stored(value: &str) -> Option<Self> {
        touchpad::wire_click_force(value.parse().ok()?)
    }

    fn stored(&self) -> String {
        (touchpad::click_force(*self) as u8).to_string()
    }
}

pub struct Touchpad {
    pad: Arc<dyn HapticPad>,
    identity: Identity,
    haptic_intensity: Mirror<Intensity>,
    click_force: Mirror<contract::ClickForce>,
}

impl Touchpad {
    /// The pad on this machine's HID bus, if it is a haptic one, keeping
    /// what the bus said it was.
    pub(crate) fn detect(hid: &hidapi::HidApi, mirrors: &Mirrors) -> Option<Self> {
        let pad = touchpad::haptic_pad(hid)?;
        let resolved = udev::usb_vendor(pad.vendor_id()).unwrap_or_default();
        let identity = Identity {
            firmware: touchpad::firmware(hid, pad)
                .map(|version| Firmware::new(FirmwareKind::Own, &version))
                .into_iter()
                .collect(),
            ..part::of_hid(PartKind::Touchpad, pad, &resolved)
        };
        Some(Self::new(Arc::new(touchpad::Hid), mirrors, identity))
    }

    pub fn new(pad: Arc<dyn HapticPad>, mirrors: &Mirrors, identity: Identity) -> Self {
        Self {
            pad,
            identity,
            haptic_intensity: mirrors.value(KEY_HAPTIC_INTENSITY, Lifetime::Permanent),
            click_force: mirrors.value(KEY_CLICK_FORCE, Lifetime::Permanent),
        }
    }
}

impl Part for Touchpad {
    fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl Touchpad {
    pub fn mirrored(&self) -> bool {
        self.haptic_intensity.current().is_some() || self.click_force.current().is_some()
    }

    /// Another system on the machine sends the pad its own settings when it
    /// boots. A setting never set here is left as that system left it.
    pub fn resend(&self) -> DeviceResult<()> {
        let intensity = self
            .haptic_intensity
            .current()
            .map_or(Ok(()), |Intensity(percent)| {
                self.pad.set_haptic_intensity(percent)
            });
        let force = self
            .click_force
            .current()
            .map_or(Ok(()), |force| self.pad.set_click_force(force));
        intensity.and(force)
    }

    fn check_haptic_intensity(percent: u8) -> DeviceResult<()> {
        if HAPTIC_INTENSITY_LEVELS.contains(&percent) {
            Ok(())
        } else {
            Err(DeviceError::InvalidArgs(format!(
                "intensity must be one of {HAPTIC_INTENSITY_LEVELS:?}"
            )))
        }
    }
}

impl TouchpadControl for Touchpad {
    async fn haptic_intensity(&self) -> DeviceResult<u8> {
        Ok(self
            .haptic_intensity
            .current()
            .map_or(DEFAULT_HAPTIC_INTENSITY, |intensity| intensity.0))
    }

    async fn set_haptic_intensity(&self, percent: u8) -> DeviceResult<()> {
        Self::check_haptic_intensity(percent)?;
        self.haptic_intensity.record(Intensity(percent), || {
            self.pad.set_haptic_intensity(percent)
        })
    }

    async fn click_force(&self) -> DeviceResult<contract::ClickForce> {
        Ok(self
            .click_force
            .current()
            .unwrap_or(touchpad::DEFAULT_CLICK_FORCE))
    }

    async fn set_click_force(&self, force: contract::ClickForce) -> DeviceResult<()> {
        self.click_force
            .record(force, || self.pad.set_click_force(force))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use frameguin_contract::{ClickForce, DeviceError, TouchpadControl};

    use super::{KEY_CLICK_FORCE, KEY_HAPTIC_INTENSITY, Touchpad};
    use crate::part::Part;
    use crate::state::Store;
    use crate::testing::{Haptic, Memory, OTHER_SYSTEMS, mirrors, ready, touchpad_identity};

    fn over(pad: Arc<Haptic>, store: &Arc<Memory>) -> Touchpad {
        Touchpad::new(pad, &mirrors(store, None, None), touchpad_identity())
    }

    fn taking() -> Arc<Haptic> {
        Arc::new(Haptic::default())
    }

    fn refusing() -> Arc<Haptic> {
        Arc::new(Haptic {
            refusing: true,
            ..Haptic::default()
        })
    }

    #[test]
    fn a_descriptor_without_a_serial_is_a_part_without_one() {
        let store = Arc::new(Memory::default());
        let identity = over(taking(), &store).identity().clone();
        assert_eq!(identity.serial, "");
        assert_eq!(identity.id, "hid:093a:1343");
    }

    #[test]
    fn an_empty_store_answers_the_factory_defaults() {
        let store = Arc::new(Memory::default());
        let touchpad = over(taking(), &store);
        assert_eq!(ready(touchpad.haptic_intensity()), Ok(75));
        assert_eq!(ready(touchpad.click_force()), Ok(ClickForce::Medium));
    }

    #[test]
    fn a_write_the_pad_takes_is_mirrored_and_stored() {
        let store = Arc::new(Memory::default());
        let touchpad = over(taking(), &store);
        ready(touchpad.set_haptic_intensity(25)).unwrap();
        ready(touchpad.set_click_force(ClickForce::High)).unwrap();
        assert_eq!(ready(touchpad.haptic_intensity()), Ok(25));
        assert_eq!(ready(touchpad.click_force()), Ok(ClickForce::High));
        let reloaded = over(taking(), &store);
        assert_eq!(ready(reloaded.haptic_intensity()), Ok(25));
        assert_eq!(ready(reloaded.click_force()), Ok(ClickForce::High));
    }

    #[test]
    fn a_write_the_pad_refuses_leaves_the_mirror_standing() {
        let store = Arc::new(Memory::default());
        let touchpad = over(refusing(), &store);
        assert!(ready(touchpad.set_haptic_intensity(25)).is_err());
        assert!(ready(touchpad.set_click_force(ClickForce::Low)).is_err());
        assert_eq!(ready(touchpad.haptic_intensity()), Ok(75));
        assert_eq!(ready(touchpad.click_force()), Ok(ClickForce::Medium));
        assert_eq!(store.get(KEY_HAPTIC_INTENSITY), None);
    }

    /// The direct path refuses what the bus would, before the pad is asked.
    #[test]
    fn an_intensity_off_the_steps_is_an_invalid_argument() {
        let store = Arc::new(Memory::default());
        let touchpad = over(refusing(), &store);
        assert!(matches!(
            ready(touchpad.set_haptic_intensity(33)),
            Err(DeviceError::InvalidArgs(_))
        ));
        assert!(matches!(
            Touchpad::check_haptic_intensity(33),
            Err(DeviceError::InvalidArgs(_))
        ));
    }

    #[test]
    fn a_stored_value_the_pad_could_not_hold_reads_as_the_default() {
        let store = Arc::new(Memory::default());
        store.set(KEY_HAPTIC_INTENSITY, Some("33".into()));
        store.set(KEY_CLICK_FORCE, Some("9".into()));
        let touchpad = over(taking(), &store);
        assert_eq!(ready(touchpad.haptic_intensity()), Ok(75));
        assert_eq!(ready(touchpad.click_force()), Ok(ClickForce::Medium));
    }

    #[test]
    fn a_resend_writes_what_was_set_here_over_what_another_system_sent() {
        let store = Arc::new(Memory::default());
        let pad = taking();
        let touchpad = over(pad.clone(), &store);
        ready(touchpad.set_haptic_intensity(25)).unwrap();
        ready(touchpad.set_click_force(ClickForce::High)).unwrap();
        pad.sent_by_another_system();
        let rebooted = over(pad.clone(), &store);
        assert!(rebooted.mirrored());
        rebooted.resend().unwrap();
        assert_eq!(pad.held(), (Some(25), Some(ClickForce::High)));
    }

    #[test]
    fn a_pad_never_set_here_is_left_as_it_is() {
        let store = Arc::new(Memory::default());
        let pad = taking();
        pad.sent_by_another_system();
        let touchpad = over(pad.clone(), &store);
        assert!(!touchpad.mirrored());
        touchpad.resend().unwrap();
        assert_eq!(pad.held(), OTHER_SYSTEMS);
    }
}
