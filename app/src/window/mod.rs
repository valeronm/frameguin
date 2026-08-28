//! The preferences window: its widgets, the reads that fill them, and the
//! writes they send.
//!
//! [`Sink`] and the `apply_*` functions live here rather than beside the tray
//! because the window is the end with somewhere to report; a tray preset
//! borrows them when a window has been built and answers for itself when one
//! has not.
//!
//! A control with a module of its own in `frameguin_model` has a group
//! module of its own here — its widgets, how a snapshot moves them, and what
//! its handlers dispatch — and the rest of this file is the controls not yet
//! moved.

pub(crate) mod power_led;
mod touchpad;
pub(crate) mod touchscreen;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use frameguin_model::control::Controls;
use frameguin_wire::{
    BatteryState, Capability, DeviceError, FrameguinProxy, NO_CHARGE_CURRENT_LIMIT,
};
use gtk4 as gtk;
use gtk4::gdk;
use gtk4::gio;
use gtk4::glib;

use crate::bus::Bus;
use crate::caps::Capabilities;
use crate::format::{
    CHARGE_LIMIT_CUSTOM, CHARGE_SPEED_CUSTOM, CHARGE_SPEED_LABELS, CUSTOM_CHARGE_STEP_MA,
    MIN_CHARGE_LIMIT, MIN_CUSTOM_CHARGE_MA, NO_CHARGE_LIMIT, amps, charge_flow_label,
    charge_limit_labels, charge_limit_percent, charge_limit_position, charge_speed_labels,
    charge_speed_milliamps, charge_speed_position, percent_label, scale_milliamps, scale_percent,
    with_custom_row,
};
use crate::mapped::poll_while_mapped;
use crate::reading::{Feed, Probe, Wants, show_while_mapped};
use crate::tray::{TrayIcon, TrayValues, tray_push};
use crate::{APP_ID, about, autostart, battery, board, parts};

const SLIDER_DEBOUNCE: Duration = Duration::from_millis(200);
/// Keys and the wheel on a slider that otherwise writes only when a drag
/// ends. Longer than the live sliders wait, for the same reason that one
/// writes on release: nothing shows the values passed through, and each of
/// them would be another authorized EC write.
const SETTLE_DEBOUNCE: Duration = Duration::from_millis(700);
const KBD_SYNC_SECONDS: u32 = 2;
/// How long the window waits before asking an unreachable daemon again, and
/// the ceiling the wait doubles up to. Bounded rather than endless-fast: the
/// service is bus-activated, so every attempt is a start attempt.
const FIRST_RETRY_SECONDS: u32 = 2;
const MAX_RETRY_SECONDS: u32 = 30;
/// The stack's two faces. Named rather than spelled at each call because
/// `set_visible_child_name` answers a name it doesn't know with nothing at
/// all — a typo would be a window that silently stops switching.
const CONTROLS_PAGE: &str = "controls";
const EMPTY_PAGE: &str = "empty";
/// The empty page's icon, sized here rather than left to the theme. One
/// number, so trying another size is one edit.
const EMPTY_ICON_CLASS: &str = "frameguin-empty-icon";
const EMPTY_ICON_PIXELS: u8 = 32;
/// The one sentence the header and the empty page both say, so a reword
/// cannot leave the window carrying two versions of it.
const NO_HARDWARE: &str = "No Framework hardware detected";

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

/// The empty page's secondary text: the error and what the app is doing
/// about it, which sit adjacent and have to read as one voice. Hidden to
/// start, since each is shown only in the states that have something to put
/// in it.
fn caption_label() -> gtk::Label {
    let label = gtk::Label::builder().visible(false).build();
    label.add_css_class("dim-label");
    label.add_css_class("caption");
    label
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
    /// Set while widgets are being moved to mirror the hardware, so their
    /// change handlers don't echo the reading back as a write.
    syncing: Cell<bool>,
    kbd_scale: gtk::Scale,
    power_led: power_led::Group,
    touchpad: touchpad::Group,
    touchscreen: touchscreen::Group,
    tray: Option<ksni::blocking::Handle<TrayIcon>>,
    /// Where the status row's reading comes from, shared with the battery
    /// report so the two windows cost the EC one walk between them rather than
    /// one apiece.
    feed: Rc<Feed>,
}

