//! The battery: the pack, the two limits that shape its charging, and the
//! presets and the figures behind them. What a reading is called is
//! [`reading`]'s.
//!
//! Row order is settled here. A control that caps something starts at the
//! setting that caps nothing and tightens down the list; the row that
//! reveals a slider trails the presets it extends.

pub mod reading;

use std::rc::Rc;

use frameguin_wire::{
    BatteryCondition, BatteryControl, BatteryFeature, BatteryInfo, DeviceResult as Result,
    NO_CHARGE_CURRENT_LIMIT,
};

use super::{names, present};

pub struct Battery<C> {
    control: Rc<C>,
    features: Vec<BatteryFeature>,
}

impl<C: BatteryControl> Battery<C> {
    pub fn new(control: Rc<C>, features: Vec<BatteryFeature>) -> Self {
        Self { control, features }
    }

    /// Probed by the features, which are wanted anyway and fixed for the
    /// device's run — a read of the block here would only be repeated by the
    /// first fill.
    pub async fn detect(control: &Rc<C>) -> Result<Option<Self>> {
        Ok(present(control.features().await)?.map(|features| Self::new(control.clone(), features)))
    }

    #[must_use]
    pub fn has(&self, feature: BatteryFeature) -> bool {
        self.features.contains(&feature)
    }

    /// Every feature this battery offers, for a front-end that keeps its own
    /// copy.
    #[must_use]
    pub fn features(&self) -> &[BatteryFeature] {
        &self.features
    }

    pub async fn read(&self) -> Result<BatteryInfo> {
        self.control.info().await
    }

    pub async fn condition(&self) -> Result<BatteryCondition> {
        self.control.condition().await
    }

    pub async fn charge_limit(&self) -> Result<u8> {
        self.control.charge_limit().await
    }

    /// True where the hardware was written, which is what earns a toast.
    pub async fn set_charge_limit(&self, percent: u8) -> Result<bool> {
        self.control.set_charge_limit(percent).await
    }

    pub async fn charge_current_limit(&self) -> Result<u32> {
        self.control.charge_current_limit().await
    }

    pub async fn set_charge_current_limit(&self, milliamps: u32) -> Result<bool> {
        self.control.set_charge_current_limit(milliamps).await
    }
}

const CHARGE_PRESETS: [u8; 3] = [100, 80, 60];

/// The ceiling that is no ceiling. A presentation fact rather than a wire
/// one: the daemon takes 100 and writes it to the EC like any other
/// percentage, and it is only here that the value stops being a limit and
/// starts being the absence of one.
pub const NO_CHARGE_LIMIT: u8 = 100;

/// The window's combo carries one row past the presets, for a ceiling the
/// user dials in; the tray offers the presets alone.
pub const CHARGE_LIMIT_CUSTOM: usize = CHARGE_PRESETS.len();

/// The charge speeds the combo offers, each beside the divisor it applies to
/// the battery's 1C design current; `None` is full speed, which the daemon
/// takes as no limit at all.
const CHARGE_SPEEDS: [(&str, Option<u32>); 3] = [
    ("Full speed", None),
    ("Half", Some(2)),
    ("Quarter", Some(4)),
];

/// The window's combo carries one row past the presets, for a rate the user
/// dials in. The tray offers only the presets: a slider has no menu form, and
/// a preset menu that can't reach every state is the honest half.
pub const CHARGE_SPEED_CUSTOM: usize = CHARGE_SPEEDS.len();

/// The slowest the custom slider will ask for. The EC takes anything above
/// zero, but a limit this side of it charges so slowly that it reads as a
/// fault rather than a setting.
pub const MIN_CUSTOM_CHARGE_MA: u32 = 100;

/// What the custom slider rounds to. A `GtkScale` is continuous while
/// dragged — its step increment reaches only keys and the wheel — so without
/// this a drag lands on a value like 984 mA that the row then displays as
/// "1.0 A", reporting a current nobody chose.
pub const CUSTOM_CHARGE_STEP_MA: u32 = 100;

/// The milliamps a charge speed row asks the daemon for; None for a row
/// nothing is listed at. Shared by the window and the tray so the two can't
/// disagree about what "Half" sends.
#[must_use]
pub fn charge_speed_at(design_capacity: u32, row: usize) -> Option<u32> {
    let (_, divisor) = CHARGE_SPEEDS.get(row)?;
    Some(divisor.map_or(NO_CHARGE_CURRENT_LIMIT, |divisor| design_capacity / divisor))
}

