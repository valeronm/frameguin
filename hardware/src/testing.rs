//! Stubs for the roles and the store, so a device's logic runs against them
//! in this crate's tests and in another crate's. A stub's `Default` is the
//! machine that takes every write and answers every read; each knob names
//! one way of not doing so.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use frameguin_wire::{
    BatteryCondition, BatteryInfo, BatteryState, ChargeFlow, ClickForce, DeviceError, DeviceResult,
    Identity, PartKind, PowerLedLevel,
};

use crate::ec::{Charger, Pack, PowerLedEc};
use crate::led::LedClass;
use crate::lifetime::{EcBoot, Holders};
use crate::mirror::Mirrors;
use crate::part;
use crate::state::{self, Store};
use crate::touchpad::HapticPad;
use crate::touchscreen::TouchSwitch;

/// An EC boot a mirror's evidence can name and a later reading can match.
pub const EC_BOOT: EcBoot = EcBoot::from_clocks(500_000, 1_000_000);

/// The EC an hour after [`EC_BOOT`], having restarted a minute ago.
pub const EC_RESTARTED: EcBoot = EcBoot::from_clocks(60, 1_003_600);

/// The haptic touchpad as detection would identify it.
pub fn touchpad_identity() -> Identity {
    part::hid(
        PartKind::Touchpad,
        0x093a,
        0x1343,
        "PixArt",
        "Haptic touchpad",
        "",
    )
}

/// The touch panel's controller as detection would identify it.
pub fn panel_identity() -> Identity {
    part::hid(PartKind::Touchscreen, 0x2c68, 0x0100, "", "", "")
}

/// A store that never touches disk.
#[derive(Default)]
pub struct Memory(Mutex<BTreeMap<String, String>>);

impl Store for Memory {
    fn get(&self, key: &str) -> Option<String> {
        self.0.lock().unwrap().get(key).cloned()
    }

    fn set(&self, key: &str, value: Option<String>) {
        state::apply(&mut self.0.lock().unwrap(), key, value);
    }
}

/// Mirrors over a store in memory, on a machine whose holders answer as
/// named.
pub fn mirrors(store: &Arc<Memory>, ec: Option<EcBoot>, host: Option<&str>) -> Mirrors {
    Mirrors::new(store.clone(), Holders::new(ec, host.map(str::to_owned)))
}

/// Polls once: the direct implementation never pends.
pub fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!("the direct implementation never pends"),
    }
}

/// The block [`Gauge`] answers with.
pub fn block() -> BatteryInfo {
    BatteryInfo {
        state: BatteryState {
            percent: 80,
            flow: ChargeFlow::Idle,
            milliamps: 0,
            millivolts: 15_000,
        },
        remaining_capacity: 3_000,
        last_full_capacity: 3_600,
        design_capacity: 3_900,
        design_millivolts: 15_400,
        cycle_count: 12,
        charger_connected: true,
        critical: false,
        manufacturer: "NVT".into(),
        model: "FRANGWA".into(),
        serial: "0001".into(),
        chemistry: "LION".into(),
        manufactured: "2026-01-01".into(),
    }
}

/// A pack that answers its block, and its condition unless told not to.
pub struct Gauge {
    pub answering: bool,
}

impl Default for Gauge {
    fn default() -> Self {
        Self { answering: true }
    }
}

impl Pack for Gauge {
    fn identity(&self) -> Option<Identity> {
        Some(part::sbs("NVT", "FRANGWA", "0001"))
    }

    fn info(&self) -> Option<BatteryInfo> {
        Some(block())
    }

    fn condition(&self) -> Option<BatteryCondition> {
        self.answering.then(|| BatteryCondition {
            cell_millivolts: vec![3_750, 3_751, 3_749, 3_750],
            alarms: Vec::new(),
            decicelsius: 301,
        })
    }
}

/// A charger holding one ceiling, taking every cap unless told to refuse
/// them, and logging what it took.
pub struct EcCharger {
    pub limit: Mutex<u8>,
    pub caps: bool,
    pub refusing: bool,
    pub written: Mutex<Vec<u32>>,
}

impl Default for EcCharger {
    fn default() -> Self {
        Self {
            limit: Mutex::new(100),
            caps: true,
            refusing: false,
            written: Mutex::default(),
        }
    }
}

