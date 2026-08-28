//! The mirrors `Daemon` still holds for the controls not yet moved into
//! devices of their own, read and written as one through the store.
//!
//! What each is dated against is [`frameguin_hardware::lifetime`]'s
//! vocabulary: the charge current limit against the EC that took it.
//! Nothing is re-applied at startup: the EC keeps the limit until it
//! restarts.

use frameguin_hardware::lifetime::EcStamp;
use frameguin_hardware::state::Store;
use frameguin_wire::NO_CHARGE_CURRENT_LIMIT;

// Single source for the store's keys: the loader matches on them and the
// writer spells them, so a rename can't quietly break the round trip.
const KEY_CURRENT_LIMIT: &str = "charge_current_limit";
const KEY_CURRENT_LIMIT_STAMP: &str = "charge_current_limit_stamp";
const KEY_POWER_LED_OFF_STAMP: &str = "power_led_off_stamp";

/// A charge current limit together with the stamp that dates it.
#[derive(Clone, Copy)]
pub(crate) struct ChargeCurrentLimit {
    pub(crate) milliamps: u32,
    pub(crate) stamp: EcStamp,
}

pub(crate) struct State {
    pub(crate) charge_current_limit: ChargeCurrentLimit,
    pub(crate) power_led_off: Option<EcStamp>,
}

pub(crate) fn load(store: &dyn Store) -> State {
    State {
        charge_current_limit: ChargeCurrentLimit {
            // A zero here would mirror a limit the setter refuses to write,
            // so read it as the absence of one.
            milliamps: store
                .get(KEY_CURRENT_LIMIT)
                .and_then(|v| v.parse().ok())
                .filter(|&v| v != 0)
                .unwrap_or(NO_CHARGE_CURRENT_LIMIT),
            stamp: store
                .get(KEY_CURRENT_LIMIT_STAMP)
                .and_then(|v| EcStamp::parse(&v))
                .unwrap_or_default(),
        },
        power_led_off: store
            .get(KEY_POWER_LED_OFF_STAMP)
            .and_then(|v| EcStamp::parse(&v)),
    }
}

pub(crate) fn save(store: &dyn Store, state: &State) {
    store.set(
        KEY_CURRENT_LIMIT,
        Some(state.charge_current_limit.milliamps.to_string()),
    );
    store.set(
        KEY_CURRENT_LIMIT_STAMP,
        Some(state.charge_current_limit.stamp.stored()),
    );
    // Absent rather than zeroed while the LED is lit: the stamp only ever
    // withdraws a claim, and a zeroed one would withdraw every time.
    store.set(
        KEY_POWER_LED_OFF_STAMP,
        state.power_led_off.map(EcStamp::stored),
    );
}
