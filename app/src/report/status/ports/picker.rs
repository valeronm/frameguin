//! Every port at once, one of them picked: an outline of the machine seen
//! from the keyboard with what each placed port holds beside its socket or
//! its side, and the ports nobody placed listed under it, or listed alone
//! where the board places none.
//!
//! Where each socket sits is `frameguin_modelview::port_map`'s, and what it
//! shows of its port `frameguin_modelview::ports`'s.

use std::cell::RefCell;
use std::f64::consts::{FRAC_PI_2, PI};
use std::rc::Rc;

use adw::prelude::*;
use frameguin_contract::PortState;
use frameguin_model::control::ports::Flow;
use frameguin_model::port::Placement;
use frameguin_modelview::port_map::{Edge, Map, Outline, Slot};
use frameguin_modelview::ports::{DISPLAY_PORT, Socket, port_line, socket, spoken};
use gtk4 as gtk;
use gtk4::{cairo, glib};

use crate::report::status::layers;

/// The height of a band, which the buttons beside it may stretch.
const BAND: i32 = 56;
const CHASSIS_BASE_WIDTH: i32 = 110;
const CHASSIS_WIDTH_PER_BAND: i32 = 30;
const SOCKET_LONG: f64 = 18.0;
const SOCKET_SHORT: f64 = 7.0;
const SOCKET_RADIUS: f64 = 2.5;
const CHASSIS_RADIUS: f64 = 10.0;
const OUTLINE_WIDTH: f64 = 2.0;
const LISTED_WIDTH: f64 = 4.0;
const SOCKET_LINE_WIDTH: f64 = 1.5;
/// Beyond a socket's outer half, for the arrow showing its port's flow.
const ARROW_ROOM: i32 = 12;
const ARROW_GAP: f64 = 3.0;
const ARROW_WIDTH: f64 = 10.0;
const CARD_PADDING: i32 = 12;
const OUTLINE_ALPHA: f32 = 0.5;
const DECK_ALPHA: f32 = 0.15;

type Looks = Rc<RefCell<Vec<(u8, Socket)>>>;

type Paint = fn(&cairo::Context, &Frame, &Outline, &[(u8, Socket)]) -> Result<(), cairo::Error>;

const LAYERS: [(Option<&str>, Paint); 4] = [
    (None, paint_chassis),
    (Some("dim-label"), paint_unused),
    (Some(FLOW_OUT_CLASS), |cr, frame, outline, looks| {
        paint_flowing(cr, frame, outline, looks, |flow| flow == Some(Flow::Out))
    }),
    (Some(FLOW_IN_CLASS), |cr, frame, outline, looks| {
        paint_flowing(cr, frame, outline, looks, |flow| flow == Some(Flow::In))
    }),
];

const FLOW_IN_CLASS: &str = "success";
const FLOW_OUT_CLASS: &str = "accent";

pub(super) struct Picker {
    group: adw::PreferencesGroup,
    placement: Placement,
    choices: Vec<Choice>,
    /// The ports whose socket is drawn, with an arrow for its flow.
    drawn: Vec<u8>,
    looks: Looks,
    layers: Vec<gtk::DrawingArea>,
}

struct Choice {
    index: u8,
    button: gtk::ToggleButton,
    main: gtk::Label,
    display: gtk::Image,
    caption: gtk::Label,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Column {
    Left = 0,
    Back = 1,
    Right = 2,
}

impl Column {
    /// The direction away from the machine at a socket on this edge.
    fn outward(self) -> (f64, f64) {
        match self {
            Self::Left => (-1.0, 0.0),
            Self::Back => (0.0, -1.0),
            Self::Right => (1.0, 0.0),
        }
    }
}

impl Picker {
    pub(super) fn new(placement: Placement, indices: &[u8]) -> Self {
        let map = Map::of(placement, indices);
        let drawn = indices
            .iter()
            .copied()
            .filter(|&index| map.drawn(index))
            .collect();
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(CARD_PADDING)
            .margin_bottom(CARD_PADDING)
            .margin_start(CARD_PADDING)
            .margin_end(CARD_PADDING)
            .build();
        // Vertical, so the card measures its height at its real width:
        // horizontal, it measures each wrapping label at its narrowest.
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["card"])
            .build();
        card.append(&content);
        let group = adw::PreferencesGroup::new();
        group.add(&card);
        let mut choices = Vec::new();
        let looks = Looks::default();
        let layers = match map.outline {
            Some(drawing) => {
                let (grid, layers) = outline(drawing, &mut choices, &looks);
                content.append(&grid);
                layers
            }
            None => Vec::new(),
        };
        if !map.unplaced.is_empty() {
            let list = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .spacing(6)
                .build();
            for index in map.unplaced {
                list.append(&button(&mut choices, index, gtk::Align::Start));
            }
            content.append(&list);
        }
        // Side columns of one width keep the outline centered on the card.
        let uniform = gtk::SizeGroup::new(gtk::SizeGroupMode::Both);
        for choice in &choices {
            uniform.add_widget(&choice.button);
        }
        if let Some((first, rest)) = choices.split_first() {
            for choice in rest {
                choice.button.set_group(Some(&first.button));
            }
        }
        Self {
            group,
            placement,
            choices,
            drawn,
            looks,
            layers,
        }
    }