impl Charger for EcCharger {
    fn charge_limit(&self) -> DeviceResult<u8> {
        Ok(*self.limit.lock().unwrap())
    }

    fn set_charge_limit(&self, percent: u8) -> DeviceResult<()> {
        *self.limit.lock().unwrap() = percent;
        Ok(())
    }

    fn set_charge_current_limit(&self, milliamps: u32) -> DeviceResult<()> {
        if self.refusing {
            return Err(DeviceError::Failed("no EC".into()));
        }
        self.written.lock().unwrap().push(milliamps);
        Ok(())
    }

    fn charge_current_limit_supported(&self) -> bool {
        self.caps
    }
}

/// Every write the EC and the kernel took, in the order they took them.
pub type Log = Arc<Mutex<Vec<String>>>;

/// An EC holding one level, logging every write, and refusing them all
/// once told to.
pub struct LedEc {
    pub level: Mutex<(u8, PowerLedLevel)>,
    pub custom: bool,
    pub refusing: bool,
    pub log: Log,
}

impl Default for LedEc {
    fn default() -> Self {
        Self {
            level: Mutex::new((55, PowerLedLevel::High)),
            custom: true,
            refusing: false,
            log: Log::default(),
        }
    }
}

impl PowerLedEc for LedEc {
    fn power_led_level(&self) -> DeviceResult<(u8, PowerLedLevel)> {
        Ok(*self.level.lock().unwrap())
    }

    fn set_power_led_level(&self, level: PowerLedLevel) -> DeviceResult<()> {
        if self.refusing {
            return Err(DeviceError::Failed("no EC".into()));
        }
        self.level.lock().unwrap().1 = level;
        self.log.lock().unwrap().push(format!("level {level:?}"));
        Ok(())
    }

    fn set_power_led_percentage(&self, percent: u8) -> DeviceResult<()> {
        if self.refusing {
            return Err(DeviceError::Failed("no EC".into()));
        }
        *self.level.lock().unwrap() = (percent, PowerLedLevel::Custom);
        self.log.lock().unwrap().push(format!("percent {percent}"));
        Ok(())
    }

    fn custom_power_led_levels(&self) -> bool {
        self.custom
    }
}

/// A LED class with one node, or none, keeping the kernel's account of
/// whether the LED is held dark.
pub struct Leds {
    pub node: Option<PathBuf>,
    pub dark: Mutex<bool>,
    pub refusing_release: bool,
    pub log: Log,
}

impl Default for Leds {
    fn default() -> Self {
        Self {
            node: Some(PathBuf::from("/sys/class/leds/power")),
            dark: Mutex::new(false),
            refusing_release: false,
            log: Log::default(),
        }
    }
}

impl LedClass for Leds {
    fn controllable(&self) -> Option<PathBuf> {
        self.node.clone()
    }

    fn held_dark(&self) -> Option<PathBuf> {
        self.dark
            .lock()
            .unwrap()
            .then(|| self.node.clone())
            .flatten()
    }

    fn darken(&self, _dir: &Path) -> DeviceResult<()> {
        *self.dark.lock().unwrap() = true;
        self.log.lock().unwrap().push("darken".into());
        Ok(())
    }

    fn release(&self, _dir: &Path) -> DeviceResult<()> {
        if self.refusing_release {
            return Err(DeviceError::Failed("trigger: permission denied".into()));
        }
        *self.dark.lock().unwrap() = false;
        self.log.lock().unwrap().push("release".into());
        Ok(())
    }
}

/// A pad that takes every write, or refuses every one.
#[derive(Default)]
pub struct Haptic {
    pub refusing: bool,
}

impl Haptic {
    fn answer(&self) -> DeviceResult<()> {
        if self.refusing {
            Err(DeviceError::Failed("no pad".into()))
        } else {
            Ok(())
        }
    }
}

impl HapticPad for Haptic {
    fn set_haptic_intensity(&self, _percent: u8) -> DeviceResult<()> {
        self.answer()
    }

    fn set_click_force(&self, _force: ClickForce) -> DeviceResult<()> {
        self.answer()
    }
}

/// A route holding a level it reports — the pad's, on by default — or
/// holding nothing, and refusing every write once told to.
pub struct Route {
    pub level: Mutex<Option<bool>>,
    pub refusing: bool,
}

impl Default for Route {
    fn default() -> Self {
        Self {
            level: Mutex::new(Some(true)),
            refusing: false,
        }
    }
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
