//! The Battery group: the pack's reading, the two limits — each a combo of
//! presets with a slider its Custom row reveals — and the writes both
//! front-ends make through them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use frameguin_model::control::battery::{
    self, CHARGE_LIMIT_CUSTOM, CHARGE_SPEED_CUSTOM, CHARGE_SPEED_LABELS, CUSTOM_CHARGE_STEP_MA,
    MIN_CUSTOM_CHARGE_MA, NO_CHARGE_LIMIT, amps, charge_flow_label, charge_limit_labels,
    charge_limit_percent, charge_limit_position, charge_speed_labels, charge_speed_milliamps,
    charge_speed_position, percent_label, with_custom_row,
};
use frameguin_wire::{BatteryFeature, BatteryState, MIN_CHARGE_LIMIT, NO_CHARGE_CURRENT_LIMIT};
use gtk4 as gtk;
use gtk4::glib;

use crate::bus::Bus;
use crate::reading::{Wants, show_while_mapped};
use crate::tray::TrayValues;
use crate::window::{Sink, Ui, build_scale, combo_index, debounce, scale_percent, string_list};

pub(crate) type Battery = battery::Battery<Bus>;

/// Keys and the wheel on a slider that otherwise writes only when a drag
/// ends. Longer than the live sliders wait, for the same reason that one
/// writes on release: nothing shows the values passed through, and each of
/// them would be another authorized EC write.
const SETTLE_DEBOUNCE: Duration = Duration::from_millis(700);

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
    /// The slider's row; shown only while the ceiling is Custom.
    limit_custom_row: adw::ActionRow,
    speed_combo: adw::ComboRow,
    speed_scale: gtk::Scale,
    /// The slider's row; shown only while the speed is Custom.
    speed_custom_row: adw::ActionRow,
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
            .action_name(format!("app.{}", crate::battery::ACTION))
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
        limit_custom_row.set_visible(false);
        widget.add(&limit_custom_row);
        let speed_combo = adw::ComboRow::builder()
            .title("Charge speed")
            .subtitle("Maximum charging rate")
            .model(&gtk::StringList::new(&CHARGE_SPEED_LABELS))
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
        speed_custom_row.set_visible(false);
        widget.add(&speed_custom_row);
        Self {
            widget,
            state_row,
            state_percent,
            limit_combo,
            limit_scale,
            limit_custom_row,
            speed_combo,
            speed_scale,
            speed_custom_row,
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
        let preset = charge_limit_position(percent);
        let index = custom_or(&self.limit_combo, CHARGE_LIMIT_CUSTOM, preset, custom);
        ui.sync(|| {
            self.limit_combo.set_selected(combo_index(index));
            self.limit_custom_row
                .set_visible(index == CHARGE_LIMIT_CUSTOM);
            self.limit_scale.set_value(f64::from(percent));
        });
    }

    /// Moves the charge-speed widgets onto a limit without writing it back.
    /// Shared by the reload and the write, so the combo, the slider and the
    /// slider's visibility can't disagree about which one is in effect.
    fn show_charge_speed(&self, ui: &Ui, milliamps: u32, custom: Custom) {
        let Some(capacity) = self.design_capacity.get() else {
            ui.sync(|| self.speed_combo.set_selected(gtk::INVALID_LIST_POSITION));
            return;
        };
        let preset = charge_speed_position(capacity, milliamps);
        let index = custom_or(&self.speed_combo, CHARGE_SPEED_CUSTOM, preset, custom);
        ui.sync(|| {
            self.speed_combo.set_selected(combo_index(index));
            self.speed_custom_row
                .set_visible(index == CHARGE_SPEED_CUSTOM);
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

        let limit_ui = ui.clone();
        let limit_control = control.clone();
        self.limit_combo.connect_selected_notify(move |row| {
            if limit_ui.syncing.get() {
                return;
            }
            let Ok(index) = usize::try_from(row.selected()) else {
                return;
            };
            if index > CHARGE_LIMIT_CUSTOM {
                return;
            }
            // Choosing Custom writes nothing: the row only reveals the slider
            // that can change the ceiling.
            if index == CHARGE_LIMIT_CUSTOM {
                limit_ui.battery.limit_custom_row.set_visible(true);
                return;
            }
            let percent = charge_limit_percent(index);
            let ui = limit_ui.clone();
            let control = limit_control.clone();
            glib::spawn_future_local(async move {
                apply_charge_limit(Sink::Window(&ui), &control, percent, Custom::Rederive).await;
            });
        });

        // Slider: a raw ceiling, reachable only while the combo is on Custom.
        let scale_ui = ui.clone();
        let scale_control = control.clone();
        connect_slider_writes(ui, &self.limit_scale, scale_percent, move |percent| {
            let ui = scale_ui.clone();
            let control = scale_control.clone();
            glib::spawn_future_local(async move {
                apply_charge_limit(Sink::Window(&ui), &control, percent, Custom::Keep).await;
            });
        });

        let speed_ui = ui.clone();
        let speed_control = control.clone();
        self.speed_combo.connect_selected_notify(move |row| {
            if speed_ui.syncing.get() {
                return;
            }
            // An unselected row reports INVALID_LIST_POSITION, which is not
            // an index — reading it as one would land on "full speed" and
            // lift a limit nobody asked to lift.
            let Ok(index) = usize::try_from(row.selected()) else {
                return;
            };
            if index > CHARGE_SPEED_CUSTOM {
                return;
            }
            // Choosing Custom writes nothing: the limit in effect is already
            // whatever it is, and the row only reveals the slider that can
            // change it. Unlike the power LED's custom level, there is no EC
            // state to enter here — a dialled-in current is just a current.
            if index == CHARGE_SPEED_CUSTOM {
                speed_ui.battery.speed_custom_row.set_visible(true);
                return;
            }
            // The row stays insensitive until the capacity is read, so a
            // preset can't be picked without one; the early return only
            // keeps the conversion total.
            let Some(design_capacity) = speed_ui.battery.design_capacity.get() else {
                return;
            };
            let milliamps = charge_speed_milliamps(design_capacity, index);
            let ui = speed_ui.clone();
            let control = speed_control.clone();
            glib::spawn_future_local(async move {
                apply_charge_speed(Sink::Window(&ui), &control, milliamps, Custom::Rederive).await;
            });
        });

        // Slider: a raw current, reachable only while the combo is on Custom.
        let scale_ui = ui.clone();
        let scale_control = control.clone();
        connect_slider_writes(ui, &self.speed_scale, scale_milliamps, move |milliamps| {
            let ui = scale_ui.clone();
            let control = scale_control.clone();
            glib::spawn_future_local(async move {
                apply_charge_speed(Sink::Window(&ui), &control, milliamps, Custom::Keep).await;
            });
        });
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
    pub(crate) async fn load(&self, ui: &Rc<Ui>, control: &Rc<Battery>, values: &mut TrayValues) {
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
                Err(e) => ui.toast_error("Reading charge limit", e),
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
                Err(e) => ui.toast_error("Reading charge speed", e),
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
    if custom == Custom::Keep && combo.selected() == combo_index(custom_index) {
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

/// Wires a slider whose value reaches the hardware when a drag ends rather
/// than as it moves: these controls show nothing while they change, and every
/// value passed through would be one more authorized EC write. Keyboard and
/// wheel changes raise no release, so they settle on a debounce instead.
///
/// `read` turns the slider's position into the value that gets written, and
/// is also what decides whether a drag moved at all — comparing positions
/// would count a nudge that rounds back to where it started.
fn connect_slider_writes<T: Copy + PartialEq + 'static>(
    ui: &Rc<Ui>,
    scale: &gtk::Scale,
    read: impl Fn(f64) -> T + 'static,
    write: impl Fn(T) + 'static,
) {
    let read = Rc::new(read);
    let write = Rc::new(write);
    let dragging: Rc<Cell<Option<T>>> = Rc::new(Cell::new(None));

    let slot = Rc::new(RefCell::new(None));
    let keys_ui = ui.clone();
    let keys_dragging = dragging.clone();
    let (keys_read, keys_write) = (read.clone(), write.clone());
    scale.connect_value_changed(move |scale| {
        if keys_ui.syncing.get() || keys_dragging.get().is_some() {
            return;
        }
        let value = keys_read(scale.value());
        let write = keys_write.clone();
        debounce(&slot, SETTLE_DEBOUNCE, move || write(value));
    });

    // Raw events, not a gesture: the scale's own drag gesture claims the
    // pointer sequence, which cancels any competing gesture instead of
    // releasing it — so a GestureClick here would see the press and never the
    // release, and the drag would never end.
    let drag = gtk::EventControllerLegacy::new();
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    let drag_scale = scale.clone();
    drag.connect_event(move |_, event| {
        match event.event_type() {
            gtk::gdk::EventType::ButtonPress | gtk::gdk::EventType::TouchBegin => {
                dragging.set(Some(read(drag_scale.value())));
            }
            gtk::gdk::EventType::ButtonRelease
            | gtk::gdk::EventType::TouchEnd
            | gtk::gdk::EventType::TouchCancel => {
                let value = read(drag_scale.value());
                // A press that lands where the handle already sat changes
                // nothing, and writing it would announce a value nobody moved.
                if dragging.replace(None) != Some(value) {
                    write(value);
                }
            }
            _ => {}
        }
        glib::Propagation::Proceed
    });
    scale.add_controller(drag);
}

fn show_limit(sink: Sink<'_>, percent: u8, custom: Custom) {
    sink.push_tray(TrayValues::charge_limit(percent));
    if let Sink::Window(ui) = sink {
        ui.battery.show_charge_limit(ui, percent, custom);
    }
}

fn show_speed(sink: Sink<'_>, milliamps: u32, custom: Custom) {
    sink.push_tray(TrayValues {
        // Only a window holds a capacity to send. The capacity the tray
        // already has is the one its menu was drawn from, so a tray write
        // has nothing to teach it here.
        design_capacity: match sink {
            Sink::Window(ui) => ui.battery.design_capacity.get(),
            Sink::Tray(_) => None,
        },
        charge_current_limit: Some(milliamps),
        ..TrayValues::default()
    });
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
    match control.set_charge_limit(percent).await {
        // Silent when the daemon found the ceiling already there: announcing
        // a write that didn't happen is a confirmation of nothing.
        Ok(written) => {
            if written {
                if percent == NO_CHARGE_LIMIT {
                    sink.toast("Charge limit switched off");
                } else {
                    sink.toast(&format!("Charge limit set to {percent}%"));
                }
            }
            show_limit(sink, percent, custom);
        }
        Err(e) => sink.toast_error("Setting charge limit", e),
    }
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
            sink.toast_error("Setting charge speed", e);
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
