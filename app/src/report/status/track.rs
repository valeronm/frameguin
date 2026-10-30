//! A reading drawn on a line among the marks it is judged by, and a legend
//! for those marks.
//!
//! Where each mark sits is `frameguin_modelview`'s.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use frameguin_modelview::track::{Mark, Track};
use gtk4 as gtk;
use gtk4::cairo;

use crate::report::status::layers;
use crate::report::value_label;

/// A `PreferencesRow` gives a child of its own none of an action row's
/// padding or height.
const ROW_PADDING: i32 = 12;
const ROW_MARGIN: i32 = 10;
const ROW_HEADER_HEIGHT: i32 = 30;

const HEIGHT: i32 = 30;
/// Room at each end for half the dot.
const INSET: f64 = 6.0;
const LINE_Y: f64 = 8.0;
const DOT_RADIUS: f64 = 5.0;
const LIMIT_HALF_HEIGHT: f64 = 6.0;
const TICK_BASELINE: f64 = 28.0;
const TICK_SIZE: f64 = 11.0;
const TICK_SPACING: f64 = 4.0;
const BAND_WIDTH: f64 = 6.0;
const LIMIT_WIDTH: f64 = 3.0;
const SWATCH: i32 = 14;
const LIMIT_SWATCH_WIDTH: i32 = 5;

const BAND_CLASS: &str = layers::BLUE_CLASS;

const fn mark_class(mark: Mark) -> &'static str {
    match mark {
        Mark::Caution => "warning",
        Mark::Shutdown => "error",
        Mark::Target => "success",
    }
}

type Paint = fn(&cairo::Context, &Track, f64) -> Result<(), cairo::Error>;

const LAYERS: [(Option<&str>, Paint); 6] = [
    (Some("dim-label"), paint_line),
    (Some(BAND_CLASS), paint_band),
    (Some(mark_class(Mark::Caution)), |cr, track, width| {
        paint_marks(cr, track, Mark::Caution, width)
    }),
    (Some(mark_class(Mark::Shutdown)), |cr, track, width| {
        paint_marks(cr, track, Mark::Shutdown, width)
    }),
    (Some(mark_class(Mark::Target)), |cr, track, width| {
        paint_marks(cr, track, Mark::Target, width)
    }),
    (None, paint_dot),
];

/// A title and its value over a track, which an action row has no place for.
pub(super) struct TrackRow {
    header: gtk::Box,
    pub(super) value: gtk::Label,
    pub(super) track: TrackView,
}

impl TrackRow {
    pub(super) fn new(group: &adw::PreferencesGroup, title: &str) -> Self {
        let name = gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        let value = value_label();
        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(ROW_PADDING)
            .height_request(ROW_HEADER_HEIGHT)
            .build();
        header.append(&name);
        header.append(&value);
        let track = TrackView::new();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .margin_top(ROW_MARGIN)
            .margin_bottom(ROW_MARGIN)
            .margin_start(ROW_PADDING)
            .margin_end(ROW_PADDING)
            .build();
        content.append(&header);
        content.append(&track.root);
        let row = adw::PreferencesRow::builder()
            .title(title)
            .activatable(false)
            .child(&content)
            .build();
        group.add(&row);
        Self {
            header,
            value,
            track,
        }
    }

    /// Placed after the value.
    pub(super) fn add_suffix(&self, widget: &impl IsA<gtk::Widget>) {
        self.header.append(widget);
    }
}

pub(super) struct TrackView {
    root: gtk::Overlay,
    layers: Vec<gtk::DrawingArea>,
    track: Rc<RefCell<Option<Track>>>,
}

impl TrackView {
    fn new() -> Self {
        let root = gtk::Overlay::builder()
            .accessible_role(gtk::AccessibleRole::Img)
            .visible(false)
            .build();
        let track: Rc<RefCell<Option<Track>>> = Rc::default();
        let drawn = Rc::clone(&track);
        let layers = layers::stack(&root, &LAYERS, move |area, cr, width, _, paint| {
            let drawn = drawn.borrow();
            let Some(track) = drawn.as_ref() else {
                return Ok(());
            };
            prepare(area, cr);
            paint(cr, track, f64::from(width))
        });
        for layer in &layers {
            layer.set_content_height(HEIGHT);
            layer.set_hexpand(true);
        }
        Self {
            root,
            layers,
            track,
        }
    }

    /// Hidden for None.
    pub(super) fn show(&self, track: Option<Track>) {
        {
            let drawn = self.track.borrow();
            if *drawn == track {
                return;
            }
            if drawn.is_some() != track.is_some() {
                self.root.set_visible(track.is_some());
            }
            let words = description(track.as_ref());
            if description(drawn.as_ref()) != words {
                self.root
                    .update_property(&[gtk::accessible::Property::Description(words)]);
            }
        }
        self.track.replace(track);
        for layer in &self.layers {
            layer.queue_draw();
        }
    }
}

fn description(track: Option<&Track>) -> &str {
    track.map_or("", |track| track.description.as_str())
}

fn prepare(area: &gtk::DrawingArea, cr: &cairo::Context) {
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

fn stroke_band(cr: &cairo::Context, from: f64, to: f64, y: f64) -> Result<(), cairo::Error> {
    cr.set_line_width(BAND_WIDTH);
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

fn band_swatch(cr: &cairo::Context, width: f64, height: f64) -> Result<(), cairo::Error> {
    stroke_band(cr, BAND_WIDTH / 2.0, width - BAND_WIDTH / 2.0, height / 2.0)
}

fn limit_swatch(cr: &cairo::Context, width: f64, height: f64) -> Result<(), cairo::Error> {
    stroke_limit(cr, width / 2.0, height / 2.0, height / 2.0 - LIMIT_WIDTH)
}

pub(super) fn legend(band: &str, caution: &str, shutdown: &str) -> gtk::Box {
    let legend = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .valign(gtk::Align::Center)
        .build();
    let entries: [(&str, i32, Swatch, &str); 3] = [
        (BAND_CLASS, SWATCH, band_swatch, band),
        (
            mark_class(Mark::Caution),
            LIMIT_SWATCH_WIDTH,
            limit_swatch,
            caution,
        ),
        (
            mark_class(Mark::Shutdown),
            LIMIT_SWATCH_WIDTH,
            limit_swatch,
            shutdown,
        ),
    ];
    for (class, width, draw, words) in entries {
        let swatch = gtk::DrawingArea::builder()
            .content_width(width)
            .content_height(SWATCH)
            .valign(gtk::Align::Center)
            .css_classes([class])
            .build();
        layers::draw_in_own_color(&swatch, move |area, cr, width, height| {
            prepare(area, cr);
            draw(cr, f64::from(width), f64::from(height))
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

fn paint_band(cr: &cairo::Context, track: &Track, width: f64) -> Result<(), cairo::Error> {
    let Some((from, to)) = track.band else {
        return Ok(());
    };
    stroke_band(cr, x(from, width), x(to, width), LINE_Y)
}

fn paint_marks(
    cr: &cairo::Context,
    track: &Track,
    mark: Mark,
    width: f64,
) -> Result<(), cairo::Error> {
    track
        .placed(mark)
        .try_for_each(|at| stroke_limit(cr, x(at, width), LINE_Y, LIMIT_HALF_HEIGHT))
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