/// Which row a limit sits on, and `None` when it matches no preset —
/// `framework_tool` can set any value, and guessing the nearest would
/// misreport it.
#[must_use]
pub fn charge_speed_row(design_capacity: u32, milliamps: u32) -> Option<usize> {
    (0..CHARGE_SPEEDS.len()).find(|&row| charge_speed_at(design_capacity, row) == Some(milliamps))
}

/// The bare preset names, for a menu whose title already brackets a rate.
#[must_use]
pub fn charge_speed_names() -> Vec<String> {
    names(&CHARGE_SPEEDS)
}

/// Combo labels carrying the rate each fraction works out to — "Half" alone
/// doesn't say half of what.
#[must_use]
pub fn charge_speed_labels(design_capacity: u32) -> Vec<String> {
    CHARGE_SPEEDS
        .iter()
        .map(|(name, divisor)| match divisor {
            Some(divisor) => format!("{name} ({})", reading::amps(design_capacity / divisor)),
            None => (*name).to_string(),
        })
        .collect()
}

/// The ceiling a preset row asks the daemon for; None for a row nothing is
/// listed at.
#[must_use]
pub fn charge_limit_at(row: usize) -> Option<u8> {
    CHARGE_PRESETS.get(row).copied()
}

/// Which row a ceiling sits on, and `None` when it matches none — the EC's
/// own battery extender lowers the limit unasked, and guessing the nearest
/// preset would misreport it.
#[must_use]
pub fn charge_limit_row(percent: u8) -> Option<usize> {
    CHARGE_PRESETS.iter().position(|preset| *preset == percent)
}

/// Preset names, shared so the window's combo and the tray's menu can't
/// disagree about what a ceiling is called. The window's combo appends
/// "Custom"; the tray's menu takes these as they are.
#[must_use]
pub fn charge_limit_labels() -> Vec<String> {
    CHARGE_PRESETS
        .iter()
        // Named as a state rather than as the absence of one, so a title
        // quoting the selected row still reads.
        .map(|percent| {
            if *percent == NO_CHARGE_LIMIT {
                "Off".to_string()
            } else {
                reading::percent_label(*percent)
            }
        })
        .collect()
}