impl Ui {
    fn toast(&self, message: &str) {
        self.toasts.add_toast(adw::Toast::new(message));
    }

    /// A call that failed, named by what was being attempted — "Setting the
    /// charge limit", "Reading the battery". The sentence is built here rather
    /// than at each site so that every failure loses the D-Bus error name and
    /// none has to remember to; what each site still spells is its own half,
    /// which is the half no other site could supply. Takes a bus error or a
    /// device's `DeviceError` alike, the conversion being what drops the name.
    fn toast_error(&self, attempt: &str, error: impl Into<DeviceError>) {
        self.toast(&format!("{attempt} failed: {}", error.into()));
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
        self.state_percent.set_label(&percent_label(state.percent));
        self.state_row.set_subtitle(&charge_flow_label(state));
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
// A control's group connects only where the control is, its handlers needing
// one to dispatch to.
fn connect_handlers(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>, controls: &Controls<Bus>) {
    connect_battery_state(ui);
    connect_charge_setter(ui, proxy);
    connect_charge_speed_setter(ui, proxy);
    connect_kbd_setter(ui, proxy);
    if let Some(power_led) = &controls.power_led {
        ui.power_led.connect(ui, power_led);
    }
    if let Some(touchpad) = &controls.touchpad {
        ui.touchpad.connect(ui, touchpad);
    }
    if let Some(touchscreen) = &controls.touchscreen {
        ui.touchscreen.connect(ui, touchscreen);
    }
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
        // it. Unlike the power LED's custom level, there is no EC state to
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
/// or not anyone touches the app.
///
/// Fed rather than polled. The report shows the same walk of the same block,
/// and a row that read it for itself would have the two windows asking the EC
/// separately for one answer — see [`crate::reading`]. This row wants none of
/// the extras, so it asks for none, and the feed spends those calls only where
/// something would show them.
///
/// The feed deliberately tells the tray nothing. Every push blocks on the
/// tray's thread and makes it rebuild and re-signal the whole menu, which is
/// not something to do twice a minute for a menu nobody has opened — the
/// tray asks for its own reading when its menu is about to show.
fn connect_battery_state(ui: &Rc<Ui>) {
    let row_ui = ui.clone();
    show_while_mapped(&ui.feed, &ui.state_row, Wants::default(), move |reading| {
        row_ui.show_battery_state(reading.info.state);
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

    fn toast_error(&self, attempt: &str, error: impl Into<DeviceError>) {
        if let Sink::Window(ui) = self {
            ui.toast_error(attempt, error);
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
}

/// The one write for the charge limit. The window's row and the tray preset
/// both come here, so neither can drift from the other on what it reports or
/// what it tells the tray.
///
/// [`apply_charge_speed`] is the same shape for the other Battery control.
/// They are deliberately two functions rather than one generic: the values
/// differ (`u8` against `u32`), the speed resolves its presets against the
/// battery's capacity where the ceiling's are constants, and each carries its
/// own sentinel for no limit at all. A change to one is usually a change to
/// both — read the sibling before editing either.
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
                if percent == NO_CHARGE_LIMIT {
                    sink.toast("Charge limit switched off");
                } else {
                    sink.toast(&format!("Charge limit set to {percent}%"));
                }
            }
            sink.show_charge_limit(percent, custom);
        }
        Err(e) => sink.toast_error("Setting charge limit", e),
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
    sink.show_charge_speed(milliamps, custom);
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
                    ui.toast_error("Setting keyboard backlight", e);
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

#[expect(clippy::too_many_lines, reason = "flat widget construction")]
pub(crate) fn build_window(
    app: &adw::Application,
    tray: Option<ksni::blocking::Handle<TrayIcon>>,
    feed: Rc<Feed>,
) -> (adw::ApplicationWindow, Rc<Ui>) {
    let page = adw::PreferencesPage::new();

    let battery = adw::PreferencesGroup::builder().title("Battery").build();
    // The one row here that opens something rather than setting something.
    // Named as an action rather than wired to a handler, so the row and the
    // tray's reading reach the report the same way and neither has to hold a
    // bus connection to offer it.
    let state_row = adw::ActionRow::builder()
        .title("Status")
        .activatable(true)
        .action_name(format!("app.{}", battery::ACTION))
        .build();
    let state_percent = gtk::Label::new(None);
    state_row.add_suffix(&state_percent);
    state_row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    battery.add(&state_row);
    let limit_labels = with_custom_row(charge_limit_labels());
    let limit_combo = adw::ComboRow::builder()
        .title("Charge limit")
        .subtitle("Stops charging before the battery is full")
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
    let speed_custom_row = adw::ActionRow::builder().title("Maximum current").build();
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

    let power_led = power_led::Group::build();
    page.add(&power_led.widget);

    let touchpad = touchpad::Group::build();
    page.add(&touchpad.widget);

    let touchscreen = touchscreen::Group::build();
    page.add(&touchscreen.widget);

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
    let detected = board::detected().unwrap_or_else(|| NO_HARDWARE.to_string());

    let view = adw::ToolbarView::new();
    let header = adw::HeaderBar::new();
    header.set_title_widget(Some(&adw::WindowTitle::new("Frameguin", &detected)));

    let menu = gio::Menu::new();
    menu.append(Some("_Parts"), Some(&format!("app.{}", parts::ACTION)));
    menu.append(Some("_About Frameguin"), Some("app.about"));
    menu.append(Some("_Quit"), Some("app.quit"));
    let menu_button = gtk::MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .menu_model(&menu)
        .tooltip_text("Main menu")
        .build();
    header.pack_end(&menu_button);
    view.add_top_bar(&header);
    let empty = build_empty_page(&page);
    view.set_content(Some(&empty.stack));
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
        syncing: Cell::new(false),
        kbd_scale,
        power_led,
        touchpad,
        touchscreen,
        tray,
        feed,
    });

    let autostart_ui = ui.clone();
    autostart_row.connect_active_notify(move |row| {
        if let Err(e) = autostart::set(row.is_active()) {
            // Quoted off the row rather than spelled again: a message naming
            // a row by a title the row no longer has is worse than a vaguer
            // one.
            autostart_ui.toast(&format!("Setting “{}” failed: {e}", row.title()));
        }
    });

    let groups = CapabilityWidgets {
        battery: battery.clone(),
        state_row: ui.state_row.clone(),
        limit_combo: ui.limit_combo.clone(),
        speed_combo: ui.speed_combo.clone(),
        keyboard: keyboard.clone(),
    };
    // The report goes into the issue body rather than onto the clipboard with
    // instructions to paste it somewhere.
    let report_ui = ui.clone();
    empty.report.connect_clicked(move |button| {
        // Insensitive for the length of the gather: this is offered from the
        // state where nothing answers, so a second click would open a second
        // bus connection and a second browser tab.
        button.set_sensitive(false);
        let ui = report_ui.clone();
        let button = button.clone();
        glib::spawn_future_local(async move {
            if let Err(e) = about::report_issue().await {
                ui.toast(&format!("Opening the browser failed: {e}"));
            }
            button.set_sensitive(true);
        });
    });
    let init = Rc::new(Init {
        ui: ui.clone(),
        groups,
        empty,
        answered: Cell::default(),
        probing: Cell::default(),
        next_attempt: Cell::new(FIRST_RETRY_SECONDS),
        retry: Cell::default(),
    });
    let map_init = init.clone();
    window.connect_map(move |_| {
        let init = map_init.clone();
        glib::spawn_future_local(async move { init.refresh().await });
    });
    // A window hidden to the tray does no periodic work, the rule every other
    // timer here follows. Nothing is lost by stopping: the map above asks
    // again the moment anyone looks, which is also when an answer could
    // change what is on screen.
    let unmap_init = init.clone();
    window.connect_unmap(move |_| unmap_init.stop_retrying());
    glib::spawn_future_local(async move { init.fill().await });

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
}

impl CapabilityWidgets {
    /// The controls still gated on the capability list; a control with a
    /// group of its own gates that group on its device having detected
    /// itself.
    fn show_supported(&self, caps: Capabilities) {
        let battery = caps.has(Capability::Battery);
        let charge_limit = caps.has(Capability::ChargeLimit);
        let charge_speed = caps.has(Capability::ChargeCurrentLimit);
        self.battery
            .set_visible(battery || charge_limit || charge_speed);
        self.state_row.set_visible(battery);
        self.limit_combo.set_visible(charge_limit);
        self.speed_combo.set_visible(charge_speed);
        // Withheld from every board by `caps::offered`, so this reads false.
        self.keyboard
            .set_visible(caps.has(Capability::KeyboardBacklight));
    }
}

/// Why the window has no controls. Told apart because what the reader should
/// do differs: a machine that is not a Framework never will have any, an
/// unreachable service is a thing to retry, and a Framework board answering
/// with none is worth reporting. An empty window that does not say which
/// leaves all three looking like the app failing to start.
enum Empty {
    /// Carries the vendor the machine names itself with, so every variant
    /// arrives self-describing and the page's text costs no sysfs read of
    /// its own.
    NoHardware(String),
    DaemonUnavailable(String),
    NoControls,
}

/// The window's other face: a page saying why there is nothing to show, and
/// the stack that puts it in front of the controls. One type because filling
/// the page and switching to it are halves of a single act — a status page
/// nothing switches to is invisible, and a switch to an unfilled one is a
/// blank window.
struct EmptyPage {
    stack: gtk::Stack,
    status: adw::StatusPage,
    report: gtk::Button,
    detail: gtk::Label,
    progress: gtk::Label,
}

/// Builds the page and the stack that can put it in front of the controls,
/// which arrive already built: the two faces are made together because the
/// stack is what makes either of them reachable.
fn build_empty_page(controls: &adw::PreferencesPage) -> EmptyPage {
    let stack = gtk::Stack::new();
    // Homogeneous by default, which measures every page on every layout pass
    // and sizes the window to the larger of the two. Nothing here animates
    // between them, so the page that isn't showing should cost nothing.
    stack.set_hhomogeneous(false);
    stack.set_vhomogeneous(false);
    // The controls first, so a window opened before the probe answers shows
    // the page it will keep in the ordinary case rather than flashing an
    // explanation of an emptiness that is about to be filled.
    stack.add_named(controls, Some(CONTROLS_PAGE));

    let status = adw::StatusPage::new();
    // Compact, which is libadwaita's own answer to the page-high icon: this
    // window is 420px wide and the icon at full size fills it, which reads as
    // a catastrophe whatever the icon happens to be.
    status.add_css_class("compact");
    // Compact is libadwaita's smallest, and still larger than this page wants:
    // the icon is a label on a sentence, not the subject of the screen. The
    // rule is scoped to a class of this app's own so it cannot reach another
    // status page, and it names the image node because the size is the icon
    // theme's to choose otherwise.
    let icon_css = gtk::CssProvider::new();
    icon_css.load_from_data(&format!(
        ".{EMPTY_ICON_CLASS} image {{ -gtk-icon-size: {EMPTY_ICON_PIXELS}px; }}"
    ));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &icon_css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    status.add_css_class(EMPTY_ICON_CLASS);
    stack.add_named(&status, Some(EMPTY_PAGE));

    let detail = caption_label();
    // Selectable because the first thing asked of anyone reporting this is
    // what it said, and a screenshot of an error is worse than its text.
    detail.set_wrap(true);
    detail.set_justify(gtk::Justification::Center);
    detail.set_selectable(true);
    // Neither action asks the reader to do the app's work: it retries on its
    // own, and the report carries what it knows without anyone running a
    // command for it.

    // Title case against the app's sentence case: libadwaita spells this
    // action "Report an Issue" in the About window, one menu away.
    let report = gtk::Button::builder().label("Report an Issue").build();
    report.add_css_class("pill");
    report.add_css_class("suggested-action");
    // Quit beside it, on every one of these states: an app whose whole
    // purpose is controlling hardware has nothing to offer a machine with
    // none, so quitting is the conclusion rather than a consolation — there
    // is nothing to leave running for. The header menu carries Quit as well;
    // what the page adds is offering it where the reason for it is. Both
    // point at the same action, so there is nothing here to keep in step.
    let quit = gtk::Button::builder()
        .label("Quit")
        .action_name("app.quit")
        .build();
    quit.add_css_class("pill");
    let buttons = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .halign(gtk::Align::Center)
        .spacing(12)
        .build();
    buttons.append(&report);
    buttons.append(&quit);
    // What the app is doing about it, between the error and the buttons: a
    // page that says nothing while it waits looks like one that has given up.
    let progress = caption_label();

    // The error first: it is what the page is really saying, and the buttons
    // are what to do about it.
    let actions = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .build();
    actions.append(&detail);
    actions.append(&progress);
    actions.append(&buttons);
    status.set_child(Some(&actions));

    EmptyPage {
        stack,
        status,
        report,
        detail,
        progress,
    }
}

/// One empty page's contents. A struct rather than the tuple this grew out
/// of, so a state cannot silently take another's icon or leave a button on.
struct EmptyText {
    icon: &'static str,
    title: &'static str,
    /// What is known about the state, in the app's own words. None where the
    /// error below says it better.
    description: Option<String>,
    /// The machine's own words, verbatim and selectable.
    detail: Option<String>,
    report: bool,
}

impl EmptyPage {
    fn show(&self, reason: Empty) {
        // Only the unreachable service warns. It is the one state where the
        // app cannot do its job and the reader may be able to fix it; the
        // other two are reports about the machine, which an alert icon would
        // overstate.
        let text = match reason {
            Empty::NoHardware(vendor) => EmptyText {
                icon: "computer-symbolic",
                title: NO_HARDWARE,
                description: Some(format!(
                    "Frameguin controls the hardware of Framework laptops. \
                     This machine reports itself as “{vendor}”."
                )),
                detail: None,
                // Working as designed on someone else's laptop, so there is
                // nothing here anyone wants filed.
                report: false,
            },
            // No sentence of our own: what went wrong is the bus's to say,
            // and a paraphrase would add length without adding knowledge.
            // No instructions either — the app cannot tell a masked unit from
            // a crashed one, so anything it told the reader to run would be a
            // guess dressed as advice.
            Empty::DaemonUnavailable(e) => EmptyText {
                icon: "dialog-warning-symbolic",
                title: "frameguin-daemon isn’t answering",
                description: None,
                detail: Some(e),
                report: true,
            },
            Empty::NoControls => EmptyText {
                icon: "dialog-information-symbolic",
                title: "No controls for this board",
                description: Some(
                    "The daemon reached the hardware and found none of the operations \
                     this app offers."
                        .to_string(),
                ),
                detail: None,
                // A Framework board the probe finds nothing on is the report
                // this project asks for by name, and the report already
                // carries what a probe found.
                report: true,
            },
        };
        self.status.set_icon_name(Some(text.icon));
        self.status.set_title(text.title);
        self.status.set_description(text.description.as_deref());
        self.detail
            .set_text(text.detail.as_deref().unwrap_or_default());
        self.detail.set_visible(text.detail.is_some());
        self.report.set_visible(text.report);
        // Cleared here rather than by whoever set it: every path that changes
        // which state is shown comes through this, and a line about an
        // attempt that belongs to the state before it would be a lie the
        // reader has no way to spot.
        self.progress.set_visible(false);
        self.stack.set_visible_child_name(EMPTY_PAGE);
    }

    fn show_controls(&self) {
        self.stack.set_visible_child_name(CONTROLS_PAGE);
    }

    /// What the app is doing while the reader waits.
    fn show_progress(&self, line: &str) {
        self.progress.set_text(line);
        self.progress.set_visible(true);
    }
}

/// What filling the window takes: where an answer goes, and where a failure
/// says so. One type behind one `Rc` because both callers — the window
/// arriving on screen and the countdown that keeps asking — need all of it
/// and differ only in what made them run.
struct Init {
    ui: Rc<Ui>,
    groups: CapabilityWidgets,
    empty: EmptyPage,
    /// Whether a probe has reached the daemon, which is the answer to whether
    /// asking again could tell us anything new: one that answered will answer
    /// the same way for as long as it runs. A flag rather than the connection
    /// it used to be kept as — the feed holds the connection, and a handle
    /// stored to be tested for presence says the wrong thing about why it is
    /// there.
    answered: Cell<bool>,
    /// Set while a probe is in flight, so the two things that start one — the
    /// window mapping and the countdown reaching zero — cannot both be in it.
    probing: Cell<bool>,
    /// How long until the next unprompted attempt, doubling per failure.
    next_attempt: Cell<u32>,
    /// The pending tick, held so that arming replaces a countdown rather than
    /// racing it, and so the window going off screen can stop it.
    retry: Cell<Option<glib::SourceId>>,
}

impl Init {
    /// What the window does every time it comes to the screen. The hardware
    /// moves while the app sits in the tray — the EC's battery extender
    /// lowers the charge limit on its own, and `framework_tool` writes any of
    /// these behind the app's back — so a mapped window reloads rather than
    /// trusting what it read at startup. A window still holding no
    /// capabilities has nothing to reload and asks the daemon again instead,
    /// which is how a service that started late is picked up without the
    /// reader finding the button.
    async fn refresh(self: &Rc<Self>) {
        if !self.answered.get() {
            self.fill().await;
            return;
        }
        // Cached since the probe, so this cannot realistically fail; nothing
        // to say if it does, the next map asking again.
        let Ok(probe) = self.ui.feed.probe().await else {
            return;
        };
        // Answered, with nothing to reload: a board that supports none of
        // these controls and a machine that is not a Framework are what they
        // are, and asking again would spawn the root daemon once per window
        // opened to be told so a second time.
        if probe.is_empty() {
            return;
        }
        if let Ok(proxy) = self.ui.feed.proxy().await {
            load_values(&self.ui, &proxy, &probe).await;
        }
    }

    /// One attempt at filling the window, and what to do with how it went:
    /// only an unreachable service is worth another, and the waiting between
    /// them is this function's to arrange.
    async fn fill(self: &Rc<Self>) {
        // Guarded because two things start a probe — the window mapping and
        // the countdown reaching zero — and two runs finishing would connect
        // every setter twice, which is two writes and two prompts per change.
        if self.probing.replace(true) {
            return;
        }
        let reason = self.probe().await;
        self.probing.set(false);
        let Some(reason) = reason else {
            self.stop_retrying();
            self.next_attempt.set(FIRST_RETRY_SECONDS);
            return;
        };
        // Only an unreachable service is worth asking about again, and the
        // waiting is the app's to do: a service that starts on demand can be
        // slow, restarting, or not installed yet, and none of those are work
        // to hand to whoever opened the window. The other two states are what
        // the machine is.
        let again = matches!(reason, Empty::DaemonUnavailable(_));
        self.empty.show(reason);
        if again {
            let delay = self.next_attempt.get();
            self.next_attempt.set((delay * 2).min(MAX_RETRY_SECONDS));
            self.count_down(delay);
        }
    }

    /// Ticks the wait out a second at a time so the page can name what it is
    /// waiting for. A silent page and a page that has given up look identical
    /// — and with the delay doubling to half a minute, silence is most of
    /// what a reader would see.
    ///
    /// Arming replaces whatever was pending, so the window being opened
    /// mid-wait moves the countdown rather than starting a second one beside
    /// it, doubling the backoff twice as fast and writing two numbers into
    /// one label.
    fn count_down(self: &Rc<Self>, remaining: u32) {
        self.empty.show_progress(&format!(
            "Trying again in {remaining} {}",
            if remaining == 1 { "second" } else { "seconds" }
        ));
        let init = self.clone();
        let tick = glib::timeout_add_seconds_local_once(1, move || {
            // Finished the moment it fires: forget it before anything here
            // arms its replacement, so nothing removes a dead source.
            init.retry.set(None);
            if remaining > 1 {
                init.count_down(remaining - 1);
                return;
            }
            init.empty.show_progress("Trying again…");
            glib::spawn_future_local(async move { init.fill().await });
        });
        self.stop_retrying();
        self.retry.set(Some(tick));
    }

    /// Drops the pending tick, if there is one. Called where the wait has
    /// stopped mattering: the window leaving the screen, and the daemon
    /// answering.
    fn stop_retrying(&self) {
        if let Some(tick) = self.retry.take() {
            tick.remove();
        }
    }

    /// Asks the daemon what this board supports, hides what it can't do,
    /// loads current values, then connects the setters — last, so the
    /// programmatic `set_value` calls during init can't echo back into the
    /// daemon. Returns the state the window is left in, or None where it was
    /// filled.
    async fn probe(&self) -> Option<Empty> {
        let ui = &self.ui;
        // The page rather than a toast: a toast is gone in seconds and leaves
        // a window whose emptiness has no explanation on it.
        // Both through the feed rather than dialled and asked here: a fresh
        // handshake per window and a second cold probe per session are what a
        // window answering for itself costs, and the feed outlives any of
        // them. The probe's answer reaching it here is also what spares the
        // report a probe of its own.
        let proxy = match ui.feed.proxy().await {
            Ok(p) => p,
            Err(e) => return Some(Empty::DaemonUnavailable(e.to_string())),
        };
        let probe = match ui.feed.probe().await {
            Ok(probe) => probe,
            Err(e) => return Some(Empty::DaemonUnavailable(e.to_string())),
        };
        let caps = probe.caps;
        self.groups.show_supported(caps);
        ui.power_led.gate(probe.controls.power_led.as_ref());
        ui.touchpad.gate(probe.controls.touchpad.as_ref());
        ui.touchscreen.gate(probe.controls.touchscreen.as_ref());
        ui.sync_tray(TrayValues::offered(caps, &probe.controls));
        // Set whatever the answer was: what a later refresh needs to know is
        // that this daemon has said its piece, not what it said.
        self.answered.set(true);
        if probe.is_empty() {
            // The daemon gates its EC on the same vendor string the app
            // reads, so an empty answer is expected on anything else and says
            // nothing about the board; only a Framework answering with none
            // is a finding.
            return Some(match board::detected() {
                Some(_) => Empty::NoControls,
                None => Empty::NoHardware(board::dmi("sys_vendor")),
            });
        }
        // Back to the controls, for a run that got here after an earlier one
        // put the empty page up.
        self.empty.show_controls();
        load_values(ui, &proxy, &probe).await;
        connect_handlers(ui, &proxy, &probe.controls);
        None
    }
}

/// The Battery group's half of a reload: the reading at the top, then the
/// ceiling and the speed with their combos and sliders. Returns what the
/// tray should be told, for the one push [`load_values`] makes at the end.
async fn load_battery_values(
    ui: &Rc<Ui>,
    proxy: &FrameguinProxy<'static>,
    caps: Capabilities,
) -> TrayValues {
    let mut values = TrayValues::default();
    // The design capacity the reading below carries, kept for the charge
    // speeds further down: it is numerically the pack's 1C current, which is
    // what turns their fractions into milliamps.
    let mut design_capacity = None;
    if caps.has(Capability::Battery) {
        // Read here as well as fed: the feed's first tick is a couple of
        // seconds after the window appears, and an empty row until then reads
        // as a control that failed rather than one still filling. Through the
        // feed rather than around it, so the row and whatever else is showing
        // the pack are painted from one walk of the block — and so the tray is
        // pushed the same reading the row got rather than a second one taken a
        // moment later.
        match ui.feed.read().await {
            Ok(info) => {
                ui.show_battery_state(info.state);
                values.battery = Some(info.state);
                design_capacity = Some(info.design_capacity);
            }
            Err(e) => ui.toast_error("Reading the battery", e),
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
            Err(e) => ui.toast_error("Reading charge limit", e),
        }
    }
    if caps.has(Capability::ChargeCurrentLimit) {
        // A pack's design capacity can't change under a running app, so it is
        // read once and the labels built from it stay put.
        // Nothing to fall back to where the reading failed, and nothing that
        // should be: every rate this control offers or sends is a fraction of
        // this figure, so a second guess at it would be a second answer to
        // "how fast is full speed". The combo stays insensitive below until it
        // arrives, and the next reload asks again.
        if ui.design_capacity.get().is_none()
            && let Some(capacity) = design_capacity
        {
            ui.design_capacity.set(Some(capacity));
            let labels = with_custom_row(charge_speed_labels(capacity));
            // 1C is as fast as the pack ever asks, so a slider beyond it would
            // only offer limits that never bind. Floored to the step the value
            // rounds to, so the far end of the track is a position that sends
            // what it shows rather than a sliver that rounds back down.
            let top = (f64::from(capacity) / CUSTOM_CHARGE_STEP_MA).floor() * CUSTOM_CHARGE_STEP_MA;
            ui.sync(|| {
                ui.speed_combo.set_model(Some(&string_list(&labels)));
                ui.speed_scale
                    .adjustment()
                    .set_upper(top.max(MIN_CUSTOM_CHARGE_MA));
            });
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
            Err(e) => ui.toast_error("Reading charge speed", e),
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
async fn load_values(ui: &Rc<Ui>, proxy: &FrameguinProxy<'static>, probe: &Probe) {
    let caps = probe.caps;
    let controls = &probe.controls;
    let mut values = load_battery_values(ui, proxy, caps).await;
    if caps.has(Capability::KeyboardBacklight) {
        match proxy.get_keyboard_backlight().await {
            Ok(percent) => ui.sync(|| {
                ui.kbd_scale.set_value(f64::from(percent));
                ui.kbd_scale.set_sensitive(true);
            }),
            Err(e) => ui.toast_error("Reading keyboard backlight", e),
        }
    }
    if let Some(power_led) = &controls.power_led {
        match power_led.read().await {
            Ok(snapshot) => {
                ui.power_led.show(ui, power_led, snapshot);
                values.power_led_level = Some(snapshot.level);
            }
            Err(e) => ui.toast_error("Reading power button LED brightness", e),
        }
    }
    if let Some(touchpad) = &controls.touchpad {
        match touchpad.read().await {
            Ok(snapshot) => ui.touchpad.show(ui, snapshot),
            Err(e) => ui.toast_error("Reading the touchpad", e),
        }
    }
    if let Some(touchscreen) = &controls.touchscreen {
        match touchscreen.read().await {
            Ok(enabled) => {
                ui.touchscreen.show(ui, enabled);
                values.touchscreen = Some(enabled);
            }
            Err(e) => ui.toast_error("Reading the touchscreen", e),
        }
    }
    ui.sync_tray(values);
}
