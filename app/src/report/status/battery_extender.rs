//! The battery extender: one row naming its stage, and a page adding
//! when the first stage starts and what resets it.
//!
//! What each value is *called* is
//! `frameguin_model::control::battery::extender`'s.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::control::battery::extender::{
    first_stage_label, reset_label, stage_label, trigger_label,
};
use frameguin_model::reading::Request;
use gtk4 as gtk;

use super::Sidebar;
use crate::reading::Feed;
use crate::report::{value, value_row};

pub(super) fn add(sidebar: &Rc<Sidebar>, list: &gtk::ListBox, feed: &Rc<Feed>) {
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    let stage = value(&group, "Stage");
    let (first_stage_row, first_stage) = value_row(&group, "Starts in");
    let (reset_row, reset) = value_row(&group, "Resets after");
    page.add(&group);
    let row = sidebar.add(list, "Extender", &page);

    let request = Request {
        extender: true,
        ..Request::default()
    };
    sidebar.follow(feed, request, move |reading| {
        let Some(extender) = reading.extender else {
            return;
        };
        let label = stage_label(extender, reading.charge_limit);
        row.set_subtitle(&label);
        stage.set_label(&label);
        let countdown = first_stage_label(extender);
        first_stage_row.set_visible(countdown.is_some());
        if let Some(countdown) = countdown {
            first_stage.set_label(&countdown);
        }
        first_stage_row.set_subtitle(&trigger_label(extender.trigger_days));
        reset_row.set_visible(extender.enabled);
        reset.set_label(&reset_label(extender.reset_minutes));
    });
}
