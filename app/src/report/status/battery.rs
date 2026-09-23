//! The battery section: one row carrying the charge, and a page naming what
//! the pack is doing.
//!
//! What each value is *called* is `frameguin_model::control::battery::reading`'s;
//! which rows there are and what fills them is this module's.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::control::battery::Battery;
use frameguin_model::control::battery::reading::{
    alarms_label, capacity, cell_spread, cell_voltages, charge_brief, charge_direction,
    charger_label, milliamps, percent_label, power_label, retention_label, temperature, volts,
};
use frameguin_wire::BatteryFeature;
use gtk4 as gtk;

use super::{Sidebar, Target};
use crate::bus::Bus;
use crate::reading::{Feed, Reading, Wants, show_while_mapped};
use crate::report::{described_value, value, value_row};

/// Every field is a descendant of the page the feed's subscription hangs on,
/// the subscription's closure holding this struct.
struct Report {
    charge_row: adw::ActionRow,
    charge: gtk::Label,
    charger: gtk::Label,
    current: gtk::Label,
    voltage: gtk::Label,
    power: gtk::Label,
    /// Hidden on a board whose pack will not answer over I2C, which is the
    /// same feature the spread's row below waits on.
    temperature_row: adw::ActionRow,
    temperature: gtk::Label,
    /// Shown only while the EC's own low-charge alarm stands, which is the
    /// one thing here a reader should not have to go looking for.
    critical_row: adw::ActionRow,
    /// The pack's own alarms, which are its gauge's rather than the EC's.
    /// Shown only while it raises any: a row saying nothing is wrong is a row
    /// that trains people to stop reading it.
    alarm_row: adw::ActionRow,
    /// The spread carries the value and the row's subtitle the cells behind
    /// it — the number that matters first, the working shown under it. Hidden
    /// on a board whose pack will not answer over I2C.
    spread_row: adw::ActionRow,
    spread: gtk::Label,
    remaining: gtk::Label,
    last_full: gtk::Label,
    design_capacity: gtk::Label,
    retention: gtk::Label,
    cycles: gtk::Label,
}

impl Report {
    /// An absent extra — no sensor on this board, or a read that missed —
    /// leaves its rows as they were rather than blanking them, which is what
    /// keeps a single unlucky transfer from reading as a fault.
    fn show(&self, reading: &Reading) {
        // This page is only built where a pack answered, so an absent block
        // is a read that missed.
        let Some(info) = &reading.info else {
            return;
        };
        if let Some(condition) = &reading.condition {
            self.temperature
                .set_label(&temperature(condition.decicelsius));
            if let Some(spread) = cell_spread(&condition.cell_millivolts) {
                self.spread.set_label(&spread);
                self.spread_row
                    .set_subtitle(&cell_voltages(&condition.cell_millivolts));
            }
            let alarms = alarms_label(&condition.alarms);
            self.alarm_row.set_visible(!alarms.is_empty());
            self.alarm_row.set_subtitle(&alarms);
        }
        self.charge.set_label(&percent_label(info.state.percent));
        self.charge_row.set_subtitle(charge_direction(info.state));
        self.charger
            .set_label(charger_label(info.charger_connected));
        self.current.set_label(&milliamps(info.state.milliamps));
        self.voltage.set_label(&volts(info.state.millivolts));
        self.power.set_label(&power_label(info.state));
        self.critical_row.set_visible(info.critical);

        // All three against the nominal voltage rather than the one the pack
        // reads now: a rating is what it is compared with, and taking a charge
        // against the terminal voltage of the moment would make the same pack
        // read differently full than empty.
        self.remaining
            .set_label(&capacity(info.remaining_capacity, info.design_millivolts));
        self.last_full
            .set_label(&capacity(info.last_full_capacity, info.design_millivolts));
        self.design_capacity
            .set_label(&capacity(info.design_capacity, info.design_millivolts));
        self.retention.set_label(&retention_label(
            info.last_full_capacity,
            info.design_capacity,
        ));
        self.cycles.set_label(&info.cycle_count.to_string());
    }
}

/// The pack's condition costs a transfer per cell, so only the page asks for
/// it.
pub(super) fn add(sidebar: &Rc<Sidebar>, feed: &Rc<Feed>, battery: &Battery<Bus>) {
    let list = sidebar.section(None);
    let page = adw::PreferencesPage::new();
    let report = build_rows(&page);
    let row = sidebar.add(&list, "Battery", &page);

    let answering: gtk::ListBoxRow = row.clone().upcast();
    sidebar.answer(move |target| (target == Target::Battery).then(|| answering.clone()));

    let summary = Wants {
        battery: true,
        ..Wants::default()
    };
    sidebar.follow(feed, summary, move |reading| {
        if let Some(info) = &reading.info {
            row.set_subtitle(&charge_brief(info.state));
        }
    });

    let condition = battery.has(BatteryFeature::Condition);
    // Both rows read the pack over I2C, so one feature answers for the pair.
    report.temperature_row.set_visible(condition);
    report.spread_row.set_visible(condition);
    let wants = Wants {
        battery: true,
        condition,
        ..Wants::default()
    };
    show_while_mapped(feed, &page, wants, move |reading| report.show(reading));
}

/// Every row of the page, added in the order they are read in. The first group
/// goes untitled, the page's own title already naming it.
fn build_rows(page: &adw::PreferencesPage) -> Rc<Report> {
    let status_group = adw::PreferencesGroup::new();
    let (charge_row, charge) = value_row(&status_group, "Charge");
    let charger = value(&status_group, "Charger");
    let current = value(&status_group, "Current");
    let voltage = value(&status_group, "Voltage");
    // Under the two it is the product of, so the arithmetic is visible.
    let power = value(&status_group, "Power");
    let (temperature_row, temperature) = value_row(&status_group, "Temperature");
    // Hidden until the pack's features say there is a sensor; a row that
    // appeared empty would read as one that failed to fill.
    temperature_row.set_visible(false);
    let (spread_row, spread) = value_row(&status_group, "Cell balance");
    // Hidden until the pack answers over I2C, and its subtitle filled with the
    // cells behind the figure once it does.
    spread_row.set_visible(false);
    let critical_row = adw::ActionRow::builder()
        .title("Charge critically low")
        .subtitle("The EC has raised its own low-charge alarm")
        .visible(false)
        .build();
    critical_row.add_css_class("error");
    status_group.add(&critical_row);
    // The pack's own alarms rather than the EC's flag above, so the two sit
    // together and a reader need not know which device raised what.
    let alarm_row = adw::ActionRow::builder()
        .title("Battery problem reported")
        .visible(false)
        .build();
    alarm_row.add_css_class("error");
    status_group.add(&alarm_row);
    page.add(&status_group);

    let capacity_group = adw::PreferencesGroup::builder().title("Capacity").build();
    let remaining = value(&capacity_group, "Remaining");
    let last_full = value(&capacity_group, "Last full charge");
    let design_capacity = value(&capacity_group, "Design capacity");
    let retention = described_value(
        &capacity_group,
        "Retention",
        "Last full charge against design capacity",
    );
    let cycles = value(&capacity_group, "Charge cycles");
    page.add(&capacity_group);

    Rc::new(Report {
        charge_row,
        charge,
        charger,
        current,
        voltage,
        power,
        temperature_row,
        temperature,
        critical_row,
        alarm_row,
        spread_row,
        spread,
        remaining,
        last_full,
        design_capacity,
        retention,
        cycles,
    })
}
