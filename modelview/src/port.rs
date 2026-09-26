//! What a port is called: where its socket is, for a port the board's
//! [`Placement`] places, and its number for one it does not.

use frameguin_model::port::{Depth, Placement, Position, Side};

/// What to call a port: where it is, for a port that is placed, and its
/// number for one that is not. The position leads because it is what
/// someone looking for the cable can act on; the number is the EC's index
/// into its controllers and means nothing on the chassis.
#[must_use]
pub fn label(placement: Placement, index: u8) -> String {
    placement
        .position(index)
        .map_or_else(|| number(index), |position| position_label(position, index))
}

/// What to call a port inside a line already separating its parts with
/// ` · `, where a side alone takes its number after a comma instead.
#[must_use]
pub fn inline(placement: Placement, index: u8) -> String {
    match placement.position(index) {
        Some(Position::Side { side, depth: None }) => {
            format!("{}, port {index}", side_label(side))
        }
        _ => label(placement, index),
    }
}

/// The port's number as a line of its own, for showing under a [`label`]
/// that named a position instead — the number is still what a reader has to
/// match against a tool that only counts. None where the label already
/// carries the number.
#[must_use]
pub fn secondary(placement: Placement, index: u8) -> Option<String> {
    match placement.position(index)? {
        Position::Side { depth: None, .. } => None,
        _ => Some(number(index)),
    }
}

/// Spelled as a heading. A side alone carries the port's number, two
/// sockets on one side otherwise sharing a name.
fn position_label(position: Position, index: u8) -> String {
    match position {
        Position::Side { side, depth: None } => {
            format!("{} · {}", side_label(side), number(index))
        }
        Position::Side {
            side,
            depth: Some(depth),
        } => format!("{} {}", side_label(side), depth_label(depth)),
        Position::Back => "Back".to_owned(),
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

    use super::{inline, label, secondary};

    #[test]
    fn the_default_places_nothing() {
        assert_eq!(label(Placement::default(), 2), "Port 2");
    }

    #[test]
    fn a_measured_board_leads_with_where_the_socket_is() {
        assert_eq!(label(Placement::of(MEASURED), 2), "Left rear");
        assert_eq!(
            secondary(Placement::of(MEASURED), 2).as_deref(),
            Some("Port 2")
        );
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
        assert_eq!(secondary(Placement::of(Platform::Unknown), 0), None);
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
        assert_eq!(secondary(Placement::of(SIDED), 2), None);
    }

    #[test]
    fn inside_a_line_a_side_takes_its_number_after_a_comma() {
        assert_eq!(inline(Placement::of(SIDED), 2), "Left, port 2");
        assert_eq!(inline(Placement::of(MEASURED), 2), "Left rear");
        assert_eq!(inline(Placement::of(Platform::Unknown), 2), "Port 2");
    }

    #[test]
    fn a_port_past_the_measured_ones_gets_no_position() {
        assert_eq!(label(Placement::of(MEASURED), 4), "Port 4");
        assert_eq!(secondary(Placement::of(MEASURED), 4), None);
    }

    #[test]
    fn a_laptop_16_places_its_bay_port_at_the_back_and_its_others_by_side() {
        for platform in [Platform::Laptop16Amd7040, Platform::Laptop16AmdAi300] {
            let placement = Placement::of(platform);
            assert_eq!(label(placement, 4), "Back");
            assert_eq!(secondary(placement, 4).as_deref(), Some("Port 4"));
            assert_eq!(label(placement, 0), "Right · Port 0");
            assert_eq!(label(placement, 3), "Left · Port 3");
        }
    }
}
