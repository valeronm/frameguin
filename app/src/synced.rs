//! Moving a widget to match the hardware without its handler writing the
//! value back, for every window whose widgets write.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk4 as gtk;
use gtk4::glib;

/// Set while widgets are being moved to mirror the hardware; a clone is the
/// same guard.
#[derive(Clone, Default)]
pub(crate) struct Guard(Rc<Cell<bool>>);

impl Guard {
    pub(crate) fn sync(&self, update: impl FnOnce()) {
        self.0.set(true);
        update();
        self.0.set(false);
    }

    pub(crate) fn syncing(&self) -> bool {
        self.0.get()
    }
}

/// A widget holding one on-or-off value.
pub(crate) trait Switch: IsA<gtk::Widget> {
    fn is_active(&self) -> bool;
    fn set_active(&self, active: bool);
    fn connect_active_notify<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId;
}

impl Switch for adw::SwitchRow {
    fn is_active(&self) -> bool {
        adw::SwitchRow::is_active(self)
    }

    fn set_active(&self, active: bool) {
        adw::SwitchRow::set_active(self, active);
    }

    fn connect_active_notify<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        adw::SwitchRow::connect_active_notify(self, f)
    }
}

impl Switch for gtk::Switch {
    fn is_active(&self) -> bool {
        gtk::Switch::is_active(self)
    }

    fn set_active(&self, active: bool) {
        gtk::Switch::set_active(self, active);
    }

    fn connect_active_notify<F: Fn(&Self) + 'static>(&self, f: F) -> glib::SignalHandlerId {
        gtk::Switch::connect_active_notify(self, f)
    }
}

/// Made usable here because a switch is only ever filled from a read that
/// succeeded.
pub(crate) fn show_switch(guard: &Guard, switch: &impl Switch, active: bool) {
    guard.sync(|| {
        switch.set_active(active);
        switch.set_sensitive(true);
    });
}

/// A switch a sync moved must not be written back, and a switch wired here
/// cannot forget that.
pub(crate) fn connect_switch<O: 'static, C: 'static, Fut: Future<Output = ()> + 'static>(
    guard: &Guard,
    owner: &Rc<O>,
    control: &Rc<C>,
    switch: &impl Switch,
    write: impl Fn(Rc<O>, Rc<C>, bool) -> Fut + 'static,
) {
    let (guard, owner, control) = (guard.clone(), owner.clone(), control.clone());
    switch.connect_active_notify(move |switch| {
        if guard.syncing() {
            return;
        }
        glib::spawn_future_local(write(owner.clone(), control.clone(), switch.is_active()));
    });
}

/// `StringList::new` takes borrowed strs, where labels arrive as owned
/// strings.
pub(crate) fn string_list(labels: &[String]) -> gtk::StringList {
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    gtk::StringList::new(&labels)
}

/// Positions index fixed arrays, so the fallback is never taken.
fn combo_index(position: usize) -> u32 {
    u32::try_from(position).unwrap_or(gtk::INVALID_LIST_POSITION)
}

/// The row to select, where the hardware may be sitting on none of them —
/// GTK spells "nothing selected" as a sentinel position rather than as an
/// absent one.
pub(crate) fn combo_selection(position: Option<usize>) -> u32 {
    position.map_or(gtk::INVALID_LIST_POSITION, combo_index)
}

/// The inverse of [`combo_selection`]: the row a combo reports, and None for
/// the sentinel GTK uses where it sits on none.
pub(crate) fn combo_position(selected: u32) -> Option<usize> {
    (selected != gtk::INVALID_LIST_POSITION).then_some(selected as usize)
}

/// A row a sync moved the combo to must not be written back, and a combo
/// wired here cannot forget that.
pub(crate) fn connect_combo<O: 'static, C: 'static, T, Fut: Future<Output = ()> + 'static>(
    guard: &Guard,
    owner: &Rc<O>,
    control: &Rc<C>,
    combo: &adw::ComboRow,
    at: impl Fn(usize) -> Option<T> + 'static,
    write: impl Fn(Rc<O>, Rc<C>, T) -> Fut + 'static,
) {
    let (guard, owner, control) = (guard.clone(), owner.clone(), control.clone());
    combo.connect_selected_notify(move |row| {
        if guard.syncing() {
            return;
        }
        if let Some(value) = combo_position(row.selected()).and_then(&at) {
            glib::spawn_future_local(write(owner.clone(), control.clone(), value));
        }
    });
}

/// The button takes no second press while the write it started is in flight.
pub(crate) fn connect_button<O: 'static, C: 'static, Fut: Future<Output = ()> + 'static>(
    owner: &Rc<O>,
    control: &Rc<C>,
    button: &gtk::Button,
    write: impl Fn(Rc<O>, Rc<C>) -> Fut + 'static,
) {
    let (owner, control) = (owner.clone(), control.clone());
    button.connect_clicked(move |button| {
        button.set_sensitive(false);
        let (pressed, write) = (button.downgrade(), write(owner.clone(), control.clone()));
        glib::spawn_future_local(async move {
            write.await;
            if let Some(button) = pressed.upgrade() {
                button.set_sensitive(true);
            }
        });
    });
}
