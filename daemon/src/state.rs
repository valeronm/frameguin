//! The mirror for hardware that cannot be read back.
//!
//! The haptic touchpad ACKs `GET_FEATURE` with zeros, the charge current
//! limit has no readback in any command version, and the touch panel's own
//! enable command asks for no reply, so what was written is only knowable
//! from here. Load and save share this file so the round trip cannot come
//! apart: nothing is re-applied at startup, since the touchpad keeps its own
//! state in flash and the EC keeps the limit until it restarts.
//!
//! A mirror is worth no more than the life of whatever holds the state it
//! claims, and two of them are dated to say so: the charge current limit
//! against the EC that took it, the touch panel against the boot that
//! switched it. The panel's is the weaker claim of the two, since what its
//! controller survives is not established — so the boot is taken as the
//! ceiling the documents point at rather than a measured one, and a mirror
//! stamped with an earlier boot is dropped rather than believed. Within one
//! boot it can still be wrong, a suspend or anything else having moved the
//! panel; there is no reading to prefer to it.

use frameguin_wire::{HAPTIC_INTENSITY_LEVELS, NO_CHARGE_CURRENT_LIMIT};

use crate::ec::EcStamp;
use crate::host::BootStamp;
use crate::touchpad;

const DEFAULT_HAPTIC_INTENSITY: u8 = 75;
const STATE_FILE: &str = "/var/lib/frameguin/state";

// Single source for the state file's keys: the loader matches on them and the
// writer spells them, so a rename can't quietly break the round trip.
const KEY_HAPTIC_INTENSITY: &str = "haptic_intensity";
const KEY_CLICK_FORCE: &str = "click_force";
const KEY_CURRENT_LIMIT: &str = "charge_current_limit";
const KEY_CURRENT_LIMIT_UPTIME: &str = "charge_current_limit_ec_uptime";
const KEY_CURRENT_LIMIT_WRITTEN_AT: &str = "charge_current_limit_written_at";
const KEY_POWER_LED_OFF_UPTIME: &str = "power_led_off_ec_uptime";
const KEY_POWER_LED_OFF_WRITTEN_AT: &str = "power_led_off_written_at";
const KEY_TOUCHSCREEN_OFF_BOOT: &str = "touchscreen_off_boot";

/// A charge current limit together with the stamp that dates it.
#[derive(Clone, Copy)]
pub(crate) struct ChargeCurrentLimit {
    pub(crate) milliamps: u32,
    pub(crate) stamp: EcStamp,
}

pub(crate) struct State {
    pub(crate) haptic_intensity: u8,
    pub(crate) click_force: u8,
    pub(crate) charge_current_limit: ChargeCurrentLimit,
    pub(crate) power_led_off: Option<EcStamp>,
    /// The boot the panel was switched off in, and None while it is
    /// reporting. Absent rather than false for the reason `power_led_off` is:
    /// the stamp being there is the whole of the claim that a switch was
    /// made, so there is no state in which a value and its date can disagree.
    pub(crate) touchscreen_off: Option<BootStamp>,
}

/// Loads the mirrored control state, falling back to the factory defaults.
/// A missing file on a machine whose touchpad was already changed by other
/// means will misreport until the first write — unavoidable, since the
/// hardware can't be read.
pub(crate) fn load() -> State {
    let mut state = State {
        haptic_intensity: DEFAULT_HAPTIC_INTENSITY,
        // Stored as the device's own code, which is what the pad is written
        // with and what a reload has to be able to name again.
        click_force: touchpad::click_force(touchpad::DEFAULT_CLICK_FORCE) as u8,
        charge_current_limit: ChargeCurrentLimit {
            milliamps: NO_CHARGE_CURRENT_LIMIT,
            stamp: EcStamp::default(),
        },
        power_led_off: None,
        // A panel nobody has switched is reporting, which is also what a
        // machine with no file at all has to be assumed to be doing.
        touchscreen_off: None,
    };
    if let Ok(content) = std::fs::read_to_string(STATE_FILE) {
        for line in content.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            match key {
                KEY_HAPTIC_INTENSITY => {
                    if let Ok(v) = value.parse()
                        && HAPTIC_INTENSITY_LEVELS.contains(&v)
                    {
                        state.haptic_intensity = v;
                    }
                }
                KEY_CLICK_FORCE => {
                    if let Ok(v) = value.parse()
                        && touchpad::wire_click_force(v).is_some()
                    {
                        state.click_force = v;
                    }
                }
                KEY_CURRENT_LIMIT => {
                    // A zero here would mirror a limit the setter refuses to
                    // write, so read it as the absence of one.
                    if let Ok(v) = value.parse()
                        && v != 0
                    {
                        state.charge_current_limit.milliamps = v;
                    }
                }
                KEY_CURRENT_LIMIT_UPTIME => {
                    state.charge_current_limit.stamp.ec_uptime = value.parse().unwrap_or(0);
                }
                KEY_CURRENT_LIMIT_WRITTEN_AT => {
                    state.charge_current_limit.stamp.written_at = value.parse().unwrap_or(0);
                }
                KEY_POWER_LED_OFF_UPTIME => {
                    state.power_led_off.get_or_insert_default().ec_uptime =
                        value.parse().unwrap_or(0);
                }
                KEY_POWER_LED_OFF_WRITTEN_AT => {
                    state.power_led_off.get_or_insert_default().written_at =
                        value.parse().unwrap_or(0);
                }
                // Weighed here rather than after the loop, so that keeping a
                // stamp and dropping a stale one cannot come apart: a stamp
                // naming an earlier boot is no evidence about this one, and
                // the panel comes up reporting, so one wrongly kept would
                // claim touch is off on hardware that has it on.
                KEY_TOUCHSCREEN_OFF_BOOT => {
                    state.touchscreen_off =
                        Some(BootStamp::stored(value)).filter(BootStamp::still_current);
                }
                _ => {}
            }
        }
    }
    state
}

// The directory is provisioned by StateDirectory= in the systemd unit.
pub(crate) fn save(state: &State) {
    // Absent rather than zeroed while the LED is lit: the stamp only ever
    // withdraws a claim, and a zeroed one would withdraw every time.
    let power_led_off = match state.power_led_off {
        Some(stamp) => format!(
            "{KEY_POWER_LED_OFF_UPTIME}={}\n{KEY_POWER_LED_OFF_WRITTEN_AT}={}\n",
            stamp.ec_uptime, stamp.written_at
        ),
        None => String::new(),
    };
    // Absent rather than false while the panel is reporting, as the LED's
    // stamp is: a panel nothing has switched needs no record, and the stamp
    // being there is the whole of the claim that one was made.
    let touchscreen_off = match &state.touchscreen_off {
        Some(stamp) => format!("{KEY_TOUCHSCREEN_OFF_BOOT}={}\n", stamp.as_str()),
        None => String::new(),
    };
    let content = format!(
        "{KEY_HAPTIC_INTENSITY}={}\n{KEY_CLICK_FORCE}={}\n{KEY_CURRENT_LIMIT}={}\n\
         {KEY_CURRENT_LIMIT_UPTIME}={}\n{KEY_CURRENT_LIMIT_WRITTEN_AT}={}\n\
         {power_led_off}{touchscreen_off}",
        state.haptic_intensity,
        state.click_force,
        state.charge_current_limit.milliamps,
        state.charge_current_limit.stamp.ec_uptime,
        state.charge_current_limit.stamp.written_at,
    );
    if let Err(e) = std::fs::write(STATE_FILE, content) {
        eprintln!("failed to persist state: {e}");
    }
}
