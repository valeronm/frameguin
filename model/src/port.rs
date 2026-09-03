//! Where a port's socket is on the machine, for the boards it has been
//! measured on.
//!
//! The EC numbers a port by which controller drives it and which of that
//! controller's two connectors it is — nothing in that number says where the
//! socket sits. The translation is a separate table inside the firmware, it
//! differs between boards that are otherwise alike, and `framework_lib`'s
//! one guess at it has this machine's ports on the right sides and front and
//! rear the wrong way round on both. So the table below is measured rather
//! than derived, one board at a time, and a board absent from it gets no
//! position at all: a wrong "left rear" reads exactly like a right one,
//! where a bare port number cannot mislead anyone.
//!
//! Positions are as seen from the keyboard with the lid open, which is the
//! only viewpoint a window on that screen can mean. Turning the machine over
//! to read its underside mirrors every one of them, so a position measured
//! that way is entered here flipped.

use frameguin_wire::BOARD_LAPTOP13_PRO_ULTRA_3;

/// The sockets of one board, in the EC's port order.
struct Layout {
    /// The board as its own firmware names it, matched whole — the product
    /// name, which is what a caller passes in.
    product: &'static str,
    /// Written as a heading, since that is where each one is shown.
    positions: &'static [&'static str],
}

const LAYOUTS: &[Layout] = &[Layout {
    product: BOARD_LAPTOP13_PRO_ULTRA_3,
    positions: &["Right front", "Right rear", "Left rear", "Left front"],
}];

/// Where port `index` is on `product`, and None on a board nobody has
/// measured or for a port past the ones that were.
#[must_use]
pub fn position(product: &str, index: u8) -> Option<&'static str> {
    LAYOUTS
        .iter()
        .find(|layout| layout.product == product)?
        .positions
        .get(usize::from(index))
        .copied()
}

/// What to call a port: where it is, on a board that has been measured, and
/// its number on one that has not. The position leads because it is what
/// someone looking for the cable can act on; the number is the EC's index
/// into its controllers and means nothing on the chassis.
#[must_use]
pub fn label(product: &str, index: u8) -> String {
    position(product, index).map_or_else(|| number(index), str::to_owned)
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
    use super::{label, position, secondary};

    use frameguin_wire::BOARD_LAPTOP13_PRO_ULTRA_3 as MEASURED;

    #[test]
    fn a_measured_board_leads_with_where_the_socket_is() {
        assert_eq!(position(MEASURED, 2), Some("Left rear"));
        assert_eq!(label(MEASURED, 2), "Left rear");
        assert_eq!(secondary(MEASURED, 2).as_deref(), Some("Port 2"));
    }

    /// Both controllers' pairs run rear-to-front against the EC's numbering
    /// on this board, which is the thing that cannot be guessed.
    #[test]
    fn the_measured_board_pairs_each_controller_to_one_side() {
        let sides: Vec<&str> = (0..4)
            .map(|index| position(MEASURED, index).unwrap())
            .collect();
        assert_eq!(
            sides,
            ["Right front", "Right rear", "Left rear", "Left front"]
        );
    }

    /// The number is the whole name where there is no position, so nothing
    /// repeats it underneath.
    #[test]
    fn an_unmeasured_board_is_named_by_its_number_alone() {
        assert_eq!(position("Laptop 16", 0), None);
        assert_eq!(label("Laptop 16", 0), "Port 0");
        assert_eq!(secondary("Laptop 16", 0), None);
    }

    #[test]
    fn a_port_past_the_measured_ones_gets_no_position() {
        assert_eq!(position(MEASURED, 4), None);
        assert_eq!(label(MEASURED, 4), "Port 4");
        assert_eq!(secondary(MEASURED, 4), None);
    }
}
