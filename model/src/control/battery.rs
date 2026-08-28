//! The battery: the pack's reading, the two limits that shape its charging,
//! and the words for all of it — the presets, the figures behind them, and
//! how each value is named.
//!
//! Row order is settled here. A control that caps something starts at the
//! setting that caps nothing and tightens down the list; the row that
//! reveals a slider trails the presets it extends.

use std::rc::Rc;

use frameguin_wire::{
    BatteryAlarm, BatteryCondition, BatteryControl, BatteryFeature, BatteryInfo, BatteryState,
    ChargeFlow, DeviceResult as Result, NO_CHARGE_CURRENT_LIMIT,
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

/// Milliamps as the amps a person reads off a charger.
#[must_use]
pub fn amps(milliamps: u32) -> String {
    format!("{:.1} A", f64::from(milliamps) / 1000.0)
}

/// The rate as power, which is the figure a charger and a machine's draw are
/// both rated in. `None` when the pack reports no voltage, for the reason a
/// zero rate is dropped: the number would read as a fault rather than as a
/// reading.
fn watts(state: BatteryState) -> Option<String> {
    (state.millivolts != 0).then(|| {
        let watts = f64::from(state.milliamps) * f64::from(state.millivolts) / 1_000_000.0;
        format!("{watts:.1} W")
    })
}

/// Which way charge is moving, and nothing else — for a reader who has the
/// current, the voltage and the power on their own rows below, where each is
/// exact and this would only be a rounded copy.
///
/// "Plugged in, not charging" carries its own direction rather than naming a
/// rate of nothing, which is why it reads as a sentence where the others read
/// as a word.
///
/// A rate arriving under `Idle` names the charge ending rather than nothing at
/// all. That pairing has one source: the daemon reaches `Idle` with a rate only
/// where the EC claims neither direction, which is the state its charge limiter
/// holds the pack in while the current falls away. A pack simply resting at its
/// ceiling reports a clean zero, so the two never collide.
#[must_use]
pub fn charge_direction(state: BatteryState) -> &'static str {
    match state.flow {
        ChargeFlow::Charging => "Charging",
        ChargeFlow::Discharging => "Discharging",
        ChargeFlow::Idle if state.milliamps > 0 => "Finishing charge",
        ChargeFlow::Idle => "Plugged in, not charging",
    }
}

/// The rate as power, for the row that shows it beside the current and the
/// voltage whose product it is. Unknown rather than zero where the pack
/// reports no voltage: a watt figure computed from a voltage that isn't there
/// would read as a measurement.
#[must_use]
pub fn power_label(state: BatteryState) -> String {
    watts(state).unwrap_or_else(|| UNKNOWN.to_string())
}

/// Which way charge is moving, with the rate where there is one to name — for
/// the window's row and the tray's line, which have nowhere else to put it.
///
/// A rate of zero is dropped rather than rendered: "0.0 A" is what a pack
/// reports in the moment either side of a direction changing, and it reads as
/// a fault.
#[must_use]
pub fn charge_flow_label(state: BatteryState) -> String {
    let direction = charge_direction(state);
    if state.milliamps == 0 {
        return direction.to_string();
    }
    let rate = amps(state.milliamps);
    match watts(state) {
        Some(watts) => format!("{direction} at {rate} ({watts})"),
        None => format!("{direction} at {rate}"),
    }
}

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
pub fn capacity(milliamp_hours: u32, design_millivolts: u32) -> String {
    format!(
        "{} ({milliamp_hours} mAh)",
        watt_hours(milliamp_hours, design_millivolts)
    )
}

/// Millivolts as the volts a pack is rated in.
#[must_use]
pub fn volts(millivolts: u32) -> String {
    format!("{:.2} V", f64::from(millivolts) / 1000.0)
}

/// A rate at the precision the EC reports it, where [`amps`] rounds to the
/// tenth a charger is labelled with. The report is where the exact figure
/// belongs: "1.2 A" cannot show a current settling, which is most of what
/// there is to watch as a charge ends.
#[must_use]
pub fn milliamps(milliamps: u32) -> String {
    format!("{milliamps} mA")
}

