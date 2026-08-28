//! The touch panel: one switch, and the mirror that answers for it on the
//! route that keeps no account of its own.

use std::sync::{Arc, Mutex};

use frameguin_wire::{DeviceResult, TouchscreenControl};

use crate::lifetime::HostStamp;
use crate::part::{self, Firmware, Identity, Part, PartKind};
use crate::state::Store;
use crate::touchscreen::{self, TouchSwitch};

const KEY_OFF_HOST: &str = "touchscreen_off_host";

pub struct Touchscreen {
    route: Box<dyn TouchSwitch>,
    store: Arc<dyn Store>,
    identity: Identity,
    /// When the panel was last switched off, and None while nothing has
    /// switched it. Dated against the host, whose boot and whose sleep both
    /// bring the panel back — and weighed as it is read, the sleep being
    /// the one of those that moves under a running process.
    off: Mutex<Option<HostStamp>>,
}

impl Touchscreen {
    /// The panel this machine can switch, by whichever route it has — see
    /// [`touchscreen::find`] for what qualifies one.
    pub fn detect(hid: &hidapi::HidApi, store: Arc<dyn Store>) -> Option<Self> {
        let (route, controller) = touchscreen::find(hid)?;
        let identity = Identity {
            firmware: route
                .firmware(hid, controller)
                .map(|version| Firmware::new("Controller", &version))
                .into_iter()
                .collect(),
            ..part::of_hid(PartKind::Touchscreen, controller)
        };
        Some(Self::new(Box::new(route), store, identity))
    }

    pub fn new(route: Box<dyn TouchSwitch>, store: Arc<dyn Store>, identity: Identity) -> Self {
        let off = store.get(KEY_OFF_HOST).and_then(|v| HostStamp::parse(&v));
        Self {
            route,
            store,
            identity,
            off: Mutex::new(off),
        }
    }

    /// What the hardware itself says, and None on the route that keeps no
    /// account.
    pub fn reading(&self) -> DeviceResult<Option<bool>> {
        self.route.reading()
    }

    fn remember(&self, stamp: Option<HostStamp>) {
        self.store
            .set(KEY_OFF_HOST, stamp.as_ref().map(HostStamp::stored));
        *self.off.lock().unwrap() = stamp;
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
        let off = self.off.lock().unwrap();
        Ok(!off.as_ref().is_some_and(HostStamp::still_current))
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
        // Dated before the write, so that a restart between the two reads
        // as having dropped it.
        let stamp = if enabled { None } else { HostStamp::now() };
        self.route.set_enabled(enabled)?;
        self.remember(stamp);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use frameguin_wire::{DeviceError, DeviceResult, TouchscreenControl};

    use super::{KEY_OFF_HOST, Touchscreen};
    use crate::part::{self, PartKind};
    use crate::state::Store;
    use crate::state::tests::Memory;
    use crate::testing::ready;
    use crate::touchscreen::TouchSwitch;

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
        Touchscreen::new(Box::new(route), store.clone(), identity)
    }

    #[test]
    fn a_route_with_a_reading_answers_from_the_hardware_and_keeps_no_record() {
        let store = Arc::new(Memory::default());
        let touchscreen = over(&PAD, &store);
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
        ready(touchscreen.set_enabled(false)).unwrap();
        assert_eq!(touchscreen.reading(), Ok(Some(false)));
        assert_eq!(ready(touchscreen.enabled()), Ok(false));
        assert_eq!(store.get(KEY_OFF_HOST), None);
    }

    #[test]
    fn a_route_with_no_reading_answers_from_the_mirror() {
        let store = Arc::new(Memory::default());
        let touchscreen = over(&PANEL, &store);
        assert_eq!(touchscreen.reading(), Ok(None));
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
        ready(touchscreen.set_enabled(false)).unwrap();
        assert_eq!(ready(touchscreen.enabled()), Ok(false));
        assert!(store.get(KEY_OFF_HOST).is_some());
        let reloaded = over(&PANEL, &store);
        assert_eq!(ready(reloaded.enabled()), Ok(false));
        ready(touchscreen.set_enabled(true)).unwrap();
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
        assert_eq!(store.get(KEY_OFF_HOST), None);
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
        assert_eq!(store.get(KEY_OFF_HOST), None);
    }

    /// A stamp from a boot this is not reads as the panel reporting, which
    /// is what a reboot leaves it doing.
    #[test]
    fn a_stamp_from_another_boot_is_not_believed() {
        let store = Arc::new(Memory::default());
        store.set(
            KEY_OFF_HOST,
            Some("00000000-0000-4000-8000-000000000001:0".into()),
        );
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
