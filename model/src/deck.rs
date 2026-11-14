//! What the input deck's rail powers, by Framework's own account of each
//! machine.

use frameguin_contract::{Platform, Series};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeckLoad {
    /// The keyboard is wired to the EC apart from the rail.
    Touchpad,
    /// The keyboard and every other module in the deck.
    InputModules,
}

/// None for a machine Framework names no load for.
#[must_use]
pub const fn load(platform: Platform) -> Option<DeckLoad> {
    match platform.series() {
        Some(Series::Laptop13 | Series::Laptop12) => Some(DeckLoad::Touchpad),
        Some(Series::Laptop16) => Some(DeckLoad::InputModules),
        Some(Series::Desktop) | None => None,
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::Platform;

    use super::{DeckLoad, load};

    #[test]
    fn only_a_laptop_16_powers_its_keyboard_from_the_deck_rail() {
        assert_eq!(
            load(Platform::Laptop16AmdAi300),
            Some(DeckLoad::InputModules)
        );
        assert_eq!(load(Platform::Laptop13ProUltra3), Some(DeckLoad::Touchpad));
        assert_eq!(load(Platform::Laptop12Core3), Some(DeckLoad::Touchpad));
        assert_eq!(load(Platform::DesktopAmdAiMax300), None);
        assert_eq!(load(Platform::Unknown), None);
    }
}
