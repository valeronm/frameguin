//! The battery report: a window naming everything the EC says about the pack.
//!
//! Its own module because it is the app's one window that only reads. Nothing
//! here writes, so none of [`crate::ui`]'s machinery applies — no sync guard,
//! no debounce, no tray push — and keeping it apart is what stops that
//! machinery being reached for out of habit when a row is added.
//!
//! Both front-ends open it, the window's charge row and the tray's reading,
//! and neither builds it: they activate `app.battery-details`, which lands
//! here. That is what lets the tray open the report with no window built, and
//! what keeps a second click from putting a second report on screen — the one
//! already open is found in the application's own window list rather than
//! remembered in a slot that a closed window would leave stale.
//!
//! What each value is *called* stays in [`crate::format`] with the rest of the
//! labels; which rows there are and what fills them is this module's.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_wire::Capability;
use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;

use crate::format::{
    alarms_label, capacity, cell_spread, cell_voltages, charge_direction, charger_label,
    health_label, milliamps, percent_label, power_label, temperature, text_or_unknown, volts,
};
use crate::reading::{Feed, Reading, Wants, show_while_mapped};

/// Names the report in the application's window list, which is how a second
/// open finds the one already on screen. Spelled once, because a lookup and a
/// name that disagreed would open reports without limit.
const WINDOW_NAME: &str = "frameguin-battery-report";

/// The application action that opens the report, and the only way in — see
/// this module's own doc. Spelled once here: the window's row addresses it
/// with GTK's `app.` prefix and the tray activates it without one, and a name
/// they disagreed about would be a row that silently does nothing.
pub(crate) const ACTION: &str = "battery-details";

/// The labels the report fills, and the rows that carry more than a label.
///
/// Every field is a descendant of the page the feed's subscription hangs on,
/// and deliberately so: the subscription's closure holds this struct, so a
/// field reaching back up the tree — the toast overlay above all — would make
/// a loop that outlives the window it belongs to. What toasts is the fill
/// below, which has the overlay in hand without this holding one.
struct Report {
    /// Carries the direction as its subtitle, and only the direction. The
    /// window's row and the tray's line name the rate there too, because
    /// neither has anywhere else to put it; here the three rows below say the
    /// same thing exactly, and a rounded second copy above them would be the
    /// one a reader trusted.
    charge_row: adw::ActionRow,
    charge: gtk::Label,
    current: gtk::Label,
    voltage: gtk::Label,
    power: gtk::Label,
    charger: gtk::Label,
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
    health: gtk::Label,
    cycles: gtk::Label,
    chemistry: gtk::Label,
    design_voltage: gtk::Label,
    manufacturer: gtk::Label,
    model: gtk::Label,
    serial: gtk::Label,
    manufacture_date: gtk::Label,
}

impl Report {
    /// An absent extra — no sensor on this board, or a read that missed —
    /// leaves its rows as they were rather than blanking them, which is what
    /// keeps a single unlucky transfer from reading as a fault.
    fn show(&self, reading: &Reading) {
        let info = &reading.info;
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
        self.current.set_label(&milliamps(info.state.milliamps));
        self.voltage.set_label(&volts(info.state.millivolts));
        self.power.set_label(&power_label(info.state));
        self.charger
            .set_label(charger_label(info.charger_connected));
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
        self.health
            .set_label(&health_label(info.last_full_capacity, info.design_capacity));
        self.cycles.set_label(&info.cycle_count.to_string());

        self.chemistry.set_label(text_or_unknown(&info.chemistry));
        self.design_voltage
            .set_label(&volts(info.design_millivolts));
        self.manufacturer
            .set_label(text_or_unknown(&info.manufacturer));
        self.model.set_label(text_or_unknown(&info.model));
        self.serial.set_label(text_or_unknown(&info.serial));
        self.manufacture_date
            .set_label(text_or_unknown(&info.manufactured));
    }
}

/// A row naming one value, and the label that carries it. Every row here has
/// the same shape, so building them one way is what keeps a window this long
/// from drifting into a different row per value. The row itself is worth
/// keeping only where something moves it later, hence [`value`] beside this.
fn value_row(group: &adw::PreferencesGroup, title: &str) -> (adw::ActionRow, gtk::Label) {
    let row = adw::ActionRow::builder().title(title).build();
    let value = gtk::Label::new(None);
    value.add_css_class("dim-label");
    row.add_suffix(&value);
    group.add(&row);
    (row, value)
}

/// A row whose value is all anyone needs back — most of them.
fn value(group: &adw::PreferencesGroup, title: &str) -> gtk::Label {
    value_row(group, title).1
}

/// A row whose title needs a second line to say what it measures against.
fn described_value(group: &adw::PreferencesGroup, title: &str, subtitle: &str) -> gtk::Label {
    let (row, value) = value_row(group, title);
    row.set_subtitle(subtitle);
    value
}

/// The action that opens the report, for the application to register.
///
/// Built here rather than in `main.rs` so that [`present`] can stay private:
/// with the action the only thing that reaches it, no caller can grow its own
/// way in and leave the two front-ends opening the report differently.
pub(crate) fn action(feed: Rc<Feed>) -> gio::ActionEntry<adw::Application> {
    gio::ActionEntry::builder(ACTION)
        .activate(move |app: &adw::Application, _, _| present(app, &feed))
        .build()
}

