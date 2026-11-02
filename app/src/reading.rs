//! The machine's reading, taken once however many windows are showing it.
//!
//! The main window's battery row and the Readings window's battery page render
//! the same walk of the EC's battery block; the charger row and the ports
//! pages render the same walk of the USB-C ports. Polled per window that is
//! one read twice on two schedules — an EC paying twice for one answer, and
//! two windows that can sit a tick apart on a figure neither of them owns. So
//! a view says what it wants shown and the feed does the reading: one timer,
//! one call, every view fed from the same answer.
//!
//! That holds for a window filling itself too, which is why [`Feed::fill`]
//! takes the feed's own read rather than one beside it: a fill is broadcast
//! like any tick, so opening a window cannot leave another showing what it saw
//! before.
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
use std::time::{Duration, Instant};

use frameguin_contract::DeviceResult;
use frameguin_model::reading::{self, Failure, Reading, Request};
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;

use crate::daemon::Daemon;
use crate::mapped::{Timer, while_mapped};

/// How often the block is read while anything is showing it. One rate for
/// every view sharing the read: the battery row's, which is the pace a
/// charge current settling after a limit engages is worth watching at.
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

type Show = dyn Fn(&Reading);

/// Where one view wants the reading put, and what it needs read for it.
struct View {
    request: Request,
    /// Weak, the widget holding the subscription rather than the other way
    /// round.
    widget: glib::WeakRef<gtk::Widget>,
    /// Behind an `Rc` so a tick can take a copy and let go of the list before
    /// showing anything: a view is free to close its window, and so to
    /// unsubscribe, from inside the call that shows it.
    show: Rc<Show>,
}

impl View {
    fn sits_in(&self, root: &gtk::Root) -> bool {
        self.widget
            .upgrade()
            .and_then(|widget| widget.root())
            .as_ref()
            == Some(root)
    }
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

/// A read's place in [`Feed::reading`], or a write's in [`Feed::writing`],
/// counted for as long as it is held.
pub(crate) struct InFlight<'a>(&'a Cell<u32>);

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

/// The reading last shown.
struct Last {
    reading: Rc<Reading>,
    request: Request,
    taken: Instant,
}

impl Last {
    fn covers(&self, request: Request) -> bool {
        self.request.with(request) == self.request
    }

