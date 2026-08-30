//! The Battery group: the pack's reading, the two limits — each a combo of
//! presets with a slider its Custom row reveals — and the writes both
//! front-ends make through them.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::control::battery::{
    self, CHARGE_LIMIT_CUSTOM, CHARGE_SPEED_CUSTOM, CUSTOM_CHARGE_STEP_MA, MIN_CUSTOM_CHARGE_MA,
    NO_CHARGE_LIMIT, amps, charge_flow_label, charge_limit_at, charge_limit_labels,
    charge_limit_row, charge_speed_at, charge_speed_labels, charge_speed_names, charge_speed_row,
    percent_label, with_custom_row,
};
use frameguin_wire::{BatteryFeature, BatteryState, MIN_CHARGE_LIMIT, NO_CHARGE_CURRENT_LIMIT};
use gtk4 as gtk;
use gtk4::glib;

use crate::bus::Bus;
use crate::reading::{Wants, show_while_mapped};
use crate::tray::TrayValues;
use crate::window::widgets::{
    SliderWrites, build_scale, combo_index, combo_position, combo_selection, connect_combo,
    connect_slider_writes, reveal_under, scale_percent, string_list,
};
use crate::window::{Sink, Ui};

pub(crate) type Battery = battery::Battery<Bus>;

/// What a value landing on a preset should do to a combo sitting on Custom.
/// Only a slider write keeps it: the user is dialling a number in, and a
/// number that happens to equal a preset shouldn't fold the slider away
/// under them. Everything else — a preset picked here or in the tray, a
/// reload of what the hardware actually holds — re-derives the row.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Custom {
    Keep,
    Rederive,
}

pub(crate) struct Group {
    pub(crate) widget: adw::PreferencesGroup,
    /// The battery reading: the row carries the direction as its subtitle,
    /// the label at its end the charge. The charge is the one figure here
    /// the desktop already shows for itself, and it earns its place by
    /// sitting directly above the ceiling — a pack level with its limit is
    /// the answer to why nothing is charging.
    state_row: adw::ActionRow,
    state_percent: gtk::Label,
    limit_combo: adw::ComboRow,
    limit_scale: gtk::Scale,
    speed_combo: adw::ComboRow,
    speed_scale: gtk::Scale,
    /// The battery's design capacity in mAh, None until read. Numerically it
    /// is the 1C current, which is what turns the combo's fractions into the
    /// milliamps the daemon takes.
    design_capacity: Cell<Option<u32>>,
}

impl Group {
    pub(crate) fn build() -> Self {
        let widget = adw::PreferencesGroup::builder().title("Battery").build();
        // The one row here that opens something rather than setting
        // something. Named as an action rather than wired to a handler, so
        // the row and the tray's reading reach the report the same way and
        // neither has to hold a bus connection to offer it.
        let state_row = adw::ActionRow::builder()
            .title("Status")
            .activatable(true)
            .action_name(format!("app.{}", crate::report::battery::ACTION))
            .build();
        let state_percent = gtk::Label::new(None);
        state_row.add_suffix(&state_percent);
        state_row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        widget.add(&state_row);
        let limit_labels = with_custom_row(charge_limit_labels());
        let limit_combo = adw::ComboRow::builder()
            .title("Charge limit")
            .subtitle("Stops charging before the battery is full")
            .model(&string_list(&limit_labels))
            .sensitive(false)
            .build();
        widget.add(&limit_combo);
        let limit_custom_row = adw::ActionRow::builder().title("Maximum charge").build();
        let floor = f64::from(MIN_CHARGE_LIMIT);
        let limit_adjustment = gtk::Adjustment::new(floor, floor, 100.0, 5.0, 5.0, 0.0);
        let limit_scale = build_scale(&limit_adjustment, |value| format!("{value:.0}%"));
        limit_custom_row.add_suffix(&limit_scale);
        reveal_under(&limit_combo, &limit_custom_row, CHARGE_LIMIT_CUSTOM);
        widget.add(&limit_custom_row);
        let speed_combo = adw::ComboRow::builder()
            .title("Charge speed")
            .subtitle("Maximum charging rate")
            .model(&string_list(&charge_speed_names()))
            .sensitive(false)
            .build();
        widget.add(&speed_combo);
        let speed_custom_row = adw::ActionRow::builder().title("Maximum current").build();
        // The upper bound is the battery's 1C current, filled in once it is
        // read; asking for more than the pack requests would be a limit that
        // never binds. Explicit adjustment: with_range would set
        // page_increment to 10x the step, and a mouse wheel click on a
        // GtkRange moves by the page increment.
        let floor = f64::from(MIN_CUSTOM_CHARGE_MA);
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
        widget.add(&speed_custom_row);
        Self {
            widget,
            state_row,
            state_percent,
            limit_combo,
            limit_scale,
            speed_combo,
            speed_scale,
            design_capacity: Cell::default(),
        }
    }

