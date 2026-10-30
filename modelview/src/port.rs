//! What a port is called: where its socket is, for a port the board's
//! [`Placement`] places, and its number for one it does not.

use frameguin_model::port::{Depth, Placement, Position, Side};

/// Where a port is, as the forms below spell it.
enum Place {
    Unplaced,
    /// A side with no depth, which two sockets can share, so the port's
    /// number goes beside it.
    Shared(Side),
    Own(Socket),
}

#[derive(Clone, Copy)]
enum Socket {
    At(Side, Depth),
    Back,
}

impl Socket {
    fn heading(self) -> String {
        match self {
            Self::At(side, depth) => format!("{} {}", side_label(side), depth_label(depth)),
            Self::Back => "Back".to_owned(),
        }
    }

    fn sentence(self) -> String {
        match self {
            Self::At(..) => self.heading().to_lowercase(),
            Self::Back => "the back".to_owned(),
        }
    }
}

fn place_of(placement: Placement, index: u8) -> Place {
    match placement.position(index) {
        None => Place::Unplaced,
        Some(Position::Side { side, depth: None }) => Place::Shared(side),
        Some(Position::Side {
            side,
            depth: Some(depth),
        }) => Place::Own(Socket::At(side, depth)),
        Some(Position::Back) => Place::Own(Socket::Back),
    }
}

/// What to call a port: where it is, for a port that is placed, and its
/// number for one that is not. The position leads because it is what
/// someone looking for the cable can act on; the number is the EC's index
/// into its controllers and means nothing on the chassis.
#[must_use]
pub(crate) fn label(placement: Placement, index: u8) -> String {
    match place_of(placement, index) {
        Place::Unplaced => number(index),
        Place::Shared(side) => format!("{} · {}", side_label(side), number(index)),
        Place::Own(socket) => socket.heading(),
    }
}

/// A port's position and its number together, for heading what is shown of
/// it. The number is still what a reader has to match against a tool that
/// only counts.
#[must_use]
pub fn heading(placement: Placement, index: u8) -> String {
    match place_of(placement, index) {
        Place::Own(socket) => format!("{} · {}", socket.heading(), number(index)),
        _ => label(placement, index),
    }
}

/// What to call a port inside a line already separating its parts with
/// ` · `, where a side alone takes its number after a comma instead.
#[must_use]
pub(crate) fn inline(placement: Placement, index: u8) -> String {
    match place_of(placement, index) {
        Place::Shared(side) => format!("{}, {}", side_label(side), number(index).to_lowercase()),
        _ => label(placement, index),
    }
}

/// Where a port is, inside a sentence.
#[must_use]
pub(crate) fn place(placement: Placement, index: u8) -> String {
    match place_of(placement, index) {
        Place::Unplaced => number(index).to_lowercase(),
        Place::Shared(side) => format!(
            "{} on the {}",
            number(index).to_lowercase(),
            side_label(side).to_lowercase()
        ),
        Place::Own(socket) => socket.sentence(),
    }
}

fn side_label(side: Side) -> &'static str {
    match side {
        Side::Left => "Left",
        Side::Right => "Right",
    }
}

fn depth_label(depth: Depth) -> &'static str {
    match depth {
        Depth::Rear => "rear",
        Depth::Middle => "middle",
        Depth::Front => "front",
    }
}

fn number(index: u8) -> String {
    format!("Port {index}")
}

#[cfg(test)]
mod tests {
    use frameguin_contract::Platform;
    use frameguin_contract::Platform::Laptop13AmdAi300 as SIDED;
    use frameguin_contract::Platform::Laptop13ProUltra3 as MEASURED;
    use frameguin_model::port::Placement;

    use super::{heading, inline, label, place};

    #[test]
    fn the_default_places_nothing() {
        assert_eq!(label(Placement::default(), 2), "Port 2");
    }

    #[test]
    fn a_measured_board_leads_with_where_the_socket_is() {
        assert_eq!(label(Placement::of(MEASURED), 2), "Left rear");
        assert_eq!(heading(Placement::of(MEASURED), 2), "Left rear · Port 2");
    }

    #[test]
    fn the_measured_board_pairs_each_controller_to_one_side() {
        let sides: Vec<String> = (0..4)
            .map(|index| label(Placement::of(MEASURED), index))
            .collect();
        assert_eq!(
            sides,
            ["Right front", "Right rear", "Left rear", "Left front"]
        );
    }

    #[test]
    fn an_unknown_board_is_named_by_its_number_alone() {
        assert_eq!(label(Placement::of(Platform::Unknown), 0), "Port 0");
        assert_eq!(heading(Placement::of(Platform::Unknown), 0), "Port 0");
    }

    #[test]
    fn a_board_known_by_its_sides_names_the_side_and_the_number_together() {
        let names: Vec<String> = (0..4)
            .map(|index| label(Placement::of(SIDED), index))
            .collect();
        assert_eq!(
            names,
            [
                "Right · Port 0",
                "Right · Port 1",
                "Left · Port 2",
                "Left · Port 3"
            ]
        );
        assert_eq!(heading(Placement::of(SIDED), 2), "Left · Port 2");
    }

    #[test]
    fn a_heading_carries_the_position_and_the_number() {
        assert_eq!(heading(Placement::of(MEASURED), 2), "Left rear · Port 2");
        assert_eq!(heading(Placement::of(SIDED), 2), "Left · Port 2");
        assert_eq!(
            heading(Placement::of(Platform::Laptop16Amd7040), 4),
            "Back · Port 4"
        );
        assert_eq!(heading(Placement::of(Platform::Unknown), 2), "Port 2");
    }

    #[test]
    fn inside_a_line_a_side_takes_its_number_after_a_comma() {
        assert_eq!(inline(Placement::of(SIDED), 2), "Left, port 2");
        assert_eq!(inline(Placement::of(MEASURED), 2), "Left rear");
        assert_eq!(inline(Placement::of(Platform::Unknown), 2), "Port 2");
    }

    #[test]
    fn inside_a_sentence_a_place_is_lowercase() {
        assert_eq!(place(Placement::of(MEASURED), 2), "left rear");
        assert_eq!(place(Placement::of(SIDED), 2), "port 2 on the left");
        assert_eq!(
            place(Placement::of(Platform::Laptop16Amd7040), 4),
            "the back"
        );
        assert_eq!(place(Placement::of(Platform::Unknown), 2), "port 2");
    }

    #[test]
    fn a_port_past_the_measured_ones_gets_no_position() {
        assert_eq!(label(Placement::of(MEASURED), 4), "Port 4");
        assert_eq!(heading(Placement::of(MEASURED), 4), "Port 4");
    }

    #[test]
    fn a_laptop_16_places_its_bay_port_at_the_back_and_its_others_rear_and_middle() {
        for platform in [Platform::Laptop16Amd7040, Platform::Laptop16AmdAi300] {
            let placement = Placement::of(platform);
            assert_eq!(label(placement, 4), "Back");
            assert_eq!(heading(placement, 4), "Back · Port 4");
            let sides: Vec<String> = (0..4).map(|index| label(placement, index)).collect();
            assert_eq!(
                sides,
                ["Right rear", "Right middle", "Left middle", "Left rear"]
            );
        }
    }
}
