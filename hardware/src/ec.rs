//! The embedded controller: one method per operation the daemon performs on
//! it.
//!
//! [`Ec`] is the only thing in the daemon holding a `CrosEc`. Every method
//! takes the lock and releases it before returning, and none calls another
//! through the handle — `Mutex` does not re-enter, so a method that wants two
//! commands under one lock issues both against the guard it already holds, as
//! [`Ec::set_charge_current_limit`] does.
//!
//! Two devices are deliberately absent: the power LED's off, which the
//! kernel arbitrates ([`crate::led`]), and the haptic touchpad, which
//! `framework_lib` drives over HID ([`crate::touchpad`]).

use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use frameguin_wire::{self as wire, DeviceError, DeviceResult};
use framework_lib::chromium_ec::command::{EcCommands, EcRequestRaw};
use framework_lib::chromium_ec::commands::{EcRequestGetUptimeInfo, FpLedBrightnessLevel};
use framework_lib::chromium_ec::i2c_passthrough::i2c_read;
use framework_lib::chromium_ec::{CrosEc, EcResult};
use framework_lib::power;

use crate::dmi;
use crate::lifetime::EcBoot;
use crate::part::{self, Identity};
use crate::sbs;

/// The EC's I2C port the pack hangs off, the same on every Framework board
/// — one Nuvoton EC.
const BATTERY_I2C_PORT: u8 = 3;

/// What the power LED's device needs of the EC: the level it holds, the two
/// writes that move it, and whether the firmware has the levels that came
/// with command v1.
pub trait PowerLedEc: Send + Sync {
    /// The brightness percentage and the level the EC reports it as.
    /// `Custom` is what it answers after any raw percentage write.
    fn power_led_level(&self) -> DeviceResult<(u8, wire::PowerLedLevel)>;
    /// Refuses `Custom` and `Off`, the two levels the EC has no setting for.
    fn set_power_led_level(&self, level: wire::PowerLedLevel) -> DeviceResult<()>;
    fn set_power_led_percentage(&self, percent: u8) -> DeviceResult<()>;
    /// Whether the firmware takes a raw percentage, and with it the
    /// ultra-low and auto levels.
    fn custom_power_led_levels(&self) -> bool;
}

/// What the battery's device needs of the pack: whether one answers in the
/// EC's block and what it is, the block itself, and what the pack says past
/// it.
pub trait Pack: Send + Sync {
    /// The pack as a part, and None where none answers in the block. This is
    /// the presence check, and it reads the block rather than the report a
    /// caller would build from it — the report's own reads behind the cycle
    /// count and the manufacturing date run once per run and remember an
    /// absence, and one unlucky transfer here would fix that for the whole
    /// of it.
    fn identity(&self) -> Option<Identity>;
    fn info(&self) -> Option<wire::BatteryInfo>;
    fn condition(&self) -> Option<wire::BatteryCondition>;
}

/// What the battery's device needs of the charger: the ceiling, and the
/// current cap.
pub trait Charger: Send + Sync {
    fn charge_limit(&self) -> DeviceResult<u8>;
    fn set_charge_limit(&self, percent: u8) -> DeviceResult<()>;
    fn set_charge_current_limit(&self, milliamps: u32) -> DeviceResult<()>;
    /// Whether the firmware implements the current cap at all, there being
    /// no readback to probe it by.
    fn charge_current_limit_supported(&self) -> bool;
}

/// An EC failure as a device raises it.
fn device_error(e: impl std::fmt::Debug) -> DeviceError {
    DeviceError::Failed(format!("EC error: {e:?}"))
}

/// The daemon's one way of asking the embedded controller anything.
///
/// Where an answer comes from is this type's business and no caller's: most
/// are read on the spot, a few are remembered, and nothing outside this module
/// can tell which — that is what makes remembering more of them later a change
/// here rather than everywhere. A value is remembered only where asking again
/// could not change what the answer settles.
pub struct Ec {
    ec: Mutex<CrosEc>,
    memo: Memo,
}

