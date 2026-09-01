//! The machine's reading, taken once however many windows are showing it.
//!
//! The window's status row and the battery report render the same walk of the
//! EC's battery block; the charger row and the ports report render the same
//! walk of the USB-C ports. Polled per window that is one read twice on two
//! schedules — an EC paying twice for one answer, and two windows that can sit
//! a tick apart on a figure neither of them owns. So a view says what it wants
//! shown and the feed does the reading: one timer, one call, every view fed
//! from the same answer.
//!
//! That holds for a window filling itself too, which is why [`Feed::read`] is
//! what fills one rather than a read beside it: a fill is broadcast like any
//! tick, so opening a window cannot leave another showing what it saw before.
//!
//! Nothing here needs a pack. The block is an absent extra on a board with
//! none, the way a failed read is, and the ports are still read.
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

use frameguin_wire::{BatteryCondition, BatteryInfo, DeviceResult, PortState};
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;

use crate::daemon::Daemon;
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
    /// The EC's battery block.
    pub(crate) battery: bool,
    pub(crate) condition: bool,
    /// The USB-C ports. One call however many there are, and no EC transfer
    /// past its own host commands, so this rides the base cadence rather
    /// than being spaced the way the condition is.
    pub(crate) ports: bool,
}

impl Wants {
    /// What the feed must read to satisfy every view at once.
    fn with(self, other: Self) -> Self {
        Self {
            battery: self.battery || other.battery,
            condition: self.condition || other.condition,
            ports: self.ports || other.ports,
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
    /// None on a board with no pack, which is a machine this feed still
    /// serves: the USB-C ports are read whether or not one answered.
    pub(crate) info: Option<BatteryInfo>,
    pub(crate) condition: Option<BatteryCondition>,
    pub(crate) ports: Option<Vec<PortState>>,
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

/// One read's place in [`Feed::reading`], counted for as long as it is held.
struct InFlight<'a>(&'a Cell<u32>);

impl<'a> InFlight<'a> {
    fn enter(count: &'a Cell<u32>) -> Self {
        count.set(count.get() + 1);
        Self(count)
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

pub(crate) struct Feed {
    daemon: Rc<Daemon>,
    views: RefCell<Vec<(u64, View)>>,
    next_id: Cell<u64>,
    /// The pending tick. Armed as the first view arrives and dropped as the
    /// last one goes — a [`Timer`] rather than the bare source id it wraps, so
    /// stopping is what dropping it does rather than something the code
    /// removing it has to remember.
    timer: Cell<Option<Timer>>,
    /// How many reads are in flight. The daemon runs these blocking against
    /// one executor thread, so a tick arriving behind one would queue rather
    /// than overtake it, and the two would land as a burst.
    ///
    /// A count rather than a flag because reads nest: a fill never skips, so
    /// one can start while a tick is still going, and a flag the first to
    /// finish cleared would unlock the tick behind it.
    reading: Cell<u32>,
    /// Reads taken, for spacing the ones that cost more than a memmap walk —
    /// see [`CONDITION_EVERY`]. Rewound whenever a view arrives, so what it
    /// spaces is repetition and never a window's first sight of anything.
    ticks: Cell<u32>,
}

impl Feed {
    pub(crate) fn new(daemon: Rc<Daemon>) -> Self {
        Self {
            daemon,
            views: RefCell::default(),
            next_id: Cell::default(),
            timer: Cell::default(),
            reading: Cell::default(),
            ticks: Cell::default(),
        }
    }

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

    fn arm(self: &Rc<Self>) {
        let feed = self.clone();
        self.timer
            .set(Some(Timer::every_seconds(READING_SECONDS, move || {
                // Skipped while any read is going, a window's fill included.
                // Tested before the task is spawned rather than inside it:
                // `read` takes its place in the count before its first
                // suspension, so there is nothing an async context would see
                // that this does not.
                if feed.reading.get() > 0 {
                    return;
                }
                let feed = feed.clone();
                glib::spawn_future_local(async move {
                    let _ = feed.read().await;
                });
            })));
    }

    /// Takes one reading, shows it on every view, and hands it back.
    ///
    /// Reads what the subscribed views want and nothing else, so a window
    /// filling itself subscribes first.
    ///
    /// Returned as well as broadcast, so a fill is the feed's own read rather
    /// than a second assembly beside it — and so any fill refreshes every
    /// other view at the same instant rather than leaving them a tick behind.
    ///
    /// The error is for a caller filling a window, which is the one placed to
    /// say so, and it is only ever raised for something a view asked for —
    /// so a report's toast names what that report was reading. The tick above
    /// drops it, silence being the rule for a read with a successor seconds
    /// behind it. A device the board does not have is not a failure: its
    /// field arrives as None, as one nobody asked for does.
    pub(crate) async fn read(&self) -> DeviceResult<Reading> {
        // Counted for the length of the read, however it leaves: the `?`s
        // below are why this is a guard rather than a pair of writes, a read
        // that returned early having otherwise locked the tick out for the
        // rest of the process.
        let _in_flight = InFlight::enter(&self.reading);
        let controls = self.daemon.controls().await?;
        let wants = self
            .views
            .borrow()
            .iter()
            .fold(Wants::default(), |wants, (_, view)| wants.with(view.wants));
        let battery = controls.battery.as_ref();
        let info = match battery.filter(|_| wants.battery) {
            Some(battery) => Some(battery.read().await?),
            None => None,
        };
        // Every read wants the block; the condition only on the reads that come
        // round to it. Subscribing rewinds the count, so the fill that follows
        // a view arriving is always one of them and the spacing only applies
        // to the repeats after it.
        let ticks = self.ticks.get();
        self.ticks.set(ticks.wrapping_add(1));
        let condition =
            match battery.filter(|_| wants.condition && ticks.is_multiple_of(CONDITION_EVERY)) {
                Some(battery) => battery.condition().await.ok(),
                None => None,
            };
        // Asked of the ports control rather than the pack's: a board can have
        // one and not the other. Its failure is raised rather than swallowed,
        // unlike the condition's, because a window showing only the ports has
        // nothing else for its toast to be about.
        let ports = match controls.ports.as_ref().filter(|_| wants.ports) {
            Some(ports) => Some(ports.read().await?),
            None => None,
        };
        let reading = Reading {
            info,
            condition,
            ports,
        };
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
        Ok(reading)
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