/// A charge as a percentage: what the window's row and the report's first line
/// both show, spelled once so the two windows cannot render one reading two
/// ways.
#[must_use]
pub fn percent_label(percent: u8) -> String {
    format!("{percent}%")
}

/// How much of its rating the pack still holds, or the word for one with
/// nothing to measure it against. A new pack that outperforms its rating
/// reads above 100%, left as it stands: unlike a charge, this has no ceiling
/// that makes more than full meaningless.
#[must_use]
pub fn retention_label(last_full_capacity: u32, design_capacity: u32) -> String {
    match retention_percent(last_full_capacity, design_capacity) {
        Some(percent) => format!("{percent}%"),
        None => UNKNOWN.to_string(),
    }
}

/// What the pack can still hold against what it was built to hold. `None`
/// where the design capacity reads zero — nothing to compare against, and 0%
/// would name a dead pack rather than an unanswered question.
fn retention_percent(last_full_capacity: u32, design_capacity: u32) -> Option<u32> {
    if design_capacity == 0 {
        return None;
    }
    // Widened for the multiply: the product of two plausible mAh figures
    // leaves u32 long before either of them does.
    let retained = u64::from(last_full_capacity) * 100 / u64::from(design_capacity);
    u32::try_from(retained).ok()
}

/// The pack's temperature, to the tenth of a degree its sensor resolves.
#[must_use]
pub fn temperature(decicelsius: i16) -> String {
    format!("{:.1} °C", f64::from(decicelsius) / 10.0)
}

#[must_use]
pub fn charger_label(connected: bool) -> &'static str {
    if connected {
        "Connected"
    } else {
        "Not connected"
    }
}

/// The gap between the pack's highest and lowest cell, which is what four cell
/// voltages are worth knowing. The EC publishes only their sum, and a pack
/// whose total reads healthy can still have one cell drifting away from the
/// rest — that drift is the earliest sign a pack is failing, and the only
/// place it shows. `None` on a pack that reports no cells.
#[must_use]
pub fn cell_spread(cell_millivolts: &[u32]) -> Option<String> {
    let high = cell_millivolts.iter().max()?;
    let low = cell_millivolts.iter().min()?;
    Some(format!("{} mV", high - low))
}

/// Every cell's voltage on one line, to sit under the spread. To three
/// decimals where [`volts`] gives two: the whole point is the millivolts
/// between them, which two decimals would round away.
#[must_use]
pub fn cell_voltages(cell_millivolts: &[u32]) -> String {
    let cells: Vec<String> = cell_millivolts
        .iter()
        .map(|millivolts| format!("{:.3}", f64::from(*millivolts) / 1000.0))
        .collect();
    format!("{} V", cells.join(" · "))
}

fn alarm_label(alarm: BatteryAlarm) -> &'static str {
    match alarm {
        BatteryAlarm::OverCharged => "Charged past its safe limit",
        BatteryAlarm::OverTemperature => "Too hot",
        // What the pack is doing, not the flags it is doing it with: a reader
        // wants to know the battery has stopped, not that two bits are set.
        BatteryAlarm::SafetyFault => "Refusing to charge or discharge",
    }
}

/// Every alarm the pack is raising, on one line. Empty for a pack raising
/// none, which is the caller's cue to show nothing at all rather than a row
/// announcing that nothing is wrong.
#[must_use]
pub fn alarms_label(alarms: &[BatteryAlarm]) -> String {
    alarms
        .iter()
        .map(|alarm| alarm_label(*alarm))
        .collect::<Vec<_>>()
        .join(" · ")
}

/// What the report says where the pack answered with nothing. One spelling for
/// every such row, so a value the EC left blank and a figure with no
/// denominator read the same way rather than as two different faults.
const UNKNOWN: &str = "Unknown";

/// A name the pack left blank, which some do for the serial. Named rather than
/// left empty: a blank value reads as a row that failed to fill.
#[must_use]
pub fn text_or_unknown(text: &str) -> &str {
    if text.is_empty() { UNKNOWN } else { text }
}

