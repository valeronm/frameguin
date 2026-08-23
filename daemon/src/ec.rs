//! What the embedded controller is asked, and the vocabulary the two ends
//! spell it in.
//!
//! Not every EC call: the plain ones are sent from the bus method that wants
//! them. What lands here is a call needing more than the raw command — a
//! correction, a cache, or a translation between the EC's terms and the
//! wire's. Two devices are deliberately absent: the fingerprint LED's off,
//! which the kernel arbitrates ([`crate::led`]), and the haptic touchpad,
//! which `framework_lib` drives over HID ([`crate::touchpad`]).

use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use frameguin_wire as wire;
use framework_lib::chromium_ec::command::EcRequestRaw;
use framework_lib::chromium_ec::commands::{
    EcRequestGetUptimeInfo, EcRequestPwmGetKeyboardBacklight, FpLedBrightnessLevel,
};
use framework_lib::chromium_ec::{CrosEc, EcResult};
use framework_lib::power;

use crate::board;

/// The EC handle, and None on hardware that has none. `CrosEc::new()` panics
/// outright when `framework_lib` finds no driver (an empty driver list on
/// e.g. aarch64 without `/dev/cros_ec`), so the vendor check is what keeps it
/// from being constructed there rather than a courtesy.
pub(crate) fn open() -> Option<Mutex<CrosEc>> {
    board::is_framework().then(|| Mutex::new(CrosEc::new()))
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
    pub(crate) fn now(ec: &CrosEc) -> EcResult<Self> {
        Ok(Self {
            ec_uptime: ec_uptime_secs(ec)?,
            written_at: unix_now(),
        })
    }

    pub(crate) fn still_current(self, ec: &CrosEc) -> EcResult<bool> {
        Ok(self.same_boot(ec_uptime_secs(ec)?, unix_now()))
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
fn ec_uptime_secs(ec: &CrosEc) -> EcResult<u64> {
    let uptime_ms = EcRequestGetUptimeInfo {}.send_command(ec)?.time_since_ec_boot;
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
pub(crate) fn wire_fp_level(level: Option<&FpLedBrightnessLevel>) -> wire::FpLevel {
    match level {
        Some(FpLedBrightnessLevel::High) => wire::FpLevel::High,
        Some(FpLedBrightnessLevel::Medium) => wire::FpLevel::Medium,
        Some(FpLedBrightnessLevel::Low) => wire::FpLevel::Low,
        Some(FpLedBrightnessLevel::UltraLow) => wire::FpLevel::UltraLow,
        Some(FpLedBrightnessLevel::Auto) => wire::FpLevel::Auto,
        Some(FpLedBrightnessLevel::Custom) | None => wire::FpLevel::Custom,
    }
}

/// `framework_lib`'s `get_keyboard_backlight()` reads via `PWM_GET_DUTY`, and the
/// percent survives two floor divisions (percent→duty in the EC, then
/// duty→percent in the lib), coming back one low for most values — 5% reads
/// as 4%. This EC command returns the exact stored percent instead.
pub(crate) fn kbd_backlight_percent(ec: &CrosEc) -> EcResult<u8> {
    Ok(EcRequestPwmGetKeyboardBacklight {}.send_command(ec)?.percent)
}

/// The design capacity, kept for the process rather than per `Daemon`: a pack
/// cannot change under a running daemon, and reaching it walks the EC's whole
/// memmap battery block. Set only from a walk already being made for some
/// other answer, so nothing here spends a round trip on it alone.
static DESIGN_CAPACITY: OnceLock<u32> = OnceLock::new();

/// The EC's battery block in the wire's terms, and None when no pack
/// answers. Direction and charger presence share one flag byte, so both come
/// from the same read; the rate is unsigned whichever way charge is moving.
pub(crate) fn battery_state(ec: &CrosEc) -> Option<wire::BatteryState> {
    let info = power::power_info(ec)?;
    let battery = info.battery?;
    // This walk already carries the capacity, so the reading that runs every
    // couple of seconds is what spares the first caller its own walk.
    let _ = DESIGN_CAPACITY.set(battery.design_capacity);
    Some(wire::BatteryState {
        // Against the last full charge, which is the EC's own denominator;
        // a pack reporting more than full is clamped rather than shown.
        percent: u8::try_from(battery.charge_percentage.min(100)).unwrap_or(100),
        flow: charge_flow(battery.charging, info.ac_present, battery.present_rate),
        milliamps: battery.present_rate,
    })
}

/// The battery's design capacity in mAh, `None` when no pack answers. Only
/// successful reads are remembered, so a battery that is momentarily
/// unreadable isn't taken for an absent one for the rest of the run.
pub(crate) fn battery_design_capacity(ec: &CrosEc) -> Option<u32> {
    if let Some(capacity) = DESIGN_CAPACITY.get() {
        return Some(*capacity);
    }
    let capacity = power::power_info(ec)?.battery?.design_capacity;
    let _ = DESIGN_CAPACITY.set(capacity);
    Some(capacity)
}

/// What the pack is doing, from the EC's charging flag, its charger flag and
/// the rate.
///
/// The discharging flag is deliberately not a parameter: it means "not being
/// charged" rather than "supplying the machine", and a full pack on a
/// connected charger sets it — a smart battery reporting zero charge
/// current. The rate is what separates a pack at rest from one running the
/// machine, and it reads a clean 0 at rest; a charger attached does not by
/// itself mean nothing is draining, since too weak a one leaves the pack
/// covering the difference.
fn charge_flow(charging: bool, ac_present: bool, milliamps: u32) -> wire::ChargeFlow {
    if charging {
        wire::ChargeFlow::Charging
    } else if ac_present && milliamps == 0 {
        wire::ChargeFlow::Idle
    } else {
        wire::ChargeFlow::Discharging
    }
}

#[cfg(test)]
mod tests {
    use super::{EcStamp, charge_flow, ec_fp_level, wire, wire_fp_level};

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
        assert_eq!(charge_flow(false, true, 0), wire::ChargeFlow::Idle);
    }

    /// A charger too weak for the load leaves the pack covering the
    /// difference, which the rate is the only witness to.
    #[test]
    fn a_pack_draining_under_a_weak_charger_is_discharging() {
        assert_eq!(charge_flow(false, true, 900), wire::ChargeFlow::Discharging);
    }

    #[test]
    fn nothing_attached_leaves_the_pack_running_the_machine() {
        assert_eq!(charge_flow(false, false, 1400), wire::ChargeFlow::Discharging);
        // Between two readings a pack can report no rate at all; with no
        // charger it is still the only thing powering the machine.
        assert_eq!(charge_flow(false, false, 0), wire::ChargeFlow::Discharging);
    }

    #[test]
    fn charge_arriving_outranks_the_rest() {
        assert_eq!(charge_flow(true, true, 2320), wire::ChargeFlow::Charging);
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