    pub(super) fn group(&self) -> &adw::PreferencesGroup {
        &self.group
    }

    /// `picked` runs with a port's index each time that port becomes the
    /// picked one, by a click or by [`Self::select`].
    pub(super) fn connect_picked(&self, picked: impl Fn(u8) + Clone + 'static) {
        for choice in &self.choices {
            let (index, picked) = (choice.index, picked.clone());
            choice.button.connect_toggled(move |button| {
                if button.is_active() {
                    picked(index);
                }
            });
        }
    }

    pub(super) fn select(&self, index: u8) {
        if let Some(choice) = self.choices.iter().find(|choice| choice.index == index) {
            choice.button.set_active(true);
        }
    }

    pub(super) fn show(&self, ports: &[PortState]) {
        let mut looks = Vec::with_capacity(self.choices.len());
        for choice in &self.choices {
            let Some(state) = ports.iter().find(|state| state.index == choice.index) else {
                continue;
            };
            let line = port_line(self.placement, state, self.drawn.contains(&choice.index));
            let caption = line.caption();
            if choice.main.label() != line.main
                || choice.caption.label() != caption
                || choice.display.is_visible() != line.display
            {
                choice.main.set_label(&line.main);
                choice.display.set_visible(line.display);
                choice.caption.set_label(&caption);
                choice.caption.set_visible(!caption.is_empty());
                choice
                    .button
                    .update_property(&[gtk::accessible::Property::Label(&spoken(
                        self.placement,
                        state,
                    ))]);
            }
            let look = socket(state);
            let color: &[&str] = match look {
                Socket::InUse(Some(Flow::In)) => &[FLOW_IN_CLASS],
                Socket::InUse(Some(Flow::Out)) => &[FLOW_OUT_CLASS],
                _ => &[],
            };
            if choice
                .main
                .css_classes()
                .iter()
                .map(glib::GString::as_str)
                .ne(color.iter().copied())
            {
                choice.main.set_css_classes(color);
            }
            looks.push((choice.index, look));
        }
        if *self.looks.borrow() != looks {
            self.looks.replace(looks);
            for layer in &self.layers {
                layer.queue_draw();
            }
        }
    }
}

fn outline(
    outline: Outline,
    choices: &mut Vec<Choice>,
    looks: &Looks,
) -> (gtk::Grid, Vec<gtk::DrawingArea>) {
    let bands = i32::try_from(outline.bands).unwrap_or(1);
    let grid = gtk::Grid::builder()
        .row_homogeneous(true)
        .column_spacing(12)
        .halign(gtk::Align::Fill)
        .build();
    let top = i32::from(outline.back.is_some());
    if let Some(index) = outline.back {
        let toggle = button(choices, index, gtk::Align::Center);
        grid.attach(&toggle, Column::Back as i32, 0, 1, 1);
    }
    for (edge, column, halign) in [
        (&outline.left, Column::Left, gtk::Align::End),
        (&outline.right, Column::Right, gtk::Align::Start),
    ] {
        match edge {
            Edge::Placed(slots) => {
                for (band, slot) in (top..).zip(slots) {
                    if let Some(Slot::Port(index)) = slot {
                        let toggle = button(choices, *index, halign);
                        toggle.set_hexpand(true);
                        grid.attach(&toggle, column as i32, band, 1, 1);
                    }
                }
            }
            Edge::Listed(ports) => {
                let stack = gtk::Box::builder()
                    .orientation(gtk::Orientation::Vertical)
                    .spacing(6)
                    .valign(gtk::Align::Center)
                    .hexpand(true)
                    .build();
                for &index in ports {
                    stack.append(&button(choices, index, halign));
                }
                grid.attach(&stack, column as i32, top, 1, bands);
            }
        }
    }
    let drawing = gtk::Overlay::builder()
        .accessible_role(gtk::AccessibleRole::Presentation)
        .width_request(CHASSIS_BASE_WIDTH + CHASSIS_WIDTH_PER_BAND * bands + 2 * ARROW_ROOM)
        .height_request(BAND * bands)
        .build();
    let looks = Rc::clone(looks);
    let layers = layers::stack(&drawing, &LAYERS, move |area, cr, width, height, paint| {
        let frame = Frame::new(&outline, area.color(), f64::from(width), f64::from(height));
        frame.ink(cr, 1.0);
        paint(cr, &frame, &outline, &looks.borrow())
    });
    grid.attach(&drawing, Column::Back as i32, top, 1, bands);
    (grid, layers)
}

