//! The preferences window: its widgets, the reads that fill them, and the
//! writes they send.
//!
//! [`Sink`] and the `apply_*` functions live here rather than beside the tray
//! because the window is the end with somewhere to report; a tray preset
//! borrows them when a window has been built and answers for itself when one
//! has not.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use frameguin_wire::{
    BatteryState, Capability, ClickForce, FpLevel, FrameguinProxy, HAPTIC_INTENSITY_LEVELS,
    NO_CHARGE_CURRENT_LIMIT,
};
use gtk4 as gtk;
use gtk4::gio;
use gtk4::glib;

use crate::caps::{Capabilities, fp_rows};
use crate::format::{
    CHARGE_LIMIT_CUSTOM, CHARGE_SPEED_CUSTOM, CHARGE_SPEED_LABELS, CUSTOM_CHARGE_STEP_MA,
    MIN_CHARGE_LIMIT, MIN_CUSTOM_CHARGE_MA, amps, charge_flow_label, charge_limit_labels,
    charge_limit_percent, charge_limit_position, charge_speed_labels, charge_speed_milliamps,
    charge_speed_position, click_force_label, fp_level_labels, haptic_labels, scale_milliamps,
    scale_percent, with_custom_row,
};
use crate::tray::{TrayIcon, TrayValues, tray_push};
use crate::{APP_ID, autostart, board, daemon_proxy};

const SLIDER_DEBOUNCE: Duration = Duration::from_millis(200);
/// Keys and the wheel on a slider that otherwise writes only when a drag
/// ends. Longer than the live sliders wait, for the same reason that one
/// writes on release: nothing shows the values passed through, and each of
/// them would be another authorized EC write.
const SETTLE_DEBOUNCE: Duration = Duration::from_millis(700);
const KBD_SYNC_SECONDS: u32 = 2;
const BATTERY_STATE_SECONDS: u32 = 2;

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

/// The chrome every slider in this window shares, so a change to how one
/// reads doesn't have to be made four times. `format` renders the value in
/// the control's own unit, which is the only part that differs.
fn build_scale(
    adjustment: &gtk::Adjustment,
    format: impl Fn(f64) -> String + 'static,
) -> gtk::Scale {
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(adjustment));
    scale.set_size_request(180, -1);
    scale.set_valign(gtk::Align::Center);
    scale.set_draw_value(true);
    scale.set_value_pos(gtk::PositionType::Left);
    scale.set_format_value_func(move |_, value| format(value));
    scale.set_sensitive(false);
    scale
}

/// A `StringList` from owned labels, which every combo here builds from a
/// `Vec<String>` that GTK will only take as borrowed strs.
fn string_list(labels: &[String]) -> gtk::StringList {
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    gtk::StringList::new(&labels)
}

/// Names a combo row for GTK, which addresses rows by u32. Positions come
/// from fixed arrays of a handful of entries, so the fallback is unreachable
/// — it is what keeps the conversion total without a cast.
fn combo_index(position: usize) -> u32 {
    u32::try_from(position).unwrap_or(gtk::INVALID_LIST_POSITION)
}

/// The row to select, where the hardware may be sitting on none of them —
/// GTK spells "nothing selected" as a sentinel position rather than as an
/// absent one.
fn combo_selection(position: Option<usize>) -> u32 {
    position.map_or(gtk::INVALID_LIST_POSITION, combo_index)
}

pub(crate) struct Ui {
    toasts: adw::ToastOverlay,
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
    /// What the daemon said this board supports, so a later reload knows
    /// which values to ask for.
    caps: Cell<Capabilities>,
    /// Set while widgets are being moved to mirror the hardware, so their
    /// change handlers don't echo the reading back as a write.
    syncing: Cell<bool>,
    kbd_scale: gtk::Scale,
    fp_scale: gtk::Scale,
    fp_combo: adw::ComboRow,
    /// The slider's row; shown only while the level is Custom.
    fp_custom_row: adw::ActionRow,
    /// The levels behind the combo's rows, narrowed to what this board
    /// supports once capabilities are known.
    fp_levels: RefCell<Vec<FpLevel>>,
    haptic_combo: adw::ComboRow,
    force_combo: adw::ComboRow,
    tray: Option<ksni::blocking::Handle<TrayIcon>>,
}

