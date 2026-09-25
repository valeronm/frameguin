//! What a report of a failed read calls the attempt.

use frameguin_model::reading::Extra;

#[must_use]
pub fn attempt(extra: Extra) -> &'static str {
    match extra {
        Extra::Battery => "Reading the battery",
        Extra::Condition => "Reading the battery's condition",
        Extra::Ports => "Reading the USB-C ports",
        Extra::Chassis => "Reading the chassis",
        Extra::Deck => "Reading the input deck",
        Extra::PrivacySwitches => "Reading the privacy switches",
        Extra::Extender => "Reading the battery extender",
        Extra::ChargeLimit => "Reading the charge limit",
        Extra::Usb => "Reading the USB devices",
        Extra::ChargingLedSide => "Reading the charging LED's side",
    }
}

#[cfg(test)]
mod tests {
    use frameguin_model::reading::Extra::{
        Battery, ChargeLimit, ChargingLedSide, Chassis, Condition, Deck, Extender, Ports,
        PrivacySwitches, Usb,
    };

    use super::attempt;

    #[test]
    fn every_extra_has_a_phrase() {
        for extra in [
            Battery,
            Condition,
            Ports,
            Chassis,
            Deck,
            PrivacySwitches,
            Extender,
            ChargeLimit,
            Usb,
            ChargingLedSide,
        ] {
            assert!(attempt(extra).starts_with("Reading the "));
        }
    }
}
