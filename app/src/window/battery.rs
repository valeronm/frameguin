//! The Power tab's groups: the charging state — what is coming in and the
//! pack's reading — and the charging limits, each a combo of presets with a
//! slider its Custom row reveals, with the writes both front-ends make
//! through them.
//!
//! Titled for the subject rather than for the battery whose control they
//! otherwise hold: the charger row at the state's head reads the USB-C
//! ports, which have no group of their own, and a mainboard running
//! standalone has that row and no pack at all.

use std::cell::Cell;
use std::num::NonZeroU32;
use std::rc::Rc;

use adw::prelude::*;
use frameguin_contract::{BatteryFeature, BatteryState, ChargeCurrentLimit, PortState};
use frameguin_model::control::battery::{
    self, CUSTOM_CHARGE_STEP_MA, ChargeSpeeds, MIN_CHARGE_LIMIT, MIN_CUSTOM_CHARGE_MA,
    custom_charge_ma,
};
use frameguin_model::control::ports;
use frameguin_model::port::Placement;
use frameguin_model::reading::Request;
use frameguin_modelview::battery::{
    CHARGE_LIMIT_CUSTOM, CHARGE_SPEED_CUSTOM, NO_CHARGE_LIMIT, charge_limit_at,
    charge_limit_labels, charge_limit_row, charge_speed_at, charge_speed_labels,
    charge_speed_names, charge_speed_row, fastest_custom,
    reading::{amps, charge_flow_label, percent_label},
    with_custom_row,
};
use frameguin_modelview::ports::{supply_label, supply_port};
use frameguin_modelview::rows::Custom;
use frameguin_wire::Bus;
use gtk4 as gtk;

use crate::reading::show_while_mapped;
use crate::report::status::{self, Target};
use crate::tray::TrayValues;
use crate::window::widgets::{
    SliderWrites, build_scale, combo_selection, connect_combo, connect_slider_writes, reveal_under,
    scale_percent, select_row, string_list,
};
use crate::window::{Sink, Ui};

pub(crate) type Battery = battery::Battery<Bus>;
/// The other device the state group draws a row for.
pub(crate) type Ports = ports::Ports<Bus>;

/// Named as an action rather than wired to a handler, so offering the report
/// needs no bus connection.
fn report_row(
    title: &str,
    action: &str,
    target: &gtk::glib::Variant,
) -> (adw::ActionRow, gtk::Label) {
    let row = adw::ActionRow::builder()
        .title(title)
        .activatable(true)
        .action_name(format!("app.{action}"))
        .build();
    row.set_action_target_value(Some(target));
    let value = gtk::Label::new(None);
    row.add_suffix(&value);
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    (row, value)
}

pub(crate) struct Group {
    pub(crate) state: adw::PreferencesGroup,
    pub(crate) limits: adw::PreferencesGroup,
    /// The battery reading: the row carries the direction as its subtitle,
    /// the label at its end the charge. The charge is the one figure here
    /// the desktop already shows for itself, and it earns its place by
    /// sitting directly above the ceiling — a pack level with its limit is
    /// the answer to why nothing is charging.
    state_row: adw::ActionRow,
    state_percent: gtk::Label,
    /// What is powering the machine, and which port it comes in on. Beside
    /// the pack's own row because the two answer one question between them —
    /// a pack discharging with a charger attached is a supply too weak to
    /// cover the machine, and neither row says that alone. Hidden on a board
    /// with no ports device, the row having nothing to read.
    charger_row: adw::ActionRow,
    charger: gtk::Label,
    limit_combo: adw::ComboRow,
    limit_scale: gtk::Scale,
    speed_combo: adw::ComboRow,
    speed_scale: gtk::Scale,
    /// What the speed combo's rows were built from, so a row sends the limit
    /// it names; None until the pack has been read.
    speeds: Cell<Option<ChargeSpeeds>>,
    /// Where the ports are, set when gated.
    placement: Cell<Placement>,
}