impl Ui {
    fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }

    /// Moves widgets to match the hardware without their handlers writing the
    /// value straight back. Every setter returns early while this is set.
    fn sync(&self, update: impl FnOnce()) {
        self.syncing.set(true);
        update();
        self.syncing.set(false);
    }

    fn sync_tray(&self, values: TrayValues) {
        if let Some(handle) = &self.tray {
            tray_push(handle, values);
        }
    }

    /// Shows a battery reading. No `sync` guard and no `Custom` question,
    /// unlike every other show_ here: nothing on this row writes back, so
    /// there is no handler to hold off.
    fn show_battery_state(&self, state: BatteryState) {
        self.state_percent.set_label(&format!("{}%", state.percent));
        self.state_row.set_subtitle(&charge_flow_label(state));
    }

    /// Moves the fingerprint widgets onto a level without writing it back.
    /// Unlike the charge controls, Custom is a state the EC reports, so the
    /// row it belongs on is read off the level rather than remembered.
    fn show_fp_level(&self, level: FpLevel) {
        self.sync(|| {
            self.fp_combo.set_selected(self.fp_combo_index(level));
            self.fp_custom_row.set_visible(level == FpLevel::Custom);
        });
    }

    /// Moves the charge-limit widgets onto a ceiling without writing it back,
    /// the counterpart of [`Ui::show_charge_speed`] — change one and read the
    /// other.
    fn show_charge_limit(&self, percent: u8, custom: Custom) {
        let preset = charge_limit_position(percent);
        let index = custom_or(&self.limit_combo, CHARGE_LIMIT_CUSTOM, preset, custom);
        self.sync(|| {
            self.limit_combo.set_selected(combo_index(index));
            self.limit_custom_row
                .set_visible(index == CHARGE_LIMIT_CUSTOM);
            self.limit_scale.set_value(f64::from(percent));
        });
    }

    /// Moves the charge-speed widgets onto a limit without writing it back.
    /// Shared by the reload and the write, so the combo, the slider and the
    /// slider's visibility can't disagree about which one is in effect.
    fn show_charge_speed(&self, milliamps: u32, custom: Custom) {
        let Some(capacity) = self.design_capacity.get() else {
            self.sync(|| self.speed_combo.set_selected(gtk::INVALID_LIST_POSITION));
            return;
        };
        let preset = charge_speed_position(capacity, milliamps);
        let index = custom_or(&self.speed_combo, CHARGE_SPEED_CUSTOM, preset, custom);
        self.sync(|| {
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

    fn fp_combo_index(&self, level: FpLevel) -> u32 {
        combo_selection(self.fp_levels.borrow().iter().position(|l| *l == level))
    }
}

/// (Re)arms a debounce slot: cancels any pending source and schedules `action`
/// after `delay`.
fn debounce(
    slot: &Rc<RefCell<Option<glib::SourceId>>>,
    delay: Duration,
    action: impl FnOnce() + 'static,
) {
    if let Some(source) = slot.borrow_mut().take() {
        source.remove();
    }
    let cleared = slot.clone();
    let id = glib::timeout_add_local_once(delay, move || {
        cleared.replace(None);
        action();
    });
    slot.replace(Some(id));
}

// Widgets for absent capabilities stay hidden and insensitive, so their
// handlers can never fire; connecting unconditionally keeps one wiring path.
fn connect_handlers(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    connect_battery_state(ui, proxy);
    connect_charge_setter(ui, proxy);
    connect_charge_speed_setter(ui, proxy);
    connect_kbd_setter(ui, proxy);
    connect_fp_setter(ui, proxy);
    connect_touchpad_setters(ui, proxy);
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

fn connect_charge_speed_setter(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let speed_ui = ui.clone();
    let speed_proxy = proxy.clone();
    ui.speed_combo.connect_selected_notify(move |row| {
        if speed_ui.syncing.get() {
            return;
        }
        // An unselected row reports INVALID_LIST_POSITION, which is not an
        // index — reading it as one would land on "full speed" and lift a
        // limit nobody asked to lift.
        let Ok(index) = usize::try_from(row.selected()) else {
            return;
        };
        if index > CHARGE_SPEED_CUSTOM {
            return;
        }
        // Choosing Custom writes nothing: the limit in effect is already
        // whatever it is, and the row only reveals the slider that can change
        // it. Unlike the fingerprint's custom level, there is no EC state to
        // enter here — a dialled-in current is just a current.
        if index == CHARGE_SPEED_CUSTOM {
            speed_ui.speed_custom_row.set_visible(true);
            return;
        }
        // The row stays insensitive until the capacity is read, so a preset
        // can't be picked without one; the early return only keeps the
        // conversion total.
        let Some(design_capacity) = speed_ui.design_capacity.get() else {
            return;
        };
        let milliamps = charge_speed_milliamps(design_capacity, index);
        let ui = speed_ui.clone();
        let proxy = speed_proxy.clone();
        glib::spawn_future_local(async move {
            apply_charge_speed(Sink::Window(&ui), &proxy, milliamps, Custom::Rederive).await;
        });
    });

    // Slider: a raw current, reachable only while the combo is on Custom.
    let scale_ui = ui.clone();
    let scale_proxy = proxy.clone();
    connect_slider_writes(ui, &ui.speed_scale, scale_milliamps, move |milliamps| {
        let ui = scale_ui.clone();
        let proxy = scale_proxy.clone();
        glib::spawn_future_local(async move {
            apply_charge_speed(Sink::Window(&ui), &proxy, milliamps, Custom::Keep).await;
        });
    });
}

/// The one row nothing writes to: it follows the pack, which moves whether
/// or not anyone touches the app. Silent on a failed read — a toast every
/// tick would bury the window over a reading that is due again in seconds.
///
/// The tick deliberately tells the tray nothing. Every push blocks on the
/// tray's thread and makes it rebuild and re-signal the whole menu, which is
/// not something to do twice a minute for a menu nobody has opened — the
/// tray asks for its own reading when its menu is about to show.
fn connect_battery_state(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let poll_ui = ui.clone();
    let poll_proxy = proxy.clone();
    poll_while_mapped(&ui.state_row, BATTERY_STATE_SECONDS, move || {
        let ui = poll_ui.clone();
        let proxy = poll_proxy.clone();
        glib::spawn_future_local(async move {
            if let Ok(state) = proxy.get_battery_state().await {
                ui.show_battery_state(state);
            }
        });
    });
}

fn connect_touchpad_setters(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let haptic_ui = ui.clone();
    let haptic_proxy = proxy.clone();
    ui.haptic_combo.connect_selected_notify(move |row| {
        if haptic_ui.syncing.get() {
            return;
        }
        let percent = HAPTIC_INTENSITY_LEVELS[row.selected() as usize];
        let ui = haptic_ui.clone();
        let proxy = haptic_proxy.clone();
        glib::spawn_future_local(async move {
            if let Err(e) = proxy.set_haptic_intensity(percent).await {
                ui.toast(&format!("Setting haptic intensity failed: {e}"));
            }
        });
    });

    let force_ui = ui.clone();
    let force_proxy = proxy.clone();
    ui.force_combo.connect_selected_notify(move |row| {
        if force_ui.syncing.get() {
            return;
        }
        let force = ClickForce::ALL[row.selected() as usize];
        let ui = force_ui.clone();
        let proxy = force_proxy.clone();
        glib::spawn_future_local(async move {
            if let Err(e) = proxy.set_touchpad_click_force(force).await {
                ui.toast(&format!("Setting click force failed: {e}"));
            }
        });
    });
}

/// Where a write reports back to. A tray preset can arrive in a session whose
/// window has never been built, and building a widget tree to hold a toast
/// nobody will see is not worth it — so the tray answers for itself, and only
/// the window carries the parts a window has.
#[derive(Clone, Copy)]
pub(crate) enum Sink<'a> {
    Window(&'a Ui),
    Tray(&'a ksni::blocking::Handle<TrayIcon>),
}

impl Sink<'_> {
    fn toast(&self, message: &str) {
        if let Sink::Window(ui) = self {
            ui.toast(message);
        }
    }

    /// Sends what this sink can vouch for to the tray, wherever it lives.
    fn push_tray(&self, values: TrayValues) {
        match self {
            Sink::Window(ui) => ui.sync_tray(values),
            Sink::Tray(handle) => tray_push(handle, values),
        }
    }

    fn show_charge_limit(&self, percent: u8, custom: Custom) {
        self.push_tray(TrayValues::charge_limit(percent));
        if let Sink::Window(ui) = self {
            ui.show_charge_limit(percent, custom);
        }
    }

    fn show_charge_speed(&self, milliamps: u32, custom: Custom) {
        self.push_tray(TrayValues {
            // Only a window holds a capacity to send. The capacity the tray
            // already has is the one its menu was drawn from, so a tray write
            // has nothing to teach it here.
            design_capacity: match self {
                Sink::Window(ui) => ui.design_capacity.get(),
                Sink::Tray(_) => None,
            },
            charge_current_limit: Some(milliamps),
            ..TrayValues::default()
        });
        if let Sink::Window(ui) = self {
            ui.show_charge_speed(milliamps, custom);
        }
    }

    fn show_fp_level(&self, level: FpLevel) {
        self.push_tray(TrayValues::fp_level(level));
        if let Sink::Window(ui) = self {
            ui.show_fp_level(level);
        }
    }
}

/// The one write for the charge limit. The window's row and the tray preset
/// both come here, so neither can drift from the other on what it reports or
/// what it tells the tray.
///
/// [`apply_charge_speed`] is the same shape for the other Battery control.
/// They are deliberately two functions rather than one generic: the values
/// differ (`u8` against `u32`), the speed resolves its presets against the
/// battery's capacity where the ceiling's are constants, and only the speed
/// has a "full speed means no limit" case. A change to one is usually a
/// change to both — read the sibling before editing either.
pub(crate) async fn apply_charge_limit(
    sink: Sink<'_>,
    proxy: &FrameguinProxy<'static>,
    percent: u8,
    custom: Custom,
) {
    match proxy.set_charge_limit(percent).await {
        // Silent when the daemon found the ceiling already there: announcing
        // a write that didn't happen is a confirmation of nothing.
        Ok(written) => {
            if written {
                sink.toast(&format!("Charge limit set to {percent}%"));
            }
            sink.show_charge_limit(percent, custom);
        }
        Err(e) => sink.toast(&format!("Setting charge limit failed: {e}")),
    }
}

/// The one write for the charge speed, in mA or `NO_CHARGE_CURRENT_LIMIT`.
/// Callers resolve a speed to milliamps against the battery capacity they
/// hold — the window's, or the tray's own copy.
pub(crate) async fn apply_charge_speed(
    sink: Sink<'_>,
    proxy: &FrameguinProxy<'static>,
    milliamps: u32,
    custom: Custom,
) {
    let written = match proxy.set_charge_current_limit(milliamps).await {
        Ok(written) => written,
        Err(e) => {
            sink.toast(&format!("Setting charge speed failed: {e}"));
            return;
        }
    };
    if written {
        if milliamps == NO_CHARGE_CURRENT_LIMIT {
            sink.toast("Charging at full speed");
        } else {
            sink.toast(&format!("Charge speed capped at {}", amps(milliamps)));
        }
    }
    sink.show_charge_speed(milliamps, custom);
}

/// The one write for a fingerprint preset. Custom is not one: the EC reports
/// it after a raw percentage write, which goes through
/// [`apply_fp_brightness`] instead.
pub(crate) async fn apply_fp_level(
    sink: Sink<'_>,
    proxy: &FrameguinProxy<'static>,
    level: FpLevel,
) {
    if let Err(e) = proxy.set_fingerprint_level(level).await {
        sink.toast(&format!("Setting fingerprint level failed: {e}"));
        return;
    }
    sink.show_fp_level(level);
    // The preset resolves to a percentage only the EC knows, and only the
    // window has anywhere to put it.
    if let Sink::Window(ui) = sink
        && let Ok((percent, _)) = proxy.get_fingerprint_brightness().await
    {
        ui.sync(|| ui.fp_scale.set_value(f64::from(percent)));
    }
}

/// The one write for a custom fingerprint percentage. Any raw percentage
/// leaves the EC reporting "custom", so this owns that consequence rather
/// than leaving each caller to remember it.
async fn apply_fp_brightness(ui: &Ui, proxy: &FrameguinProxy<'static>, percent: u8) {
    if let Err(e) = proxy.set_fingerprint_brightness(percent).await {
        ui.toast(&format!("Setting fingerprint brightness failed: {e}"));
        return;
    }
    ui.sync_tray(TrayValues::fp_level(FpLevel::Custom));
    ui.sync(|| ui.fp_custom_row.set_visible(true));
}

fn connect_fp_setter(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    // Slider: a raw percentage write; only reachable while the level is
    // Custom, so combo and tray already reflect it.
    let fp_slot = Rc::new(RefCell::new(None));
    let fp_ui = ui.clone();
    let fp_proxy = proxy.clone();
    ui.fp_scale.connect_value_changed(move |scale| {
        if fp_ui.syncing.get() {
            return;
        }
        let value = scale_percent(scale.value());
        let ui = fp_ui.clone();
        let proxy = fp_proxy.clone();
        debounce(&fp_slot, SLIDER_DEBOUNCE, move || {
            glib::spawn_future_local(async move {
                apply_fp_brightness(&ui, &proxy, value).await;
            });
        });
    });

    // Combo: presets write the level and re-read so the slider carries the
    // percentage the preset resolved to; Custom reveals the slider and
    // applies its value, making the EC state actually custom.
    let combo_ui = ui.clone();
    let combo_proxy = proxy.clone();
    ui.fp_combo.connect_selected_notify(move |row| {
        if combo_ui.syncing.get() {
            return;
        }
        let level = combo_ui.fp_levels.borrow()[row.selected() as usize];
        let ui = combo_ui.clone();
        let proxy = combo_proxy.clone();
        glib::spawn_future_local(async move {
            if level == FpLevel::Custom {
                let percent = scale_percent(ui.fp_scale.value());
                apply_fp_brightness(&ui, &proxy, percent).await;
                return;
            }
            apply_fp_level(Sink::Window(&ui), &proxy, level).await;
        });
    });
}

fn connect_charge_setter(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let limit_ui = ui.clone();
    let limit_proxy = proxy.clone();
    ui.limit_combo.connect_selected_notify(move |row| {
        if limit_ui.syncing.get() {
            return;
        }
        let Ok(index) = usize::try_from(row.selected()) else {
            return;
        };
        if index > CHARGE_LIMIT_CUSTOM {
            return;
        }
        // Choosing Custom writes nothing — the ceiling in effect is already
        // whatever it is, and the row only reveals the slider that moves it.
        if index == CHARGE_LIMIT_CUSTOM {
            limit_ui.limit_custom_row.set_visible(true);
            return;
        }
        let percent = charge_limit_percent(index);
        let ui = limit_ui.clone();
        let proxy = limit_proxy.clone();
        glib::spawn_future_local(async move {
            apply_charge_limit(Sink::Window(&ui), &proxy, percent, Custom::Rederive).await;
        });
    });

    // Slider: a raw ceiling, reachable only while the combo is on Custom.
    let scale_ui = ui.clone();
    let scale_proxy = proxy.clone();
    connect_slider_writes(ui, &ui.limit_scale, scale_percent, move |percent| {
        let ui = scale_ui.clone();
        let proxy = scale_proxy.clone();
        glib::spawn_future_local(async move {
            apply_charge_limit(Sink::Window(&ui), &proxy, percent, Custom::Keep).await;
        });
    });
}

fn connect_kbd_setter(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let kbd_slot = Rc::new(RefCell::new(None));
    let kbd_ui = ui.clone();
    let kbd_proxy = proxy.clone();
    let kbd_write_slot = kbd_slot.clone();
    ui.kbd_scale.connect_value_changed(move |scale| {
        if kbd_ui.syncing.get() {
            return;
        }
        let value = scale_percent(scale.value());
        let ui = kbd_ui.clone();
        let proxy = kbd_proxy.clone();
        debounce(&kbd_write_slot, SLIDER_DEBOUNCE, move || {
            glib::spawn_future_local(async move {
                if let Err(e) = proxy.set_keyboard_backlight(value).await {
                    ui.toast(&format!("Setting backlight failed: {e}"));
                }
            });
        });
    });

    // The EC is a second writer to the backlight (Fn+Space, and on newer
    // boards a firmware auto mode that overrides host writes), so while the
    // slider is on screen it follows the actual value. The tick skips while a
    // write is pending so it can't yank the slider mid-drag.
    let kbd_poll_ui = ui.clone();
    let kbd_poll_proxy = proxy.clone();
    poll_while_mapped(&ui.kbd_scale, KBD_SYNC_SECONDS, move || {
        if kbd_slot.borrow().is_some() {
            return;
        }
        let ui = kbd_poll_ui.clone();
        let proxy = kbd_poll_proxy.clone();
        glib::spawn_future_local(async move {
            if let Ok(percent) = proxy.get_keyboard_backlight().await
                && percent != scale_percent(ui.kbd_scale.value())
            {
                ui.sync(|| ui.kbd_scale.set_value(f64::from(percent)));
            }
        });
    });
}

/// Runs `tick` every `seconds` for as long as `widget` is on screen: a
/// resident app whose window is hidden does no periodic work, and neither
/// does one whose board lacks the control, since an unsupported row is never
/// mapped.
fn poll_while_mapped(widget: &impl IsA<gtk::Widget>, seconds: u32, tick: impl Fn() + 'static) {
    let tick = Rc::new(tick);
    let source: Rc<RefCell<Option<glib::SourceId>>> = Rc::default();
    let arm: Rc<dyn Fn()> = {
        let source = source.clone();
        Rc::new(move || {
            let tick = tick.clone();
            let id = glib::timeout_add_seconds_local(seconds, move || {
                tick();
                glib::ControlFlow::Continue
            });
            if let Some(old) = source.replace(Some(id)) {
                old.remove();
            }
        })
    };
    let map_arm = arm.clone();
    widget.as_ref().connect_map(move |_| map_arm());
    let unmap_source = source;
    widget.as_ref().connect_unmap(move |_| {
        if let Some(id) = unmap_source.take() {
            id.remove();
        }
    });
    // The window is usually already on screen when setters connect (init is
    // async), so map won't fire for the current visibility.
    if widget.as_ref().is_mapped() {
        arm();
    }
}

#[expect(clippy::too_many_lines, reason = "flat widget construction")]
pub(crate) fn build_window(
    app: &adw::Application,
    tray: Option<ksni::blocking::Handle<TrayIcon>>,
) -> (adw::ApplicationWindow, Rc<Ui>) {
    let page = adw::PreferencesPage::new();

    let battery = adw::PreferencesGroup::builder().title("Battery").build();
    let state_row = adw::ActionRow::builder().title("Charge").build();
    let state_percent = gtk::Label::new(None);
    state_row.add_suffix(&state_percent);
    battery.add(&state_row);
    let limit_labels = with_custom_row(charge_limit_labels());
    let limit_combo = adw::ComboRow::builder()
        .title("Charge limit")
        .subtitle("Maximum charge percentage")
        .model(&string_list(&limit_labels))
        .sensitive(false)
        .build();
    battery.add(&limit_combo);
    let limit_custom_row = adw::ActionRow::builder().title("Maximum charge").build();
    let limit_adjustment =
        gtk::Adjustment::new(MIN_CHARGE_LIMIT, MIN_CHARGE_LIMIT, 100.0, 5.0, 5.0, 0.0);
    let limit_scale = build_scale(&limit_adjustment, |value| format!("{value:.0}%"));
    limit_custom_row.add_suffix(&limit_scale);
    limit_custom_row.set_visible(false);
    battery.add(&limit_custom_row);
    let speed_combo = adw::ComboRow::builder()
        .title("Charge speed")
        .subtitle("Maximum charging rate")
        .model(&gtk::StringList::new(&CHARGE_SPEED_LABELS))
        .sensitive(false)
        .build();
    battery.add(&speed_combo);
    let speed_custom_row = adw::ActionRow::builder().title("Charge current").build();
    // The upper bound is the battery's 1C current, filled in once it is read;
    // asking for more than the pack requests would be a limit that never
    // binds. Explicit adjustment for the same reason as the backlight's.
    let speed_adjustment = gtk::Adjustment::new(
        MIN_CUSTOM_CHARGE_MA,
        MIN_CUSTOM_CHARGE_MA,
        MIN_CUSTOM_CHARGE_MA,
        100.0,
        100.0,
        0.0,
    );
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "slider is bounded by its adjustment, which holds milliamps"
    )]
    let speed_scale = build_scale(&speed_adjustment, |value| amps(value as u32));
    speed_custom_row.add_suffix(&speed_scale);
    speed_custom_row.set_visible(false);
    battery.add(&speed_custom_row);
    page.add(&battery);

    let keyboard = adw::PreferencesGroup::builder().title("Keyboard").build();
    let kbd_row = adw::ActionRow::builder().title("Backlight").build();
    // Explicit adjustment: with_range would set page_increment to 10x the
    // step, and a mouse wheel click on a GtkRange moves by the page
    // increment — which would jump the slider across its whole range.
    let kbd_adjustment = gtk::Adjustment::new(0.0, 0.0, 100.0, 10.0, 10.0, 0.0);
    let kbd_scale = build_scale(&kbd_adjustment, |value| format!("{value:.0}%"));
    kbd_row.add_suffix(&kbd_scale);
    keyboard.add(&kbd_row);
    page.add(&keyboard);

    let fingerprint = adw::PreferencesGroup::builder()
        .title("Fingerprint")
        .build();
    // No model: which levels a board has is the daemon's answer, and the row
    // it would show meanwhile is one the board may not have.
    let fp_combo = adw::ComboRow::builder()
        .title("LED level")
        .sensitive(false)
        .build();
    fingerprint.add(&fp_combo);
    let fp_row = adw::ActionRow::builder().title("LED brightness").build();
    // The EC accepts 1-100 for the fingerprint LED; 0 is not a valid level.
    let fp_adjustment = gtk::Adjustment::new(1.0, 1.0, 100.0, 10.0, 10.0, 0.0);
    let fp_scale = build_scale(&fp_adjustment, |value| format!("{value:.0}%"));
    fp_row.add_suffix(&fp_scale);
    fp_row.set_visible(false);
    fingerprint.add(&fp_row);
    page.add(&fingerprint);

    let touchpad = adw::PreferencesGroup::builder().title("Touchpad").build();
    let haptic_combo = adw::ComboRow::builder()
        .title("Haptic intensity")
        .subtitle("Strength of the click feedback")
        .model(&string_list(&haptic_labels()))
        .sensitive(false)
        .build();
    touchpad.add(&haptic_combo);
    let force_combo = adw::ComboRow::builder()
        .title("Click force")
        .subtitle("How hard you press to click")
        .model(&gtk::StringList::new(
            &ClickForce::ALL.map(click_force_label),
        ))
        .sensitive(false)
        .build();
    touchpad.add(&force_combo);
    page.add(&touchpad);

    let application = adw::PreferencesGroup::builder()
        .title("Application")
        .build();
    let autostart_row = adw::SwitchRow::builder()
        .title("Start at login")
        .subtitle("Show only the tray icon until opened")
        .build();
    autostart_row.set_active(autostart::entry_path().exists());
    application.add(&autostart_row);
    page.add(&application);

    // Detected hardware as the header subtitle: one line, no key/value rows.
    let detected =
        board::detected().unwrap_or_else(|| "No Framework hardware detected".to_string());

    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("Frameguin", &detected)));

    let menu = gio::Menu::new();
    menu.append(Some("_About Frameguin"), Some("app.about"));
    menu.append(Some("_Quit"), Some("app.quit"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Main menu")
        .build();
    header.pack_end(&menu_button);
    view.add_top_bar(&header);
    view.set_content(Some(&page));
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&view));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Frameguin")
        .default_width(420)
        // Tall enough for every control group at the default font scale;
        // re-measure when the rows change.
        .default_height(710)
        .content(&toasts)
        .icon_name(APP_ID)
        .build();

    // Hiding instead of closing only makes sense while a tray icon exists to
    // bring the window back.
    window.set_hide_on_close(tray.is_some());

    let ui = Rc::new(Ui {
        toasts,
        state_row,
        state_percent,
        limit_combo,
        limit_scale,
        limit_custom_row,
        speed_combo,
        speed_scale,
        speed_custom_row,
        design_capacity: Cell::default(),
        caps: Cell::default(),
        syncing: Cell::new(false),
        kbd_scale,
        fp_scale,
        fp_combo,
        fp_custom_row: fp_row,
        fp_levels: RefCell::new(Vec::new()),
        haptic_combo,
        force_combo,
        tray,
    });

    let autostart_ui = ui.clone();
    autostart_row.connect_active_notify(move |row| {
        if let Err(e) = autostart::set(row.is_active()) {
            autostart_ui.toast(&format!("Updating autostart failed: {e}"));
        }
    });

    let groups = CapabilityWidgets {
        battery: battery.clone(),
        state_row: ui.state_row.clone(),
        limit_combo: ui.limit_combo.clone(),
        speed_combo: ui.speed_combo.clone(),
        keyboard: keyboard.clone(),
        fingerprint: fingerprint.clone(),
        touchpad: touchpad.clone(),
    };
    glib::spawn_future_local(init_from_daemon(ui.clone(), groups, window.clone()));

    (window, ui)
}

