//! The power button LED: the EC's levels, the kernel's off, and who holds
//! the LED now.
//!
//! It has two possible drivers — the EC's own policy and the kernel holding
//! it dark — and only one at a time. [`crate::led`] is the kernel half's
//! mechanism; this is the arbitration: which one holds it now, dating the
//! handover against the EC's life, and taking the LED back before any write
//! the EC has to be the one to make.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_io::Timer;
use frameguin_wire::{DeviceError, DeviceResult, PowerLedControl, PowerLedLevel};

use crate::ec::{Ec, EcClock, PowerLedEc};
use crate::led::{self, LedClass};
use crate::lifetime::EcStamp;
use crate::state::Store;

const KEY_OFF_STAMP: &str = "power_led_off_stamp";

/// How long the EC's own deferred hook takes to move the LED's PWM duty to a
/// level just written, plus margin — the hook is scheduled at 100 ms, not
/// promised for then. Waited out rather than polled: nothing the EC answers
/// reports the duty, only the level it will eventually become.
const LEVEL_SETTLE: Duration = Duration::from_millis(150);

/// A settled write, resolved from its level before anything is written:
/// both ways of a level being impossible are answered by the resolving.
enum Write {
    Level(PowerLedLevel),
    Dark(PathBuf),
}

pub struct PowerLed {
    ec: Arc<dyn PowerLedEc>,
    clock: Arc<dyn EcClock>,
    leds: Box<dyn LedClass>,
    store: Arc<dyn Store>,
    levels: Vec<PowerLedLevel>,
    /// When this device last darkened the LED, and None when it has not.
    /// The kernel holds the LED state itself, so what is mirrored is only
    /// the date of the write, which [`Self::off_node`] weighs.
    off: Mutex<Option<EcStamp>>,
}

impl PowerLed {
    /// The LED the EC answers for, by the getter's own read.
    pub fn detect(ec: &Arc<Ec>, store: Arc<dyn Store>) -> Option<Self> {
        ec.power_led_level().ok()?;
        Some(Self::new(
            ec.clone(),
            ec.clone(),
            Box::new(led::Sysfs),
            store,
        ))
    }

    /// Which levels the board has is settled here, once: the fixed levels on
    /// every firmware, the rest where the firmware takes a percentage, and
    /// off where the kernel has a node this could take and give back.
    pub fn new(
        ec: Arc<dyn PowerLedEc>,
        clock: Arc<dyn EcClock>,
        leds: Box<dyn LedClass>,
        store: Arc<dyn Store>,
    ) -> Self {
        let custom = ec.custom_power_led_levels();
        let off_node = leds.controllable().is_some();
        let levels = PowerLedLevel::ALL
            .into_iter()
            .filter(|level| match level {
                PowerLedLevel::High | PowerLedLevel::Medium | PowerLedLevel::Low => true,
                PowerLedLevel::Auto | PowerLedLevel::UltraLow | PowerLedLevel::Custom => custom,
                PowerLedLevel::Off => off_node,
            })
            .collect();
        let off = store.get(KEY_OFF_STAMP).and_then(|v| EcStamp::parse(&v));
        Self {
            ec,
            clock,
            leds,
            store,
            levels,
            off: Mutex::new(off),
        }
    }

    /// Separate from the setter so a server can refuse a level before it
    /// prompts for authorization. Both ways of being impossible are answered
    /// here: `Custom` is what the EC reports after a percentage write, not a
    /// level to set, and `Off` needs a kernel node to hold the LED with.
    pub fn check_level(&self, level: PowerLedLevel) -> DeviceResult<()> {
        self.write_for(level).map(drop)
    }

    /// The EC accepts 1-100; 0 is rejected (it will not let the host
    /// extinguish the indicator) and 0xFF is the protocol's read sentinel.
    pub fn check_brightness(percent: u8) -> DeviceResult<()> {
        if (1..=100).contains(&percent) {
            Ok(())
        } else {
            Err(DeviceError::InvalidArgs("brightness must be 1-100".into()))
        }
    }