/// What this run has already learned from the EC and will not ask for again.
///
/// Private, so a remembered answer can never become part of a caller's
/// vocabulary: everything here is reached through the method that would
/// otherwise have done the reading.
#[derive(Default)]
struct Memo {
    /// The one value here that can change while it is held — see
    /// [`Ec::cycle_count`]. A cycle takes hours to accumulate and this daemon
    /// exits after five idle minutes, so "will not have changed" stands in for
    /// the "cannot have changed" the rest of this struct is held to.
    cycle_count: OnceLock<Option<u32>>,
    /// When the pack was built, which the EC publishes nowhere and which
    /// cannot change at all.
    manufacture_date: OnceLock<Option<String>>,
}

/// Remembers what a read answered, absence included, and asks only once.
///
/// The two entries below reach the pack over I2C, and both are read on every
/// walk of the battery block — which the window's charge row asks for every
/// couple of seconds, not just the report. So what has to be remembered is the
/// *answer* rather than the success: a pack that keeps no manufacturing date,
/// or a board whose passthrough does not answer at all, would otherwise be
/// asked again on every one of those walks, forever, for something that cannot
/// arrive. The daemon exits after five idle minutes, which bounds how long a
/// remembered absence stands.
fn remembered<T: Clone>(slot: &OnceLock<Option<T>>, read: impl FnOnce() -> Option<T>) -> Option<T> {
    slot.get_or_init(read).clone()
}

impl Ec {
    /// The EC, and None on hardware that has none. `CrosEc::new()` panics
    /// outright when `framework_lib` finds no driver (an empty driver list on
    /// e.g. aarch64 without `/dev/cros_ec`), so the vendor check is what keeps
    /// it from being constructed there rather than a courtesy.
    pub fn open() -> Option<Self> {
        dmi::is_framework().then(|| Self {
            ec: Mutex::new(CrosEc::new()),
            memo: Memo::default(),
        })
    }

    fn ec(&self) -> MutexGuard<'_, CrosEc> {
        self.ec.lock().unwrap()
    }

    /// The EC's whole memmap battery block.
    fn power(&self) -> Option<power::PowerInfo> {
        power::power_info(&self.ec())
    }

    /// How many cycles the pack counts, asked of the pack rather than read
    /// from the EC's memmap copy, and None where it will not answer.
    ///
    /// The memmap copy is part of the EC's *static* battery block, which
    /// `update_static_battery_info` fills only while its `need_static` flag is
    /// set — on a battery presence change, or on the paths that revive a pack
    /// that was unresponsive or deeply discharged — and which clears that flag
    /// as soon as one read succeeds. So the published count is the one taken
    /// when the EC last initialized the battery, and since the EC outlives
    /// host reboots that can be weeks ago. Everything else in the block is
    /// either genuinely fixed (design capacity, the strings) or lives in the
    /// dynamic half that every charger pass refreshes; the cycle count is the
    /// one value that both moves and is published as static.
    ///
    /// Read once per daemon run and kept — see [`remembered`] for why the
    /// answer is held even when it is "the pack will not say". A cycle takes
    /// hours to accumulate and this daemon exits after five idle minutes, so
    /// the held value is never older than the session asking for it.
    fn cycle_count(&self) -> Option<u32> {
        remembered(&self.memo.cycle_count, || {
            Some(u32::from(self.sb_word(sbs::CYCLE_COUNT)?))
        })
    }

    /// When the pack was built, as `YYYY-MM-DD`, from the pack's own register:
    /// the EC's block has no room for a date and publishes none.
    fn manufacture_date(&self) -> Option<String> {
        remembered(&self.memo.manufacture_date, || {
            sbs::manufactured_iso(self.sb_word(sbs::MANUFACTURE_DATE)?)
        })
    }

    /// One word from the pack over the EC's I2C passthrough, which is how
    /// everything the EC does not publish for itself is reached. Every such
    /// read is a transfer to a device the EC is also driving, so callers ask
    /// for one only where the EC's own copy is absent or known stale.
    fn sb_word(&self, register: u16) -> Option<u16> {
        let response = i2c_read(&self.ec(), BATTERY_I2C_PORT, sbs::I2C_ADDR, register, 2).ok()?;
        response.is_successful().ok()?;
        Some(u16::from_le_bytes([
            *response.data.first()?,
            *response.data.get(1)?,
        ]))
    }

    /// A version is never worth a failed detection, so a silent EC reads as
    /// none.
    pub fn version(&self) -> Option<String> {
        self.ec().version_info().ok()
    }

    /// Whether a write-only command can be offered, asked of the firmware by
    /// `GET_CMD_VERSIONS`, which is side-effect-free and about the exact
    /// command a setter sends. An EC that won't answer is read as "no": a
    /// device settles its offer once per run, so offering on a silent read
    /// would keep offering a control that may not be there for the whole of
    /// it.
    fn offers(&self, command: EcCommands, version: u8) -> bool {
        self.ec()
            .cmd_version_supported(command as u32, version)
            .unwrap_or(false)
    }

    /// When the EC booted, from its uptime and the wall clock read together.
    pub fn boot(&self) -> DeviceResult<EcBoot> {
        let uptime = uptime_secs(&self.ec()).map_err(device_error)?;
        Ok(EcBoot::from_clocks(uptime, unix_now()))
    }
}