fn button(choices: &mut Vec<Choice>, index: u8, halign: gtk::Align) -> gtk::ToggleButton {
    let (xalign, justify) = match halign {
        gtk::Align::End => (1.0, gtk::Justification::Right),
        gtk::Align::Center => (0.5, gtk::Justification::Center),
        _ => (0.0, gtk::Justification::Left),
    };
    let label = |classes: &[&str]| {
        gtk::Label::builder()
            .xalign(xalign)
            .justify(justify)
            .wrap(true)
            .max_width_chars(26)
            .css_classes(classes)
            .build()
    };
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .valign(gtk::Align::Center)
        .build();
    let main = label(&[]);
    let display = gtk::Image::builder()
        .icon_name("video-display-symbolic")
        .tooltip_text(DISPLAY_PORT)
        .visible(false)
        .build();
    let headline = gtk::Box::builder().spacing(6).halign(halign).build();
    headline.append(&main);
    headline.append(&display);
    let caption = label(&["caption", "dim-label"]);
    caption.set_visible(false);
    content.append(&headline);
    content.append(&caption);
    let button = gtk::ToggleButton::builder()
        .child(&content)
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    choices.push(Choice {
        index,
        button: button.clone(),
        main,
        display,
        caption,
    });
    button
}

/// The chassis's edges within the area, inset for the half of each socket
/// outside them and the arrow beyond it.
struct Frame {
    color: gtk::gdk::RGBA,
    left: f64,
    right: f64,
    top: f64,
    bottom: f64,
    /// The area's, whose rows the buttons beside it share.
    area_height: f64,
    bands: f64,
}

impl Frame {
    fn new(outline: &Outline, color: gtk::gdk::RGBA, width: f64, height: f64) -> Self {
        let inset = SOCKET_SHORT / 2.0 + f64::from(ARROW_ROOM) + OUTLINE_WIDTH;
        let top = if outline.back.is_some() {
            inset
        } else {
            OUTLINE_WIDTH
        };
        Self {
            color,
            left: inset,
            right: width - inset,
            top,
            bottom: height - OUTLINE_WIDTH,
            area_height: height,
            #[expect(clippy::cast_precision_loss, reason = "a board has a handful of bands")]
            bands: outline.bands as f64,
        }
    }

    /// The area's own color at `alpha` of its opacity.
    fn ink(&self, cr: &cairo::Context, alpha: f32) {
        cr.set_source_color(&self.color.with_alpha(self.color.alpha() * alpha));
    }

    fn band_y(&self, band: usize) -> f64 {
        #[expect(clippy::cast_precision_loss, reason = "a board has a handful of bands")]
        let band = band as f64;
        (band + 0.5) * self.area_height / self.bands
    }

    fn width(&self) -> f64 {
        self.right - self.left
    }

    fn height(&self) -> f64 {
        self.bottom - self.top
    }

    fn back_x(&self) -> f64 {
        f64::midpoint(self.left, self.right)
    }

    /// Each socket's center and the edge it is on, with what it holds.
    fn sockets(&self, outline: &Outline) -> Vec<(f64, f64, Column, Slot)> {
        let mut sockets = Vec::new();
        if let Some(index) = outline.back {
            sockets.push((self.back_x(), self.top, Column::Back, Slot::Port(index)));
        }
        for (edge, x, column) in [
            (&outline.left, self.left, Column::Left),
            (&outline.right, self.right, Column::Right),
        ] {
            if let Edge::Placed(slots) = edge {
                for (band, slot) in slots.iter().enumerate() {
                    if let Some(slot) = slot {
                        sockets.push((x, self.band_y(band), column, *slot));
                    }
                }
            }
        }
        sockets
    }
}

fn rounded(cr: &cairo::Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    cr.new_sub_path();
    cr.arc(x + width - radius, y + radius, radius, -FRAC_PI_2, 0.0);
    cr.arc(
        x + width - radius,
        y + height - radius,
        radius,
        0.0,
        FRAC_PI_2,
    );
    cr.arc(x + radius, y + height - radius, radius, FRAC_PI_2, PI);
    cr.arc(x + radius, y + radius, radius, PI, PI + FRAC_PI_2);
    cr.close_path();
}

