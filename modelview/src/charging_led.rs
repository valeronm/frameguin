//! The charging LED: which side of the chassis a reading names.

use frameguin_contract::ChargingLedSide;

#[must_use]
pub fn side_label(side: ChargingLedSide) -> &'static str {
    match side {
        ChargingLedSide::Neither => "None",
        ChargingLedSide::Left => "Left",
        ChargingLedSide::Right => "Right",
        ChargingLedSide::Both => "Both",
    }
}
