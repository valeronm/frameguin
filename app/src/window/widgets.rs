//! The chrome the groups share: sliders, combos, and the sync guard,
//! debounce and spawn their writes go through — a handler answers with the
//! write as a future and spawns nothing for itself.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk4 as gtk;
use gtk4::gdk;
use gtk4::glib;

use super::Ui;

/// A row that shows one value and opens a report, with the label carrying
/// the value and the chevron saying it leads somewhere.
///
/// Named as an action rather than wired to a handler, so a row here and the
/// tray's line reach the same report the same way and neither has to hold a
/// bus connection to offer it.
pub(crate) fn report_row(title: &str, action: &str) -> (adw::ActionRow, gtk::Label) {
    let row = adw::ActionRow::builder()
        .title(title)
        .activatable(true)
        .action_name(format!("app.{action}"))
        .build();
    let value = gtk::Label::new(None);
    row.add_suffix(&value);
    row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    (row, value)
}
use crate::mapped::Once;

/// GTK carries adjustment values as f64. The cast alone saturates at 255, so
/// the clamp is what holds the result inside the range the daemon accepts;
/// each control's own floor is enforced by its adjustment.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped into range before the cast"
)]
pub(super) fn scale_percent(value: f64) -> u8 {
    value.round().clamp(0.0, 100.0) as u8
}

/// The chrome every slider shares, so a change to how one reads doesn't have
/// to be made once per slider. `format` renders the value in the control's
/// own unit, which is the only part that differs.
pub(super) fn build_scale(
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

/// A `StringList` from owned labels, which every combo builds from a
/// `Vec<String>` that GTK will only take as borrowed strs.
pub(super) fn string_list(labels: &[String]) -> gtk::StringList {
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
pub(super) fn combo_selection(position: Option<usize>) -> u32 {
    position.map_or(gtk::INVALID_LIST_POSITION, combo_index)
}

/// The inverse of [`combo_selection`]: the row a combo reports, and None for
/// the sentinel GTK uses where it sits on none.
fn combo_position(selected: u32) -> Option<usize> {
    (selected != gtk::INVALID_LIST_POSITION).then_some(selected as usize)
}

/// Moves a combo to the row `row_for` names. The model's answer weighs where
/// the combo sits against what moved it, so the read has to happen before the
/// move — which is why both ends are here rather than at each call site.
pub(super) fn select_row(
    combo: &adw::ComboRow,
    row_for: impl FnOnce(Option<usize>) -> Option<usize>,
) {
    let row = row_for(combo_position(combo.selected()));
    combo.set_selected(combo_selection(row));
}

/// A row a sync moved the combo to must not be written back, and a combo
/// wired here cannot forget that.
pub(super) fn connect_combo<C: 'static, T, Fut: Future<Output = ()> + 'static>(
    ui: &Rc<Ui>,
    control: &Rc<C>,
    combo: &adw::ComboRow,
    at: impl Fn(usize) -> Option<T> + 'static,
    write: impl Fn(Rc<Ui>, Rc<C>, T) -> Fut + 'static,
) {
    let (ui, control) = (ui.clone(), control.clone());
    combo.connect_selected_notify(move |row| {
        if ui.syncing.get() {
            return;
        }
        if let Some(value) = combo_position(row.selected()).and_then(&at) {
            glib::spawn_future_local(write(ui.clone(), control.clone(), value));
        }
    });
}

/// A switch a sync moved must not be written back, and a switch wired here
/// cannot forget that.
pub(super) fn connect_switch<C: 'static, Fut: Future<Output = ()> + 'static>(
    ui: &Rc<Ui>,
    control: &Rc<C>,
    switch: &adw::SwitchRow,
    write: impl Fn(Rc<Ui>, Rc<C>, bool) -> Fut + 'static,
) {
    let (ui, control) = (ui.clone(), control.clone());
    switch.connect_active_notify(move |row| {
        if ui.syncing.get() {
            return;
        }
        glib::spawn_future_local(write(ui.clone(), control.clone(), row.is_active()));
    });
}

/// Shows `row` exactly while `combo` sits at `index`, so a slider a combo
/// row reveals is never left showing under another row, whoever moved the
/// combo.
pub(super) fn reveal_under(combo: &adw::ComboRow, row: &impl IsA<gtk::Widget>, index: usize) {
    combo
        .bind_property("selected", row, "visible")
        .transform_to(move |_, selected: u32| Some(combo_position(selected) == Some(index)))
        .sync_create()
        .build();
}

/// When a slider's value reaches the hardware.
#[derive(Clone, Copy)]
pub(super) enum SliderWrites {
    /// When a drag ends, keys and the wheel settling on a longer debounce:
    /// the control shows nothing while it changes, and every value passed
    /// through would be one more authorized EC write.
    OnRelease,
    /// As it moves, on a short debounce: the control shows every value, so
    /// the passes are the point.
    Live,
}

impl SliderWrites {
    fn delay(self) -> Duration {
        match self {
            Self::OnRelease => Duration::from_millis(700),
            Self::Live => Duration::from_millis(200),
        }
    }
}

/// Wires a slider to `write`, under the `writes` policy. `read` turns the
/// slider's position into the value that gets written, and is also what
/// decides whether a drag moved at all — comparing positions would count a
/// nudge that rounds back to where it started.
pub(super) fn connect_slider_writes<
    C: 'static,
    T: Copy + PartialEq + 'static,
    Fut: Future<Output = ()> + 'static,
>(
    ui: &Rc<Ui>,
    control: &Rc<C>,
    scale: &gtk::Scale,
    read: impl Fn(f64) -> T + 'static,
    write: impl Fn(Rc<Ui>, Rc<C>, T) -> Fut + 'static,
    writes: SliderWrites,
) {
    let read = Rc::new(read);
    let write = {
        let (ui, control) = (ui.clone(), control.clone());
        Rc::new(move |value: T| {
            glib::spawn_future_local(write(ui.clone(), control.clone(), value));
        })
    };
    let dragging: Rc<Cell<Option<T>>> = Rc::default();

    let slot: Rc<Cell<Option<Once>>> = Rc::default();
    let keys_ui = ui.clone();
    let keys_dragging = dragging.clone();
    let (keys_read, keys_write) = (read.clone(), write.clone());
    scale.connect_value_changed(move |scale| {
        if keys_ui.syncing.get() || keys_dragging.get().is_some() {
            return;
        }
        let value = keys_read(scale.value());
        let write = keys_write.clone();
        slot.set(Some(Once::after(writes.delay(), move || write(value))));
    });
    if matches!(writes, SliderWrites::Live) {
        return;
    }

    // Raw events, not a gesture: the scale's own drag gesture claims the
    // pointer sequence, which cancels any competing gesture instead of
    // releasing it — so a GestureClick here would see the press and never the
    // release, and the drag would never end.
    let drag = gtk::EventControllerLegacy::new();
    drag.set_propagation_phase(gtk::PropagationPhase::Capture);
    let drag_scale = scale.clone();
    drag.connect_event(move |_, event| {
        match event.event_type() {
            gdk::EventType::ButtonPress | gdk::EventType::TouchBegin => {
                dragging.set(Some(read(drag_scale.value())));
            }
            gdk::EventType::ButtonRelease
            | gdk::EventType::TouchEnd
            | gdk::EventType::TouchCancel => {
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