impl PowerLedEc for Ec {
    fn power_led_level(&self) -> DeviceResult<(u8, wire::PowerLedLevel)> {
        let (percent, level) = self.ec().get_fp_led_level().map_err(device_error)?;
        Ok((percent, wire_power_led_level(level.as_ref())))
    }

    fn set_power_led_level(&self, level: wire::PowerLedLevel) -> DeviceResult<()> {
        let Some(level) = ec_power_led_level(level) else {
            return Err(DeviceError::InvalidArgs(format!(
                "{level:?} is not a level the EC takes"
            )));
        };
        self.ec().set_fp_led_level(level).map_err(device_error)
    }

    fn set_power_led_percentage(&self, percent: u8) -> DeviceResult<()> {
        self.ec()
            .set_fp_led_percentage(percent)
            .map_err(device_error)
    }

    /// Older EC firmware implements only command v0 of `FpLedLevelControl`:
    /// presets high/medium/low. V1 added the raw-percentage write, and the
    /// same firmware generation added the ultra-low and auto levels
    /// (framework-system issue #211) — so V1 support stands in for all of
    /// them.
    fn custom_power_led_levels(&self) -> bool {
        self.offers(EcCommands::FpLedLevelControl, 1)
    }
}

impl Pack for Ec {
    fn identity(&self) -> Option<Identity> {
        let info = self.power()?;
        let battery = info.battery.as_ref()?;
        Some(part::sbs(
            &battery.manufacturer,
            &battery.model_number,
            &battery.serial_number,
        ))
    }

    /// One walk, so the reading it carries is that walk's rather than a
    /// second one taken a moment later.
    fn info(&self) -> Option<wire::BatteryInfo> {
        let info = self.power()?;
        let battery = info.battery.as_ref()?;
        Some(wire::BatteryInfo {
            state: wire_battery_state(&info, battery),
            remaining_capacity: battery.remaining_capacity,
            last_full_capacity: battery.last_full_charge_capacity,
            design_capacity: battery.design_capacity,
            design_millivolts: battery.design_voltage,
            // The pack's own count where it answers, the EC's published copy
            // otherwise — that copy is frozen at the last battery init, so it
            // is a floor rather than a reading.
            cycle_count: self.cycle_count().unwrap_or(battery.cycle_count),
            charger_connected: info.ac_present,
            critical: battery.level_critical,
            manufacturer: battery.manufacturer.clone(),
            model: battery.model_number.clone(),
            serial: battery.serial_number.clone(),
            chemistry: battery.battery_type.clone(),
            manufactured: self.manufacture_date().unwrap_or_default(),
        })
    }

    /// Not memoized and not in the EC's block at all. The memmap publishes
    /// one voltage for the whole pack, no temperature of its own and none of
    /// the alarms, and all of these move, so they are read afresh — a
    /// transfer per cell plus two, which is why only a caller showing them
    /// asks.
    fn condition(&self) -> Option<wire::BatteryCondition> {
        let cell_millivolts: Vec<u32> = sbs::CELL_VOLTAGES
            .iter()
            .map(|register| self.sb_word(*register).map(u32::from))
            .collect::<Option<_>>()?;
        Some(wire::BatteryCondition {
            cell_millivolts,
            alarms: sbs::alarms(self.sb_word(sbs::BATTERY_STATUS)?),
            decicelsius: sbs::decicelsius(self.sb_word(sbs::TEMPERATURE)?),
        })
    }
}