impl Group {
    pub(crate) fn build() -> Self {
        let state = adw::PreferencesGroup::builder()
            .title("Charging State")
            .build();
        let limits = adw::PreferencesGroup::builder()
            .title("Charging Limits")
            .build();
        // The two rows here that open something rather than setting
        // something.
        let (state_row, state_percent) = report_row(
            "Battery",
            status::ACTION,
            &status::target(Some(Target::Battery)),
        );
        // Above the pack's own row: what is coming in is what decides what
        // the pack is doing, so it reads in the order the power arrives.
        let (charger_row, charger) = report_row(
            "Charger",
            status::ACTION,
            &status::target(Some(Target::Charger)),
        );
        state.add(&charger_row);
        state.add(&state_row);
        let limit_labels = with_custom_row(charge_limit_labels());
        let limit_combo = adw::ComboRow::builder()
            .title("Charge limit")
            .subtitle("Stops charging before the battery is full")
            .model(&string_list(&limit_labels))
            .sensitive(false)
            .build();
        limits.add(&limit_combo);
        let limit_custom_row = adw::ActionRow::builder().title("Maximum charge").build();
        let floor = f64::from(MIN_CHARGE_LIMIT);
        let limit_adjustment = gtk::Adjustment::new(floor, floor, 100.0, 5.0, 5.0, 0.0);
        let limit_scale = build_scale(&limit_adjustment, |value| format!("{value:.0}%"));
        limit_custom_row.add_suffix(&limit_scale);
        reveal_under(&limit_combo, &limit_custom_row, CHARGE_LIMIT_CUSTOM);
        limits.add(&limit_custom_row);
        let speed_combo = adw::ComboRow::builder()
            .title("Charge speed")
            .subtitle("Maximum charging rate")
            .model(&string_list(&charge_speed_names()))
            .sensitive(false)
            .build();
        limits.add(&speed_combo);
        let speed_custom_row = adw::ActionRow::builder().title("Maximum current").build();
        // The upper bound waits on the pack's speeds. Explicit adjustment:
        // with_range would set page_increment to 10x the step, and a mouse
        // wheel click on a GtkRange moves by the page increment.
        let floor = f64::from(MIN_CUSTOM_CHARGE_MA.get());
        let step = f64::from(CUSTOM_CHARGE_STEP_MA);
        let speed_adjustment = gtk::Adjustment::new(floor, floor, floor, step, step, 0.0);
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "slider is bounded by its adjustment, which holds milliamps"
        )]
        let speed_scale = build_scale(&speed_adjustment, |value| amps(value as u32));
        speed_custom_row.add_suffix(&speed_scale);
        reveal_under(&speed_combo, &speed_custom_row, CHARGE_SPEED_CUSTOM);
        limits.add(&speed_custom_row);
        Self {
            state,
            limits,
            state_row,
            state_percent,
            charger_row,
            charger,
            limit_combo,
            limit_scale,
            speed_combo,
            speed_scale,
            speeds: Cell::default(),
            placement: Cell::default(),
        }
    }

    /// Shows the state where the board has either of its devices, the limits
    /// where the pack takes either of them, and within each only the rows
    /// its device answers for.
    ///
    /// Two controls where every sibling group takes one: the state group
    /// draws two devices' rows. Narrower than the whole set, which would let
    /// these groups gate on a device that is not their own.
    pub(crate) fn gate(&self, control: Option<&Rc<Battery>>, ports: Option<&Rc<Ports>>) {
        self.placement
            .set(ports.map(|ports| ports.placement()).unwrap_or_default());
        self.state.set_visible(control.is_some() || ports.is_some());
        self.charger_row.set_visible(ports.is_some());
        let has = |feature| control.is_some_and(|battery| battery.has(feature));
        let (limit, speed) = (
            has(BatteryFeature::ChargeLimit),
            has(BatteryFeature::ChargeCurrentLimit),
        );
        self.limits.set_visible(limit || speed);
        self.limit_combo.set_visible(limit);
        self.speed_combo.set_visible(speed);
    }

    pub(crate) fn has_fed_rows(&self) -> bool {
        self.state.is_visible()
    }

    /// Shows a battery reading. No `sync` guard and no `Custom` question,
    /// unlike every other show here: nothing on this row writes back, so
    /// there is no handler to hold off.
    fn show_state(&self, state: BatteryState) {
        self.state_percent.set_label(&percent_label(state.percent));
        self.state_row.set_subtitle(&charge_flow_label(state));
    }

    fn show_charger(&self, ports: &[PortState]) {
        self.charger.set_label(&supply_label(ports));
        self.charger_row
            .set_subtitle(&supply_port(ports, self.placement.get()).unwrap_or_default());
    }

    /// Moves the charge-limit widgets onto a ceiling without writing it back,
    /// the counterpart of [`Group::show_charge_speed`] — change one and read
    /// the other.
    fn show_charge_limit(&self, ui: &Ui, percent: u8, custom: Custom) {
        ui.sync(|| {
            select_row(&self.limit_combo, |selected| {
                charge_limit_row(percent, selected, custom)
            });
            self.limit_scale.set_value(f64::from(percent));
        });
    }

    /// Moves the charge-speed widgets onto a limit without writing it back.
    /// Shared by the reload and the write, so the combo and the slider can't
    /// disagree about which one is in effect.
    fn show_charge_speed(&self, ui: &Ui, limit: ChargeCurrentLimit, custom: Custom) {
        let Some(speeds) = self.speeds.get() else {
            ui.sync(|| self.speed_combo.set_selected(combo_selection(None)));
            return;
        };
        ui.sync(|| {
            select_row(&self.speed_combo, |selected| {
                charge_speed_row(speeds, limit, selected, custom)
            });
            // Full speed is the absence of a limit, not a position on a
            // slider that can only express one.
            if let Some(milliamps) = limit.milliamps() {
                self.speed_scale.set_value(f64::from(milliamps.get()));
            }
        });
    }

    /// Subscribes the rows nothing writes to: they follow devices that move
    /// whether or not anyone touches the app. Fed rather than polled, for the
    /// reason [`crate::reading`] gives.
    ///
    /// The feed deliberately tells the tray nothing: every push rebuilds and
    /// re-signals the whole menu, and the tray asks for its own reading when
    /// its menu is about to show.
    pub(crate) fn watch(&self, ui: &Rc<Ui>) {
        let row_ui = ui.clone();
        let request = Request {
            battery: true,
            ports: true,
            ..Request::default()
        };
        show_while_mapped(&ui.feed, &self.state_row, request, move |reading| {
            let group = &row_ui.battery;
            if let Some(info) = &reading.info {
                group.show_state(info.state);
            }
            // The load has only what an earlier reading left, so the first
            // reading after it names the rates where the load found none.
            if let Some(speeds) = reading.charge_speeds {
                group.learn_speeds(&row_ui, speeds);
            }
            if let Some(ports) = &reading.ports {
                group.show_charger(ports);
            }
        });
    }

    pub(crate) fn connect(&self, ui: &Rc<Ui>, control: &Rc<Battery>) {
        connect_combo(
            ui,
            control,
            &self.limit_combo,
            charge_limit_at,
            |ui, control, percent| async move {
                apply_charge_limit(Sink::Window(&ui), &control, percent, Custom::Rederive).await;
            },
        );

        // Slider: a raw ceiling, reachable only while the combo is on Custom.
        connect_slider_writes(
            ui,
            control,
            &self.limit_scale,
            scale_percent,
            |ui, control, percent| async move {
                apply_charge_limit(Sink::Window(&ui), &control, percent, Custom::Keep).await;
            },
            SliderWrites::OnRelease,
        );

        let at_ui = ui.clone();
        connect_combo(
            ui,
            control,
            &self.speed_combo,
            move |index| charge_speed_at(at_ui.battery.speeds.get()?, index),
            |ui, control, limit| async move {
                apply_charge_speed(Sink::Window(&ui), &control, limit, Custom::Rederive).await;
            },
        );

        // Slider: a raw current, reachable only while the combo is on Custom.
        connect_slider_writes(
            ui,
            control,
            &self.speed_scale,
            scale_milliamps,
            |ui, control, milliamps| async move {
                let limit = ChargeCurrentLimit::Limit(milliamps);
                apply_charge_speed(Sink::Window(&ui), &control, limit, Custom::Keep).await;
            },
            SliderWrites::OnRelease,
        );
    }

    fn learn_speeds(&self, ui: &Ui, speeds: ChargeSpeeds) {
        if self.speeds.replace(Some(speeds)) == Some(speeds) {
            return;
        }
        let labels = with_custom_row(charge_speed_labels(speeds));
        ui.sync(|| {
            self.speed_combo.set_model(Some(&string_list(&labels)));
            self.speed_scale
                .adjustment()
                .set_upper(f64::from(fastest_custom(speeds).get()));
        });
    }

    /// The limits' reload: the ceiling and the speed with their
    /// combos and sliders, which only a pack has. What the tray should be
    /// told goes into `values`, for the one push the window makes at the end.
    pub(crate) async fn load(&self, ui: &Ui, control: &Battery, values: &mut TrayValues) {
        if control.has(BatteryFeature::ChargeLimit) {
            match control.charge_limit().await {
                Ok(limit) => {
                    self.show_charge_limit(ui, limit, Custom::Rederive);
                    ui.sync(|| {
                        self.limit_combo.set_sensitive(true);
                        self.limit_scale.set_sensitive(true);
                    });
                    values.charge_limit = Some(limit);
                }
                Err(e) => ui.toast_error("Reading the charge limit", e),
            }
        }
        if control.has(BatteryFeature::ChargeCurrentLimit) {
            if let Some(speeds) = control.charge_speeds() {
                self.learn_speeds(ui, speeds);
            }
            match control.charge_current_limit().await {
                Ok(limit) => {
                    self.show_charge_speed(ui, limit, Custom::Rederive);
                    // Without the battery's capacity the fractions have no
                    // milliamps behind them, so the row stays read-only.
                    let known = self.speeds.get().is_some();
                    ui.sync(|| {
                        self.speed_combo.set_sensitive(known);
                        self.speed_scale.set_sensitive(known);
                    });
                    values.charge_current_limit = Some(limit);
                }
                Err(e) => ui.toast_error("Reading the charge speed", e),
            }
        }
        values.charge_speeds = self.speeds.get();
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a float cast saturates, and the floor is applied after it"
)]
fn scale_milliamps(value: f64) -> NonZeroU32 {
    let step = f64::from(CUSTOM_CHARGE_STEP_MA);
    let snapped = (value / step).round() * step;
    custom_charge_ma(snapped as u32)
}

