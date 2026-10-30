//! A drawing in layers, one drawing area per color, stacked in an overlay.

use std::rc::Rc;

use gtk4 as gtk;
use gtk4::cairo;
use gtk4::prelude::*;

/// Stacks one drawing area per entry of `layers` in `overlay`, the first as
/// its child, each drawn by `draw` with that entry's painter. A drawing
/// area's only themed color is its own CSS `color`, which the entry's
/// libadwaita style class sets, and None leaves the foreground's.
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
            area.set_draw_func(move |area, cr, width, height| {
                // A cairo error stays on its context, and each draw is
                // handed a new one.
                let _ = draw(area, cr, width, height, paint);
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
