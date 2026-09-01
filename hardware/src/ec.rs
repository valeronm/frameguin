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
use framework_lib::chromium_ec::commands::{
    EcRequestGetPdPortState, EcRequestGetUptimeInfo, EcRequestReadPdVersionV0,
    EcRequestReadPdVersionV1, FpLedBrightnessLevel,
};
use framework_lib::chromium_ec::i2c_passthrough::i2c_read;
use framework_lib::chromium_ec::{CrosEc, EcError, EcResponseStatus, EcResult};
use framework_lib::power;

use crate::dmi;
use crate::lifetime::EcBoot;
use crate::part::{self, Identity};
use crate::pd;
use crate::sbs;

/// The EC's I2C port the pack hangs off, the same on every Framework board
/// — one Nuvoton EC.
const BATTERY_I2C_PORT: u8 = 3;

/// What the power LED's device needs of the EC: the level it holds, the two
/// writes that move it, and whether the firmware has the levels that came
/// with command v1.
pub trait PowerLedEc: Send + Sync {
    /// The brightness percentage and the level the EC reports it as.
    /// `Custom` is what it answers after any raw percentage write, and on
    /// firmware that names no level, for a percentage no level stands for.
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

/// What the ports' device needs of the EC: how many PD controllers answered,
/// and one port's state.
pub trait PdPorts: Send + Sync {
    /// How many controllers the EC reports a version for. Each drives at
    /// most two ports, which is what bounds the walk — the EC cannot be
    /// asked how many ports a board has, and asking it past the last one is
    /// not safe (see `docs/hardware.md`).
    fn pd_controllers(&self) -> u8;
    /// None where the EC refuses the number as out of range. A second bound
    /// rather than the only one, since a board has been seen to answer past
    /// its last port instead of refusing.
    fn port_state(&self, port: u8) -> DeviceResult<Option<wire::PortState>>;
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
    /// The PD controllers' versions, which the EC caches at controller
    /// bring-up and which two devices ask for at detection — the mainboard
    /// for the firmware it runs, the ports for how many controllers there
    /// are to bound their walk.
    pd_versions: OnceLock<Option<Vec<[u8; pd::VERSION_LEN]>>>,
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

    pub(crate) fn version(&self) -> EcResult<String> {
        self.ec().version_info()
    }

    /// The version the EC holds for each PD controller, in the EC's own
    /// controller order, and empty where it will not answer.
    ///
    /// The EC caches these while bringing the controllers up, so this asks
    /// the EC rather than the controllers and needs neither an I2C transfer
    /// nor the per-board table of controller addresses one would be sent to.
    /// What it costs is the detail that table buys: the silicon id, which
    /// image is running, and the versions of the two that are not — this is
    /// the main firmware's version whichever image the controller booted.
    ///
    /// Command version 1 answers with a count and that many blobs, stopping
    /// at the first controller it has nothing for; version 0 answers for two
    /// and pads an absent one with zeros. Either way the blobs are chunked
    /// rather than counted, so a short answer costs the entries it truncated
    /// and not a misread of the ones before them.
    pub(crate) fn pd_versions(&self) -> Vec<[u8; pd::VERSION_LEN]> {
        remembered(&self.memo.pd_versions, || {
            let counted = self.offers(EcCommands::ReadPdVersion, 1);
            let ec = self.ec();
            let raw = if counted {
                EcRequestReadPdVersionV1 {}.send_command_vec(&ec)
            } else {
                EcRequestReadPdVersionV0 {}.send_command_vec(&ec)
            }
            .ok()?;
            // Version 1 leads with the count, which the chunking below makes
            // nothing of; version 0 leads with the first blob.
            let blobs = raw.get(usize::from(counted)..).unwrap_or_default();
            Some(
                blobs
                    .chunks_exact(pd::VERSION_LEN)
                    // Every chunk is exactly this long; the conversion is
                    // what gives the array its type, not a filter.
                    .map(|blob| <[u8; pd::VERSION_LEN]>::try_from(blob).unwrap_or_default())
                    .collect(),
            )
        })
        .unwrap_or_default()
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
        Ok((percent, wire_power_led_level(level.as_ref(), percent)))
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

impl PdPorts for Ec {
    fn pd_controllers(&self) -> u8 {
        u8::try_from(self.pd_versions().len()).unwrap_or(u8::MAX)
    }

    fn port_state(&self, port: u8) -> DeviceResult<Option<wire::PortState>> {
        let request = EcRequestGetPdPortState { port };
        match request.send_command(&self.ec()) {
            Ok(raw) => Ok(Some(pd::port_state(port, &raw))),
            Err(EcError::Response(EcResponseStatus::InvalidParameter)) => Ok(None),
            Err(e) => Err(device_error(e)),
        }
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

/// The percentages the three levels every firmware has stand for.
const POWER_LED_HIGH: u8 = 55;
const POWER_LED_MEDIUM: u8 = 40;
const POWER_LED_LOW: u8 = 15;

/// The level the EC named, or the one its percentage stands for where it
/// named none. Firmware that names none stores only these three or a zero
/// meaning the level was never set, so the deduction is exhaustive over what
/// such a board holds and the zero is the one reading left custom.
fn wire_power_led_level(level: Option<&FpLedBrightnessLevel>, percent: u8) -> wire::PowerLedLevel {
    match level {
        Some(FpLedBrightnessLevel::High) => wire::PowerLedLevel::High,
        Some(FpLedBrightnessLevel::Medium) => wire::PowerLedLevel::Medium,
        Some(FpLedBrightnessLevel::Low) => wire::PowerLedLevel::Low,
        Some(FpLedBrightnessLevel::UltraLow) => wire::PowerLedLevel::UltraLow,
        Some(FpLedBrightnessLevel::Auto) => wire::PowerLedLevel::Auto,
        Some(FpLedBrightnessLevel::Custom) => wire::PowerLedLevel::Custom,
        None => match percent {
            POWER_LED_HIGH => wire::PowerLedLevel::High,
            POWER_LED_MEDIUM => wire::PowerLedLevel::Medium,
            POWER_LED_LOW => wire::PowerLedLevel::Low,
            _ => wire::PowerLedLevel::Custom,
        },
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
                assert_eq!(wire_power_led_level(Some(&ec_level), 0), level);
            }
        }
    }

    /// Written out rather than taken from the constants the deduction reads,
    /// which would move both sides of the assertion together. Zero is the
    /// only other reading such a board has: never set, or cleared for
    /// shipping.
    #[test]
    fn only_the_three_percentages_a_level_stands_for_are_named() {
        let named = |percent| wire_power_led_level(None, percent);
        assert_eq!(named(55), wire::PowerLedLevel::High);
        assert_eq!(named(40), wire::PowerLedLevel::Medium);
        assert_eq!(named(15), wire::PowerLedLevel::Low);
        assert_eq!(named(0), wire::PowerLedLevel::Custom);
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
