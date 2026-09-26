//! A stub pack's figures and a stub port, for this crate's tests and those
//! of the crates above it.

use std::num::NonZeroU32;

use crate::control::battery::ChargeSpeeds;
use frameguin_contract::{
    BatteryState, CcPolarity, ChargeCurrentLimit, ChargeFlow, DataRole, Epr, PortPartner,
    PortState, PowerRole,
};

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

#[must_use]
pub fn port(index: u8) -> PortState {
    let charging = index == 0;
    PortState {
        index,
        partner: if charging {
            PortPartner::Source
        } else {
            PortPartner::Nothing
        },
        contract: charging,
        power_role: PowerRole::Sink,
        data_role: if charging {
            DataRole::DownstreamFacing
        } else {
            DataRole::UpstreamFacing
        },
        millivolts: if charging { 20_000 } else { 0 },
        milliamps: if charging { 5000 } else { 0 },
        charging,
        video: false,
        vconn: charging,
        cc: CcPolarity::Cc1,
        epr: Epr::Unsupported,
        registers: None,
    }
}
