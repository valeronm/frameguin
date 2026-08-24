//! Who owns the fingerprint LED.
//!
//! It has two possible drivers — the EC's own policy and the kernel holding
//! it dark — and only one at a time. [`crate::led`] is the kernel half's
//! mechanism; this is the arbitration: which one holds it now, dating the
//! handover against the EC's life, and taking the LED back before any write
//! the EC has to be the one to make.

use std::path::{Path, PathBuf};

use frameguin_wire as wire;
use framework_lib::chromium_ec::commands::FpLedBrightnessLevel;
use zbus::fdo;

use crate::ec::Ec;
use crate::{Daemon, ec, ec_err, internal_err, led};

/// A settled fingerprint write, resolved from its arguments before anyone is
/// asked to authorize one. `Dark` carries the LED's node because finding it is
/// half of deciding the write is possible at all.
pub(crate) enum FpWrite {
    Level(FpLedBrightnessLevel),
    Percentage(u8),
    Dark(PathBuf),
}

impl FpWrite {
    /// Which write a level asks for, settled before anyone is authorized —
    /// `Off` is the kernel's to make and the rest are the EC's, and both ways
    /// of being impossible are answered here rather than after a prompt.
    pub(crate) fn for_level(level: wire::FpLevel) -> fdo::Result<Self> {
        if level == wire::FpLevel::Off {
            return led::controllable_power().map(FpWrite::Dark).ok_or_else(|| {
                fdo::Error::NotSupported("no kernel LED node for the power LED".into())
            });
        }
        // Off is answered above, so the level left without an EC setting is
        // the one the EC only ever reports.
        ec::ec_fp_level(level).map(FpWrite::Level).ok_or_else(|| {
            fdo::Error::InvalidArgs(
                "custom is what the EC reports after a percentage write, not a level to set".into(),
            )
        })
    }
}

impl Daemon {
    /// The LED's node when the fingerprint LED is off — the kernel holding it
    /// dark, on an EC that has not restarted since it was darkened — and None
    /// whenever it is lit. Answering with the node rather than a bool is what
    /// lets the caller that acts on it skip looking the LED up again.
    pub(crate) fn fp_off_led(&self, ec: &Ec) -> Option<PathBuf> {
        let dir = led::controllable_power()?;
        if !led::dark_in_kernel(&dir) {
            return None;
        }
        // The stamp can only ever withdraw the kernel's account, never supply
        // one: a LED this daemon did not darken has no stamp to date, and the
        // kernel's record is then the only account of it there is.
        match *self.fp_off.lock().unwrap() {
            Some(stamp) => ec.same_boot_as(stamp).unwrap_or(false).then_some(dir),
            None => Some(dir),
        }
    }

    /// The one path to the fingerprint LED. An EC-driven write has to have the
    /// LED back before it goes out, or it lands on one the host is holding and
    /// the EC no longer lights — so that release belongs here, in the write
    /// itself, rather than in each caller's memory of it.
    ///
    /// The `&Ec` handed down from here settles the no-EC case once for the
    /// whole write; it is not one lock spanning it, since each call through
    /// the handle takes and drops its own.
    pub(crate) fn write_fingerprint(&self, write: FpWrite) -> fdo::Result<()> {
        let ec = self.ec()?;
        match write {
            FpWrite::Dark(dir) => self.darken_fp_led(&dir, ec),
            FpWrite::Level(level) => {
                self.release_fp_led(ec);
                ec.set_fp_level(level).map_err(ec_err)
            }
            FpWrite::Percentage(percent) => {
                self.release_fp_led(ec);
                ec.set_fp_percentage(percent).map_err(ec_err)
            }
        }
    }

    fn darken_fp_led(&self, dir: &Path, ec: &Ec) -> fdo::Result<()> {
        // Dated before the write rather than after it, so a restart between
        // the two is read as having dropped it.
        let stamp = ec.stamp().map_err(ec_err)?;
        led::darken(dir).map_err(internal_err)?;
        *self.fp_off.lock().unwrap() = Some(stamp);
        self.save_state();
        Ok(())
    }

    /// Returns the LED to the EC if this daemon is holding it dark, so that a
    /// write of a level or a percentage is visible rather than swallowed by
    /// an LED the EC no longer drives. Only what the daemon itself arranged
    /// is undone.
    fn release_fp_led(&self, ec: &Ec) {
        let Some(dir) = self.fp_off_led(ec) else {
            return;
        };
        let _ = led::release(&dir);
        *self.fp_off.lock().unwrap() = None;
        self.save_state();
    }
}
