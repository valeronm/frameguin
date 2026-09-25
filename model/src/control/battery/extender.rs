//! Framework's battery extender: the window each stage holds a charged pack
//! in.

use frameguin_contract::ExtenderStage;

/// The state-of-charge window the EC's `battery_extender` holds a stage at,
/// and None for the stage that holds nothing.
#[must_use]
pub const fn stage_window(stage: ExtenderStage) -> Option<(u8, u8)> {
    match stage {
        ExtenderStage::Inactive => None,
        ExtenderStage::First => Some((90, 95)),
        ExtenderStage::Second => Some((85, 87)),
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::ExtenderStage;

    use super::stage_window;

    #[test]
    fn only_the_two_stages_hold_a_window() {
        assert_eq!(stage_window(ExtenderStage::Inactive), None);
        assert_eq!(stage_window(ExtenderStage::First), Some((90, 95)));
        assert_eq!(stage_window(ExtenderStage::Second), Some((85, 87)));
    }
}