/// The whole state on one line, for the tray, which has no second line to
/// put the charge on.
#[must_use]
pub fn battery_summary(state: BatteryState) -> String {
    format!(
        "{} · {}",
        percent_label(state.percent),
        charge_flow_label(state)
    )
}

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
            Some(divisor) => format!("{name} ({})", amps(design_capacity / divisor)),
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
                format!("{percent}%")
            }
        })
        .collect()
}

/// A combo's rows: the presets, then the one that reveals a slider. Both
/// preset-plus-custom controls build their model this way, so neither can
/// leave the extra row off and address it anyway.
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
        BatteryCondition, BatteryControl, BatteryFeature, BatteryInfo, BatteryState, ChargeFlow,
        DeviceError, DeviceResult as Result, NO_CHARGE_CURRENT_LIMIT,
    };

    use super::{
        Battery, CHARGE_SPEEDS, NO_CHARGE_LIMIT, battery_summary, capacity, charge_direction,
        charge_flow_label, charge_limit_at, charge_limit_labels, charge_limit_row, charge_speed_at,
        charge_speed_labels, charge_speed_row, power_label, retention_label, volts, watt_hours,
        with_custom_row,
    };
    use crate::testing::{Fault, ready};

    /// A 4640 mAh pack, the Laptop 13's.
    const CAPACITY: u32 = 4640;

    /// Mid-charge on the same pack's four cells.
    const MILLIVOLTS: u32 = 15_400;

    /// What that pack is rated at, which is what its energy is measured
    /// against however charged it happens to be.
    const NOMINAL_MILLIVOLTS: u32 = 15_640;

    fn state(flow: ChargeFlow, milliamps: u32) -> BatteryState {
        BatteryState {
            percent: 62,
            flow,
            milliamps,
            millivolts: MILLIVOLTS,
        }
    }

    fn block() -> BatteryInfo {
        BatteryInfo {
            state: state(ChargeFlow::Charging, 2320),
            remaining_capacity: 2843,
            last_full_capacity: 4176,
            design_capacity: CAPACITY,
            design_millivolts: NOMINAL_MILLIVOLTS,
            cycle_count: 40,
            charger_connected: true,
            critical: false,
            manufacturer: "NVT".into(),
            model: "FRANGWA".into(),
            serial: String::new(),
            chemistry: "LION".into(),
            manufactured: String::new(),
        }
    }

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
        let stub = Stub::failing(DeviceError::Absent("no such interface".into()));
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

    /// A pack that has lost some of what it was built to hold, which is what
    /// the report's retention row exists to say.
    #[test]
    fn retention_is_the_last_full_charge_against_the_design_capacity() {
        assert_eq!(retention_label(4176, CAPACITY), "90%");
        assert_eq!(retention_label(CAPACITY, CAPACITY), "100%");
    }

    /// A new pack can hold more than it was rated for, and the row says so
    /// rather than clamping — nothing about retention above 100% is nonsense
    /// the way a charge above full would be.
    #[test]
    fn retention_above_the_design_capacity_is_left_as_it_reads() {
        assert_eq!(retention_label(4736, CAPACITY), "102%");
    }

    /// A pack reporting no design capacity leaves nothing to divide by, and
    /// 0% would name a dead pack rather than a missing answer.
    #[test]
    fn retention_against_no_design_capacity_is_not_a_number() {
        assert_eq!(retention_label(4176, 0), "Unknown");
    }

    #[test]
    fn millivolts_read_as_the_volts_a_pack_is_rated_in() {
        assert_eq!(volts(MILLIVOLTS), "15.40 V");
    }

    /// The figure Framework quote a pack by, which milliamp-hours alone cannot
    /// be checked against.
    #[test]
    fn a_capacity_leads_with_the_energy_it_holds() {
        assert_eq!(capacity(CAPACITY, NOMINAL_MILLIVOLTS), "72.6 Wh (4640 mAh)");
    }

    /// Taken against the pack's rating, so the same charge reads the same
    /// whatever the terminal voltage is doing at the time.
    #[test]
    fn energy_is_the_charge_against_the_nominal_voltage() {
        assert_eq!(watt_hours(2843, NOMINAL_MILLIVOLTS), "44.5 Wh");
        assert_eq!(watt_hours(0, NOMINAL_MILLIVOLTS), "0.0 Wh");
    }

    #[test]
    fn a_moving_charge_is_named_with_its_rate_and_its_power() {
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Charging, 2320)),
            "Charging at 2.3 A (35.7 W)"
        );
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Discharging, 1400)),
            "Discharging at 1.4 A (21.6 W)"
        );
    }

    /// Power is the rate against the voltage of that same moment, so a pack
    /// that answers with no voltage names the rate alone rather than 0.0 W.
    #[test]
    fn a_rate_without_a_voltage_is_named_without_its_power() {
        let unread = BatteryState {
            millivolts: 0,
            ..state(ChargeFlow::Discharging, 1400)
        };
        assert_eq!(charge_flow_label(unread), "Discharging at 1.4 A");
    }

    /// A pack reports no rate for a moment either side of a direction
    /// changing, and "at 0.0 A" reads as a fault rather than as a reading.
    #[test]
    fn a_direction_without_a_rate_is_named_alone() {
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Charging, 0)),
            "Charging"
        );
    }

    /// A full pack on a charger is the state the EC's own flags describe as
    /// discharging, and the one a rate would say nothing useful about.
    #[test]
    fn a_pack_resting_on_its_charger_names_neither_a_direction_nor_a_rate() {
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Idle, 0)),
            "Plugged in, not charging"
        );
    }

    /// The charge limiter holds the pack with the current still falling away,
    /// and the watts are the point: saying only "not charging" there hides a
    /// reading the pack is still giving.
    #[test]
    fn a_charge_winding_down_at_the_limit_is_named_with_its_rate() {
        assert_eq!(
            charge_flow_label(state(ChargeFlow::Idle, 2320)),
            "Finishing charge at 2.3 A (35.7 W)"
        );
    }

    /// The report says the same states in one word, because the rows under it
    /// carry the rate exactly. Every state the full label names has to survive
    /// the shortening — the resting pack above most of all, whose sentence is
    /// its direction rather than an omission.
    #[test]
    fn the_report_names_a_direction_without_the_rate_beneath_it() {
        assert_eq!(
            charge_direction(state(ChargeFlow::Charging, 2320)),
            "Charging"
        );
        assert_eq!(
            charge_direction(state(ChargeFlow::Discharging, 1400)),
            "Discharging"
        );
        assert_eq!(
            charge_direction(state(ChargeFlow::Idle, 2320)),
            "Finishing charge"
        );
        assert_eq!(
            charge_direction(state(ChargeFlow::Idle, 0)),
            "Plugged in, not charging"
        );
    }

    /// The row the rate was taken out of the subtitle for.
    #[test]
    fn power_is_the_rate_against_the_voltage_of_the_moment() {
        assert_eq!(power_label(state(ChargeFlow::Charging, 2320)), "35.7 W");
        // A pack either side of a direction change, where zero is the reading
        // rather than a fault — the row beside it says 0 mA.
        assert_eq!(power_label(state(ChargeFlow::Charging, 0)), "0.0 W");
    }

    /// No voltage means no product to report, and 0.0 W would read as one.
    #[test]
    fn power_without_a_voltage_is_not_a_number() {
        let unread = BatteryState {
            millivolts: 0,
            ..state(ChargeFlow::Discharging, 1400)
        };
        assert_eq!(power_label(unread), "Unknown");
    }

    /// The window splits charge and direction across a row's two halves; the
    /// tray has one line for both.
    #[test]
    fn the_trays_line_carries_the_charge_as_well() {
        assert_eq!(
            battery_summary(state(ChargeFlow::Charging, 2320)),
            "62% · Charging at 2.3 A (35.7 W)"
        );
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