/// Brings the report to the screen: the one already open where there is one,
/// a new one otherwise.
fn present(app: &adw::Application, feed: &Rc<Feed>) {
    if let Some(open) = app
        .windows()
        .into_iter()
        .find(|window| window.widget_name() == WINDOW_NAME)
    {
        open.present();
        return;
    }
    build(app, feed).present();
}

/// The report, built and left to fill itself.
///
/// Closing it destroys it, unlike the main window, which hides to the tray: a
/// hidden window stays registered with the application and would hold a
/// tray-less app alive with nothing on screen. Nothing is lost by rebuilding —
/// the connection it reads over belongs to the feed, which outlives any one
/// window.
fn build(app: &adw::Application, feed: &Rc<Feed>) -> adw::Window {
    let page = adw::PreferencesPage::new();

    let charge_group = adw::PreferencesGroup::builder().title("Charge").build();
    let (charge_row, charge) = value_row(&charge_group, "Charge");
    let current = value(&charge_group, "Current");
    let voltage = value(&charge_group, "Voltage");
    // Under the two it is the product of, so the arithmetic is visible.
    let power = value(&charge_group, "Power");
    let charger = value(&charge_group, "Charger");
    let (temperature_row, temperature) = value_row(&charge_group, "Temperature");
    // Hidden until the daemon's probe says there is a sensor; a row that
    // appeared empty would read as one that failed to fill.
    temperature_row.set_visible(false);
    let (spread_row, spread) = value_row(&charge_group, "Cell balance");
    // Hidden until the pack answers over I2C, and its subtitle filled with the
    // cells behind the figure once it does.
    spread_row.set_visible(false);
    let critical_row = adw::ActionRow::builder()
        .title("Charge critically low")
        .subtitle("The EC has raised its own low-charge alarm")
        .visible(false)
        .build();
    critical_row.add_css_class("error");
    charge_group.add(&critical_row);
    // The pack's own alarms rather than the EC's flag above, so the two sit
    // together and a reader need not know which device raised what.
    let alarm_row = adw::ActionRow::builder()
        .title("The battery is reporting a problem")
        .visible(false)
        .build();
    alarm_row.add_css_class("error");
    charge_group.add(&alarm_row);
    page.add(&charge_group);

    let capacity_group = adw::PreferencesGroup::builder().title("Capacity").build();
    let remaining = value(&capacity_group, "Charge now");
    let last_full = value(&capacity_group, "Last full charge");
    let design_capacity = value(&capacity_group, "Design capacity");
    let health = described_value(
        &capacity_group,
        "Health",
        "Last full charge against design capacity",
    );
    let cycles = value(&capacity_group, "Charge cycles");
    page.add(&capacity_group);

    let pack_group = adw::PreferencesGroup::builder().title("Pack").build();
    let chemistry = value(&pack_group, "Chemistry");
    let design_voltage = described_value(
        &pack_group,
        "Nominal voltage",
        "What the pack is rated at, not what it reads now",
    );
    let manufacturer = value(&pack_group, "Manufacturer");
    let model = value(&pack_group, "Model");
    let serial = value(&pack_group, "Serial number");
    let manufacture_date = value(&pack_group, "Manufactured");
    page.add(&pack_group);

    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&page));
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&view));

    let window = adw::Window::builder()
        .application(app)
        .title("Battery")
        .default_width(420)
        // Tall enough for every group at the default font scale; re-measure
        // when the rows change.
        .default_height(680)
        .content(&toasts)
        .build();
    window.set_widget_name(WINDOW_NAME);

    let report = Rc::new(Report {
        charge_row,
        charge,
        current,
        voltage,
        power,
        charger,
        temperature,
        critical_row,
        alarm_row,
        spread_row,
        spread,
        remaining,
        last_full,
        design_capacity,
        health,
        cycles,
        chemistry,
        design_voltage,
        manufacturer,
        model,
        serial,
        manufacture_date,
    });

    let feed = feed.clone();
    glib::spawn_future_local(async move {
        // Asked only for the rows that a board can lack. The report is
        // reachable only from a reading the board already has, so nothing else
        // here is in question by the time this window exists — and an ask that
        // fails leaves those rows out, which is the same as a board without
        // them.
        let caps = feed.capabilities().await.unwrap_or_default();
        let wants = Wants {
            condition: caps.has(Capability::BatteryCondition),
        };
        // Both rows read the pack over I2C, so one capability answers for the
        // pair — and neither is a field of `Report`, since nothing after this
        // moves them.
        temperature_row.set_visible(wants.condition);
        report.spread_row.set_visible(wants.condition);
        // Subscribed before the window is filled, so that filling it is the
        // feed's own read rather than a second assembly of one here — two
        // spellings of what a reading consists of would drift the first time
        // it grows a part. The subscription hangs on the page rather than the
        // window: it is the widget that unmaps with the report, and every row
        // fed from here is inside it.
        show_while_mapped(&feed, &page, wants, move |reading| report.show(reading));
        // The one read here that announces a failure. From now on the feed
        // reads on its own schedule, silently, as every repeating read in this
        // app does.
        if let Err(e) = feed.read().await {
            toasts.add_toast(adw::Toast::new(&format!("Reading the battery failed: {e}")));
        }
    });

    window
}
