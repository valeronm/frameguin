//! A drawing in layers, one drawing area per color, stacked in an overlay.

use std::rc::Rc;

use gtk4 as gtk;
use gtk4::cairo;
use gtk4::prelude::*;

use crate::window::add_css;

/// libadwaita's blue, for one of several colors drawn together: the user may
/// set the accent to any of the others.
pub(super) const BLUE_CLASS: &str = "frameguin-blue";

/// libadwaita 1.5's default accent as a foreground, on a light and on a dark
/// background.
const LIGHT_BLUE: &str = "#1c71d8";
const DARK_BLUE: &str = "#78aeed";

/// Registers [`BLUE_CLASS`]'s color for the display, and swaps its shade
/// whenever libadwaita switches between light and dark. Each call adds a
/// provider and a handler of its own.
pub(crate) fn style() {
    let styles = adw::StyleManager::default();
    let css = add_css(&blue_rule(styles.is_dark()));
    styles.connect_dark_notify(move |styles| css.load_from_data(&blue_rule(styles.is_dark())));
}

fn blue_rule(dark: bool) -> String {
    let blue = if dark { DARK_BLUE } else { LIGHT_BLUE };
    format!(".{BLUE_CLASS} {{ color: {blue}; }}")
}

/// Draws `area` with its own color as the source. A drawing area's only
/// themed color is its own CSS `color`, which a libadwaita style class on it
/// sets.
pub(super) fn draw_in_own_color(
    area: &gtk::DrawingArea,
    draw: impl Fn(&gtk::DrawingArea, &cairo::Context, i32, i32) -> Result<(), cairo::Error> + 'static,
) {
    area.set_draw_func(move |area, cr, width, height| {
        cr.set_source_color(&area.color());
        // A cairo error stays on its context, and each draw is handed a new
        // one.
        let _ = draw(area, cr, width, height);
    });
}

/// Stacks one drawing area per entry of `layers` in `overlay`, the first as
/// its child, each drawn by `draw` with that entry's painter through
/// [`draw_in_own_color`]. An entry's class of None leaves the foreground's
/// color.
pub(super) fn stack<P: Copy + 'static>(
    overlay: &gtk::Overlay,
    layers: &[(Option<&str>, P)],
    draw: impl Fn(&gtk::DrawingArea, &cairo::Context, i32, i32, P) -> Result<(), cairo::Error> + 'static,
) -> Vec<gtk::DrawingArea> {
    let draw = Rc::new(draw);
    let areas: Vec<gtk::DrawingArea> = layers
        .iter()
        .map(|&(class, paint)| {
            let area = gtk::DrawingArea::builder().can_target(false).build();
            if let Some(class) = class {
                area.add_css_class(class);
            }
            let draw = Rc::clone(&draw);
            draw_in_own_color(&area, move |area, cr, width, height| {
                draw(area, cr, width, height, paint)
            });
            area
        })
        .collect();
    for (index, area) in areas.iter().enumerate() {
        if index == 0 {
            overlay.set_child(Some(area));
        } else {
            overlay.add_overlay(area);
        }
    }
    areas
}