    fn write_for(&self, level: PowerLedLevel) -> DeviceResult<Write> {
        match level {
            PowerLedLevel::Off => self.leds.controllable().map(Write::Dark).ok_or_else(|| {
                DeviceError::NotSupported("no kernel LED node for the power LED".into())
            }),
            PowerLedLevel::Custom => Err(DeviceError::InvalidArgs(
                "custom is what the EC reports after a percentage write, not a level to set".into(),
            )),
            level => Ok(Write::Level(level)),
        }
    }

    /// The LED's node while the LED is off — the kernel holding it dark, on
    /// an EC that has not restarted since it was darkened — and None whenever
    /// it is lit. Answering with the node rather than a bool is what lets the
    /// caller that acts on it skip looking the LED up again.
    fn off_node(&self) -> Option<PathBuf> {
        let dir = self.leds.held_dark()?;
        // The stamp can only ever withdraw the kernel's account, never supply
        // one: a LED this device did not darken has no stamp to date, and the
        // kernel's record is then the only account of it there is.
        (*self.off.lock().unwrap())
            .is_none_or(|stamp| self.clock.same_boot_as(stamp).unwrap_or(false))
            .then_some(dir)
    }

    fn darken(&self, dir: &Path) -> DeviceResult<()> {
        // Dated before the write rather than after it, so a restart between
        // the two is read as having dropped it — and taken as best effort,
        // because the darkening itself is the kernel's and wants nothing from
        // the EC. An undatable write costs only the later detection that an
        // EC restart took the LED back; refusing it would cost the control
        // itself, on the strength of a read the write does not depend on.
        let stamp = self.clock.stamp().ok();
        self.leds.darken(dir)?;
        self.remember(stamp);
        Ok(())
    }

    /// Returns the LED to the EC if this device is holding it dark, so that a
    /// write of a level or a percentage is visible rather than swallowed by
    /// an LED the EC no longer drives. Only what the device itself arranged
    /// is undone.
    ///
    /// Waits [`LEVEL_SETTLE`] out first: the EC applies a level late, and
    /// lighting the LED before it lands shows the previous level — a flash of
    /// the old brightness on the way out of Off.
    async fn release(&self) {
        let Some(dir) = self.off_node() else {
            return;
        };
        Timer::after(LEVEL_SETTLE).await;
        let _ = self.leds.release(&dir);
        self.remember(None);
    }

    fn remember(&self, stamp: Option<EcStamp>) {
        // Absent rather than zeroed while the LED is lit: the stamp only ever
        // withdraws a claim, and a zeroed one would withdraw every time.
        self.store.set(KEY_OFF_STAMP, stamp.map(EcStamp::stored));
        *self.off.lock().unwrap() = stamp;
    }
}

impl PowerLedControl for PowerLed {
    async fn brightness(&self) -> DeviceResult<(u8, PowerLedLevel)> {
        let (percent, level) = self.ec.power_led_level()?;
        if self.off_node().is_some() {
            return Ok((percent, PowerLedLevel::Off));
        }
        Ok((percent, level))
    }

    async fn levels(&self) -> DeviceResult<Vec<PowerLedLevel>> {
        Ok(self.levels.clone())
    }

    /// The one path to the LED, and the order a change out of Off has to be
    /// made in: the level first, then the LED handed back. The release lives
    /// here rather than in each caller's memory of it, since an EC-driven
    /// write that skipped it would never be seen.
    async fn set_level(&self, level: PowerLedLevel) -> DeviceResult<()> {
        match self.write_for(level)? {
            Write::Dark(dir) => self.darken(&dir),
            Write::Level(level) => {
                self.ec.set_power_led_level(level)?;
                self.release().await;
                Ok(())
            }
        }
    }

    async fn set_brightness(&self, percent: u8) -> DeviceResult<()> {
        Self::check_brightness(percent)?;
        self.ec.set_power_led_percentage(percent)?;
        self.release().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};

    use frameguin_wire::{DeviceError, DeviceResult, PowerLedControl, PowerLedLevel};

    use super::{KEY_OFF_STAMP, PowerLed};
    use crate::ec::{EcClock, PowerLedEc};
    use crate::led::LedClass;
    use crate::lifetime::EcStamp;
    use crate::state::Store;
    use crate::state::tests::Memory;
    use crate::testing::ready;