    /// Shows the group where the board has a pack, and within it only the
    /// limits the charger takes.
    pub(crate) fn gate(&self, control: Option<&Rc<Battery>>) {
        self.widget.set_visible(control.is_some());
        let has = |feature| control.is_some_and(|battery| battery.has(feature));
        self.limit_combo
            .set_visible(has(BatteryFeature::ChargeLimit));
        self.speed_combo
            .set_visible(has(BatteryFeature::ChargeCurrentLimit));
    }

    /// Shows a battery reading. No `sync` guard and no `Custom` question,
    /// unlike every other show here: nothing on this row writes back, so
    /// there is no handler to hold off.
    fn show_state(&self, state: BatteryState) {
        self.state_percent.set_label(&percent_label(state.percent));
        self.state_row.set_subtitle(&charge_flow_label(state));
    }

    /// Moves the charge-limit widgets onto a ceiling without writing it back,
    /// the counterpart of [`Group::show_charge_speed`] — change one and read
    /// the other.
    fn show_charge_limit(&self, ui: &Ui, percent: u8, custom: Custom) {
        let preset = charge_limit_row(percent);
        let index = custom_or(&self.limit_combo, CHARGE_LIMIT_CUSTOM, preset, custom);
        ui.sync(|| {
            self.limit_combo.set_selected(combo_index(index));
            self.limit_scale.set_value(f64::from(percent));
        });
    }

    /// Moves the charge-speed widgets onto a limit without writing it back.
    /// Shared by the reload and the write, so the combo and the slider can't
    /// disagree about which one is in effect.
    fn show_charge_speed(&self, ui: &Ui, milliamps: u32, custom: Custom) {
        let Some(capacity) = self.design_capacity.get() else {
            ui.sync(|| self.speed_combo.set_selected(combo_selection(None)));
            return;
        };
        let preset = charge_speed_row(capacity, milliamps);
        let index = custom_or(&self.speed_combo, CHARGE_SPEED_CUSTOM, preset, custom);
        ui.sync(|| {
            self.speed_combo.set_selected(combo_index(index));
            // Full speed is the absence of a limit, not a position on a
            // slider that can only express one.
            if milliamps != NO_CHARGE_CURRENT_LIMIT {
                self.speed_scale.set_value(f64::from(milliamps));
            }
        });
    }