fn show_limit(sink: Sink<'_>, percent: u8, custom: Custom) {
    sink.push_tray(TrayValues::charge_limit(percent));
    if let Sink::Window(ui) = sink {
        ui.battery.show_charge_limit(ui, percent, custom);
    }
}

fn show_speed(sink: Sink<'_>, limit: ChargeCurrentLimit, custom: Custom) {
    sink.push_tray(TrayValues::charge_speed(limit));
    if let Sink::Window(ui) = sink {
        ui.battery.show_charge_speed(ui, limit, custom);
    }
}

/// The one write for the charge limit. The window's row and the tray preset
/// both come here, so neither can drift from the other on what it reports or
/// what it tells the tray.
///
/// [`apply_charge_speed`] is the same shape for the other control here.
/// They are deliberately two functions rather than one generic: the values
/// differ (a percentage against a `ChargeCurrentLimit`), the speed resolves its
/// presets against the battery's capacity where the ceiling's are constants,
/// and each names no limit its own way. A change to one is usually a change to
/// both — read the sibling before editing either.
pub(crate) async fn apply_charge_limit(
    sink: Sink<'_>,
    control: &Battery,
    percent: u8,
    custom: Custom,
) {
    let written = match control.set_charge_limit(percent).await {
        Ok(written) => written,
        Err(e) => {
            sink.toast_error("Setting the charge limit", e);
            return;
        }
    };
    // Announcing a write that didn't happen would confirm nothing.
    if written {
        if percent == NO_CHARGE_LIMIT {
            sink.toast("Charge limit switched off");
        } else {
            sink.toast(&format!("Charge limit set to {percent}%"));
        }
    }
    show_limit(sink, percent, custom);
}

