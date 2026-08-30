//! The power button LED: the EC's levels, the kernel's off, and who holds
//! the LED now.
//!
//! It has two possible drivers — the EC's own policy and the kernel holding
//! it dark — and only one at a time. [`crate::led`] is the kernel half's
//! mechanism; this is the arbitration: which one holds it now, and taking
//! the LED back before any write the EC has to be the one to make.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_io::Timer;
use frameguin_wire::{DeviceError, DeviceResult, PowerLedControl, PowerLedLevel};

use crate::ec::{Ec, PowerLedEc};
use crate::led::{self, LedClass};

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
    leds: Box<dyn LedClass>,
    levels: Vec<PowerLedLevel>,
}

impl PowerLed {
    /// The LED the EC answers for, by the getter's own read.
    pub fn detect(ec: &Arc<Ec>) -> Option<Self> {
        ec.power_led_level().ok()?;
        Some(Self::new(ec.clone(), Box::new(led::Sysfs)))
    }

    /// Which levels the board has is settled here, once: the fixed levels on
    /// every firmware, the rest where the firmware takes a percentage, and
    /// off where the kernel has a node this could take and give back.
    pub fn new(ec: Arc<dyn PowerLedEc>, leds: Box<dyn LedClass>) -> Self {
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
        Self { ec, leds, levels }
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

    /// Returns the LED to the EC if the kernel is holding it dark, so that a
    /// write of a level or a percentage is visible rather than swallowed by
    /// an LED the EC no longer drives.
    ///
    /// Waits [`LEVEL_SETTLE`] out first: the EC applies a level late, and
    /// lighting the LED before it lands shows the previous level — a flash of
    /// the old brightness on the way out of Off.
    async fn release(&self) {
        let Some(dir) = self.leds.held_dark() else {
            return;
        };
        Timer::after(LEVEL_SETTLE).await;
        // A release the kernel refused shows as the LED still reading Off.
        let _ = self.leds.release(&dir);
    }
}

impl PowerLedControl for PowerLed {
    async fn brightness(&self) -> DeviceResult<(u8, PowerLedLevel)> {
        let (percent, level) = self.ec.power_led_level()?;
        if self.leds.held_dark().is_some() {
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
            Write::Dark(dir) => self.leds.darken(&dir),
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
    use std::sync::Arc;

    use frameguin_wire::{DeviceError, PowerLedControl, PowerLedLevel};

    use super::PowerLed;
    use crate::testing::{LedEc, Leds, Log, ready};

    struct Machine {
        custom: bool,
        node: bool,
        refusing: bool,
    }

    const FULL: Machine = Machine {
        custom: true,
        node: true,
        refusing: false,
    };

    struct Bench {
        led: PowerLed,
        log: Log,
    }

    fn over(machine: &Machine) -> Bench {
        let log = Log::default();
        let ec = Arc::new(LedEc {
            custom: machine.custom,
            refusing: machine.refusing,
            log: log.clone(),
            ..LedEc::default()
        });
        let leds = Box::new(Leds {
            node: machine.node.then(|| Leds::default().node).flatten(),
            log: log.clone(),
            ..Leds::default()
        });
        Bench {
            led: PowerLed::new(ec, leds),
            log,
        }
    }

    fn writes(log: &Log) -> Vec<String> {
        log.lock().unwrap().clone()
    }

    #[test]
    fn the_fixed_levels_are_every_firmwares_and_the_rest_are_earned() {
        let bare = over(&Machine {
            custom: false,
            node: false,
            ..FULL
        });
        assert_eq!(
            ready(bare.led.levels()),
            Ok(vec![
                PowerLedLevel::High,
                PowerLedLevel::Medium,
                PowerLedLevel::Low
            ])
        );
        let full = over(&FULL);
        assert_eq!(ready(full.led.levels()), Ok(PowerLedLevel::ALL.to_vec()));
    }

    #[test]
    fn off_darkens_through_the_kernel() {
        let Bench { led, log } = over(&FULL);
        ready(led.set_level(PowerLedLevel::Off)).unwrap();
        assert_eq!(writes(&log), ["darken"]);
        assert_eq!(ready(led.brightness()), Ok((55, PowerLedLevel::Off)));
    }

    /// The level lands before the LED is handed back, so the EC lights it at
    /// the new level rather than flashing the old one.
    #[test]
    fn a_level_out_of_off_is_written_before_the_led_is_released() {
        let Bench { led, log } = over(&FULL);
        ready(led.set_level(PowerLedLevel::Off)).unwrap();
        async_io::block_on(led.set_level(PowerLedLevel::Low)).unwrap();
        assert_eq!(writes(&log), ["darken", "level Low", "release"]);
        assert_eq!(ready(led.brightness()), Ok((55, PowerLedLevel::Low)));
    }

    #[test]
    fn a_percentage_out_of_off_releases_the_led_too() {
        let Bench { led, log } = over(&FULL);
        ready(led.set_level(PowerLedLevel::Off)).unwrap();
        async_io::block_on(led.set_brightness(20)).unwrap();
        assert_eq!(writes(&log), ["darken", "percent 20", "release"]);
        assert_eq!(ready(led.brightness()), Ok((20, PowerLedLevel::Custom)));
    }

    /// Nothing to release where the LED was never held: a lit LED takes the
    /// level and nothing else happens.
    #[test]
    fn a_level_on_a_lit_led_touches_only_the_ec() {
        let Bench { led, log } = over(&FULL);
        ready(led.set_level(PowerLedLevel::Medium)).unwrap();
        assert_eq!(writes(&log), ["level Medium"]);
    }

    #[test]
    fn a_write_the_ec_refuses_leaves_the_led_held() {
        let Bench { led, log } = over(&Machine {
            refusing: true,
            ..FULL
        });
        ready(led.set_level(PowerLedLevel::Off)).unwrap();
        assert!(ready(led.set_level(PowerLedLevel::High)).is_err());
        assert_eq!(writes(&log), ["darken"]);
        assert_eq!(ready(led.brightness()), Ok((55, PowerLedLevel::Off)));
    }

    #[test]
    fn the_levels_the_ec_cannot_take_are_refused_before_anything_is_written() {
        let Bench { led, log } = over(&Machine {
            node: false,
            ..FULL
        });
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
