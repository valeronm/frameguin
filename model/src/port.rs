//! Where a port's socket is on the machine, for the ports whose place is
//! known.
//!
//! The EC numbers a port by which controller drives it and which of that
//! controller's two connectors it is — nothing in that number says where the
//! socket sits, and which socket a connector reaches differs between boards
//! that are otherwise alike. So the table below holds only what is known: the
//! side Framework's own controller table names for a controller, a full
//! position where a board was measured, and the back where only one socket
//! can be behind a controller. A port it does not place gets no position at
//! all: a wrong "left rear" reads exactly like a right one, where a bare port
//! number cannot mislead anyone.
//!
//! Positions are as seen from the keyboard with the lid open, which is the
//! only viewpoint a window on that screen can mean. Turning the machine over
//! to read its underside mirrors every one of them, so a position measured
//! that way is entered here flipped.

use frameguin_wire::{
    BOARD_LAPTOP12_13TH_GEN, BOARD_LAPTOP12_CORE_3, BOARD_LAPTOP13_11TH_GEN,
    BOARD_LAPTOP13_12TH_GEN, BOARD_LAPTOP13_13TH_GEN, BOARD_LAPTOP13_AMD_7040,
    BOARD_LAPTOP13_AMD_7040_UNSPACED, BOARD_LAPTOP13_AMD_AI_300, BOARD_LAPTOP13_PRO_ULTRA_3,
    BOARD_LAPTOP13_ULTRA_1, BOARD_LAPTOP16_AMD_7040, BOARD_LAPTOP16_AMD_AI_300,
};

/// Declared in the order ports are listed, which the derived `Ord` follows.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    Left,
    Right,
}

impl Side {
    fn label(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }
}

/// Declared rear to front, the order ports along one side are listed in.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Depth {
    Rear,
    #[expect(
        dead_code,
        reason = "a Laptop 16 has three slots a side, and no Laptop 16 is measured yet"
    )]
    Middle,
    Front,
}

impl Depth {
    fn label(self) -> &'static str {
        match self {
            Self::Rear => "rear",
            Self::Middle => "middle",
            Self::Front => "front",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Position {
    /// The derived `Ord` compares side before depth. No depth is a side that
    /// was named rather than measured.
    Side { side: Side, depth: Option<Depth> },
    /// The Laptop 16's expansion bay, whose module carries a port of its own.
    Back,
}

impl Position {
    const fn at(side: Side, depth: Depth) -> Self {
        Self::Side {
            side,
            depth: Some(depth),
        }
    }

    /// Spelled as a heading. A side alone carries the port's number, two
    /// sockets on one side otherwise sharing a name.
    fn label(self, index: u8) -> String {
        match self {
            Self::Side { side, depth: None } => format!("{} · {}", side.label(), number(index)),
            Self::Side {
                side,
                depth: Some(depth),
            } => format!("{} {}", side.label(), depth.label()),
            Self::Back => "Back".to_owned(),
        }
    }
}

/// The sockets of the boards named, in the EC's port order, None for one
/// nobody has placed.
struct Layout {
    /// Each board by its DMI product name, matched whole.
    products: &'static [&'static str],
    positions: &'static [Option<Position>],
}

const RIGHT: Option<Position> = Some(Position::Side {
    side: Side::Right,
    depth: None,
});
const LEFT: Option<Position> = Some(Position::Side {
    side: Side::Left,
    depth: None,
});

const LAYOUTS: &[Layout] = &[
    // Framework's own controller table — `framework_lib`'s `ccgx::device`,
    // `PdPort::Right01` and `PdPort::Left23` — puts the first controller's
    // ports on the right and the second's on the left, and names no front or
    // rear.
    Layout {
        products: &[
            BOARD_LAPTOP13_11TH_GEN,
            BOARD_LAPTOP13_12TH_GEN,
            BOARD_LAPTOP13_13TH_GEN,
            BOARD_LAPTOP13_ULTRA_1,
            BOARD_LAPTOP13_AMD_7040,
            BOARD_LAPTOP13_AMD_7040_UNSPACED,
            BOARD_LAPTOP13_AMD_AI_300,
            BOARD_LAPTOP12_13TH_GEN,
            // framework-system gives this board the 13th-gen Laptop 12's PD
            // layout.
            BOARD_LAPTOP12_CORE_3,
        ],
        positions: &[RIGHT, RIGHT, LEFT, LEFT],
    },
    Layout {
        products: &[BOARD_LAPTOP13_PRO_ULTRA_3],
        positions: &[
            Some(Position::at(Side::Right, Depth::Front)),
            Some(Position::at(Side::Right, Depth::Rear)),
            Some(Position::at(Side::Left, Depth::Rear)),
            Some(Position::at(Side::Left, Depth::Front)),
        ],
    },
    // The same controller table's sides, and the EC declaring the bay's
    // controller third, driving one port.
    Layout {
        products: &[BOARD_LAPTOP16_AMD_7040, BOARD_LAPTOP16_AMD_AI_300],
        positions: &[RIGHT, RIGHT, LEFT, LEFT, Some(Position::Back)],
    },
];

/// Where port `index` is on `product`, and None on a board with no layout,
/// for a port past its layout, and for one the layout leaves unplaced.
fn position(product: &str, index: u8) -> Option<Position> {
    LAYOUTS
        .iter()
        .find(|layout| layout.products.contains(&product))?
        .positions
        .get(usize::from(index))
        .copied()
        .flatten()
}

/// What to call a port: where it is, for a port that is placed, and its
/// number for one that is not. The position leads because it is what
/// someone looking for the cable can act on; the number is the EC's index
/// into its controllers and means nothing on the chassis.
#[must_use]
pub fn label(product: &str, index: u8) -> String {
    position(product, index).map_or_else(|| number(index), |position| position.label(index))
}

/// What to call a port inside a line already separating its parts with
/// ` · `, where a side alone takes its number after a comma instead.
#[must_use]
pub fn inline(product: &str, index: u8) -> String {
    match position(product, index) {
        Some(Position::Side { side, depth: None }) => format!("{}, port {index}", side.label()),
        _ => label(product, index),
    }
}

/// The key ports are listed by: left side then right, each rear to front
/// where measured and by number where only the side is known, then by number
/// where no position is known, and the back last — the side slots are the
/// ports every such machine has, the bay's only the module fitted in it.
#[must_use]
pub fn order(product: &str, index: u8) -> impl Ord {
    let position = position(product, index);
    let group = match position {
        Some(Position::Side { .. }) => 0,
        None => 1,
        Some(Position::Back) => 2,
    };
    (group, position, index)
}

/// The port's number as a line of its own, for showing under a [`label`]
/// that named a position instead — the number is still what a reader has to
/// match against a tool that only counts. None where the label already
/// carries the number.
#[must_use]
pub fn secondary(product: &str, index: u8) -> Option<String> {
    match position(product, index)? {
        Position::Side { depth: None, .. } => None,
        _ => Some(number(index)),
    }
}

fn number(index: u8) -> String {
    format!("Port {index}")
}

#[cfg(test)]
mod tests {
    use super::{inline, label, order, secondary};

