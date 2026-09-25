//! A battery part's capacity and voltage, as figures.

/// A capacity as the energy it holds, taken against the pack's nominal
/// voltage, which is the convention a pack is rated by — and the unit
/// Framework quote their batteries in, so it is the figure a reader can check
/// against the spec. Milliamp-hours alone are only half of it: they say
/// nothing about the voltage the cells deliver them at.
fn watt_hours(milliamp_hours: u32, design_millivolts: u32) -> String {
    let watt_hours = f64::from(milliamp_hours) * f64::from(design_millivolts) / 1_000_000.0;
    format!("{watt_hours:.1} Wh")
}

/// A capacity in both units, energy first because that is what the pack is
/// sold as, with the charge the EC actually reported after it.
#[must_use]
pub(crate) fn capacity(milliamp_hours: u32, design_millivolts: u32) -> String {
    format!(
        "{} ({milliamp_hours} mAh)",
        watt_hours(milliamp_hours, design_millivolts)
    )
}

/// Millivolts as the volts a pack is rated in.
#[must_use]
pub(crate) fn volts(millivolts: u32) -> String {
    format!("{:.2} V", f64::from(millivolts) / 1000.0)
}
