//! The mirror for hardware that cannot be read back.
//!
//! The haptic touchpad ACKs `GET_FEATURE` with zeros, the charge current
//! limit has no readback in any command version, and the touch panel's own
//! enable command asks for no reply, so what was written is only knowable
//! from here. Load and save share this file so the round trip cannot come
//! apart: nothing is re-applied at startup, since the touchpad keeps its own
//! state in flash and the EC keeps the limit until it restarts.
//!
//! What each mirror is dated against is [`crate::lifetime`]'s vocabulary: the
//! charge current limit against the EC that took it, the touch panel against
//! the host that switched it.
//!
//! An unknown key is ignored, so a file another version wrote costs the
//! mirrors the two spell differently, each absence under-claiming until that
//! control is next written.

use frameguin_wire::{HAPTIC_INTENSITY_LEVELS, NO_CHARGE_CURRENT_LIMIT};

use crate::lifetime::{EcStamp, HostStamp};
use crate::touchpad;

const DEFAULT_HAPTIC_INTENSITY: u8 = 75;
const STATE_FILE: &str = "/var/lib/frameguin/state";

// Single source for the state file's keys: the loader matches on them and the
// writer spells them, so a rename can't quietly break the round trip.
const KEY_HAPTIC_INTENSITY: &str = "haptic_intensity";
const KEY_CLICK_FORCE: &str = "click_force";
const KEY_CURRENT_LIMIT: &str = "charge_current_limit";
const KEY_CURRENT_LIMIT_STAMP: &str = "charge_current_limit_stamp";
const KEY_POWER_LED_OFF_STAMP: &str = "power_led_off_stamp";
const KEY_TOUCHSCREEN_OFF_HOST: &str = "touchscreen_off_host";

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
    /// None while the panel is reporting, a stamp's presence being the whole
    /// of the claim that a switch was made.
    pub(crate) touchscreen_off: Option<HostStamp>,
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
                KEY_CURRENT_LIMIT_STAMP => {
                    if let Some(stamp) = EcStamp::parse(value) {
                        state.charge_current_limit.stamp = stamp;
                    }
                }
                KEY_POWER_LED_OFF_STAMP => {
                    state.power_led_off = EcStamp::parse(value);
                }
                // Weighed here rather than after the loop, so that keeping a
                // stamp and dropping a stale one cannot come apart.
                KEY_TOUCHSCREEN_OFF_HOST => {
                    state.touchscreen_off =
                        HostStamp::parse(value).filter(HostStamp::still_current);
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
        Some(stamp) => format!("{KEY_POWER_LED_OFF_STAMP}={}\n", stamp.stored()),
        None => String::new(),
    };
    // Absent rather than false while the panel is reporting, as the LED's
    // stamp is: a panel nothing has switched needs no record, and the stamp
    // being there is the whole of the claim that one was made.
    let touchscreen_off = match &state.touchscreen_off {
        Some(stamp) => format!("{KEY_TOUCHSCREEN_OFF_HOST}={}\n", stamp.stored()),
        None => String::new(),
    };
    let content = format!(
        "{KEY_HAPTIC_INTENSITY}={}\n{KEY_CLICK_FORCE}={}\n{KEY_CURRENT_LIMIT}={}\n\
         {KEY_CURRENT_LIMIT_STAMP}={}\n{power_led_off}{touchscreen_off}",
        state.haptic_intensity,
        state.click_force,
        state.charge_current_limit.milliamps,
        state.charge_current_limit.stamp.stored(),
    );
    if let Err(e) = std::fs::write(STATE_FILE, content) {
        eprintln!("failed to persist state: {e}");
    }
}
