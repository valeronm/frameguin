//! Where a fan sits in the machine, for the boards with more than one.
//!
//! The EC numbers its fans by slot and carries no position for them; the
//! Laptop 16's two are placed as Framework's own `framework_tool` names them.

use frameguin_contract::{Platform, Series};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FanPlacement {
    Left,
    Right,
}

/// None for a fan nobody placed.
#[must_use]
pub const fn placement(platform: Platform, index: u8) -> Option<FanPlacement> {
    match (platform.series(), index) {
        (Some(Series::Laptop16), 0) => Some(FanPlacement::Left),
        (Some(Series::Laptop16), 1) => Some(FanPlacement::Right),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::Platform;

    use super::{FanPlacement, placement};

    #[test]
    fn the_laptop_16s_fans_are_left_and_right() {
        assert_eq!(
            placement(Platform::Laptop16AmdAi300, 0),
            Some(FanPlacement::Left)
        );
        assert_eq!(
            placement(Platform::Laptop16Amd7040, 1),
            Some(FanPlacement::Right)
        );
    }

    #[test]
    fn a_single_fan_board_places_nothing() {
        assert_eq!(placement(Platform::Laptop13ProUltra3, 0), None);
        assert_eq!(placement(Platform::Unknown, 0), None);
    }
}