    /// Every write the EC and the kernel took, in the order they took them.
    type Log = Arc<Mutex<Vec<String>>>;

    /// An EC holding one level, logging every write, and refusing them all
    /// once told to.
    struct Fp {
        level: Mutex<(u8, PowerLedLevel)>,
        custom: bool,
        refusing: bool,
        log: Log,
    }

    impl PowerLedEc for Fp {
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

    /// A clock answering one way about every stamp, or not at all.
    struct Clock {
        same_boot: Mutex<Option<bool>>,
    }

    impl Clock {
        fn answer(&self) -> DeviceResult<bool> {
            self.same_boot
                .lock()
                .unwrap()
                .ok_or_else(|| DeviceError::Failed("no EC".into()))
        }
    }

    impl EcClock for Clock {
        fn stamp(&self) -> DeviceResult<EcStamp> {
            self.answer().map(|_| EcStamp::taken(500, 1_000_000))
        }

        fn same_boot_as(&self, _stamp: EcStamp) -> DeviceResult<bool> {
            self.answer()
        }
    }

    /// A LED class with one node, or none, keeping the kernel's account of
    /// whether the LED is held dark.
    struct Leds {
        node: Option<PathBuf>,
        dark: Mutex<bool>,
        log: Log,
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

        fn darken(&self, _dir: &Path) -> io::Result<()> {
            *self.dark.lock().unwrap() = true;
            self.log.lock().unwrap().push("darken".into());
            Ok(())
        }

        fn release(&self, _dir: &Path) -> io::Result<()> {
            *self.dark.lock().unwrap() = false;
            self.log.lock().unwrap().push("release".into());
            Ok(())
        }
    }

    struct Machine {
        custom: bool,
        node: bool,
        refusing: bool,
        same_boot: Option<bool>,
    }

    const FULL: Machine = Machine {
        custom: true,
        node: true,
        refusing: false,
        same_boot: Some(true),
    };

    struct Bench {
        led: PowerLed,
        log: Log,
        clock: Arc<Clock>,
    }

    fn over(machine: &Machine, store: &Arc<Memory>) -> Bench {
        let log: Log = Arc::default();
        let ec = Arc::new(Fp {
            level: Mutex::new((55, PowerLedLevel::High)),
            custom: machine.custom,
            refusing: machine.refusing,
            log: log.clone(),
        });
        let clock = Arc::new(Clock {
            same_boot: Mutex::new(machine.same_boot),
        });
        let leds = Box::new(Leds {
            node: machine.node.then(|| PathBuf::from("/sys/class/leds/power")),
            dark: Mutex::new(false),
            log: log.clone(),
        });
        Bench {
            led: PowerLed::new(ec, clock.clone(), leds, store.clone()),
            log,
            clock,
        }
    }

    fn writes(log: &Log) -> Vec<String> {
        log.lock().unwrap().clone()
    }

    #[test]
    fn the_fixed_levels_are_every_firmwares_and_the_rest_are_earned() {
        let store = Arc::new(Memory::default());
        let bare = over(
            &Machine {
                custom: false,
                node: false,
                ..FULL
            },
            &store,
        );
        assert_eq!(
            ready(bare.led.levels()),
            Ok(vec![
                PowerLedLevel::High,
                PowerLedLevel::Medium,
                PowerLedLevel::Low
            ])
        );
        let full = over(&FULL, &store);
        assert_eq!(ready(full.led.levels()), Ok(PowerLedLevel::ALL.to_vec()));
    }

    #[test]
    fn off_darkens_through_the_kernel_and_dates_the_write() {
        let store = Arc::new(Memory::default());
        let Bench { led, log, .. } = over(&FULL, &store);
        ready(led.set_level(PowerLedLevel::Off)).unwrap();
        assert_eq!(writes(&log), ["darken"]);
        assert_eq!(ready(led.brightness()), Ok((55, PowerLedLevel::Off)));
        assert!(store.get(KEY_OFF_STAMP).is_some());
    }

