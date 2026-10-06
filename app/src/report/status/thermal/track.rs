//! A sensor's thresholds drawn as a line: the fan's ramp, the trip points and
//! the temperature's place among them.
//!
//! Where each mark sits is `frameguin_modelview::thermal`'s.

use std::cell::RefCell;
use std::rc::Rc;

use frameguin_modelview::thermal::{FAN_RANGE, SHUTDOWN, THROTTLE, Track};
use gtk4 as gtk;
use gtk4::cairo;
use gtk4::prelude::*;

const HEIGHT: i32 = 30;
/// Room at each end for half the dot.
const INSET: f64 = 6.0;
const LINE_Y: f64 = 8.0;
const DOT_RADIUS: f64 = 5.0;
const LIMIT_HALF_HEIGHT: f64 = 6.0;
const TICK_BASELINE: f64 = 28.0;
const TICK_SIZE: f64 = 11.0;
const TICK_SPACING: f64 = 4.0;
const RAMP_WIDTH: f64 = 6.0;
const LIMIT_WIDTH: f64 = 3.0;
const SWATCH: i32 = 14;
const LIMIT_SWATCH_WIDTH: i32 = 5;

const RAMP_CLASS: &str = "accent";
const CAUTION_CLASS: &str = "warning";
const SHUTDOWN_CLASS: &str = "error";

type Paint = fn(&cairo::Context, &Track, f64) -> Result<(), cairo::Error>;

/// One layer per color: a drawing area's only themed color is its own CSS
/// `color`, which a libadwaita style class sets.
const LAYERS: [(Option<&str>, Paint); 5] = [
    (Some("dim-label"), paint_line),
    (Some(RAMP_CLASS), paint_ramp),
    (Some(CAUTION_CLASS), paint_cautions),
    (Some(SHUTDOWN_CLASS), paint_shutdown),
    (None, paint_dot),
];

pub(super) struct TrackView {
    root: gtk::Overlay,
    layers: Vec<gtk::DrawingArea>,
    track: Rc<RefCell<Option<Track>>>,
}

impl TrackView {
    pub(super) fn new() -> Self {
        let root = gtk::Overlay::builder()
            .accessible_role(gtk::AccessibleRole::Img)
            .visible(false)
            .build();
        let track = Rc::new(RefCell::new(None));
        let layers: Vec<gtk::DrawingArea> = LAYERS
            .iter()
            .map(|(class, paint)| layer(*class, *paint, &track))
            .collect();
        for (index, layer) in layers.iter().enumerate() {
            if index == 0 {
                root.set_child(Some(layer));
            } else {
                root.add_overlay(layer);
            }
        }
        Self {
            root,
            layers,
            track,
        }
    }

    pub(super) fn widget(&self) -> &gtk::Overlay {
        &self.root
    }

    /// Hidden for None.
    pub(super) fn show(&self, track: Option<Track>) {
        if *self.track.borrow() == track {
            return;
        }
        self.root.set_visible(track.is_some());
        let description = track.as_ref().map_or("", |track| &track.description);
        self.root
            .update_property(&[gtk::accessible::Property::Description(description)]);
        self.track.replace(track);
        for layer in &self.layers {
            layer.queue_draw();
        }
    }
}

fn layer(
    class: Option<&str>,
    paint: Paint,
    track: &Rc<RefCell<Option<Track>>>,
) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(HEIGHT)
        .hexpand(true)
        .can_target(false)
        .build();
    if let Some(class) = class {
        area.add_css_class(class);
    }
    let track = Rc::clone(track);
    area.set_draw_func(move |area, cr, width, _| {
        if let Some(track) = track.borrow().as_ref() {
            prepare(area, cr);
            // A cairo error stays on its context, and each draw is handed a
            // new one.
            let _ = paint(cr, track, f64::from(width));
        }
    });
    area
}

fn prepare(area: &gtk::DrawingArea, cr: &cairo::Context) {
    cr.set_source_color(&area.color());
    cr.set_line_cap(cairo::LineCap::Round);
    let font = area.pango_context().font_description();
    let family = font.as_ref().and_then(gtk::pango::FontDescription::family);
    cr.select_font_face(
        family.as_deref().unwrap_or("sans-serif"),
        cairo::FontSlant::Normal,
        cairo::FontWeight::Normal,
    );
    cr.set_font_size(TICK_SIZE);
}

fn stroke_ramp(cr: &cairo::Context, from: f64, to: f64, y: f64) -> Result<(), cairo::Error> {
    cr.set_line_width(RAMP_WIDTH);
    cr.move_to(from, y);
    cr.line_to(to, y);
    cr.stroke()
}

fn stroke_limit(cr: &cairo::Context, x: f64, y: f64, half_height: f64) -> Result<(), cairo::Error> {
    cr.set_line_width(LIMIT_WIDTH);
    cr.move_to(x, y - half_height);
    cr.line_to(x, y + half_height);
    cr.stroke()
}

type Swatch = fn(&cairo::Context, f64, f64) -> Result<(), cairo::Error>;

fn ramp_swatch(cr: &cairo::Context, width: f64, height: f64) -> Result<(), cairo::Error> {
    stroke_ramp(cr, RAMP_WIDTH / 2.0, width - RAMP_WIDTH / 2.0, height / 2.0)
}

