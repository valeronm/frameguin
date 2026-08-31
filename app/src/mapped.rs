//! Work that lasts exactly as long as a widget is on screen.
//!
//! A resident app whose window is hidden to the tray does no periodic work,
//! and neither does one whose board lacks the row — an unsupported widget is
//! never mapped. Every timer and subscription in this app obeys that rule,
//! and the rule is written here once: what is held is the caller's, when it
//! is taken and when it is let go is not.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;

/// Holds what `acquire` returns for as long as `widget` is mapped, and drops
/// it as the widget goes.
///
/// Dropping is the whole contract: whatever `acquire` hands back must stop
/// what it started when it falls, which is why a timer is wrapped in [`Timer`]
/// rather than passed around as the bare `SourceId` that does not.
pub(crate) fn while_mapped<G: 'static>(
    widget: &impl IsA<gtk::Widget>,
    acquire: impl Fn() -> G + 'static,
) {
    let held: Rc<RefCell<Option<G>>> = Rc::default();
    // Taken before the handlers are wired rather than after: a widget is
    // usually on screen already by the time anything is attached to it, the
    // reads that fill it being async, so map won't fire for the visibility it
    // currently has. Done here, `acquire` can then simply move into the
    // handler below.
    if widget.as_ref().is_mapped() {
        held.replace(Some(acquire()));
    }
    let map_held = held.clone();
    widget.as_ref().connect_map(move |_| {
        // The replacement is taken before what it replaces is dropped, so a
        // remap never passes through nothing held — which for a subscription
        // is what keeps the feed from tearing its timer down and arming
        // another.
        map_held.replace(Some(acquire()));
    });
    widget.as_ref().connect_unmap(move |_| {
        held.replace(None);
    });
}

/// A repeating timer that stops when it is dropped, which is what lets a
/// holder forget it rather than remove it: a `glib::SourceId` removes
/// nothing on its own, and one let fall is a tick outliving whatever it fed.
pub(crate) struct Timer(Option<glib::SourceId>);

impl Timer {
    pub(crate) fn every_seconds(seconds: u32, tick: impl Fn() + 'static) -> Self {
        Timer(Some(glib::timeout_add_seconds_local(seconds, move || {
            tick();
            glib::ControlFlow::Continue
        })))
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if let Some(source) = self.0.take() {
            source.remove();
        }
    }
}

/// A one-shot timer that stops when it is dropped before firing, which is
/// what makes a `Cell<Option<Once>>` a debounce slot: arming it drops
/// whatever was pending, and with it stops. The callback forgets the id
/// before it runs, so a source that has fired is never removed — removing
/// one is a panic, and a holder cannot tell a fired source from a pending
/// one.
pub(crate) struct Once(Rc<Cell<Option<glib::SourceId>>>);

impl Once {
    pub(crate) fn after(delay: Duration, fire: impl FnOnce() + 'static) -> Self {
        let pending: Rc<Cell<Option<glib::SourceId>>> = Rc::default();
        let fired = pending.clone();
        let source = glib::timeout_add_local_once(delay, move || {
            fired.set(None);
            fire();
        });
        pending.set(Some(source));
        Once(pending)
    }
}

impl Drop for Once {
    fn drop(&mut self) {
        if let Some(source) = self.0.take() {
            source.remove();
        }
    }
}
