//! The battery extender: one row summing up its state, and a page adding its
//! effect, what comes next and every stage in order.
//!
//! What each value is *called* is `frameguin_modelview::extender`'s.

use std::rc::Rc;

use adw::prelude::*;
use frameguin_contract::ExtenderStage;
use frameguin_model::control::battery::extender::{STAGES, in_force};
use frameguin_model::reading::{ExtenderReading, Request};
use frameguin_modelview::extender::{
    Words, effect, next, stage_subtitle, stage_title, stage_value, state_label, summary,
};
use gtk4 as gtk;

use super::{Section, Sidebar};
use crate::reading::Feed;
use crate::report::{value, value_row};

pub(super) fn add(sidebar: &Rc<Sidebar>, section: Section, feed: &Rc<Feed>) {
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::new();
    let stage = value(&group, "Stage");
    let (effect_row, effect_value) = value_row(&group, "Effect");
    let (next_row, next_value) = value_row(&group, "Next");
    page.add(&group);
    let steps = adw::PreferencesGroup::builder().title("Stages").build();
    let step_rows = STAGES.map(|stage| step_row(&steps, stage));
    page.add(&steps);
    let row = sidebar.add(section, "Extender", &page);

    let request = Request {
        extender: true,
        ..Request::default()
    };
    sidebar.follow(feed, request, move |reading| {
        let Some(ExtenderReading {
            state: extender,
            charge_limit,
        }) = reading.extender
        else {
            return;
        };
        row.set_subtitle(&summary(extender, charge_limit));
        stage.set_label(state_label(extender));
        show(&effect_row, &effect_value, &effect(extender, charge_limit));
        let ahead = next(
            extender,
            reading.info.as_ref().map(|info| info.charger_connected),
        );
        next_row.set_visible(ahead.is_some());
        if let Some(ahead) = ahead {
            show(&next_row, &next_value, &ahead);
        }
        let current = in_force(extender);
        for (stage, (step, mark)) in STAGES.into_iter().zip(&step_rows) {
            step.set_subtitle(&stage_subtitle(stage, extender.trigger_days));
            show_mark(mark, current == Some(stage));
        }
    });
}

fn show(row: &adw::ActionRow, value: &gtk::Label, words: &Words) {
    value.set_label(&words.value);
    row.set_subtitle(words.subtitle.as_deref().unwrap_or_default());
}

fn step_row(group: &adw::PreferencesGroup, stage: ExtenderStage) -> (adw::ActionRow, gtk::Image) {
    let (row, value) = value_row(group, stage_title(stage));
    value.set_label(&stage_value(stage));
    let mark = gtk::Image::from_icon_name("object-select-symbolic");
    mark.update_property(&[gtk::accessible::Property::Label("Current stage")]);
    show_mark(&mark, false);
    row.add_prefix(&mark);
    (row, mark)
}

/// Hidden by opacity so every step's title starts at one edge.
fn show_mark(mark: &gtk::Image, current: bool) {
    mark.set_opacity(if current { 1.0 } else { 0.0 });
    mark.update_state(&[gtk::accessible::State::Hidden(!current)]);
}