fn limit_swatch(cr: &cairo::Context, width: f64, height: f64) -> Result<(), cairo::Error> {
    stroke_limit(cr, width / 2.0, height / 2.0, height / 2.0 - LIMIT_WIDTH)
}

/// No color is named in words: the accent is the user's to choose.
pub(super) fn legend() -> gtk::Box {
    let legend = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .valign(gtk::Align::Center)
        .build();
    let entries: [(&str, i32, Swatch, &str); 3] = [
        (RAMP_CLASS, SWATCH, ramp_swatch, FAN_RANGE),
        (CAUTION_CLASS, LIMIT_SWATCH_WIDTH, limit_swatch, THROTTLE),
        (SHUTDOWN_CLASS, LIMIT_SWATCH_WIDTH, limit_swatch, SHUTDOWN),
    ];
    for (class, width, draw, words) in entries {
        let swatch = gtk::DrawingArea::builder()
            .content_width(width)
            .content_height(SWATCH)
            .valign(gtk::Align::Center)
            .css_classes([class])
            .build();
        swatch.set_draw_func(move |area, cr, width, height| {
            prepare(area, cr);
            let _ = draw(cr, f64::from(width), f64::from(height));
        });
        let label = gtk::Label::builder()
            .label(words)
            .css_classes(["caption", "dim-label"])
            .build();
        let entry = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .build();
        entry.append(&swatch);
        entry.append(&label);
        legend.append(&entry);
    }
    legend
}

fn x(position: u16, width: f64) -> f64 {
    INSET + (width - 2.0 * INSET) * f64::from(position) / 1000.0
}

/// Whether each span, a label's left and right edge, is clear of every
/// earlier one that was kept.
fn fitting(spans: &[(f64, f64)]) -> Vec<bool> {
    let mut kept: Vec<(f64, f64)> = Vec::new();
    spans
        .iter()
        .map(|&(left, right)| {
            let clear = kept
                .iter()
                .all(|&(l, r)| right + TICK_SPACING <= l || r + TICK_SPACING <= left);
            if clear {
                kept.push((left, right));
            }
            clear
        })
        .collect()
}

fn paint_line(cr: &cairo::Context, track: &Track, width: f64) -> Result<(), cairo::Error> {
    cr.set_line_width(2.0);
    cr.move_to(x(0, width), LINE_Y);
    cr.line_to(x(1000, width), LINE_Y);
    cr.stroke()?;
    let mut spans = Vec::with_capacity(track.ticks.len());
    for (position, label) in &track.ticks {
        let advance = cr.text_extents(label)?.x_advance();
        let left = (x(*position, width) - advance / 2.0).clamp(0.0, (width - advance).max(0.0));
        spans.push((left, left + advance));
    }
    for (((_, label), (left, _)), fits) in track.ticks.iter().zip(&spans).zip(fitting(&spans)) {
        if fits {
            cr.move_to(*left, TICK_BASELINE);
            cr.show_text(label)?;
        }
    }
    Ok(())
}

fn paint_ramp(cr: &cairo::Context, track: &Track, width: f64) -> Result<(), cairo::Error> {
    let Some((off, max)) = track.ramp else {
        return Ok(());
    };
    stroke_ramp(cr, x(off, width), x(max, width), LINE_Y)
}

fn stroke_limits(cr: &cairo::Context, at: &[u16], width: f64) -> Result<(), cairo::Error> {
    at.iter()
        .try_for_each(|at| stroke_limit(cr, x(*at, width), LINE_Y, LIMIT_HALF_HEIGHT))
}

fn paint_cautions(cr: &cairo::Context, track: &Track, width: f64) -> Result<(), cairo::Error> {
    stroke_limits(cr, &track.cautions, width)
}

fn paint_shutdown(cr: &cairo::Context, track: &Track, width: f64) -> Result<(), cairo::Error> {
    stroke_limits(cr, track.shutdown.as_slice(), width)
}

fn paint_dot(cr: &cairo::Context, track: &Track, width: f64) -> Result<(), cairo::Error> {
    let Some(dot) = track.dot else {
        return Ok(());
    };
    cr.arc(
        x(dot, width),
        LINE_Y,
        DOT_RADIUS,
        0.0,
        std::f64::consts::TAU,
    );
    cr.fill()
}

#[cfg(test)]
mod tests {
    use super::fitting;

    #[test]
    fn labels_apart_all_fit() {
        assert_eq!(fitting(&[(0.0, 10.0), (50.0, 60.0)]), [true, true]);
    }

    #[test]
    fn a_label_over_an_earlier_one_is_dropped() {
        let spans = [(50.0, 70.0), (60.0, 80.0), (100.0, 110.0)];
        assert_eq!(fitting(&spans), [true, false, true]);
    }

    #[test]
    fn a_label_closer_than_the_spacing_is_dropped() {
        assert_eq!(fitting(&[(20.0, 30.0), (32.0, 40.0)]), [true, false]);
        assert_eq!(fitting(&[(20.0, 30.0), (8.0, 18.0)]), [true, false]);
    }

    #[test]
    fn a_dropped_label_takes_no_room() {
        let spans = [(50.0, 70.0), (60.0, 80.0), (76.0, 90.0)];
        assert_eq!(fitting(&spans), [true, false, true]);
    }
}
