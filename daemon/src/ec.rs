//! The embedded controller: one method per operation the daemon performs on
//! it, and the vocabulary the two ends spell those operations in.
//!
//! [`Ec`] is the only thing in the daemon holding a `CrosEc`. Every method
//! takes the lock and releases it before returning, and none calls another
//! through the handle — `Mutex` does not re-enter, so a method that wants two
//! commands under one lock issues both against the guard it already holds, as
//! [`Ec::set_charge_current_limit`] does.
//!
//! Two devices are deliberately absent: the fingerprint LED's off, which the
//! kernel arbitrates ([`crate::led`]), and the haptic touchpad, which
//! `framework_lib` drives over HID ([`crate::touchpad`]).

use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use frameguin_wire as wire;
use framework_lib::chromium_ec::command::{EcCommands, EcRequestRaw};
use framework_lib::chromium_ec::commands::{
    EcRequestGetUptimeInfo, EcRequestPwmGetKeyboardBacklight, FpLedBrightnessLevel,
};
use framework_lib::chromium_ec::{CrosEc, EcResult};
use framework_lib::power;

use crate::board;

pub(crate) struct Ec {
    ec: Mutex<CrosEc>,
    /// Read once and kept: reaching it walks the EC's whole memmap battery
    /// block, and a pack cannot change under a running daemon. Filled by
    /// whichever walk gets there first, so nothing spends a round trip on it
    /// alone.
    design_capacity: OnceLock<u32>,
}

impl Ec {
    /// The EC, and None on hardware that has none. `CrosEc::new()` panics
    /// outright when `framework_lib` finds no driver (an empty driver list on
    /// e.g. aarch64 without `/dev/cros_ec`), so the vendor check is what keeps
    /// it from being constructed there rather than a courtesy.
    pub(crate) fn open() -> Option<Self> {
        board::is_framework().then(|| Self {
            ec: Mutex::new(CrosEc::new()),
            design_capacity: OnceLock::new(),
        })
    }

