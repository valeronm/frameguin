//! The battery extender: one row naming its stage, and a page adding
//! when the first stage starts and what resets it.
//!
//! What each value is *called* is `frameguin_modelview::extender`'s.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_model::reading::Request;
use frameguin_modelview::extender::{
    description, first_stage_label, reset_words, stage_label, trigger_label,
};

use super::{Section, Sidebar};
use crate::reading::Feed;
use crate::report::{value, value_row};

pub(super) fn add(sidebar: &Rc<Sidebar>, section: Section, feed: &Rc<Feed>) {
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .description(description())
        .build();
    let stage = value(&group, "Stage");
    let (first_stage_row, first_stage) = value_row(&group, "Starts in");
    let (reset_row, reset) = value_row(&group, "");
    page.add(&group);
    let row = sidebar.add(section, "Extender", &page);

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
        let words = reset_words(
            extender,
            reading.info.as_ref().map(|info| info.charger_connected),
        );
        reset_row.set_title(words.title);
        reset.set_label(&words.value);
        reset_row.set_subtitle(words.subtitle.as_deref().unwrap_or_default());
    });
}
