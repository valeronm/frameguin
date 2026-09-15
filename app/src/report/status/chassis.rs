//! The chassis section: one row saying whether the machine is open, and a
//! page adding how often the EC has counted it opened and the keyboard deck's
//! power state.
//!
//! What each value is *called* is `frameguin_model::control::chassis`'s.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::control::chassis::{
    Chassis, deck_label, open_now_label, state_label, times_label,
};
use frameguin_wire::ChassisFeature;

use super::Sidebar;
use crate::bus::Bus;
use crate::reading::{Feed, Wants, show_while_mapped};
use crate::report::{value, value_row};

/// The deck state is a host command the summary does not need.
pub(super) fn add(sidebar: &Rc<Sidebar>, feed: &Rc<Feed>, chassis: &Chassis<Bus>) {
    let list = sidebar.section(None);
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    let open = value(&group, "Open now");
    let opened = value(&group, "Times opened");
    let found_open = value(&group, "Times found open at power-up");
    let (deck_row, deck) = value_row(&group, "Keyboard deck");
    let has_deck = chassis.has(ChassisFeature::Deck);
    deck_row.set_visible(has_deck);
    page.add(&group);
    let row = sidebar.add(&list, "Chassis", &page);

    let wants = Wants {
        chassis: true,
        ..Wants::default()
    };
    sidebar.follow(feed, wants, move |reading| {
        if let Some(state) = reading.chassis {
            row.set_subtitle(state_label(state.open));
            open.set_label(open_now_label(state.open));
            opened.set_label(&times_label(state.opened));
            found_open.set_label(&times_label(state.found_open));
        }
    });

    if has_deck {
        let wants = Wants {
            deck: true,
            ..Wants::default()
        };
        show_while_mapped(feed, &page, wants, move |reading| {
            if let Some(state) = reading.deck {
                deck.set_label(deck_label(state));
            }
        });
    }
}