    /// The level lands before the LED is handed back, so the EC lights it at
    /// the new level rather than flashing the old one.
    #[test]
    fn a_level_out_of_off_is_written_before_the_led_is_released() {
        let store = Arc::new(Memory::default());
        let Bench { led, log, .. } = over(&FULL, &store);
        ready(led.set_level(PowerLedLevel::Off)).unwrap();
        async_io::block_on(led.set_level(PowerLedLevel::Low)).unwrap();
        assert_eq!(writes(&log), ["darken", "level Low", "release"]);
        assert_eq!(ready(led.brightness()), Ok((55, PowerLedLevel::Low)));
        assert_eq!(store.get(KEY_OFF_STAMP), None);
    }

    #[test]
    fn a_percentage_out_of_off_releases_the_led_too() {
        let store = Arc::new(Memory::default());
        let Bench { led, log, .. } = over(&FULL, &store);
        ready(led.set_level(PowerLedLevel::Off)).unwrap();
        async_io::block_on(led.set_brightness(20)).unwrap();
        assert_eq!(writes(&log), ["darken", "percent 20", "release"]);
        assert_eq!(ready(led.brightness()), Ok((20, PowerLedLevel::Custom)));
    }

    /// Nothing to release where the LED was never held: a lit LED takes the
    /// level and nothing else happens.
    #[test]
    fn a_level_on_a_lit_led_touches_only_the_ec() {
        let store = Arc::new(Memory::default());
        let Bench { led, log, .. } = over(&FULL, &store);
        ready(led.set_level(PowerLedLevel::Medium)).unwrap();
        assert_eq!(writes(&log), ["level Medium"]);
    }

    #[test]
    fn a_write_the_ec_refuses_leaves_the_led_held() {
        let store = Arc::new(Memory::default());
        let Bench { led, log, .. } = over(
            &Machine {
                refusing: true,
                ..FULL
            },
            &store,
        );
        ready(led.set_level(PowerLedLevel::Off)).unwrap();
        assert!(ready(led.set_level(PowerLedLevel::High)).is_err());
        assert_eq!(writes(&log), ["darken"]);
        assert_eq!(ready(led.brightness()), Ok((55, PowerLedLevel::Off)));
    }

    /// An EC that restarted has taken the LED back, whatever the kernel's
    /// account still says.
    #[test]
    fn a_stamp_from_another_ec_boot_withdraws_the_kernels_account() {
        let store = Arc::new(Memory::default());
        let Bench { led, clock, .. } = over(&FULL, &store);
        ready(led.set_level(PowerLedLevel::Off)).unwrap();
        *clock.same_boot.lock().unwrap() = Some(false);
        assert_eq!(ready(led.brightness()), Ok((55, PowerLedLevel::High)));
    }

    /// A darkening the EC could not date is still made, and still reads as
    /// off: the kernel's account stands on its own where no stamp withdraws
    /// it.
    #[test]
    fn an_undatable_darkening_is_made_and_believed() {
        let store = Arc::new(Memory::default());
        let Bench { led, log, .. } = over(
            &Machine {
                same_boot: None,
                ..FULL
            },
            &store,
        );
        ready(led.set_level(PowerLedLevel::Off)).unwrap();
        assert_eq!(writes(&log), ["darken"]);
        assert_eq!(store.get(KEY_OFF_STAMP), None);
        assert_eq!(ready(led.brightness()), Ok((55, PowerLedLevel::Off)));
    }

    #[test]
    fn the_levels_the_ec_cannot_take_are_refused_before_anything_is_written() {
        let store = Arc::new(Memory::default());
        let Bench { led, log, .. } = over(
            &Machine {
                node: false,
                ..FULL
            },
            &store,
        );
        assert!(matches!(
            ready(led.set_level(PowerLedLevel::Off)),
            Err(DeviceError::NotSupported(_))
        ));
        assert!(matches!(
            ready(led.set_level(PowerLedLevel::Custom)),
            Err(DeviceError::InvalidArgs(_))
        ));
        for percent in [0, 101] {
            assert!(matches!(
                ready(led.set_brightness(percent)),
                Err(DeviceError::InvalidArgs(_))
            ));
        }
        assert!(writes(&log).is_empty());
    }
}