/// Everything gated on a capability, so hiding what a board lacks is one call
/// rather than one line per control. A group whose controls are probed
/// separately carries its rows here too, keeping visibility a single question
/// with a single answer.
struct CapabilityWidgets {
    battery: adw::PreferencesGroup,
    state_row: adw::ActionRow,
    limit_combo: adw::ComboRow,
    speed_combo: adw::ComboRow,
    keyboard: adw::PreferencesGroup,
    fingerprint: adw::PreferencesGroup,
    touchpad: adw::PreferencesGroup,
}

impl CapabilityWidgets {
    fn show_supported(&self, caps: Capabilities) {
        let battery_state = caps.has(Capability::BatteryState);
        let charge_limit = caps.has(Capability::ChargeLimit);
        let charge_speed = caps.has(Capability::ChargeCurrentLimit);
        self.battery
            .set_visible(battery_state || charge_limit || charge_speed);
        self.state_row.set_visible(battery_state);
        self.limit_combo.set_visible(charge_limit);
        self.speed_combo.set_visible(charge_speed);
        // Withheld from every board by `caps::offered`, so this reads false.
        self.keyboard
            .set_visible(caps.has(Capability::KeyboardBacklight));
        self.fingerprint
            .set_visible(caps.has(Capability::FpBrightness));
        self.touchpad
            .set_visible(caps.has(Capability::HapticTouchpad));
    }
}