impl Charger for Ec {
    /// The ceiling the EC holds. Its command answers with a floor as well,
    /// which nothing here sets or reports.
    fn charge_limit(&self) -> DeviceResult<u8> {
        let (_min, max) = self.ec().get_charge_limit().map_err(device_error)?;
        Ok(max)
    }

    fn set_charge_limit(&self, percent: u8) -> DeviceResult<()> {
        self.ec().set_charge_limit(0, percent).map_err(device_error)
    }

    /// Always the unconditional form. The command's state-of-charge variant
    /// latches inside the EC: once applied it is never re-evaluated, so a
    /// later threshold cannot lift it (framework-system issue #342).
    fn set_charge_current_limit(&self, milliamps: u32) -> DeviceResult<()> {
        self.ec()
            .set_charge_current_limit(milliamps, None)
            .map_err(device_error)
    }

    /// No same-path probe exists: the charge current limit is write-only,
    /// with no readback in any command version (framework-system issue
    /// #180), so the firmware is asked about the command itself.
    fn charge_current_limit_supported(&self) -> bool {
        self.offers(EcCommands::ChargeCurrentLimit, 0)
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
fn ec_power_led_level(level: wire::PowerLedLevel) -> Option<FpLedBrightnessLevel> {
    Some(match level {
        wire::PowerLedLevel::High => FpLedBrightnessLevel::High,
        wire::PowerLedLevel::Medium => FpLedBrightnessLevel::Medium,
        wire::PowerLedLevel::Low => FpLedBrightnessLevel::Low,
        wire::PowerLedLevel::UltraLow => FpLedBrightnessLevel::UltraLow,
        wire::PowerLedLevel::Auto => FpLedBrightnessLevel::Auto,
        wire::PowerLedLevel::Custom | wire::PowerLedLevel::Off => return None,
    })
}

/// A level the EC does not name is custom: that is what it reports after a
/// raw percentage write.
fn wire_power_led_level(level: Option<&FpLedBrightnessLevel>) -> wire::PowerLedLevel {
    match level {
        Some(FpLedBrightnessLevel::High) => wire::PowerLedLevel::High,
        Some(FpLedBrightnessLevel::Medium) => wire::PowerLedLevel::Medium,
        Some(FpLedBrightnessLevel::Low) => wire::PowerLedLevel::Low,
        Some(FpLedBrightnessLevel::UltraLow) => wire::PowerLedLevel::UltraLow,
        Some(FpLedBrightnessLevel::Auto) => wire::PowerLedLevel::Auto,
        Some(FpLedBrightnessLevel::Custom) | None => wire::PowerLedLevel::Custom,
    }
}

/// The moving part of the battery block in the wire's terms, taken from a
/// block the caller already holds rather than read for itself — so a report
/// and the reading inside it come from one walk.
fn wire_battery_state(
    info: &power::PowerInfo,
    battery: &power::BatteryInformation,
) -> wire::BatteryState {
    wire::BatteryState {
        // Against the last full charge, which is the EC's own denominator; a
        // pack reporting more than full is clamped rather than shown.
        percent: u8::try_from(battery.charge_percentage.min(100)).unwrap_or(100),
        flow: charge_flow(ChargeSignals {
            charging: battery.charging,
            discharging: battery.discharging,
            ac_present: info.ac_present,
            milliamps: battery.present_rate,
        }),
        milliamps: battery.present_rate,
        millivolts: battery.present_voltage,
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
    use super::{ChargeSignals, charge_flow, ec_power_led_level, wire, wire_power_led_level};

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
        for level in wire::PowerLedLevel::ALL {
            if let Some(ec_level) = ec_power_led_level(level) {
                assert_eq!(wire_power_led_level(Some(&ec_level)), level);
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
}
