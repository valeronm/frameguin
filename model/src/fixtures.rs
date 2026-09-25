//! A stub pack's figures, for this crate's tests and those of the crates
//! above it.

use std::num::NonZeroU32;

use crate::control::battery::ChargeSpeeds;
use frameguin_contract::{BatteryState, ChargeCurrentLimit, ChargeFlow};

/// A 4640 mAh pack, the Laptop 13's.
pub const CAPACITY: u32 = 4640;

/// That pack's charge speeds.
pub const SPEEDS: ChargeSpeeds = ChargeSpeeds::new(CAPACITY);

/// # Panics
///
/// On zero, which is no limit rather than a cap.
#[must_use]
pub const fn cap(milliamps: u32) -> ChargeCurrentLimit {
    ChargeCurrentLimit::Limit(NonZeroU32::new(milliamps).unwrap())
}

/// Mid-charge on the same pack's four cells.
pub const MILLIVOLTS: u32 = 15_400;

/// What that pack is rated at, which is what its energy is measured against
/// however charged it happens to be.
pub const NOMINAL_MILLIVOLTS: u32 = 15_640;

#[must_use]
pub fn state(flow: ChargeFlow, milliamps: u32) -> BatteryState {
    BatteryState {
        percent: 62,
        flow,
        milliamps,
        millivolts: MILLIVOLTS,
    }
}
