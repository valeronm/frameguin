//! The pack's reading, taken once however many windows are showing it.
//!
//! The window's status row and the battery report render the same walk of the
//! EC's battery block. Polled per window that is one pack read twice on two
//! schedules — an EC paying twice for one answer, and two windows that can sit
//! a tick apart on a figure neither of them owns. So a view says what it wants
//! shown and the feed does the reading: one timer, one call, every view fed
//! from the same answer.
//!
//! The reading is always the whole block, never the summary the status row
//! alone would need. The daemon walks the same memmap for either, and the two
//! figures it reaches past the memmap for are asked of the pack once per
//! daemon run and remembered — so the wide call costs a few strings of
//! marshalling, where the narrow one would cost a branch that can be wrong,
//! and being wrong here means a report showing a row it did not read.
//!
//! The timer exists only while something is subscribed, which is what keeps a
//! window hidden to the tray costing nothing: a view subscribes as it is
//! mapped and its subscription goes with it.
//!
//! Silent on a failed read, the rule every poll in this app follows. The read
//! that *fills* a window is the one placed to announce a failure, and it is
//! that window's own; a tick has a successor a couple of seconds behind it and
//! nothing worth burying the screen for.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use frameguin_model::control::Controls;
use frameguin_wire::{BatteryCondition, BatteryInfo, DeviceError, DeviceResult, FrameguinProxy};
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;

use crate::bus::Bus;
use crate::mapped::{Timer, while_mapped};

/// How often the block is read while anything is showing it. One rate for
/// every view now that they share the read: the status row's, which is the
/// pace a charge current settling after a limit engages is worth watching at.
const READING_SECONDS: u32 = 2;

/// How many of those ticks pass between reads of what the pack says about
/// itself.
///
/// The block above is one walk of a memory-mapped region; the condition is a
/// transfer per cell plus two, each an EC host command carrying an `SMBus` pair
/// to a gauge the EC is itself polling, on the one thread every other call to
/// the daemon queues behind. Nothing it carries — a temperature to a tenth of
/// a degree, a spread in millivolts, an alarm set — moves visibly in two
/// seconds, so it inherits the block's cadence for no reason but proximity.
/// A view still gets one immediately: what this spaces out is the repeat.
const CONDITION_EVERY: u32 = 5;

/// What a view wants read for it beyond the block every reading carries.
///
/// A set rather than a flag apiece in the signatures below: each extra is its
/// own call to the daemon, over a connection whose calls block one another, so
/// a view showing none of them must cost none of them — and a third extra
/// should be a field here rather than another parameter everywhere.
#[derive(Clone, Copy, Default)]
pub(crate) struct Wants {
    pub(crate) condition: bool,
}

impl Wants {
    /// What the feed must read to satisfy every view at once.
    fn with(self, other: Self) -> Self {
        Self {
            condition: self.condition || other.condition,
        }
    }
}

/// One reading, as every view on screen sees it.
///
/// An extra is None where nothing asked for it and where the ask failed alike.
/// The two are the same to a view: it shows what arrived and leaves the rest of
/// the window standing, which is what keeps a row from emptying over one
/// unlucky transfer.
pub(crate) struct Reading {
    pub(crate) info: BatteryInfo,
    pub(crate) condition: Option<BatteryCondition>,
}

type Show = dyn Fn(&Reading);

/// Where one view wants the reading put, and what it needs read for it.
struct View {
    wants: Wants,
    /// Behind an `Rc` so a tick can take a copy and let go of the list before
    /// showing anything: a view is free to close its window, and so to
    /// unsubscribe, from inside the call that shows it.
    show: Rc<Show>,
}

/// A view's place in the feed, held for as long as it wants to be shown.
/// Dropping it is what unsubscribes, so a subscription tied to a widget's
/// mapping cannot outlive it and no view has to remember to unregister.
///
/// Addressed by id rather than by the identity of its callback: a widget being
/// re-mapped takes its new subscription before dropping the one it replaces,
/// so the same callback is briefly listed twice and removal has to name which.
struct Subscription {
    feed: Rc<Feed>,
    id: u64,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.feed.unsubscribe(self.id);
    }
}

#[derive(Default)]
pub(crate) struct Feed {
    /// Built on demand and kept: the feed outlives any one window, so a report
    /// opened, closed and opened again costs one connection rather than one
    /// apiece — and a session that only ever shows the tray opens none.
    bus: RefCell<Option<Rc<Bus>>>,
    /// The controls detected, asked once. Fixed for the daemon's run, and
    /// the cold call — so the report reopening pays for one, not one per
    /// open. None until asked; a failed ask is not remembered.
    controls: RefCell<Option<Rc<Controls<Bus>>>>,
    views: RefCell<Vec<(u64, View)>>,
    next_id: Cell<u64>,
    /// The pending tick. Armed as the first view arrives and dropped as the
    /// last one goes — a [`Timer`] rather than the bare source id it wraps, so
    /// stopping is what dropping it does rather than something the code
    /// removing it has to remember.
    timer: Cell<Option<Timer>>,
    /// Set while a read is in flight. The daemon runs these blocking against
    /// one executor thread, so a tick arriving behind a slow one would queue
    /// rather than overtake it, and the two would land as a burst.
    reading: Cell<bool>,
    /// Reads taken, for spacing the ones that cost more than a memmap walk —
    /// see [`CONDITION_EVERY`]. Rewound whenever a view arrives, so what it
    /// spaces is repetition and never a window's first sight of anything.
    ticks: Cell<u32>,
}