    fn ec(&self) -> MutexGuard<'_, CrosEc> {
        self.ec.lock().unwrap()
    }

    /// The ceiling the EC holds. Its command answers with a floor as well,
    /// which nothing here sets or reports.
    pub(crate) fn charge_limit(&self) -> EcResult<u8> {
        let (_min, max) = self.ec().get_charge_limit()?;
        Ok(max)
    }

    pub(crate) fn set_charge_limit(&self, percent: u8) -> EcResult<()> {
        self.ec().set_charge_limit(0, percent)
    }

    /// Caps the charging current and dates the write in one lock: the stamp is
    /// only worth anything taken against the same EC the write reached.
    ///
    /// Always the unconditional form. The command's state-of-charge variant
    /// latches inside the EC: once applied it is never re-evaluated, so a
    /// later threshold cannot lift it (framework-system issue #342).
    pub(crate) fn set_charge_current_limit(&self, milliamps: u32) -> EcResult<EcStamp> {
        let ec = self.ec();
        ec.set_charge_current_limit(milliamps, None)?;
        EcStamp::now(&ec)
    }

    /// The EC's whole memmap battery block, and the one place the design
    /// capacity is cached — so a walk made for any other answer pays for the
    /// next caller that wants only the capacity.
    fn power(&self) -> Option<power::PowerInfo> {
        let info = power::power_info(&self.ec())?;
        if let Some(battery) = &info.battery {
            let _ = self.design_capacity.set(battery.design_capacity);
        }
        Some(info)
    }

    /// The EC's battery block in the wire's terms, and None when no pack
    /// answers. Direction and charger presence share one flag byte, so both
    /// come from the same read; the rate is unsigned whichever way charge is
    /// moving.
    pub(crate) fn battery_state(&self) -> Option<wire::BatteryState> {
        let info = self.power()?;
        let battery = info.battery?;
        Some(wire::BatteryState {
            // Against the last full charge, which is the EC's own denominator;
            // a pack reporting more than full is clamped rather than shown.
            percent: u8::try_from(battery.charge_percentage.min(100)).unwrap_or(100),
            flow: charge_flow(ChargeSignals {
                charging: battery.charging,
                discharging: battery.discharging,
                ac_present: info.ac_present,
                milliamps: battery.present_rate,
            }),
            milliamps: battery.present_rate,
            millivolts: battery.present_voltage,
        })
    }

    /// The battery's design capacity in mAh, `None` when no pack answers. Only
    /// successful reads are remembered, so a battery that is momentarily
    /// unreadable isn't taken for an absent one for the rest of the run.
    pub(crate) fn design_capacity(&self) -> Option<u32> {
        if let Some(capacity) = self.design_capacity.get() {
            return Some(*capacity);
        }
        Some(self.power()?.battery?.design_capacity)
    }

    /// `framework_lib`'s `get_keyboard_backlight()` reads via `PWM_GET_DUTY`,
    /// and the percent survives two floor divisions (percent→duty in the EC,
    /// then duty→percent in the lib), coming back one low for most values —
    /// 5% reads as 4%. This EC command returns the exact stored percent.
    pub(crate) fn keyboard_backlight(&self) -> EcResult<u8> {
        Ok(EcRequestPwmGetKeyboardBacklight {}
            .send_command(&self.ec())?
            .percent)
    }

    pub(crate) fn set_keyboard_backlight(&self, percent: u8) {
        self.ec().set_keyboard_backlight(percent);
    }

    /// The brightness percentage and the level the EC reports it as. `Custom`
    /// is what it answers after any raw percentage write.
    pub(crate) fn fp_level(&self) -> EcResult<(u8, wire::FpLevel)> {
        let (percent, level) = self.ec().get_fp_led_level()?;
        Ok((percent, wire_fp_level(level.as_ref())))
    }

    pub(crate) fn set_fp_level(&self, level: FpLedBrightnessLevel) -> EcResult<()> {
        self.ec().set_fp_led_level(level)
    }

    pub(crate) fn set_fp_percentage(&self, percent: u8) -> EcResult<()> {
        self.ec().set_fp_led_percentage(percent)
    }

    pub(crate) fn version(&self) -> EcResult<String> {
        self.ec().version_info()
    }

    /// Whether the firmware implements a command at the given version — and
    /// `Err` when the EC would not say, which is not the same answer. What to
    /// make of a silent EC is the caller's to decide.
    pub(crate) fn command_supported(&self, command: EcCommands, version: u8) -> EcResult<bool> {
        self.ec().cmd_version_supported(command as u32, version)
    }

    /// Dates a write about to be made against the EC's own life.
    pub(crate) fn stamp(&self) -> EcResult<EcStamp> {
        EcStamp::now(&self.ec())
    }

    /// Whether the EC has been running without interruption since `stamp` was
    /// taken — which is to say whether what it was holding then is still there.
    pub(crate) fn same_boot_as(&self, stamp: EcStamp) -> EcResult<bool> {
        Ok(stamp.same_boot(uptime_secs(&self.ec())?, unix_now()))
    }
}

/// A write dated against the EC's own life: seconds the EC had been running
/// when it happened, paired with the wall time of that same moment.
#[derive(Clone, Copy, Default)]
pub(crate) struct EcStamp {
    pub(crate) ec_uptime: u64,
    pub(crate) written_at: u64,
}

impl EcStamp {
    /// Taken against both clocks at once, which is what makes a later reading
    /// of the EC's comparable to the host's.
    fn now(ec: &CrosEc) -> EcResult<Self> {
        Ok(Self {
            ec_uptime: uptime_secs(ec)?,
            written_at: unix_now(),
        })
    }

