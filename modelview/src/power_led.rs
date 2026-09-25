//! The power button LED's levels: the order a front end lists them in, and
//! the words for each one.

use frameguin_contract::PowerLedLevel;

use crate::rows::{Custom, row_for};

/// The levels a board offers, in the order a front end lists them.
pub struct LevelRows {
    rows: Vec<PowerLedLevel>,
}

impl LevelRows {
    /// `offered` may come in any order.
    #[must_use]
    pub fn new(offered: &[PowerLedLevel]) -> Self {
        let mut rows = offered.to_vec();
        rows.sort_unstable_by_key(|&level| rank(level));
        Self { rows }
    }

    /// Every level this board has, Custom included, in display order.
    #[must_use]
    pub fn rows(&self) -> &[PowerLedLevel] {
        &self.rows
    }

    #[must_use]
    pub fn labels(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|&level| level_label(level).to_string())
            .collect()
    }

    /// Which row a level sits on; None for one this board does not list.
    /// Private because a level is not on its own an answer to which row to
    /// show: the EC names the level by deducing it from the percentage it
    /// holds, so one dialled in that equals a named level's comes back under
    /// that name. [`LevelRows::row`] is the answer.
    fn preset_row(&self, level: PowerLedLevel) -> Option<usize> {
        self.rows.iter().position(|&l| l == level)
    }

    /// Where the row that reveals the slider sits, and None on a board whose
    /// firmware takes no raw percentage.
    #[must_use]
    pub fn custom_row(&self) -> Option<usize> {
        self.preset_row(PowerLedLevel::Custom)
    }

    /// Which row a combo shows for a level, given where it sits now. Firmware
    /// that takes no raw percentage can still report `Custom`, and gets no row.
    #[must_use]
    pub fn row(
        &self,
        level: PowerLedLevel,
        selected: Option<usize>,
        custom: Custom,
    ) -> Option<usize> {
        row_for(self.preset_row(level), self.custom_row(), selected, custom)
    }

    /// The level a row sends; None for a row nothing is listed at.
    #[must_use]
    pub fn at(&self, row: usize) -> Option<PowerLedLevel> {
        self.rows.get(row).copied()
    }
}

/// Where a level's row sits. A match rather than a second list of the levels,
/// so a level added to the vocabulary fails to build here rather than landing
/// wherever it happened to be declared. Ranks must be distinct: two levels
/// sharing one fall back to the order the vocabulary declares them in.
fn rank(level: PowerLedLevel) -> u8 {
    match level {
        PowerLedLevel::Auto => 0,
        PowerLedLevel::Off => 1,
        PowerLedLevel::UltraLow => 2,
        PowerLedLevel::Low => 3,
        PowerLedLevel::Medium => 4,
        PowerLedLevel::High => 5,
        PowerLedLevel::Custom => 6,
    }
}

fn level_label(level: PowerLedLevel) -> &'static str {
    match level {
        PowerLedLevel::Auto => "Auto",
        PowerLedLevel::Off => "Off",
        PowerLedLevel::UltraLow => "Ultra-low",
        PowerLedLevel::Low => "Low",
        PowerLedLevel::Medium => "Medium",
        PowerLedLevel::High => "High",
        PowerLedLevel::Custom => "Custom",
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::PowerLedLevel;

    use super::{LevelRows, rank};
    use crate::rows::Custom;

    #[test]
    fn levels_are_listed_in_rank_order_whatever_order_they_are_offered_in() {
        let rows = LevelRows::new(&[
            PowerLedLevel::High,
            PowerLedLevel::Custom,
            PowerLedLevel::Auto,
        ]);
        assert_eq!(
            rows.rows(),
            [
                PowerLedLevel::Auto,
                PowerLedLevel::High,
                PowerLedLevel::Custom
            ]
        );
        assert_eq!(rows.custom_row(), Some(2));
    }

    #[test]
    fn the_rows_are_the_offered_levels_in_display_order() {
        let rows = LevelRows::new(&[PowerLedLevel::High, PowerLedLevel::Off, PowerLedLevel::Low]);
        assert_eq!(
            rows.rows(),
            [PowerLedLevel::Off, PowerLedLevel::Low, PowerLedLevel::High]
        );
        assert_eq!(rows.preset_row(PowerLedLevel::Auto), None);
        assert_eq!(rows.at(3), None);
    }

    #[test]
    fn a_row_sends_the_level_it_is_marked_for() {
        let rows = LevelRows::new(&PowerLedLevel::ALL);
        for level in PowerLedLevel::ALL {
            let row = rows.preset_row(level).expect("every level is listed");
            assert_eq!(rows.at(row), Some(level));
        }
    }

    #[test]
    fn a_level_the_ec_named_never_moves_a_combo_off_its_custom_row() {
        let rows = LevelRows::new(&PowerLedLevel::ALL);
        let custom = Some(PowerLedLevel::ALL.len() - 1);
        assert_eq!(rows.custom_row(), custom);
        for row in 0..PowerLedLevel::ALL.len() {
            let level = rows.at(row).expect("every level is listed");
            assert_eq!(rows.row(level, custom, Custom::Keep), custom);
            assert_eq!(rows.row(level, custom, Custom::Rederive), Some(row));
        }
    }

    #[test]
    fn a_board_without_a_custom_row_keeps_nothing() {
        let offered = [
            PowerLedLevel::High,
            PowerLedLevel::Medium,
            PowerLedLevel::Low,
        ];
        let rows = LevelRows::new(&offered);
        assert_eq!(rows.custom_row(), None);
        assert_eq!(rows.row(PowerLedLevel::Custom, None, Custom::Keep), None);
        assert_eq!(rows.row(PowerLedLevel::Low, None, Custom::Keep), Some(0));
    }

    #[test]
    fn no_two_levels_share_a_row() {
        let mut ranks = PowerLedLevel::ALL.map(rank).to_vec();
        ranks.sort_unstable();
        ranks.dedup();
        assert_eq!(ranks.len(), PowerLedLevel::ALL.len());
    }
}