/// Asks the daemon what this board supports, hides what it can't do, loads
/// current values, then connects the setters — last, so the programmatic
/// `set_value` calls during init can't echo back into the daemon.
async fn init_from_daemon(ui: Rc<Ui>, groups: CapabilityWidgets, window: adw::ApplicationWindow) {
    let proxy = match daemon_proxy().await {
        Ok(p) => p,
        Err(e) => {
            ui.toast(&format!("Daemon unavailable: {e}"));
            return;
        }
    };

    let names = match proxy.get_capabilities().await {
        Ok(names) => names,
        Err(e) => {
            ui.toast(&format!("Reading capabilities failed: {e}"));
            Vec::new()
        }
    };
    let caps = Capabilities::from_probe(&names);
    ui.caps.set(caps);
    groups.show_supported(caps);
    ui.sync_tray(TrayValues::caps(caps));
    // Fixed for a board, so the combo's rows are chosen once here rather than
    // rebuilt by every reload.
    let rows = fp_rows(caps);
    ui.fp_combo
        .set_model(Some(&string_list(&fp_level_labels(&rows))));
    ui.fp_levels.replace(rows);
    load_values(&ui, &proxy).await;
    connect_handlers(&ui, &proxy);

    // The hardware moves while the app sits in the tray: the EC's battery
    // extender lowers the charge limit on its own, and framework_tool writes
    // any of these behind the app's back. So the window reloads every time it
    // returns to the screen instead of trusting what it read at startup.
    let map_ui = ui.clone();
    let map_proxy = proxy.clone();
    window.connect_map(move |_| {
        let ui = map_ui.clone();
        let proxy = map_proxy.clone();
        glib::spawn_future_local(async move { load_values(&ui, &proxy).await });
    });
}

