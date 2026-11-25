//! The Preferences switch allowing what a slip could leave hard to undo — an
//! LED, a USB-C port or the input deck turned off — and the locks it holds
//! on the widgets that would do it while it is off.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk4 as gtk;
use gtk4::glib;

use crate::mapped::while_mapped;
use crate::synced::combo_position;

/// What a locked widget says in place of its own description.
pub(crate) const LOCKED: &str = "Turning this off is locked in Preferences";

const GROUP: &str = "preferences";
const KEY: &str = "allow-dangerous";

fn path() -> PathBuf {
    glib::user_config_dir().join("frameguin/preferences.ini")
}

type Follower = dyn Fn(bool);

pub(crate) struct Dangerous {
    allowed: Cell<bool>,
    /// Weak: a follower is held by the widget it moves, while that is mapped.
    followers: RefCell<Vec<Weak<Follower>>>,
}

impl Dangerous {
    /// A file that is missing or unreadable allows nothing.
    pub(crate) fn load() -> Self {
        let file = glib::KeyFile::new();
        let allowed = file
            .load_from_file(path(), glib::KeyFileFlags::NONE)
            .and_then(|()| file.boolean(GROUP, KEY))
            .unwrap_or(false);
        Self {
            allowed: Cell::new(allowed),
            followers: RefCell::default(),
        }
    }

    pub(crate) fn allowed(&self) -> bool {
        self.allowed.get()
    }

    /// Moves every widget on screen before the file is written, so a failed
    /// write costs the next run's value and not this one's.
    pub(crate) fn set(&self, allowed: bool) -> std::io::Result<()> {
        self.allowed.set(allowed);
        let followers: Vec<Rc<Follower>> = {
            let mut followers = self.followers.borrow_mut();
            followers.retain(|follower| follower.strong_count() > 0);
            followers.iter().filter_map(Weak::upgrade).collect()
        };
        for follower in followers {
            follower(allowed);
        }
        let path = path();
        let file = glib::KeyFile::new();
        // A file that will not load is replaced by one holding this key.
        let _ = file.load_from_file(&path, glib::KeyFileFlags::KEEP_COMMENTS);
        file.set_boolean(GROUP, KEY, allowed);
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory)?;
        }
        fs::write(path, file.to_data())
    }

    /// Calls `apply` with the value whenever `widget` comes on screen, and on
    /// every change while it is there.
    pub(crate) fn follow(
        self: &Rc<Self>,
        widget: &impl IsA<gtk::Widget>,
        apply: impl Fn(bool) + 'static,
    ) {
        let (this, apply) = (self.clone(), Rc::new(apply));
        while_mapped(widget, move || {
            apply(this.allowed.get());
            let apply = apply.clone();
            let held: Rc<Follower> = Rc::new(move |allowed| apply(allowed));
            let mut followers = this.followers.borrow_mut();
            followers.retain(|follower| follower.strong_count() > 0);
            followers.push(Rc::downgrade(&held));
            held
        });
    }

    /// Draws the rows `dangerous` answers for dimmed and unpickable while
    /// they are not allowed, with [`LOCKED`] as the combo's subtitle while it
    /// sits on a row that is allowed. A combo already on a locked row stays
    /// there. The combo's handler still has to refuse such a row.
    pub(crate) fn lock_rows(
        self: &Rc<Self>,
        combo: &adw::ComboRow,
        dangerous: impl Fn(usize) -> bool + 'static,
    ) {
        let dangerous = Rc::new(dangerous);
        let factory = {
            let dangerous = dangerous.clone();
            row_factory(combo, move |row| dangerous(row))
        };
        let (this, locked, drawn) = (self.clone(), combo.downgrade(), Cell::new(None));
        let settle: Rc<dyn Fn()> = Rc::new(move || {
            let Some(combo) = locked.upgrade() else {
                return;
            };
            let rows = combo.model().map_or(0, |model| model.n_items());
            let locks = !this.allowed.get()
                && (0..rows)
                    .filter_map(combo_position)
                    .any(|row| dangerous(row));
            if drawn.replace(Some(locks)) != Some(locks) {
                combo.set_list_factory(locks.then_some(&factory));
            }
            let on_locked = combo_position(combo.selected()).is_some_and(|row| dangerous(row));
            combo.set_subtitle(if locks && !on_locked { LOCKED } else { "" });
        });
        let moved = settle.clone();
        combo.connect_selected_notify(move |_| moved());
        self.follow(combo, move |_| settle());
    }
}

/// The check follows the combo's own selection: the popup's follows the
/// pointer.
fn row_factory(
    combo: &adw::ComboRow,
    locked: impl Fn(usize) -> bool + 'static,
) -> gtk::SignalListItemFactory {
    let factory = gtk::SignalListItemFactory::new();
    let combo = combo.downgrade();
    let checks: Rc<RefCell<HashMap<gtk::ListItem, glib::SignalHandlerId>>> = Rc::default();
    factory.connect_setup(|_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        line.append(&gtk::Label::builder().xalign(0.0).hexpand(true).build());
        line.append(&gtk::Image::from_icon_name("object-select-symbolic"));
        item.set_child(Some(&line));
    });
    let (bound, checked) = (combo.clone(), checks.clone());
    factory.connect_bind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        let (Some(combo), Some(line)) = (bound.upgrade(), item.child()) else {
            return;
        };
        let text = item.item().and_downcast::<gtk::StringObject>();
        if let (Some(label), Some(text)) = (line.first_child().and_downcast::<gtk::Label>(), text) {
            label.set_label(&text.string());
        }
        let open = !combo_position(item.position()).is_some_and(&locked);
        item.set_selectable(open);
        item.set_activatable(open);
        line.set_sensitive(open);
        let Some(check) = line.last_child() else {
            return;
        };
        let position = item.position();
        let show = move |combo: &adw::ComboRow| {
            check.set_opacity(if combo.selected() == position {
                1.0
            } else {
                0.0
            });
        };
        show(&combo);
        let handler = combo.connect_selected_notify(show);
        checked.borrow_mut().insert(item.clone(), handler);
    });
    factory.connect_unbind(move |_, item| {
        let Some(item) = item.downcast_ref::<gtk::ListItem>() else {
            return;
        };
        if let (Some(combo), Some(handler)) = (combo.upgrade(), checks.borrow_mut().remove(item)) {
            combo.disconnect(handler);
        }
    });
    factory
}
