//! Volts, amps and watts as a reader sees them, one rule per unit and
//! [`Kind`].

use crate::words::trimmed;

/// How a figure moves, which decides its precision.
#[derive(Clone, Copy)]
pub enum Kind {
    /// Settled once, on a plug or a write, so a whole value reads whole.
    Static,
    /// Read afresh on every tick, at a fixed precision so the figure does not
    /// change width between readings.
    Dynamic,
}

fn spell(value: f64, decimals: usize, unit: &str, kind: Kind) -> String {
    let spelled = format!("{value:.decimals$}");
    match kind {
        Kind::Static => format!("{} {unit}", trimmed(spelled)),
        Kind::Dynamic => format!("{spelled} {unit}"),
    }
}

#[must_use]
pub fn volts(millivolts: u32, kind: Kind) -> String {
    spell(f64::from(millivolts) / 1000.0, 2, "V", kind)
}

/// A reading whose source resolves only a tenth of a volt, fixed to that
/// tenth.
#[must_use]
pub fn tenth_volts(millivolts: u32) -> String {
    spell(f64::from(millivolts) / 1000.0, 1, "V", Kind::Dynamic)
}

/// The millivolts between cells are what this line is read for, which two
/// decimals would round away.
#[must_use]
pub fn cell_volts(cell_millivolts: &[u32]) -> String {
    let cells: Vec<String> = cell_millivolts
        .iter()
        .map(|millivolts| format!("{:.3}", f64::from(*millivolts) / 1000.0))
        .collect();
    format!("{} V", cells.join(" · "))
}

#[must_use]
pub fn millivolts(millivolts: u32) -> String {
    format!("{millivolts} mV")
}

/// A current that is set or settled rather than read.
#[must_use]
pub fn amps(milliamps: u32) -> String {
    spell(f64::from(milliamps) / 1000.0, 2, "A", Kind::Static)
}

/// A current read afresh, at the precision the EC reports it.
#[must_use]
pub fn milliamps(milliamps: u32) -> String {
    format!("{milliamps} mA")
}

#[must_use]
pub fn watts(millivolts: u32, milliamps: u32, kind: Kind) -> String {
    let watts = f64::from(millivolts) * f64::from(milliamps) / 1_000_000.0;
    spell(watts, 1, "W", kind)
}

#[must_use]
pub fn watt_hours(milliamp_hours: u32, millivolts: u32) -> String {
    let watt_hours = f64::from(milliamp_hours) * f64::from(millivolts) / 1_000_000.0;
    spell(watt_hours, 1, "Wh", Kind::Static)
}

#[cfg(test)]
mod tests {
    use super::{
        Kind, amps, cell_volts, milliamps, millivolts, tenth_volts, volts, watt_hours, watts,
    };

    #[test]
    fn a_static_voltage_drops_its_trailing_zeros() {
        assert_eq!(volts(20_000, Kind::Static), "20 V");
        assert_eq!(volts(4_500, Kind::Static), "4.5 V");
        assert_eq!(volts(9_020, Kind::Static), "9.02 V");
    }

    #[test]
    fn a_dynamic_voltage_keeps_two_decimals() {
        assert_eq!(volts(15_400, Kind::Dynamic), "15.40 V");
        assert_eq!(volts(15_410, Kind::Dynamic), "15.41 V");
    }

    #[test]
    fn a_tenth_volt_reading_keeps_its_tenth() {
        assert_eq!(tenth_volts(20_000), "20.0 V");
        assert_eq!(tenth_volts(20_100), "20.1 V");
    }

    #[test]
    fn cells_read_to_the_millivolt() {
        assert_eq!(cell_volts(&[3_850, 3_851]), "3.850 · 3.851 V");
        assert_eq!(millivolts(12), "12 mV");
    }

    #[test]
    fn a_static_current_drops_its_trailing_zeros() {
        assert_eq!(amps(5_000), "5 A");
        assert_eq!(amps(4_500), "4.5 A");
        assert_eq!(amps(2_250), "2.25 A");
    }

    #[test]
    fn a_dynamic_current_reads_in_milliamps() {
        assert_eq!(milliamps(2_320), "2320 mA");
    }

    #[test]
    fn a_static_power_drops_its_trailing_zero() {
        assert_eq!(watts(20_000, 5_000, Kind::Static), "100 W");
        assert_eq!(watts(5_000, 1_500, Kind::Static), "7.5 W");
    }

    #[test]
    fn an_energy_drops_its_trailing_zero() {
        assert_eq!(watt_hours(4_640, 15_640), "72.6 Wh");
        assert_eq!(watt_hours(4_000, 15_000), "60 Wh");
        assert_eq!(watt_hours(0, 15_640), "0 Wh");
    }

    #[test]
    fn a_dynamic_power_keeps_its_tenth() {
        assert_eq!(watts(15_400, 2_320, Kind::Dynamic), "35.7 W");
        assert_eq!(watts(15_000, 2_400, Kind::Dynamic), "36.0 W");
    }
}