    pub(crate) fn connect(&self, ui: &Rc<Ui>, control: &Rc<Battery>) {
        // The one row nothing writes to: it follows the pack, which moves
        // whether or not anyone touches the app. Fed rather than polled — the
        // report shows the same walk of the same block, and a row that read
        // it for itself would have the two windows asking the EC separately
        // for one answer (see `crate::reading`). It wants none of the extras,
        // so it asks for none. The feed deliberately tells the tray nothing:
        // every push rebuilds and re-signals the whole menu, and the tray
        // asks for its own reading when its menu is about to show.
        let row_ui = ui.clone();
        show_while_mapped(
            &ui.feed,
            &self.state_row,
            Wants::default(),
            move |reading| {
                row_ui.battery.show_state(reading.info.state);
            },
        );

        let limit_control = control.clone();
        connect_combo(
            ui,
            &self.limit_combo,
            charge_limit_at,
            move |ui, percent| {
                let ui = ui.clone();
                let control = limit_control.clone();
                glib::spawn_future_local(async move {
                    apply_charge_limit(Sink::Window(&ui), &control, percent, Custom::Rederive)
                        .await;
                });
            },
        );

        // Slider: a raw ceiling, reachable only while the combo is on Custom.
        let scale_ui = ui.clone();
        let scale_control = control.clone();
        connect_slider_writes(
            ui,
            &self.limit_scale,
            scale_percent,
            move |percent| {
                let ui = scale_ui.clone();
                let control = scale_control.clone();
                glib::spawn_future_local(async move {
                    apply_charge_limit(Sink::Window(&ui), &control, percent, Custom::Keep).await;
                });
            },
            SliderWrites::OnRelease,
        );

        let at_ui = ui.clone();
        let speed_control = control.clone();
        connect_combo(
            ui,
            &self.speed_combo,
            move |index| charge_speed_at(at_ui.battery.design_capacity.get()?, index),
            move |ui, milliamps| {
                let ui = ui.clone();
                let control = speed_control.clone();
                glib::spawn_future_local(async move {
                    apply_charge_speed(Sink::Window(&ui), &control, milliamps, Custom::Rederive)
                        .await;
                });
            },
        );

        // Slider: a raw current, reachable only while the combo is on Custom.
        let scale_ui = ui.clone();
        let scale_control = control.clone();
        connect_slider_writes(
            ui,
            &self.speed_scale,
            scale_milliamps,
            move |milliamps| {
                let ui = scale_ui.clone();
                let control = scale_control.clone();
                glib::spawn_future_local(async move {
                    apply_charge_speed(Sink::Window(&ui), &control, milliamps, Custom::Keep).await;
                });
            },
            SliderWrites::OnRelease,
        );
    }

    /// A pack's design capacity can't change under a running app, so it is
    /// read once and the labels built from it stay put. Nothing to fall back
    /// to where the reading failed, and nothing that should be: every rate
    /// this control offers or sends is a fraction of this figure, so a
    /// second guess at it would be a second answer to "how fast is full
    /// speed". The combo stays insensitive until it arrives, and the next
    /// reload asks again.
    fn learn_capacity(&self, ui: &Ui, capacity: u32) {
        self.design_capacity.set(Some(capacity));
        let labels = with_custom_row(charge_speed_labels(capacity));
        // 1C is as fast as the pack ever asks, so a slider beyond it would
        // only offer limits that never bind. Floored to the step the value
        // rounds to, so the far end of the track is a position that sends
        // what it shows rather than a sliver that rounds back down.
        let step = f64::from(CUSTOM_CHARGE_STEP_MA);
        let top = (f64::from(capacity) / step).floor() * step;
        ui.sync(|| {
            self.speed_combo.set_model(Some(&string_list(&labels)));
            self.speed_scale
                .adjustment()
                .set_upper(top.max(f64::from(MIN_CUSTOM_CHARGE_MA)));
        });
    }

    /// The group's half of a reload: the reading at the top, then the
    /// ceiling and the speed with their combos and sliders. What the tray
    /// should be told goes into `values`, for the one push the window makes
    /// at the end.
    pub(crate) async fn load(&self, ui: &Ui, control: &Battery, values: &mut TrayValues) {
        // Read here as well as fed: the feed's first tick is a couple of
        // seconds after the window appears, and an empty row until then reads
        // as a control that failed rather than one still filling. Through the
        // feed rather than around it, so the row and whatever else is showing
        // the pack are painted from one walk of the block — and so the tray is
        // pushed the same reading the row got rather than a second one taken a
        // moment later.
        match ui.feed.read().await {
            Ok(info) => {
                self.show_state(info.state);
                values.battery = Some(info.state);
                if self.design_capacity.get().is_none() {
                    self.learn_capacity(ui, info.design_capacity);
                }
            }
            Err(e) => ui.toast_error("Reading the battery", e),
        }
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
            match control.charge_current_limit().await {
                Ok(milliamps) => {
                    self.show_charge_speed(ui, milliamps, Custom::Rederive);
                    // Without the battery's capacity the fractions have no
                    // milliamps behind them, so the row stays read-only.
                    let known = self.design_capacity.get().is_some();
                    ui.sync(|| {
                        self.speed_combo.set_sensitive(known);
                        self.speed_scale.set_sensitive(known);
                    });
                    values.charge_current_limit = Some(milliamps);
                }
                Err(e) => ui.toast_error("Reading the charge speed", e),
            }
        }
        // Read once per run, above.
        values.design_capacity = self.design_capacity.get();
    }
}

