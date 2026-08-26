//! Who owns the power button LED.
//!
//! It has two possible drivers — the EC's own policy and the kernel holding
//! it dark — and only one at a time. [`crate::led`] is the kernel half's
//! mechanism; this is the arbitration: which one holds it now, dating the
//! handover against the EC's life, and taking the LED back before any write
//! the EC has to be the one to make.

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_io::Timer;
use frameguin_wire as wire;
use framework_lib::chromium_ec::commands::FpLedBrightnessLevel;
use zbus::fdo;

use crate::ec::Ec;
use crate::{Daemon, ec, ec_err, internal_err, led};

/// How long the EC's own deferred hook takes to move the LED's PWM duty to a
/// level just written, plus margin — the hook is scheduled at 100 ms, not
/// promised for then. Waited out rather than polled: nothing the EC answers
/// reports the duty, only the level it will eventually become.
const LEVEL_SETTLE: Duration = Duration::from_millis(150);

/// A settled power LED write, resolved from its arguments before anyone is
/// asked to authorize one. `Dark` carries the LED's node because finding it is
/// half of deciding the write is possible at all.
pub(crate) enum PowerLedWrite {
    Level(FpLedBrightnessLevel),
    Percentage(u8),
    Dark(PathBuf),
}

impl PowerLedWrite {
    /// Which write a level asks for, settled before anyone is authorized —
    /// `Off` is the kernel's to make and the rest are the EC's, and both ways
    /// of being impossible are answered here rather than after a prompt.
    pub(crate) fn for_level(level: wire::PowerLedLevel) -> fdo::Result<Self> {
        if level == wire::PowerLedLevel::Off {
            return led::controllable_power().map(Self::Dark).ok_or_else(|| {
                fdo::Error::NotSupported("no kernel LED node for the power LED".into())
            });
        }
        // Off is answered above, so the level left without an EC setting is
        // the one the EC only ever reports.
        let Some(ec_level) = ec::ec_power_led_level(level) else {
            return Err(fdo::Error::InvalidArgs(
                "custom is what the EC reports after a percentage write, not a level to set".into(),
            ));
        };
        Ok(Self::Level(ec_level))
    }
}

impl Daemon {
    /// The LED's node when the power LED is off — the kernel holding it
    /// dark, on an EC that has not restarted since it was darkened — and None
    /// whenever it is lit. Answering with the node rather than a bool is what
    /// lets the caller that acts on it skip looking the LED up again.
    pub(crate) fn power_led_off_node(&self, ec: &Ec) -> Option<PathBuf> {
        let dir = led::power_held_dark()?;
        // The stamp can only ever withdraw the kernel's account, never supply
        // one: a LED this daemon did not darken has no stamp to date, and the
        // kernel's record is then the only account of it there is.
        (*self.power_led_off.lock().unwrap())
            .is_none_or(|stamp| ec.same_boot_as(stamp).unwrap_or(false))
            .then_some(dir)
    }

    /// The one path to the power LED, and the order a change out of Off
    /// has to be made in: the level first, then the LED handed back.
    ///
    /// The release lives in the write rather than in each caller's memory of
    /// it, since an EC-driven write that skipped it would never be seen.
    ///
    /// The `&Ec` handed down from here settles the no-EC case once for the
    /// whole write; it is not one lock spanning it, since each call through
    /// the handle takes and drops its own.
    pub(crate) async fn write_power_led(&self, write: PowerLedWrite) -> fdo::Result<()> {
        let ec = self.ec()?;
        match write {
            PowerLedWrite::Dark(dir) => self.darken_power_led(&dir, ec),
            PowerLedWrite::Level(level) => {
                ec.set_power_led_level(level).map_err(ec_err)?;
                self.release_power_led(ec).await;
                Ok(())
            }
            PowerLedWrite::Percentage(percent) => {
                ec.set_power_led_percentage(percent).map_err(ec_err)?;
                self.release_power_led(ec).await;
                Ok(())
            }
        }
    }

    fn darken_power_led(&self, dir: &Path, ec: &Ec) -> fdo::Result<()> {
        // Dated before the write rather than after it, so a restart between
        // the two is read as having dropped it.
        let stamp = ec.stamp().map_err(ec_err)?;
        led::darken(dir).map_err(internal_err)?;
        *self.power_led_off.lock().unwrap() = Some(stamp);
        self.save_state();
        Ok(())
    }

    /// Returns the LED to the EC if this daemon is holding it dark, so that a
    /// write of a level or a percentage is visible rather than swallowed by
    /// an LED the EC no longer drives. Only what the daemon itself arranged
    /// is undone.
    ///
    /// Waits [`LEVEL_SETTLE`] out first: the EC applies a level late, and
    /// lighting the LED before it lands shows the previous level — a flash of
    /// the old brightness on the way out of Off.
    async fn release_power_led(&self, ec: &Ec) {
        let Some(dir) = self.power_led_off_node(ec) else {
            return;
        };
        Timer::after(LEVEL_SETTLE).await;
        let _ = led::release(&dir);
        *self.power_led_off.lock().unwrap() = None;
        self.save_state();
    }
}
