//! Where a port's socket is on the machine, for the ports whose place is
//! known.
//!
//! The EC numbers a port by which controller drives it and which of that
//! controller's two connectors it is — nothing in that number says where the
//! socket sits. The translation is a separate table inside the firmware, it
//! differs between boards that are otherwise alike, and `framework_lib`'s
//! one guess at it has this machine's ports on the right sides and front and
//! rear the wrong way round on both. So the table below holds only what is
//! known — measured one board at a time, or fixed by the controller a
//! port's number names where only one socket can be behind it — and a port
//! it does not place gets no position at all: a wrong "left rear" reads
//! exactly like a right one, where a bare port number cannot mislead anyone.
//!
//! Positions are as seen from the keyboard with the lid open, which is the
//! only viewpoint a window on that screen can mean. Turning the machine over
//! to read its underside mirrors every one of them, so a position measured
//! that way is entered here flipped.

use frameguin_wire::{
    BOARD_LAPTOP13_PRO_ULTRA_3, BOARD_LAPTOP16_AMD_7040, BOARD_LAPTOP16_AMD_AI_300,
};

/// Declared in the order ports are listed, which the derived `Ord` follows.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    Left,
    Right,
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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Position {
    /// The derived `Ord` compares side before depth.
    Side { side: Side, depth: Depth },
    /// The Laptop 16's expansion bay, whose module carries a port of its own.
    Back,
}

impl Position {
    const fn side(side: Side, depth: Depth) -> Self {
        Self::Side { side, depth }
    }

    /// Spelled as a heading.
    fn label(self) -> &'static str {
        match self {
            Self::Side { side, depth } => match (side, depth) {
                (Side::Left, Depth::Rear) => "Left rear",
                (Side::Left, Depth::Middle) => "Left middle",
                (Side::Left, Depth::Front) => "Left front",
                (Side::Right, Depth::Rear) => "Right rear",
                (Side::Right, Depth::Middle) => "Right middle",
                (Side::Right, Depth::Front) => "Right front",
            },
            Self::Back => "Back",
        }
    }
}

/// The sockets of one board, in the EC's port order, None for one nobody
/// has placed.
struct Layout {
    /// The board as its own firmware names it, matched whole — the product
    /// name, which is what a caller passes in.
    product: &'static str,
    positions: &'static [Option<Position>],
}

/// The EC declares the bay's controller third, driving one port, so the bay
/// is port 4 whatever the side slots turn out to be; those are unmeasured.
const LAPTOP16: &[Option<Position>] = &[None, None, None, None, Some(Position::Back)];

const LAYOUTS: &[Layout] = &[
    Layout {
        product: BOARD_LAPTOP13_PRO_ULTRA_3,
        positions: &[
            Some(Position::side(Side::Right, Depth::Front)),
            Some(Position::side(Side::Right, Depth::Rear)),
            Some(Position::side(Side::Left, Depth::Rear)),
            Some(Position::side(Side::Left, Depth::Front)),
        ],
    },
    Layout {
        product: BOARD_LAPTOP16_AMD_7040,
        positions: LAPTOP16,
    },
    Layout {
        product: BOARD_LAPTOP16_AMD_AI_300,
        positions: LAPTOP16,
    },
];

/// Where port `index` is on `product`, and None on a board with no layout,
/// for a port past its layout, and for one the layout leaves unplaced.
fn position(product: &str, index: u8) -> Option<Position> {
    LAYOUTS
        .iter()
        .find(|layout| layout.product == product)?
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
    position(product, index).map_or_else(|| number(index), |position| position.label().to_owned())
}

/// The key ports are listed by: left side then right, each rear to front,
/// then by number where no position is known, and the back last — the side
/// slots are the ports every such machine has, the bay's only the module
/// fitted in it.
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
/// match against a tool that only counts. None where the label is already
/// the number, there being nothing to add.
#[must_use]
pub fn secondary(product: &str, index: u8) -> Option<String> {
    position(product, index).map(|_| number(index))
}

fn number(index: u8) -> String {
    format!("Port {index}")
}

#[cfg(test)]
mod tests {
    use super::{label, order, secondary};

    use frameguin_wire::BOARD_LAPTOP13_PRO_ULTRA_3 as MEASURED;
    use frameguin_wire::{BOARD_LAPTOP16_AMD_7040, BOARD_LAPTOP16_AMD_AI_300};

    const UNMEASURED: &str = "Precision 5560";

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
    fn an_unmeasured_board_lists_by_number() {
        assert_eq!(listed(UNMEASURED, 0..4), [0, 1, 2, 3]);
    }

    /// The number is the whole name where there is no position, so nothing
    /// repeats it underneath.
    #[test]
    fn an_unmeasured_board_is_named_by_its_number_alone() {
        assert_eq!(label(UNMEASURED, 0), "Port 0");
        assert_eq!(secondary(UNMEASURED, 0), None);
    }

    #[test]
    fn a_port_past_the_measured_ones_gets_no_position() {
        assert_eq!(label(MEASURED, 4), "Port 4");
        assert_eq!(secondary(MEASURED, 4), None);
    }

    #[test]
    fn a_laptop_16_places_its_bay_port_at_the_back_and_leaves_its_sides_unplaced() {
        for board in [BOARD_LAPTOP16_AMD_7040, BOARD_LAPTOP16_AMD_AI_300] {
            assert_eq!(label(board, 4), "Back");
            assert_eq!(secondary(board, 4).as_deref(), Some("Port 4"));
            assert_eq!(label(board, 0), "Port 0");
        }
    }

    #[test]
    fn the_back_lists_after_the_unplaced_ports() {
        assert_eq!(listed(BOARD_LAPTOP16_AMD_7040, 0..5), [0, 1, 2, 3, 4]);
    }
}