/// Which row a value belongs on. A value matching no preset can only be shown
/// by the slider, so it lands on Custom whatever the caller asked for.
/// Otherwise `Custom::Keep` leaves a combo that is already on Custom there, so
/// dialling in a number that happens to equal a preset doesn't fold the slider
/// away mid-drag.
fn custom_or(
    combo: &adw::ComboRow,
    custom_index: usize,
    preset: Option<usize>,
    custom: Custom,
) -> usize {
    let Some(preset) = preset else {
        return custom_index;
    };
    if custom == Custom::Keep && combo_position(combo.selected()) == Some(custom_index) {
        custom_index
    } else {
        preset
    }
}

/// GTK carries the slider's value as f64; the clamp is what holds the result
/// inside what the daemon accepts, its floor coming from the adjustment.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped into range before the cast"
)]
fn scale_milliamps(value: f64) -> u32 {
    let step = f64::from(CUSTOM_CHARGE_STEP_MA);
    let snapped = (value / step).round() * step;
    snapped.clamp(f64::from(MIN_CUSTOM_CHARGE_MA), f64::from(u32::MAX)) as u32
}

fn show_limit(sink: Sink<'_>, percent: u8, custom: Custom) {
    sink.push_tray(TrayValues::charge_limit(percent));
    if let Sink::Window(ui) = sink {
        ui.battery.show_charge_limit(ui, percent, custom);
    }
}

fn show_speed(sink: Sink<'_>, milliamps: u32, custom: Custom) {
    sink.push_tray(TrayValues::charge_speed(milliamps));
    if let Sink::Window(ui) = sink {
        ui.battery.show_charge_speed(ui, milliamps, custom);
    }
}

/// The one write for the charge limit. The window's row and the tray preset
/// both come here, so neither can drift from the other on what it reports or
/// what it tells the tray.
///
/// [`apply_charge_speed`] is the same shape for the other control here.
/// They are deliberately two functions rather than one generic: the values
/// differ (`u8` against `u32`), the speed resolves its presets against the
/// battery's capacity where the ceiling's are constants, and each carries its
/// own sentinel for no limit at all. A change to one is usually a change to
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
    // Silent when the daemon found the ceiling already there: announcing a
    // write that didn't happen is a confirmation of nothing.
    if written {
        if percent == NO_CHARGE_LIMIT {
            sink.toast("Charge limit switched off");
        } else {
            sink.toast(&format!("Charge limit set to {percent}%"));
        }
    }
    show_limit(sink, percent, custom);
}

/// The one write for the charge speed, in mA or `NO_CHARGE_CURRENT_LIMIT`.
/// Callers resolve a speed to milliamps against the battery capacity they
/// hold — the window's, or the tray's own copy.
pub(crate) async fn apply_charge_speed(
    sink: Sink<'_>,
    control: &Battery,
    milliamps: u32,
    custom: Custom,
) {
    let written = match control.set_charge_current_limit(milliamps).await {
        Ok(written) => written,
        Err(e) => {
            sink.toast_error("Setting the charge speed", e);
            return;
        }
    };
    if written {
        if milliamps == NO_CHARGE_CURRENT_LIMIT {
            sink.toast("Charge speed uncapped");
        } else {
            sink.toast(&format!("Charge speed capped at {}", amps(milliamps)));
        }
    }
    show_speed(sink, milliamps, custom);
}

#[cfg(test)]
mod tests {
    use frameguin_model::control::battery::MIN_CUSTOM_CHARGE_MA;

    use super::scale_milliamps;

    /// A `GtkScale` is continuous while dragged, so without snapping a drag
    /// lands on values like 984 mA that the row then displays as "1.0 A".
    #[test]
    fn the_slider_snaps_to_whole_steps() {
        assert_eq!(scale_milliamps(984.0), 1000);
        assert_eq!(scale_milliamps(1049.0), 1000);
        assert_eq!(scale_milliamps(1050.0), 1100);
    }

    #[test]
    fn the_slider_never_asks_for_a_current_that_stops_charging() {
        let floor = scale_milliamps(f64::from(MIN_CUSTOM_CHARGE_MA));
        assert!(floor > 0);
        assert_eq!(scale_milliamps(0.0), floor);
        assert_eq!(scale_milliamps(-50.0), floor);
    }
}
