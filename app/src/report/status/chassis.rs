//! The chassis: one row saying whether the machine is open and naming
//! the input deck's state where it is not on, and a page adding how often the
//! EC has counted it opened, and the input deck's power state with the mode
//! and the reset that move it.
//!
//! What each value is *called* is `frameguin_modelview::chassis`'s.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use frameguin_contract::{ChassisFeature, DeckMode, Platform};
use frameguin_model::control::chassis::Chassis;
use frameguin_model::deck::{self, DeckLoad};
use frameguin_model::reading::Request;
use frameguin_modelview::chassis::{
    DECK_RESET_SUBTITLE, chassis_summary, deck_label, deck_mode_at, deck_mode_labels,
    deck_mode_row, force_off_warning, open_now_label, times_label,
};
use frameguin_wire::Bus;
use gtk4 as gtk;

use super::{Section, Sidebar, Writes, written};
use crate::reading::Feed;
use crate::report::value;
use crate::synced::{combo_selection, connect_button, connect_combo, string_list};

pub(super) fn add(
    sidebar: &Rc<Sidebar>,
    section: Section,
    feed: &Rc<Feed>,
    chassis: &Rc<Chassis<Bus>>,
    platform: Platform,
    writes: &Rc<Writes>,
) {
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    let open = value(&group, "Open now");
    let opened = value(&group, "Times opened");
    let found_open = value(&group, "Times found open at power-up");
    page.add(&group);
    let has_deck = chassis.has(ChassisFeature::Deck);
    let deck = has_deck.then(|| Deck::build(&page, feed, chassis, deck::load(platform), writes));
    let row = sidebar.add(section, "Chassis", &page);

    let request = Request {
        chassis: true,
        deck: has_deck,
        ..Request::default()
    };
    let guard = writes.guard.clone();
    sidebar.follow(feed, request, move |reading| {
        if let Some(state) = reading.chassis {
            row.set_subtitle(&chassis_summary(state.open, reading.deck));
            open.set_label(open_now_label(state.open));
            opened.set_label(&times_label(state.opened));
            found_open.set_label(&times_label(state.found_open));
        }
        if let (Some(state), Some(deck)) = (reading.deck, &deck) {
            deck.state.set_label(deck_label(state));
            deck.shown.set(deck_mode_row(state));
            guard.sync(|| {
                deck.mode.set_selected(combo_selection(deck.shown.get()));
                deck.mode.set_sensitive(true);
            });
        }
    });
}

struct Deck {
    state: gtk::Label,
    mode: adw::ComboRow,
    /// The row the last reading put the combo on.
    shown: Rc<Cell<Option<usize>>>,
}

impl Deck {
    fn build(
        page: &adw::PreferencesPage,
        feed: &Rc<Feed>,
        chassis: &Rc<Chassis<Bus>>,
        load: Option<DeckLoad>,
        writes: &Rc<Writes>,
    ) -> Self {
        let group = adw::PreferencesGroup::builder().title("Input Deck").build();
        let state = value(&group, "State");
        let mode = adw::ComboRow::builder()
            .title("Mode")
            .model(&string_list(&deck_mode_labels()))
            .sensitive(false)
            .build();
        group.add(&mode);
        let reset = gtk::Button::builder()
            .label("Reset")
            .valign(gtk::Align::Center)
            .build();
        let reset_row = adw::ActionRow::builder()
            .title("Reset")
            .subtitle(DECK_RESET_SUBTITLE)
            .activatable_widget(&reset)
            .build();
        reset_row.add_suffix(&reset);
        group.add(&reset_row);
        page.add(&group);

        let shown = Rc::new(Cell::new(None));
        let (moved, declined, feeding) = (mode.downgrade(), shown.clone(), Rc::downgrade(feed));
        connect_combo(
            &writes.guard,
            writes,
            chassis,
            &mode,
            deck_mode_at,
            move |writes, chassis, wanted| {
                let (moved, declined, feed) = (moved.clone(), declined.clone(), feeding.clone());
                async move {
                    let (Some(combo), Some(feed)) = (moved.upgrade(), feed.upgrade()) else {
                        return;
                    };
                    if wanted == DeckMode::ForceOff && !confirmed(&combo, load).await {
                        writes
                            .guard
                            .sync(|| combo.set_selected(combo_selection(declined.get())));
                        return;
                    }
                    let written = written(&feed, &combo, None, || chassis.set_deck_mode(wanted));
                    if let Err(e) = written.await {
                        writes.toast_error("Setting the input deck's mode", e);
                    }
                }
            },
        );
        let (pressed, feeding) = (reset.downgrade(), Rc::downgrade(feed));
        connect_button(writes, chassis, &reset, move |writes, chassis| {
            let (pressed, feed) = (pressed.clone(), feeding.clone());
            async move {
                let (Some(button), Some(feed)) = (pressed.upgrade(), feed.upgrade()) else {
                    return;
                };
                if let Err(e) = written(&feed, &button, None, || chassis.reset_deck()).await {
                    writes.toast_error("Resetting the input deck", e);
                }
            }
        });
        Self { state, mode, shown }
    }
}

async fn confirmed(parent: &impl IsA<gtk::Widget>, load: Option<DeckLoad>) -> bool {
    const TURN_OFF: &str = "off";
    const CANCEL: &str = "cancel";
    let dialog = adw::AlertDialog::new(
        Some("Turn the Input Deck Off?"),
        Some(force_off_warning(load)),
    );
    dialog.add_responses(&[(CANCEL, "_Cancel"), (TURN_OFF, "_Turn Off")]);
    dialog.set_response_appearance(TURN_OFF, adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some(CANCEL));
    dialog.set_close_response(CANCEL);
    dialog.choose_future(Some(parent)).await == TURN_OFF
}