/// A combo's rows: the presets, then the one that reveals a slider. Both of
/// the battery's combos build their model this way, so neither can leave the
/// extra row off and address it anyway.
#[must_use]
pub fn with_custom_row(mut labels: Vec<String>) -> Vec<String> {
    labels.push("Custom".to_string());
    labels
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use frameguin_wire::{
        BatteryCondition, BatteryControl, BatteryFeature, BatteryInfo, DeviceError,
        DeviceResult as Result, NO_CHARGE_CURRENT_LIMIT,
    };

    use super::{
        Battery, CHARGE_SPEEDS, NO_CHARGE_LIMIT, charge_limit_at, charge_limit_labels,
        charge_limit_row, charge_speed_at, charge_speed_labels, charge_speed_row, with_custom_row,
    };
    use crate::testing::{CAPACITY, Fault, absent, block, ready};

    /// A pack answering what it was built with.
    struct Stub {
        limit: Cell<u8>,
        cap: Cell<u32>,
        fault: Fault,
    }

    impl Stub {
        fn new() -> Rc<Self> {
            Self::with(Fault::default())
        }

        fn failing(error: DeviceError) -> Rc<Self> {
            Self::with(Fault::failing(error))
        }

        fn with(fault: Fault) -> Rc<Self> {
            Rc::new(Self {
                limit: Cell::new(100),
                cap: Cell::new(NO_CHARGE_CURRENT_LIMIT),
                fault,
            })
        }
    }

    impl BatteryControl for Stub {
        async fn info(&self) -> Result<BatteryInfo> {
            self.fault.read(block())
        }

        async fn condition(&self) -> Result<BatteryCondition> {
            Ok(BatteryCondition {
                cell_millivolts: vec![3_850; 4],
                alarms: Vec::new(),
                decicelsius: 300,
            })
        }

        async fn features(&self) -> Result<Vec<BatteryFeature>> {
            self.fault.read(vec![BatteryFeature::ChargeLimit])
        }

        async fn charge_limit(&self) -> Result<u8> {
            Ok(self.limit.get())
        }

        async fn set_charge_limit(&self, percent: u8) -> Result<bool> {
            self.fault.write()?;
            self.limit.set(percent);
            Ok(true)
        }

        async fn charge_current_limit(&self) -> Result<u32> {
            Ok(self.cap.get())
        }

        async fn set_charge_current_limit(&self, milliamps: u32) -> Result<bool> {
            self.fault.write()?;
            self.cap.set(milliamps);
            Ok(true)
        }
    }

    #[test]
    fn a_pack_the_hardware_answers_for_is_detected_with_its_features() {
        let battery = ready(Battery::detect(&Stub::new())).unwrap().unwrap();
        assert!(battery.has(BatteryFeature::ChargeLimit));
        assert!(!battery.has(BatteryFeature::Condition));
    }

    #[test]
    fn a_pack_the_hardware_does_not_serve_is_absent() {
        let stub = Stub::failing(absent());
        assert!(ready(Battery::detect(&stub)).unwrap().is_none());
    }

    #[test]
    fn hardware_that_cannot_be_asked_is_not_an_absent_pack() {
        let error = DeviceError::Failed("no reply".into());
        let stub = Stub::failing(error.clone());
        assert_eq!(ready(Battery::detect(&stub)).err(), Some(error));
    }

    #[test]
    fn a_write_reaches_the_hardware_and_a_read_sees_it() {
        let stub = Stub::new();
        let battery = Battery::new(stub.clone(), Vec::new());
        assert_eq!(ready(battery.set_charge_limit(80)), Ok(true));
        assert_eq!(ready(battery.charge_limit()), Ok(80));
        assert_eq!(ready(battery.set_charge_current_limit(1_160)), Ok(true));
        assert_eq!(ready(battery.charge_current_limit()), Ok(1_160));
    }

    #[test]
    fn a_refused_write_carries_the_refusal() {
        let stub = Stub::new();
        let battery = Battery::new(stub.clone(), Vec::new());
        stub.fault.refuse();
        assert_eq!(
            ready(battery.set_charge_limit(80)),
            Err(DeviceError::AccessDenied("not authorized".into()))
        );
        assert_eq!(stub.limit.get(), 100);
    }

    /// One row is the off row, and it is the one that sends the ceiling that
    /// isn't one. The label and the toast branch on the same constant, so
    /// what this catches is a preset list where that constant sits on no row
    /// at all — the two would then agree with each other and with nothing on
    /// screen.
    #[test]
    fn the_off_row_is_the_one_that_sends_no_limit() {
        let row = charge_limit_row(NO_CHARGE_LIMIT).expect("the off row is a preset");
        assert_eq!(charge_limit_at(row), Some(NO_CHARGE_LIMIT));
        assert_eq!(charge_limit_labels()[row], "Off");
    }

    #[test]
    fn full_speed_lifts_the_limit_rather_than_naming_the_pack_rate() {
        // Sending the capacity would install a real cap at 1C; the EC only
        // stops clamping when the limit is the maximum.
        assert_eq!(charge_speed_at(CAPACITY, 0), Some(NO_CHARGE_CURRENT_LIMIT));
    }

    #[test]
    fn presets_are_fractions_of_the_pack_rate() {
        assert_eq!(charge_speed_at(CAPACITY, 1), Some(2320));
        assert_eq!(charge_speed_at(CAPACITY, 2), Some(1160));
    }

    #[test]
    fn a_preset_round_trips_to_its_own_row() {
        for row in 0..CHARGE_SPEEDS.len() {
            let milliamps = charge_speed_at(CAPACITY, row).expect("every preset has a row");
            assert_eq!(charge_speed_row(CAPACITY, milliamps), Some(row));
        }
        assert_eq!(charge_speed_at(CAPACITY, CHARGE_SPEEDS.len()), None);
    }

    #[test]
    fn a_dialled_in_value_matches_no_preset() {
        assert_eq!(charge_speed_row(CAPACITY, 1500), None);
    }

    #[test]
    fn labels_name_the_rate_and_end_with_the_custom_row() {
        let labels = with_custom_row(charge_speed_labels(CAPACITY));
        assert_eq!(labels.len(), CHARGE_SPEEDS.len() + 1);
        assert_eq!(labels[0], "Full speed");
        assert_eq!(labels[1], "Half (2.3 A)");
        assert_eq!(labels[CHARGE_SPEEDS.len()], "Custom");
    }
}