impl Feed {
    /// Registers a view, and starts the timer where this is the first.
    ///
    /// Takes no reading of its own: whoever subscribes has just filled itself,
    /// and that read is the one placed to say so when it fails.
    fn subscribe(self: &Rc<Self>, wants: Wants, show: Rc<Show>) -> Subscription {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        // The next read serves a view that has seen nothing yet, so let it be
        // a full one — where there is anything spaced for it to see. A window
        // returning from the tray asks for no extra, and would otherwise buy
        // one it cannot show.
        if wants.condition {
            self.ticks.set(0);
        }
        let first = {
            let mut views = self.views.borrow_mut();
            views.push((id, View { wants, show }));
            views.len() == 1
        };
        if first {
            self.arm();
        }
        Subscription {
            feed: self.clone(),
            id,
        }
    }

    fn unsubscribe(&self, id: u64) {
        let empty = {
            let mut views = self.views.borrow_mut();
            views.retain(|(view_id, _)| *view_id != id);
            views.is_empty()
        };
        if empty {
            self.timer.set(None);
        }
    }

    /// The daemon's root interface, on the app's one connection: dialling the
    /// bus runs a fresh handshake each time, and the feed outlives any one
    /// window, so a report opened, closed and opened again costs none.
    pub(crate) async fn proxy(&self) -> zbus::Result<FrameguinProxy<'static>> {
        Ok(self.bus().await?.frameguin.clone())
    }

    /// The bus as every control reaches it, on the one connection.
    pub(crate) async fn bus(&self) -> zbus::Result<Rc<Bus>> {
        let held = self.bus.borrow().clone();
        if let Some(bus) = held {
            return Ok(bus);
        }
        let bus = Rc::new(Bus::connect().await?);
        // Whatever landed in the slot while this one was dialling wins, rather
        // than being overwritten: two first callers racing would otherwise
        // leave the loser's connection held by whoever it was handed to, and
        // the app would keep both sockets for the rest of the run.
        Ok(self.bus.borrow_mut().get_or_insert(bus).clone())
    }

    /// The controls whose devices detected themselves, asked once for the
    /// daemon's run and shared by every window so a control is one object
    /// however many views reach it. A failure is not remembered — detection
    /// is the cold call, and caching one unlucky answer would hold the app
    /// to it for the session.
    pub(crate) async fn controls(&self) -> DeviceResult<Rc<Controls<Bus>>> {
        let held = self.controls.borrow().clone();
        if let Some(controls) = held {
            return Ok(controls);
        }
        let bus = self.bus().await?;
        // No re-check for a racing asker, unlike `bus`: two of them compute
        // the same answer from the same detection, and a control holds
        // nothing a second copy would be left owning.
        let controls = Rc::new(Controls::detect(&bus).await?);
        self.controls.replace(Some(controls.clone()));
        Ok(controls)
    }

    fn arm(self: &Rc<Self>) {
        let feed = self.clone();
        self.timer
            .set(Some(Timer::every_seconds(READING_SECONDS, move || {
                let feed = feed.clone();
                glib::spawn_future_local(async move {
                    // The guard is the tick's, not the read's: the daemon runs
                    // these blocking against one executor thread, so a tick
                    // arriving behind a slow one would queue rather than overtake
                    // it and the two would land as a burst. A window filling
                    // itself is a one-off and waits for nothing.
                    if feed.reading.replace(true) {
                        return;
                    }
                    let _ = feed.read().await;
                    feed.reading.set(false);
                });
            })));
    }

    /// Takes one reading, shows it on every view, and hands it back.
    ///
    /// Returned as well as broadcast for the caller that needs the block in
    /// hand rather than on screen — the window fills its status row from the
    /// same read it pushes to the tray, so neither is a second reading taken a
    /// moment apart from the other.
    ///
    /// The error is for a caller filling a window, which is the one placed to
    /// say so; the tick above drops it, silence being the rule for a read with
    /// a successor seconds behind it. Only the block itself can fail the call:
    /// an extra that fails arrives as None, and the row keeps what it last
    /// showed rather than emptying over a single miss.
    pub(crate) async fn read(&self) -> DeviceResult<BatteryInfo> {
        let controls = self.controls().await?;
        let battery = controls
            .battery
            .as_ref()
            // Not `Absent`, which is the bus's alone to raise: every view here
            // hangs off a pack that answered, so this is a caller's mistake
            // rather than a device's answer.
            .ok_or_else(|| DeviceError::Failed("no battery on this board".into()))?;
        let info = battery.read().await?;
        let wants = self
            .views
            .borrow()
            .iter()
            .fold(Wants::default(), |wants, (_, view)| wants.with(view.wants));
        // Every read wants the block; the condition only on the reads that come
        // round to it. Subscribing rewinds the count, so the fill that follows
        // a view arriving is always one of them and the spacing only applies
        // to the repeats after it.
        let ticks = self.ticks.get();
        self.ticks.set(ticks.wrapping_add(1));
        let condition = if wants.condition && ticks.is_multiple_of(CONDITION_EVERY) {
            battery.condition().await.ok()
        } else {
            None
        };
        let reading = Reading { info, condition };
        // Copied out of the list before anything is shown: a view may drop its
        // subscription from inside its own call, and the borrow would still be
        // held when it did.
        let showing: Vec<_> = self
            .views
            .borrow()
            .iter()
            .map(|(_, view)| view.show.clone())
            .collect();
        for show in showing {
            show(&reading);
        }
        Ok(reading.info)
    }
}

/// Shows the reading on a view for as long as `widget` is on screen, the
/// subscription being what [`while_mapped`] holds.
pub(crate) fn show_while_mapped(
    feed: &Rc<Feed>,
    widget: &impl IsA<gtk::Widget>,
    wants: Wants,
    show: impl Fn(&Reading) + 'static,
) {
    let show: Rc<Show> = Rc::new(show);
    let feed = feed.clone();
    while_mapped(widget, move || feed.subscribe(wants, show.clone()));
}