/// The Battery group's half of a reload: the reading at the top, then the
/// ceiling and the speed with their combos and sliders. Returns what the
/// tray should be told, for the one push [`load_values`] makes at the end.
async fn load_battery_values(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) -> TrayValues {
    let caps = ui.caps.get();
    let mut values = TrayValues::default();
    if caps.has(Capability::BatteryState) {
        // Read here as well as polled: the poll's first tick is a couple of
        // seconds after the window appears, and an empty row until then
        // reads as a control that failed rather than one still filling.
        match proxy.get_battery_state().await {
            Ok(state) => {
                ui.show_battery_state(state);
                values.battery = Some(state);
            }
            Err(e) => ui.toast(&format!("Reading battery state failed: {e}")),
        }
    }
    if caps.has(Capability::ChargeLimit) {
        match proxy.get_charge_limit().await {
            Ok(limit) => {
                ui.show_charge_limit(limit, Custom::Rederive);
                ui.sync(|| {
                    ui.limit_combo.set_sensitive(true);
                    ui.limit_scale.set_sensitive(true);
                });
                values.charge_limit = Some(limit);
            }
            Err(e) => ui.toast(&format!("Reading charge limit failed: {e}")),
        }
    }
    if caps.has(Capability::ChargeCurrentLimit) {
        // A pack's design capacity can't change under a running app, so it is
        // read once and the labels built from it stay put.
        if ui.design_capacity.get().is_none() {
            match proxy.get_battery_design_capacity().await {
                Ok(capacity) => {
                    ui.design_capacity.set(Some(capacity));
                    let labels = with_custom_row(charge_speed_labels(capacity));
                    // 1C is as fast as the pack ever asks, so a slider beyond
                    // it would only offer limits that never bind. Floored to
                    // the step the value rounds to, so the far end of the
                    // track is a position that sends what it shows rather
                    // than a sliver that rounds back down.
                    let top = (f64::from(capacity) / CUSTOM_CHARGE_STEP_MA).floor()
                        * CUSTOM_CHARGE_STEP_MA;
                    ui.sync(|| {
                        ui.speed_combo.set_model(Some(&string_list(&labels)));
                        ui.speed_scale
                            .adjustment()
                            .set_upper(top.max(MIN_CUSTOM_CHARGE_MA));
                    });
                }
                Err(e) => ui.toast(&format!("Reading battery capacity failed: {e}")),
            }
        }
        match proxy.get_charge_current_limit().await {
            Ok(milliamps) => {
                ui.show_charge_speed(milliamps, Custom::Rederive);
                // Without the battery's capacity the fractions have no
                // milliamps behind them, so the row stays read-only.
                let known = ui.design_capacity.get().is_some();
                ui.sync(|| {
                    ui.speed_combo.set_sensitive(known);
                    ui.speed_scale.set_sensitive(known);
                });
                values.charge_current_limit = Some(milliamps);
            }
            Err(e) => ui.toast(&format!("Reading charge speed failed: {e}")),
        }
    }
    // Read above if the capability is there at all, and only once per run.
    values.design_capacity = ui.design_capacity.get();
    values
}