    fn fresh(&self) -> bool {
        self.taken.elapsed() < Duration::from_secs(READING_SECONDS.into())
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
    /// see [`CONDITION_EVERY`]. Rewound when a view asking for the condition
    /// arrives with no fresh reading covering it: what it spaces is
    /// repetition, never a view's first sight of the condition.
    ticks: Cell<u32>,
    /// Whether a read for a newly arrived view is already spawned, so views
    /// arriving together are served by one.
    catching_up: Cell<bool>,
    /// A read that overlaps a write may have been answered from before it.
    writes: Cell<u64>,
    writing: Cell<u32>,
    /// Dropped with the last view and at a write, so nothing is shown a
    /// reading from before its window hid or from before a write.
    last: RefCell<Option<Last>>,
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
            catching_up: Cell::default(),
            writes: Cell::default(),
            writing: Cell::default(),
            last: RefCell::default(),
        }
    }

    /// Held across a write to something a view shows, so no reading that
    /// overlaps it is shown; drop it before the fill that shows what the
    /// write did.
    pub(crate) fn writing(&self) -> InFlight<'_> {
        self.writes.set(self.writes.get() + 1);
        self.last.take();
        InFlight::enter(&self.writing)
    }

    /// Registers a view, and starts the timer where this is the first.
    ///
    /// Takes no reading for the first view: its window subscribes and then
    /// fills itself, and that read is the one placed to say so when it fails.
    /// A later view is shown the last reading where it covers the view, and
    /// read for at once, silently as a tick is, unless that reading is fresh.
    fn subscribe(
        self: &Rc<Self>,
        request: Request,
        widget: glib::WeakRef<gtk::Widget>,
        show: Rc<Show>,
    ) -> Subscription {
        let id = self.next_id.get();
        self.next_id.set(id + 1);
        let (covered, fresh) = self.last.borrow().as_ref().map_or((false, false), |last| {
            let covered = last.covers(request);
            (covered, covered && last.fresh())
        });
        // A window returning from the tray asks for no condition, and a rewind
        // would buy a read it cannot show.
        if request.condition && !fresh {
            self.ticks.set(0);
        }
        let first = {
            let mut views = self.views.borrow_mut();
            views.push((
                id,
                View {
                    request,
                    widget,
                    show,
                },
            ));
            views.len() == 1
        };
        if first {
            self.arm();
        } else {
            if covered {
                self.show_last(id, request);
            }
            if !fresh {
                self.catch_up();
            }
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
            self.last.take();
        }
    }

    /// A view subscribes from its widget's map handler, often while a reading
    /// is being shown.
    fn show_last(self: &Rc<Self>, id: u64, request: Request) {
        let feed = self.clone();
        glib::idle_add_local_once(move || {
            // A tick may have replaced the reading with a narrower one.
            let Some(reading) = feed
                .last
                .borrow()
                .as_ref()
                .filter(|last| last.covers(request))
                .map(|last| last.reading.clone())
            else {
                return;
            };
            let show = feed
                .views
                .borrow()
                .iter()
                .find(|(view_id, _)| *view_id == id)
                .map(|(_, view)| view.show.clone());
            if let Some(show) = show {
                show(&reading);
            }
        });
    }

    /// A read already going leaves the new view to the next tick, since reads
    /// run one at a time against the daemon.
    fn catch_up(self: &Rc<Self>) {
        if self.catching_up.replace(true) {
            return;
        }
        let feed = self.clone();
        glib::spawn_future_local(async move {
            feed.catching_up.set(false);
            if feed.reading.get() == 0 {
                let _ = feed.read().await;
            }
        });
    }

    fn arm(self: &Rc<Self>) {
        let feed = self.clone();
        self.timer
            .set(Some(Timer::every_seconds(READING_SECONDS, move || {
                // Skipped while any read is going, a window's fill included,
                // and while a write is, whose overlapping read is never shown.
                // Tested before the task is spawned rather than inside it:
                // `read` takes its place in the count before its first
                // suspension, so there is nothing an async context would see
                // that this does not.
                if feed.reading.get() > 0 || feed.writing.get() > 0 {
                    return;
                }
                let feed = feed.clone();
                glib::spawn_future_local(async move {
                    let _ = feed.read().await;
                });
            })));
    }

    /// Takes one reading, shows it on every view, and hands it back with the
    /// failure the window `widget` sits in should announce.
    ///
    /// Reads what the subscribed views want and nothing else, so a window
    /// filling itself subscribes first.
    ///
    /// Returned as well as broadcast, so a fill is the feed's own read rather
    /// than a second assembly beside it — and so any fill refreshes every
    /// other view at the same instant rather than leaving them a tick behind.
    ///
    /// Raises only where the daemon's controls could not be had. The failure
    /// beside the reading counts only the extras the window's own views asked
    /// for: one only another window asked for is not this one's to announce.
    pub(crate) async fn fill(
        &self,
        widget: &impl IsA<gtk::Widget>,
    ) -> DeviceResult<(Rc<Reading>, Option<Failure>)> {
        let (reading, failures) = self.read().await?;
        let asked = widget.root().map_or_else(Request::default, |root| {
            self.requested(|view| view.sits_in(&root))
        });
        let failure = failures
            .into_iter()
            .find(|failure| failure.extra.requested(asked));
        Ok((reading, failure))
    }

    fn requested(&self, keep: impl Fn(&View) -> bool) -> Request {
        self.views
            .borrow()
            .iter()
            .filter(|(_, view)| keep(view))
            .fold(Request::default(), |request, (_, view)| {
                request.with(view.request)
            })
    }

    /// Takes one reading, shows it on every view, and hands it back with the
    /// extras that failed. A reading that overlapped a write is handed back
    /// and not shown.
    ///
    /// Raises only where the daemon's controls could not be had; an extra
    /// that failed arrives as None. A device the board does not have is not a
    /// failure: its field arrives as None, as one nobody asked for does.
    async fn read(&self) -> DeviceResult<(Rc<Reading>, Vec<Failure>)> {
        // The `?` below returns early, and a count left raised would lock the
        // tick out for the rest of the process.
        let _in_flight = InFlight::enter(&self.reading);
        let writes = self.writes.get();
        let controls = self.daemon.controls().await?;
        let mut request = self.requested(|_| true);
        // Every read wants the block; the condition only on the reads that come
        // round to it.
        let ticks = self.ticks.get();
        self.ticks.set(ticks.wrapping_add(1));
        request.condition &= ticks.is_multiple_of(CONDITION_EVERY);
        let (reading, failures) = reading::read(&controls, request).await;
        let reading = Rc::new(reading);
        if self.writes.get() != writes || self.writing.get() > 0 {
            return Ok((reading, failures));
        }
        self.last.replace(Some(Last {
            reading: reading.clone(),
            request,
            taken: Instant::now(),
        }));
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
        Ok((reading, failures))
    }
}

/// Shows the reading on a view for as long as `widget` is on screen, the
/// subscription being what [`while_mapped`] holds.
pub(crate) fn show_while_mapped(
    feed: &Rc<Feed>,
    widget: &impl IsA<gtk::Widget>,
    request: Request,
    show: impl Fn(&Reading) + 'static,
) {
    let show: Rc<Show> = Rc::new(show);
    let feed = feed.clone();
    let weak = widget.upcast_ref::<gtk::Widget>().downgrade();
    while_mapped(widget, move || {
        feed.subscribe(request, weak.clone(), show.clone())
    });
}