fn socket_path(cr: &cairo::Context, x: f64, y: f64, column: Column) {
    let (width, height) = if column == Column::Back {
        (SOCKET_LONG, SOCKET_SHORT)
    } else {
        (SOCKET_SHORT, SOCKET_LONG)
    };
    rounded(
        cr,
        x - width / 2.0,
        y - height / 2.0,
        width,
        height,
        SOCKET_RADIUS,
    );
}

/// Pointing into the machine for power coming in.
fn arrow_path(cr: &cairo::Context, x: f64, y: f64, column: Column, flow: Flow) {
    let (dx, dy) = column.outward();
    let near = SOCKET_SHORT / 2.0 + ARROW_GAP;
    let far = SOCKET_SHORT / 2.0 + f64::from(ARROW_ROOM);
    let (tip, base) = match flow {
        Flow::In => (near, far),
        Flow::Out => (far, near),
    };
    let half = ARROW_WIDTH / 2.0;
    cr.move_to(x + dx * tip, y + dy * tip);
    cr.line_to(x + dx * base - dy * half, y + dy * base + dx * half);
    cr.line_to(x + dx * base + dy * half, y + dy * base - dx * half);
    cr.close_path();
}

fn look(looks: &[(u8, Socket)], slot: Slot) -> Option<Socket> {
    let Slot::Port(index) = slot else {
        return None;
    };
    looks
        .iter()
        .find(|(port, _)| *port == index)
        .map(|(_, look)| *look)
}

fn paint_chassis(
    cr: &cairo::Context,
    frame: &Frame,
    outline: &Outline,
    looks: &[(u8, Socket)],
) -> Result<(), cairo::Error> {
    let (width, height) = (frame.width(), frame.height());
    frame.ink(cr, DECK_ALPHA);
    rounded(
        cr,
        frame.left + width * 0.1,
        frame.top + height * 0.1,
        width * 0.8,
        height * 0.45,
        CHASSIS_RADIUS / 3.0,
    );
    rounded(
        cr,
        frame.left + width * 0.32,
        frame.top + height * 0.64,
        width * 0.36,
        height * 0.26,
        CHASSIS_RADIUS / 3.0,
    );
    cr.fill()?;

    frame.ink(cr, OUTLINE_ALPHA);
    cr.set_line_width(OUTLINE_WIDTH);
    rounded(cr, frame.left, frame.top, width, height, CHASSIS_RADIUS);
    cr.stroke()?;

    frame.ink(cr, 1.0);

    cr.set_line_width(LISTED_WIDTH);
    cr.set_line_cap(cairo::LineCap::Round);
    for (edge, x) in [(&outline.left, frame.left), (&outline.right, frame.right)] {
        if matches!(edge, Edge::Listed(_)) {
            cr.move_to(x, frame.top + CHASSIS_RADIUS);
            cr.line_to(x, frame.bottom - CHASSIS_RADIUS);
            cr.stroke()?;
        }
    }

    let sockets = frame.sockets(outline);
    cr.save()?;
    cr.set_operator(cairo::Operator::Clear);
    for &(x, y, column, _) in &sockets {
        socket_path(cr, x, y, column);
    }
    cr.fill()?;
    cr.restore()?;

    cr.set_line_width(SOCKET_LINE_WIDTH);
    for &(x, y, column, slot) in &sockets {
        if look(looks, slot) == Some(Socket::Empty) {
            socket_path(cr, x, y, column);
        }
    }
    cr.stroke()?;

    paint_flowing(cr, frame, outline, looks, |flow| flow.is_none())
}

fn paint_unused(
    cr: &cairo::Context,
    frame: &Frame,
    outline: &Outline,
    looks: &[(u8, Socket)],
) -> Result<(), cairo::Error> {
    cr.set_line_width(SOCKET_LINE_WIDTH);
    for (x, y, column, slot) in frame.sockets(outline) {
        let dashes: &[f64] = match slot {
            Slot::Undriven => &[2.0, 2.0],
            Slot::Port(_) if look(looks, slot) == Some(Socket::Off) => &[],
            Slot::Port(_) => continue,
        };
        cr.set_dash(dashes, 0.0);
        socket_path(cr, x, y, column);
        cr.stroke()?;
    }
    Ok(())
}

/// The sockets in use whose flow `drawn` takes, each with its arrow.
fn paint_flowing(
    cr: &cairo::Context,
    frame: &Frame,
    outline: &Outline,
    looks: &[(u8, Socket)],
    drawn: impl Fn(Option<Flow>) -> bool,
) -> Result<(), cairo::Error> {
    for (x, y, column, slot) in frame.sockets(outline) {
        if let Some(Socket::InUse(flow)) = look(looks, slot)
            && drawn(flow)
        {
            socket_path(cr, x, y, column);
            if let Some(flow) = flow {
                arrow_path(cr, x, y, column, flow);
            }
        }
    }
    cr.fill()
}
