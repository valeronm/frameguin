//! The haptic touchpad: two write-only settings and the mirror that answers
//! for them.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use frameguin_wire::{
    self as wire, DeviceError, DeviceResult, HAPTIC_INTENSITY_LEVELS, TouchpadControl,
};

use crate::part::{Identity, Kind, Part};
use crate::state::Store;
use crate::touchpad::{self, HapticPad};

const DEFAULT_HAPTIC_INTENSITY: u8 = 75;

const KEY_HAPTIC_INTENSITY: &str = "haptic_intensity";
const KEY_CLICK_FORCE: &str = "click_force";

pub struct Touchpad {
    pad: Box<dyn HapticPad>,
    store: Arc<dyn Store>,
    identity: Identity,
    haptic_intensity: AtomicU8,
    /// The pad's own code for the force, which is what the store carries
    /// and what a reload has to be able to name again.
    click_force: AtomicU8,
}

impl Touchpad {
    /// The pad on this machine's HID bus, if it is a haptic one, keeping
    /// what the bus said it was.
    pub fn detect(hid: &hidapi::HidApi, store: Arc<dyn Store>) -> Option<Self> {
        let pad = touchpad::haptic_pad(hid)?;
        // No firmware version: the haptic pad's registers are in no table
        // this can trust.
        let identity = Identity::of_hid(Kind::Touchpad, pad);
        Some(Self::new(Box::new(touchpad::Hid), store, identity))
    }

    /// Answers from the mirror before anything is written, and from the
    /// factory defaults where the store holds nothing it can name.
    pub fn new(pad: Box<dyn HapticPad>, store: Arc<dyn Store>, identity: Identity) -> Self {
        let haptic_intensity = store
            .get(KEY_HAPTIC_INTENSITY)
            .and_then(|v| v.parse().ok())
            .filter(|&v| Self::check_haptic_intensity(v).is_ok())
            .unwrap_or(DEFAULT_HAPTIC_INTENSITY);
        let click_force = store
            .get(KEY_CLICK_FORCE)
            .and_then(|v| v.parse().ok())
            .filter(|&v| touchpad::wire_click_force(v).is_some())
            .unwrap_or(touchpad::click_force(touchpad::DEFAULT_CLICK_FORCE) as u8);
        Self {
            pad,
            store,
            identity,
            haptic_intensity: AtomicU8::new(haptic_intensity),
            click_force: AtomicU8::new(click_force),
        }
    }
}

impl Part for Touchpad {
    fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl Touchpad {
    /// Separate from the setter so a server can refuse an argument before it
    /// prompts for authorization.
    pub fn check_haptic_intensity(percent: u8) -> DeviceResult<()> {
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
        Ok(self.haptic_intensity.load(Ordering::Relaxed))
    }

    /// Mirrored only once the pad has taken it: a write the pad refused
    /// leaves the last accepted value standing.
    async fn set_haptic_intensity(&self, percent: u8) -> DeviceResult<()> {
        Self::check_haptic_intensity(percent)?;
        self.pad.set_haptic_intensity(percent)?;
        self.haptic_intensity.store(percent, Ordering::Relaxed);
        self.store
            .set(KEY_HAPTIC_INTENSITY, Some(percent.to_string()));
        Ok(())
    }

    async fn click_force(&self) -> DeviceResult<wire::ClickForce> {
        // A code no force maps to reads as the factory default, the same
        // answer this gives before anything has been written.
        Ok(
            touchpad::wire_click_force(self.click_force.load(Ordering::Relaxed))
                .unwrap_or(touchpad::DEFAULT_CLICK_FORCE),
        )
    }

    async fn set_click_force(&self, force: wire::ClickForce) -> DeviceResult<()> {
        self.pad.set_click_force(force)?;
        let code = touchpad::click_force(force) as u8;
        self.click_force.store(code, Ordering::Relaxed);
        self.store.set(KEY_CLICK_FORCE, Some(code.to_string()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::Arc;

    use frameguin_wire::{ClickForce, DeviceError, TouchpadControl};

    use super::{KEY_CLICK_FORCE, KEY_HAPTIC_INTENSITY, Touchpad};
    use crate::part::{Identity, Kind, Part};
    use crate::state::Store;
    use crate::state::tests::Memory;
    use crate::testing::ready;
    use crate::touchpad::HapticPad;

    /// A pad that takes every write, or refuses every one.
    struct Pad {
        refusing: bool,
    }

    impl Pad {
        fn answer(&self) -> io::Result<()> {
            if self.refusing {
                Err(io::Error::other("no pad"))
            } else {
                Ok(())
            }
        }
    }

    impl HapticPad for Pad {
        fn set_haptic_intensity(&self, _percent: u8) -> io::Result<()> {
            self.answer()
        }

        fn set_click_force(&self, _force: ClickForce) -> io::Result<()> {
            self.answer()
        }
    }

    fn over(pad: Pad, store: &Arc<Memory>) -> Touchpad {
        let identity = Identity::usb(
            Kind::Touchpad,
            0x093a,
            0x1343,
            "PixArt",
            "Haptic touchpad",
            "",
        );
        Touchpad::new(Box::new(pad), store.clone(), identity)
    }

    const TAKING: Pad = Pad { refusing: false };
    const REFUSING: Pad = Pad { refusing: true };

    #[test]
    fn a_descriptor_without_a_serial_is_a_part_without_one() {
        let store = Arc::new(Memory::default());
        let identity = over(TAKING, &store).identity().clone();
        assert_eq!(identity.serial, None);
        assert_eq!(identity.id, "usb:093a:1343");
    }

    #[test]
    fn an_empty_store_answers_the_factory_defaults() {
        let store = Arc::new(Memory::default());
        let touchpad = over(TAKING, &store);
        assert_eq!(ready(touchpad.haptic_intensity()), Ok(75));
        assert_eq!(ready(touchpad.click_force()), Ok(ClickForce::Medium));
    }

    #[test]
    fn a_write_the_pad_takes_is_mirrored_and_stored() {
        let store = Arc::new(Memory::default());
        let touchpad = over(TAKING, &store);
        ready(touchpad.set_haptic_intensity(25)).unwrap();
        ready(touchpad.set_click_force(ClickForce::High)).unwrap();
        assert_eq!(ready(touchpad.haptic_intensity()), Ok(25));
        assert_eq!(ready(touchpad.click_force()), Ok(ClickForce::High));
        let reloaded = over(TAKING, &store);
        assert_eq!(ready(reloaded.haptic_intensity()), Ok(25));
        assert_eq!(ready(reloaded.click_force()), Ok(ClickForce::High));
    }

    #[test]
    fn a_write_the_pad_refuses_leaves_the_mirror_standing() {
        let store = Arc::new(Memory::default());
        let touchpad = over(REFUSING, &store);
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
        let touchpad = over(REFUSING, &store);
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
        let touchpad = over(TAKING, &store);
        assert_eq!(ready(touchpad.haptic_intensity()), Ok(75));
        assert_eq!(ready(touchpad.click_force()), Ok(ClickForce::Medium));
    }
}