    use frameguin_wire::BOARD_LAPTOP13_AMD_AI_300 as SIDED;
    use frameguin_wire::BOARD_LAPTOP13_PRO_ULTRA_3 as MEASURED;
    use frameguin_wire::{BOARD_LAPTOP16_AMD_7040, BOARD_LAPTOP16_AMD_AI_300};

    const UNKNOWN: &str = "Precision 5560";

    fn listed(product: &str, indices: std::ops::Range<u8>) -> Vec<u8> {
        let mut indices: Vec<u8> = indices.collect();
        indices.sort_by_key(|index| order(product, *index));
        indices
    }

    #[test]
    fn a_measured_board_leads_with_where_the_socket_is() {
        assert_eq!(label(MEASURED, 2), "Left rear");
        assert_eq!(secondary(MEASURED, 2).as_deref(), Some("Port 2"));
    }

    /// Both controllers' pairs run rear-to-front against the EC's numbering
    /// on this board, which is the thing that cannot be guessed.
    #[test]
    fn the_measured_board_pairs_each_controller_to_one_side() {
        let sides: Vec<String> = (0..4).map(|index| label(MEASURED, index)).collect();
        assert_eq!(
            sides,
            ["Right front", "Right rear", "Left rear", "Left front"]
        );
    }

    #[test]
    fn a_measured_board_lists_the_left_side_then_the_right_each_rear_to_front() {
        assert_eq!(listed(MEASURED, 0..4), [2, 3, 1, 0]);
    }

    #[test]
    fn a_port_with_no_position_lists_after_the_placed_ones() {
        assert_eq!(listed(MEASURED, 0..5), [2, 3, 1, 0, 4]);
    }

    #[test]
    fn an_unknown_board_lists_by_number() {
        assert_eq!(listed(UNKNOWN, 0..4), [0, 1, 2, 3]);
    }

    #[test]
    fn an_unknown_board_is_named_by_its_number_alone() {
        assert_eq!(label(UNKNOWN, 0), "Port 0");
        assert_eq!(secondary(UNKNOWN, 0), None);
    }

    #[test]
    fn a_board_known_by_its_sides_names_the_side_and_the_number_together() {
        let names: Vec<String> = (0..4).map(|index| label(SIDED, index)).collect();
        assert_eq!(
            names,
            [
                "Right · Port 0",
                "Right · Port 1",
                "Left · Port 2",
                "Left · Port 3"
            ]
        );
        assert_eq!(secondary(SIDED, 2), None);
    }

    #[test]
    fn inside_a_line_a_side_takes_its_number_after_a_comma() {
        assert_eq!(inline(SIDED, 2), "Left, port 2");
        assert_eq!(inline(MEASURED, 2), "Left rear");
        assert_eq!(inline(UNKNOWN, 2), "Port 2");
    }

    #[test]
    fn a_board_known_by_its_sides_lists_the_left_side_then_the_right_by_number() {
        assert_eq!(listed(SIDED, 0..4), [2, 3, 0, 1]);
    }

    #[test]
    fn a_port_past_the_measured_ones_gets_no_position() {
        assert_eq!(label(MEASURED, 4), "Port 4");
        assert_eq!(secondary(MEASURED, 4), None);
    }

    #[test]
    fn a_laptop_16_places_its_bay_port_at_the_back_and_its_others_by_side() {
        for board in [BOARD_LAPTOP16_AMD_7040, BOARD_LAPTOP16_AMD_AI_300] {
            assert_eq!(label(board, 4), "Back");
            assert_eq!(secondary(board, 4).as_deref(), Some("Port 4"));
            assert_eq!(label(board, 0), "Right · Port 0");
            assert_eq!(label(board, 3), "Left · Port 3");
        }
    }

    #[test]
    fn the_back_lists_after_the_sides() {
        assert_eq!(listed(BOARD_LAPTOP16_AMD_7040, 0..5), [2, 3, 0, 1, 4]);
    }
}