    /// Whether the EC has been running without interruption since. An EC that
    /// has been up for less time than the write implies has restarted, and a
    /// restart drops everything the EC was holding in RAM. The comparison
    /// carries slack because the EC keeps its own time — its firmware
    /// documents 1% or worse frequency error against the host clock.
    ///
    /// EC uptime is a 32-bit millisecond counter, so this reads as a restart
    /// once every 49 days of EC uptime; what was written then shows as gone
    /// until it is set again.
    fn same_boot(self, ec_uptime: u64, now: u64) -> bool {
        let expected = self.ec_uptime + now.saturating_sub(self.written_at);
        expected.saturating_sub(ec_uptime) <= (expected / 20).max(60)
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Seconds since the EC last booted.
fn uptime_secs(ec: &CrosEc) -> EcResult<u64> {
    let uptime_ms = EcRequestGetUptimeInfo {}
        .send_command(ec)?
        .time_since_ec_boot;
    Ok(u64::from(uptime_ms) / 1000)
}

/// None for the levels the EC has no setting for: `Custom`, which it only
/// ever reports, and `Off`, which is not the EC's to give.
pub(crate) fn ec_fp_level(level: wire::FpLevel) -> Option<FpLedBrightnessLevel> {
    Some(match level {
        wire::FpLevel::High => FpLedBrightnessLevel::High,
        wire::FpLevel::Medium => FpLedBrightnessLevel::Medium,
        wire::FpLevel::Low => FpLedBrightnessLevel::Low,
        wire::FpLevel::UltraLow => FpLedBrightnessLevel::UltraLow,
        wire::FpLevel::Auto => FpLedBrightnessLevel::Auto,
        wire::FpLevel::Custom | wire::FpLevel::Off => return None,
    })
}

/// A level the EC does not name is custom: that is what it reports after a
/// raw percentage write.
fn wire_fp_level(level: Option<&FpLedBrightnessLevel>) -> wire::FpLevel {
    match level {
        Some(FpLedBrightnessLevel::High) => wire::FpLevel::High,
        Some(FpLedBrightnessLevel::Medium) => wire::FpLevel::Medium,
        Some(FpLedBrightnessLevel::Low) => wire::FpLevel::Low,
        Some(FpLedBrightnessLevel::UltraLow) => wire::FpLevel::UltraLow,
        Some(FpLedBrightnessLevel::Auto) => wire::FpLevel::Auto,
        Some(FpLedBrightnessLevel::Custom) | None => wire::FpLevel::Custom,
    }
}

/// The readings a direction is decided from, carried together because
/// `charging`, `discharging` and `ac_present` are bare booleans: named fields
/// are what stops a caller permuting them into a well-formed call that
/// answers a different question.
#[derive(Clone, Copy, Default)]
struct ChargeSignals {
    charging: bool,
    discharging: bool,
    ac_present: bool,
    milliamps: u32,
}

/// What the pack is doing, from the EC's charging flag, its charger flag and
/// the rate.
///
/// Neither flag set is a state of its own, and the one the ceiling produces:
/// the EC clears both while its charge limiter holds the pack there, which is
/// what ACPI's charge-limiting convention asks of it so the host stops drawing
/// a direction. The charge current decays for as long as a minute after the
/// limiter engages, and taking that decay for the pack running the machine
/// names tens of watts leaving a battery that is losing none.
///
/// Set on its own, the discharging flag means "not being charged" rather than
/// "supplying the machine" — a pack sitting full on a charger sets it. The
/// rate is what separates that pack from one running the machine, and it
/// reads a clean 0 at rest. A charger attached does not by itself mean
/// nothing is draining, since too weak a one leaves the pack covering the
/// difference, and the EC does flag that as discharging.
fn charge_flow(
    ChargeSignals {
        charging,
        discharging,
        ac_present,
        milliamps,
    }: ChargeSignals,
) -> wire::ChargeFlow {
    let draining = discharging && milliamps > 0;
    if charging {
        wire::ChargeFlow::Charging
    } else if ac_present && !draining {
        wire::ChargeFlow::Idle
    } else {
        wire::ChargeFlow::Discharging
    }
}

#[cfg(test)]
mod tests {
    use super::{ChargeSignals, EcStamp, charge_flow, ec_fp_level, wire, wire_fp_level};

    /// A charger attached and the pack held at its ceiling: the EC claiming
    /// no direction, nothing moving. Each case below names only what it
    /// changes from that.
    fn at_the_ceiling() -> ChargeSignals {
        ChargeSignals {
            ac_present: true,
            ..ChargeSignals::default()
        }
    }

    /// The two tables are written out separately — `FpLedBrightnessLevel`
    /// derives no `PartialEq`, so neither can be derived from the other the
    /// way the touchpad's forces are. Swapping two arms in one direction
    /// alone would compile, and would report back a level nobody set.
    #[test]
    fn every_level_the_ec_has_a_setting_for_comes_back_as_itself() {
        for level in wire::FpLevel::ALL {
            if let Some(ec_level) = ec_fp_level(level) {
                assert_eq!(wire_fp_level(Some(&ec_level)), level);
            }
        }
    }

    /// The state a full laptop sits in all day, and the one the EC's own
    /// flags describe as discharging. Reading that flag put "Discharging" on
    /// a machine that was plugged in and full.
    #[test]
    fn a_full_pack_on_its_charger_is_not_discharging() {
        let full = ChargeSignals {
            discharging: true,
            ..at_the_ceiling()
        };
        assert_eq!(charge_flow(full), wire::ChargeFlow::Idle);
    }

    /// The decaying window the function's own doc describes: 303 mA is well
    /// into the fall, not a pack running the machine.
    #[test]
    fn a_pack_held_at_its_ceiling_is_not_discharging_while_its_current_decays() {
        let decaying = ChargeSignals {
            milliamps: 303,
            ..at_the_ceiling()
        };
        assert_eq!(charge_flow(decaying), wire::ChargeFlow::Idle);
    }

    /// A charger too weak for the load leaves the pack covering the
    /// difference, which the EC flags as discharging.
    #[test]
    fn a_pack_draining_under_a_weak_charger_is_discharging() {
        let weak = ChargeSignals {
            discharging: true,
            milliamps: 900,
            ..at_the_ceiling()
        };
        assert_eq!(charge_flow(weak), wire::ChargeFlow::Discharging);
    }

    #[test]
    fn nothing_attached_leaves_the_pack_running_the_machine() {
        let unplugged = ChargeSignals {
            discharging: true,
            milliamps: 1400,
            ..ChargeSignals::default()
        };
        assert_eq!(charge_flow(unplugged), wire::ChargeFlow::Discharging);
        // Between two readings a pack can report no rate at all; with no
        // charger it is still the only thing powering the machine.
        let unplugged_at_rest = ChargeSignals {
            milliamps: 0,
            ..unplugged
        };
        assert_eq!(
            charge_flow(unplugged_at_rest),
            wire::ChargeFlow::Discharging
        );
        // The limiter's own state cannot arise off a charger, but a pack with
        // nothing attached is running the machine whatever the flags say.
        let unplugged_unflagged = ChargeSignals {
            discharging: false,
            ..unplugged
        };
        assert_eq!(
            charge_flow(unplugged_unflagged),
            wire::ChargeFlow::Discharging
        );
    }

    /// `framework_lib` leaves open whether both flags can stand at once.
    #[test]
    fn charge_arriving_outranks_the_rest() {
        let charging = ChargeSignals {
            charging: true,
            discharging: true,
            ac_present: true,
            milliamps: 2320,
        };
        assert_eq!(charge_flow(charging), wire::ChargeFlow::Charging);
    }

    fn taken(ec_uptime: u64, written_at: u64) -> EcStamp {
        EcStamp {
            ec_uptime,
            written_at,
        }
    }

    #[test]
    fn a_write_moments_ago_is_still_the_same_boot() {
        let stamp = taken(500_000, 1_000_000);
        assert!(stamp.same_boot(500_002, 1_000_002));
    }

    #[test]
    fn an_ec_that_has_run_the_elapsed_time_is_still_the_same_boot() {
        // A day passes with the EC up throughout.
        let stamp = taken(500_000, 1_000_000);
        assert!(stamp.same_boot(586_400, 1_086_400));
    }

    #[test]
    fn an_ec_that_restarted_is_a_different_boot() {
        // An hour of wall clock, but the EC reports a minute of uptime.
        let stamp = taken(500_000, 1_000_000);
        assert!(!stamp.same_boot(60, 1_003_600));
    }

    /// The EC's own clock is documented as 1% or worse against the host's, so
    /// a tolerance that didn't scale would call a long-standing write expired.
    #[test]
    fn clock_drift_over_a_long_uptime_is_not_a_restart() {
        let stamp = taken(0, 1_000_000);
        // Ten days later the EC is 1% short of the elapsed wall time.
        let elapsed = 10 * 86_400;
        assert!(stamp.same_boot(elapsed - elapsed / 100, 1_000_000 + elapsed));
    }

    #[test]
    fn a_recent_write_gets_the_floor_not_the_percentage() {
        // Seconds after the write, five percent of nothing is nothing, so the
        // 60s floor is what keeps a fresh one from reading as expired.
        let stamp = taken(10, 1_000_000);
        assert!(stamp.same_boot(10, 1_000_030));
        assert!(!stamp.same_boot(10, 1_000_200));
    }
}
