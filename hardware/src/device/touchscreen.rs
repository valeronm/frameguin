//! The touch panel: one switch, and the mirror that answers for it on the
//! route that keeps no account of its own.

use frameguin_wire::{DeviceResult, TouchscreenControl};

use crate::lifetime::Lifetime;
use crate::mirror::{Mirror, Mirrors};
use crate::part::Firmware;
use crate::restore::{Restorable, Wanted};
use crate::touchscreen::{self, TouchSwitch};

const KEY_OFF: &str = "touchscreen_off";

pub struct Touchscreen {
    route: Box<dyn TouchSwitch>,
    /// That the panel was switched off.
    off: Mirror<bool>,
    /// That it was asked off, which a resume or a lid opening undoes on the
    /// pad route without anyone asking.
    wanted_off: Wanted<bool>,
}

impl Touchscreen {
    /// The panel this machine can switch, by whichever route it has — see
    /// [`touchscreen::find`] for what qualifies one — and the controller's
    /// firmware version, which is the display's to report: the controller
    /// is sold in front of a panel and never on its own.
    pub fn detect(hid: &hidapi::HidApi, mirrors: &Mirrors) -> (Option<Self>, Option<Firmware>) {
        let Some((route, controller)) = touchscreen::find(hid) else {
            return (None, None);
        };
        let firmware = route
            .firmware(hid, controller)
            .map(|version| Firmware::new("Controller", &version));
        (Some(Self::new(Box::new(route), mirrors)), firmware)
    }

    pub fn new(route: Box<dyn TouchSwitch>, mirrors: &Mirrors) -> Self {
        Self {
            route,
            off: mirrors.value(KEY_OFF, Lifetime::HostAwake),
            wanted_off: mirrors.wanted(KEY_OFF),
        }
    }

    /// What the hardware itself says, and None on the route that keeps no
    /// account.
    pub fn reading(&self) -> DeviceResult<Option<bool>> {
        self.route.reading()
    }
}

impl Restorable for Touchscreen {
    /// On is what every event that moves the panel leaves it, so only off is
    /// ever written back.
    async fn restore(&self) -> DeviceResult<()> {
        if self.wanted_off.current() == Some(true) {
            self.set_enabled(false).await?;
        }
        Ok(())
    }
}

impl TouchscreenControl for Touchscreen {
    async fn enabled(&self) -> DeviceResult<bool> {
        if let Some(level) = self.route.reading()? {
            return Ok(level);
        }
        Ok(self.off.current().is_none())
    }

    async fn set_enabled(&self, enabled: bool) -> DeviceResult<()> {
        let write = || self.route.set_enabled(enabled);
        // A route with a reading of its own gets no mirror: nothing would
        // read it.
        if self.route.reading()?.is_some() {
            write()?;
        } else if enabled {
            self.off.clear(write)?;
        } else {
            self.off.record(true, write)?;
        }
        if enabled {
            self.wanted_off.forget();
        } else {
            self.wanted_off.record(&true);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use frameguin_wire::TouchscreenControl;

    use super::{KEY_OFF, Restorable, Touchscreen};
    use crate::mirror::evidence_key;
    use crate::state::Store;
    use crate::testing::{
        HOST_BOOT as BOOT, HOST_EARLIER as EARLIER, Memory, Route, mirrors, ready,
    };

    fn pad() -> Route {
        Route::default()
    }

    fn panel() -> Route {
        Route {
            level: Mutex::new(None),
            ..Route::default()
        }
    }

    fn unreadable_pad() -> Route {
        Route {
            unreadable: true,
            ..pad()
        }
    }

    fn refusing_panel() -> Route {
        Route {
            refusing: true,
            ..panel()
        }
    }

    fn over(route: Route, store: &Arc<Memory>) -> Touchscreen {
        Touchscreen::new(Box::new(route), &mirrors(store, None, Some(BOOT)))
    }

    #[test]
    fn a_route_with_a_reading_answers_from_the_hardware_and_keeps_no_record() {
        let store = Arc::new(Memory::default());
        let touchscreen = over(pad(), &store);
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
        ready(touchscreen.set_enabled(false)).unwrap();
        assert_eq!(touchscreen.reading(), Ok(Some(false)));
        assert_eq!(ready(touchscreen.enabled()), Ok(false));
        assert_eq!(store.get(KEY_OFF), None);
    }

    #[test]
    fn a_route_with_no_reading_answers_from_the_mirror() {
        let store = Arc::new(Memory::default());
        let touchscreen = over(panel(), &store);
        assert_eq!(touchscreen.reading(), Ok(None));
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
        ready(touchscreen.set_enabled(false)).unwrap();
        assert_eq!(ready(touchscreen.enabled()), Ok(false));
        assert!(store.get(KEY_OFF).is_some());
        let reloaded = over(panel(), &store);
        assert_eq!(ready(reloaded.enabled()), Ok(false));
        ready(touchscreen.set_enabled(true)).unwrap();
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
        assert_eq!(store.get(KEY_OFF), None);
    }

    #[test]
    fn a_write_the_route_refuses_leaves_the_mirror_standing() {
        let store = Arc::new(Memory::default());
        let touchscreen = over(refusing_panel(), &store);
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
        let touchscreen = over(panel(), &store);
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
    }

    #[test]
    fn the_hardware_outranks_the_mirror_where_it_answers() {
        let store = Arc::new(Memory::default());
        ready(over(panel(), &store).set_enabled(false)).unwrap();
        let touchscreen = over(pad(), &store);
        assert_eq!(ready(touchscreen.enabled()), Ok(true));
    }

    #[test]
    fn off_is_written_back_on_a_restore_and_on_never_is() {
        let store = Arc::new(Memory::default());
        let mirrors = mirrors(&store, None, Some(BOOT));
        mirrors.restore().set_enabled(true);
        ready(Touchscreen::new(Box::new(pad()), &mirrors).set_enabled(false)).unwrap();
        let touchscreen = over(pad(), &store);
        assert_eq!(touchscreen.reading(), Ok(Some(true)));
        ready(touchscreen.restore()).unwrap();
        assert_eq!(touchscreen.reading(), Ok(Some(false)));
        ready(touchscreen.set_enabled(true)).unwrap();
        let untouched = over(
            Route {
                refusing: true,
                ..pad()
            },
            &store,
        );
        ready(untouched.restore()).unwrap();
        assert_eq!(untouched.reading(), Ok(Some(true)));
    }

    /// A route with an account of its own is one the mirror never records,
    /// so a read failing into no account would start one on the very
    /// machine that must not keep it.
    #[test]
    fn a_read_the_route_fails_reaches_neither_the_mirror_nor_a_record() {
        let store = Arc::new(Memory::default());
        let touchscreen = over(unreadable_pad(), &store);
        assert!(ready(touchscreen.enabled()).is_err());
        assert!(ready(touchscreen.set_enabled(false)).is_err());
        assert_eq!(store.get(KEY_OFF), None);
    }
}