/// The one write for the charge speed. Callers resolve a speed to a limit
/// against the `ChargeSpeeds` they hold — the window's, or the tray's own
/// copy.
pub(crate) async fn apply_charge_speed(
    sink: Sink<'_>,
    control: &Battery,
    limit: ChargeCurrentLimit,
    custom: Custom,
) {
    let written = match control.set_charge_current_limit(limit).await {
        Ok(written) => written,
        Err(e) => {
            sink.toast_error("Setting the charge speed", e);
            return;
        }
    };
    if written {
        match limit {
            ChargeCurrentLimit::Limit(milliamps) => {
                sink.toast(&format!("Charge speed capped at {}", amps(milliamps.get())));
            }
            ChargeCurrentLimit::NoLimit => sink.toast("Charge speed uncapped"),
        }
    }
    show_speed(sink, limit, custom);
}

#[cfg(test)]
mod tests {
    use frameguin_model::control::battery::MIN_CUSTOM_CHARGE_MA;

    use super::scale_milliamps;

    /// A `GtkScale` is continuous while dragged, so without snapping a drag
    /// lands on values like 984 mA that the row then displays as "1.0 A".
    #[test]
    fn the_slider_snaps_to_whole_steps() {
        assert_eq!(scale_milliamps(984.0).get(), 1000);
        assert_eq!(scale_milliamps(1049.0).get(), 1000);
        assert_eq!(scale_milliamps(1050.0).get(), 1100);
    }

    #[test]
    fn the_slider_never_asks_for_a_current_that_stops_charging() {
        let floor = scale_milliamps(f64::from(MIN_CUSTOM_CHARGE_MA.get()));
        assert_eq!(floor, MIN_CUSTOM_CHARGE_MA);
        assert_eq!(scale_milliamps(0.0), floor);
        assert_eq!(scale_milliamps(-50.0), floor);
    }
}