/// Re-reads every supported control and moves the widgets to match, pushing
/// the same values to the tray. Each write goes through `Ui::sync`, so a
/// reload can't echo back as a setter call. The tray's copies are collected
/// and handed over in one go at the end: each push blocks on the tray's
/// thread and rebuilds its whole menu, which would be wasted three times over
/// on a menu nobody has opened.
async fn load_values(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>) {
    let caps = ui.caps.get();
    let mut values = load_battery_values(ui, proxy).await;
    if caps.has(Capability::KeyboardBacklight) {
        match proxy.get_keyboard_backlight().await {
            Ok(percent) => ui.sync(|| {
                ui.kbd_scale.set_value(f64::from(percent));
                ui.kbd_scale.set_sensitive(true);
            }),
            Err(e) => ui.toast(&format!("Reading keyboard backlight failed: {e}")),
        }
    }
    if caps.has(Capability::FpBrightness) {
        match proxy.get_fingerprint_brightness().await {
            Ok((percent, level)) => {
                ui.show_fp_level(level);
                ui.sync(|| {
                    ui.fp_scale.set_value(f64::from(percent));
                    ui.fp_scale.set_sensitive(true);
                    ui.fp_combo.set_sensitive(true);
                });
                values.fp_level = Some(level);
            }
            Err(e) => ui.toast(&format!("Reading fingerprint brightness failed: {e}")),
        }
    }
    if caps.has(Capability::HapticTouchpad) {
        match proxy.get_haptic_intensity().await {
            Ok(percent) => {
                let row = HAPTIC_INTENSITY_LEVELS.iter().position(|l| *l == percent);
                ui.sync(|| {
                    ui.haptic_combo.set_selected(combo_selection(row));
                    ui.haptic_combo.set_sensitive(true);
                });
            }
            Err(e) => ui.toast(&format!("Reading haptic intensity failed: {e}")),
        }
        match proxy.get_touchpad_click_force().await {
            Ok(force) => {
                let row = ClickForce::ALL.iter().position(|f| *f == force);
                ui.sync(|| {
                    ui.force_combo.set_selected(combo_selection(row));
                    ui.force_combo.set_sensitive(true);
                });
            }
            Err(e) => ui.toast(&format!("Reading click force failed: {e}")),
        }
    }
    ui.sync_tray(values);
}
