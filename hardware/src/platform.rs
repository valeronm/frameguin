//! Which board a machine's DMI strings settle.
//!
//! Apart from `dmi` so the mapping is testable without a machine. Asking
//! `framework_lib`'s `get_platform` for the same mapping prints, caches
//! globally and mutates the library's own config.

use frameguin_wire::Platform;

/// Every spelling a board's `product_name` takes, matched whole. Exhaustive
/// over [`Platform`], so a variant cannot ship without one, and empty for
/// [`Platform::Unknown`], which no firmware reports.
const fn spellings(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Laptop13Gen11 => &["Laptop"],
        Platform::Laptop13Gen12 => &["Laptop (12th Gen Intel Core)"],
        Platform::Laptop13Gen13 => &["Laptop (13th Gen Intel Core)"],
        Platform::Laptop13Ultra1 => &["Laptop 13 (Intel Core Ultra Series 1)"],
        // Some 7040 firmware ships the series without its space.
        Platform::Laptop13Amd7040 => &[
            "Laptop 13 (AMD Ryzen 7040 Series)",
            "Laptop 13 (AMD Ryzen 7040Series)",
        ],
        Platform::Laptop13AmdAi300 => &["Laptop 13 (AMD Ryzen AI 300 Series)"],
        Platform::Laptop13ProUltra3 => &["Laptop 13 Pro (Intel Core Ultra Series 3)"],
        Platform::Laptop12Gen13 => &["Laptop 12 (13th Gen Intel Core)"],
        Platform::Laptop12Core3 => &["Laptop 12 (Intel Core Series 3)"],
        Platform::Laptop16Amd7040 => &["Laptop 16 (AMD Ryzen 7040 Series)"],
        Platform::Laptop16AmdAi300 => &["Laptop 16 (AMD Ryzen AI 300 Series)"],
        Platform::DesktopAmdAiMax300 => &["Desktop (AMD Ryzen AI Max 300 Series)"],
        Platform::Unknown => &[],
    }
}

/// The vendor is [`frameguin_wire::Board::new`]'s to weigh: a product name
/// alone identifies nothing, the 11th generation board reporting the bare
/// `Laptop` that any manufacturer can ship.
pub(crate) fn of(product: &str) -> Platform {
    Platform::ALL
        .into_iter()
        .find(|platform| spellings(*platform).contains(&product))
        .unwrap_or(Platform::Unknown)
}

#[cfg(test)]
mod tests {
    use frameguin_wire::Platform;

    use super::{of, spellings};

    #[test]
    fn a_board_is_the_platform_its_product_name_settles() {
        assert_eq!(
            of("Laptop 13 Pro (Intel Core Ultra Series 3)"),
            Platform::Laptop13ProUltra3
        );
        assert_eq!(of("Laptop"), Platform::Laptop13Gen11);
    }

    #[test]
    fn both_spellings_of_the_7040_series_are_one_platform() {
        assert_eq!(
            of("Laptop 13 (AMD Ryzen 7040 Series)"),
            of("Laptop 13 (AMD Ryzen 7040Series)")
        );
    }

    #[test]
    fn a_product_name_no_spelling_covers_is_unknown() {
        assert_eq!(of("Laptop 99"), Platform::Unknown);
        assert_eq!(of(""), Platform::Unknown);
    }

    #[test]
    fn every_platform_has_a_product_name_a_machine_can_report() {
        for platform in Platform::ALL {
            assert_eq!(
                !spellings(platform).is_empty(),
                platform != Platform::Unknown,
                "{platform:?}"
            );
        }
    }
}
