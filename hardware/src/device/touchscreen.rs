//! The touch panel: one switch, and the mirror that answers for it on the
//! route that keeps no account of its own.

use frameguin_wire::{DeviceResult, TouchscreenControl};

use crate::lifetime::Lifetime;
use crate::mirror::{Mirror, Mirrors};
use crate::part::{self, Firmware, Identity, Part, PartKind};
use crate::touchscreen::{self, TouchSwitch};

const KEY_OFF: &str = "touchscreen_off";

pub struct Touchscreen {
    route: Box<dyn TouchSwitch>,
    identity: Identity,
    /// That the panel was switched off.
    off: Mirror<bool>,
}

impl Touchscreen {
    /// The panel this machine can switch, by whichever route it has — see
    /// [`touchscreen::find`] for what qualifies one.
    pub fn detect(hid: &hidapi::HidApi, mirrors: &Mirrors) -> Option<Self> {
        let (route, controller) = touchscreen::find(hid)?;
        let identity = Identity {
            firmware: route
                .firmware(hid, controller)
                .map(|version| Firmware::new("Controller", &version))
                .into_iter()
                .collect(),
            ..part::of_hid(PartKind::Touchscreen, controller)
        };
        Some(Self::new(Box::new(route), mirrors, identity))
    }

    pub fn new(route: Box<dyn TouchSwitch>, mirrors: &Mirrors, identity: Identity) -> Self {
        Self {
            route,
            identity,
            off: mirrors.value(KEY_OFF, Lifetime::HostAwake),
        }
    }

    /// What the hardware itself says, and None on the route that keeps no
    /// account.
    pub fn reading(&self) -> DeviceResult<Option<bool>> {
        self.route.reading()
    }
}

impl Part for Touchscreen {
    fn identity(&self) -> &Identity {
        &self.identity
    }
}

impl TouchscreenControl for Touchscreen {
    async fn enabled(&self) -> DeviceResult<bool> {
        if let Some(level) = self.route.reading()? {
            return Ok(level);
        }
        Ok(self.off.current().is_none())
    }

    /// Nothing re-applies the switch afterwards: the panel is put back on
    /// behind whoever asked for it off, by a resume or a lid opening on one
    /// route and by whatever the controller does not keep on the other, and
    /// re-asserting it on those events would be enforcing a policy nobody
    /// asked for.
    async fn set_enabled(&self, enabled: bool) -> DeviceResult<()> {
        // A route with a reading of its own gets no record: nothing would
        // read it.
        if self.route.reading()?.is_some() {
            return self.route.set_enabled(enabled);
        }
        let write = || self.route.set_enabled(enabled);
        if enabled {
            self.off.clear(write)
        } else {
            self.off.record(true, write)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use frameguin_wire::{DeviceError, DeviceResult, TouchscreenControl};

    use super::{KEY_OFF, Touchscreen};
    use crate::mirror::evidence_key;
    use crate::part::{self, PartKind};
    use crate::state::Store;
    use crate::state::tests::Memory;
    use crate::testing::{mirrors, ready};
    use crate::touchscreen::TouchSwitch;

    const BOOT: &str = "00000000-0000-4000-8000-000000000001";
    const EARLIER: &str = "00000000-0000-4000-8000-000000000002";

    /// A route holding a level it reports, or holding nothing, and refusing
    /// every write once told to.
    struct Route {
        level: Mutex<Option<bool>>,
        refusing: bool,
    }

    impl TouchSwitch for Route {
        fn reading(&self) -> DeviceResult<Option<bool>> {
            Ok(*self.level.lock().unwrap())
        }

        fn set_enabled(&self, enabled: bool) -> DeviceResult<()> {
            if self.refusing {
                return Err(DeviceError::Failed("no panel".into()));
            }
            if let Some(level) = self.level.lock().unwrap().as_mut() {
                *level = enabled;
            }
            Ok(())
        }
    }

    struct Machine {
        level: Option<bool>,
        refusing: bool,
    }

    const PAD: Machine = Machine {
        level: Some(true),
        refusing: false,
    };

    const PANEL: Machine = Machine {
        level: None,
        refusing: false,
    };

    fn over(machine: &Machine, store: &Arc<Memory>) -> Touchscreen {
        let route = Route {
            level: Mutex::new(machine.level),
            refusing: machine.refusing,
        };
        let identity = part::hid(PartKind::Touchscreen, 0x2c68, 0x0100, "", "", "");
        Touchscreen::new(Box::new(route), &mirrors(store, None, Some(BOOT)), identity)
    }

    #[test]
    fn a_route_with_a_reading_answers_from_the_hardware_and_keeps_no_record() {
        let store = Arc::new(Memory::default());
        let touchscreen = over(&PAD, &store);
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
        ready(touchscreen.set_enabled(false)).unwrap();
        assert_eq!(touchscreen.reading(), Ok(Some(false)));
        assert_eq!(ready(touchscreen.enabled()), Ok(false));
        assert_eq!(store.get(KEY_OFF), None);
    }

    #[test]
    fn a_route_with_no_reading_answers_from_the_mirror() {
        let store = Arc::new(Memory::default());
        let touchscreen = over(&PANEL, &store);
        assert_eq!(touchscreen.reading(), Ok(None));
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
        ready(touchscreen.set_enabled(false)).unwrap();
        assert_eq!(ready(touchscreen.enabled()), Ok(false));
        assert!(store.get(KEY_OFF).is_some());
        let reloaded = over(&PANEL, &store);
        assert_eq!(ready(reloaded.enabled()), Ok(false));
        ready(touchscreen.set_enabled(true)).unwrap();
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
        assert_eq!(store.get(KEY_OFF), None);
    }

    #[test]
    fn a_write_the_route_refuses_leaves_the_mirror_standing() {
        let store = Arc::new(Memory::default());
        let touchscreen = over(
            &Machine {
                refusing: true,
                ..PANEL
            },
            &store,
        );
        assert!(ready(touchscreen.set_enabled(false)).is_err());
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
        assert_eq!(store.get(KEY_OFF), None);
    }

    /// Evidence from a boot this is not reads as the panel reporting, which
    /// is what a reboot leaves it doing.
    #[test]
    fn evidence_from_another_boot_is_not_believed() {
        let store = Arc::new(Memory::default());
        store.set(KEY_OFF, Some("true".into()));
        store.set(&evidence_key(KEY_OFF), Some(format!("{EARLIER}:0")));
        let touchscreen = over(&PANEL, &store);
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
    }

    #[test]
    fn the_hardware_outranks_the_mirror_where_it_answers() {
        let store = Arc::new(Memory::default());
        ready(over(&PANEL, &store).set_enabled(false)).unwrap();
        let touchscreen = over(&PAD, &store);
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
    }
}
