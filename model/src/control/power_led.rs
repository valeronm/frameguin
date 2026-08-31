//! The power button LED: a level picked from the ones the board has, and
//! behind the custom one a percentage.

use std::rc::Rc;

use frameguin_wire::{DeviceResult as Result, PowerLedControl, PowerLedLevel};

use super::present;

/// What the LED is set to: the level in force, and the percentage the EC
/// lights it at — the one a preset resolved to, or the one dialled in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub percent: u8,
    pub level: PowerLedLevel,
}

pub struct PowerLed<C> {
    control: Rc<C>,
    rows: Vec<PowerLedLevel>,
}

impl<C: PowerLedControl> PowerLed<C> {
    /// `rows` is every level the device has, and is kept in the order a
    /// front-end lists them.
    pub fn new(control: Rc<C>, mut rows: Vec<PowerLedLevel>) -> Self {
        rows.sort_unstable_by_key(|&level| rank(level));
        Self { control, rows }
    }

    /// The levels are asked for once here, being fixed for the device's run.
    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        if present(control.brightness().await)?.is_none() {
            return Ok(None);
        }
        let offered = control.levels().await?;
        Ok(Some(Self::new(control.clone(), offered)))
    }

    pub async fn read(&self) -> Result<Snapshot> {
        let (percent, level) = self.control.brightness().await?;
        Ok(Snapshot { percent, level })
    }

    pub async fn set_level(&self, level: PowerLedLevel) -> Result<()> {
        self.control.set_level(level).await
    }

    pub async fn set_brightness(&self, percent: u8) -> Result<()> {
        self.control.set_brightness(percent).await
    }

    /// The window's rows: every level this board has, Custom included.
    #[must_use]
    pub fn rows(&self) -> &[PowerLedLevel] {
        &self.rows
    }

    /// The tray's rows: the window's, less the one no click can apply.
    #[must_use]
    pub fn presets(&self) -> Vec<PowerLedLevel> {
        self.rows
            .iter()
            .copied()
            .filter(|level| level.is_settable())
            .collect()
    }

    /// Which row a level sits on; None for one this board does not list.
    #[must_use]
    pub fn row(&self, level: PowerLedLevel) -> Option<usize> {
        self.rows.iter().position(|&l| l == level)
    }

    /// The level a row sends; None for a row nothing is listed at.
    #[must_use]
    pub fn at(&self, row: usize) -> Option<PowerLedLevel> {
        self.rows.get(row).copied()
    }
}

/// Where a level's row sits. A match rather than a second list of the levels,
/// so a level added to the vocabulary fails to build here rather than landing
/// wherever it happened to be declared.
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

#[must_use]
pub fn level_label(level: PowerLedLevel) -> &'static str {
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

#[must_use]
pub fn labels(levels: &[PowerLedLevel]) -> Vec<String> {
    levels
        .iter()
        .map(|&level| level_label(level).to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use frameguin_wire::{DeviceError, PowerLedLevel};

    use super::{PowerLed, Snapshot, rank};
    use crate::testing::{Board, absent, ready};

    #[test]
    fn an_led_the_hardware_answers_for_is_detected_with_its_levels() {
        let led = ready(PowerLed::detect(&Board::new())).unwrap().unwrap();
        assert_eq!(led.rows().len(), PowerLedLevel::ALL.len());
    }

    #[test]
    fn an_led_the_hardware_does_not_serve_is_absent() {
        let board = Board::failing(absent());
        assert!(ready(PowerLed::detect(&board)).unwrap().is_none());
    }

    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_led() {
        let error = DeviceError::Failed("no reply".into());
        let board = Board::failing(error.clone());
        assert_eq!(ready(PowerLed::detect(&board)).err(), Some(error));
    }

    #[test]
    fn a_read_takes_both_halves_from_the_hardware() {
        let led = PowerLed::new(Board::new(), PowerLedLevel::ALL.to_vec());
        assert_eq!(
            ready(led.read()),
            Ok(Snapshot {
                percent: 55,
                level: PowerLedLevel::High
            })
        );
    }

    #[test]
    fn a_refused_write_carries_the_refusal() {
        let board = Board::new();
        let led = PowerLed::new(board.clone(), PowerLedLevel::ALL.to_vec());
        board.power_led.refuse();
        assert_eq!(
            ready(led.set_level(PowerLedLevel::Low)),
            Err(DeviceError::AccessDenied("not authorized".into()))
        );
        assert_eq!(board.level.get(), PowerLedLevel::High);
    }

    /// The rows are the board's levels in display order, whatever order the
    /// device listed them in, and a level the board lacks has no row.
    #[test]
    fn the_rows_are_the_offered_levels_in_display_order() {
        let led = PowerLed::new(
            Board::new(),
            vec![PowerLedLevel::High, PowerLedLevel::Off, PowerLedLevel::Low],
        );
        assert_eq!(
            led.rows(),
            [PowerLedLevel::Off, PowerLedLevel::Low, PowerLedLevel::High]
        );
        assert_eq!(led.row(PowerLedLevel::Auto), None);
        assert_eq!(led.at(3), None);
    }

    #[test]
    fn the_presets_are_the_rows_less_custom() {
        let led = PowerLed::new(Board::new(), PowerLedLevel::ALL.to_vec());
        assert!(!led.presets().contains(&PowerLedLevel::Custom));
        assert_eq!(led.presets().len(), PowerLedLevel::ALL.len() - 1);
    }

    #[test]
    fn a_row_sends_the_level_it_is_marked_for() {
        let led = PowerLed::new(Board::new(), PowerLedLevel::ALL.to_vec());
        for level in PowerLedLevel::ALL {
            let row = led.row(level).expect("every level is listed");
            assert_eq!(led.at(row), Some(level));
        }
    }

    /// The match is exhaustive, so being ranked at all is the compiler's
    /// business; being ranked apart is this. Two levels sharing a rank would
    /// fall back to whichever the vocabulary lists first, which is the
    /// inheritance the rank exists to stop.
    #[test]
    fn no_two_levels_share_a_row() {
        let mut ranks = PowerLedLevel::ALL.map(rank).to_vec();
        ranks.sort_unstable();
        ranks.dedup();
        assert_eq!(ranks.len(), PowerLedLevel::ALL.len());
    }
}
