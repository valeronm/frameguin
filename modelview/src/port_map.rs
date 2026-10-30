//! Where each port sits on an outline of the machine seen from the keyboard:
//! the slots along each side, rear to front, and the one at the back.

use frameguin_model::port::{Depth, Placement, Position, Side};

/// What a slot along a measured side holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Port(u8),
    /// A slot no PD controller drives.
    Undriven,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Edge {
    /// One entry per band, rear to front, None where this side has no slot
    /// at that depth.
    Placed(Vec<Option<Slot>>),
    /// The side's ports, whose order along the edge nobody measured.
    Listed(Vec<u8>),
}

/// Every port among the indices once: on the outline where it is placed,
/// among the unplaced where it is not.
#[derive(Debug, PartialEq, Eq)]
pub struct Map {
    /// None where no port is placed.
    pub outline: Option<Outline>,
    /// In the order the indices were given.
    pub unplaced: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Outline {
    /// The rows the sides are divided into, rear to front.
    pub bands: usize,
    pub left: Edge,
    pub right: Edge,
    pub back: Option<u8>,
}

impl Map {
    #[must_use]
    pub fn of(placement: Placement, indices: &[u8]) -> Self {
        let mut placed = Vec::new();
        let mut unplaced = Vec::new();
        for &index in indices {
            match placement.position(index) {
                Some(position) => placed.push((index, position)),
                None => unplaced.push(index),
            }
        }
        Self {
            outline: (!placed.is_empty()).then(|| Outline::of(placement, &placed)),
            unplaced,
        }
    }

    /// A port listed along its side, its order there unmeasured, has no
    /// socket drawn.
    #[must_use]
    pub fn drawn(&self, index: u8) -> bool {
        let Some(outline) = &self.outline else {
            return false;
        };
        let placed = |edge: &Edge| match edge {
            Edge::Placed(slots) => slots.contains(&Some(Slot::Port(index))),
            Edge::Listed(_) => false,
        };
        outline.back == Some(index) || placed(&outline.left) || placed(&outline.right)
    }
}

impl Outline {
    fn of(placement: Placement, placed: &[(u8, Position)]) -> Self {
        let undriven = placement.undriven();
        let mut depths: Vec<Depth> = placed
            .iter()
            .map(|(_, position)| *position)
            .chain(undriven.iter().copied())
            .filter_map(|position| match position {
                Position::Side { depth, .. } => depth,
                Position::Back => None,
            })
            .collect();
        depths.sort_unstable();
        depths.dedup();
        let edge = |side: Side| {
            let on_side: Vec<(u8, Option<Depth>)> = placed
                .iter()
                .filter_map(|&(index, position)| match position {
                    Position::Side { side: at, depth } if at == side => Some((index, depth)),
                    _ => None,
                })
                .collect();
            if on_side.iter().any(|(_, depth)| depth.is_none()) {
                return Edge::Listed(on_side.iter().map(|(index, _)| *index).collect());
            }
            let undriven_at = |depth: Depth| undriven.contains(&Position::at(side, depth));
            Edge::Placed(
                depths
                    .iter()
                    .map(|&depth| {
                        on_side
                            .iter()
                            .find(|(_, at)| *at == Some(depth))
                            .map(|(index, _)| Slot::Port(*index))
                            .or(undriven_at(depth).then_some(Slot::Undriven))
                    })
                    .collect(),
            )
        };
        let (left, right) = (edge(Side::Left), edge(Side::Right));
        let listed = |edge: &Edge| match edge {
            Edge::Listed(ports) => ports.len(),
            Edge::Placed(_) => 0,
        };
        Self {
            bands: depths.len().max(listed(&left)).max(listed(&right)).max(1),
            left,
            right,
            back: placed
                .iter()
                .find(|(_, position)| *position == Position::Back)
                .map(|(index, _)| *index),
        }
    }
}

#[cfg(test)]
mod tests {
    use frameguin_contract::Platform;
    use frameguin_model::port::Placement;

    use super::{Edge, Map, Outline, Slot};

    fn map(platform: Platform, ports: u8) -> Map {
        let indices: Vec<u8> = (0..ports).collect();
        Map::of(Placement::of(platform), &indices)
    }

    #[test]
    fn only_a_port_at_a_depth_or_the_back_has_its_socket_drawn() {
        assert!((0..4).all(|index| map(Platform::Laptop13ProUltra3, 4).drawn(index)));
        assert!((0..4).all(|index| !map(Platform::Laptop13AmdAi300, 4).drawn(index)));
        assert!(map(Platform::Laptop16AmdAi300, 5).drawn(4));
        assert!(!map(Platform::Unknown, 4).drawn(0));
    }

    #[test]
    fn a_measured_board_puts_each_port_at_its_depth() {
        assert_eq!(
            map(Platform::Laptop13ProUltra3, 4),
            Map {
                outline: Some(Outline {
                    bands: 2,
                    left: Edge::Placed(vec![Some(Slot::Port(2)), Some(Slot::Port(3))]),
                    right: Edge::Placed(vec![Some(Slot::Port(1)), Some(Slot::Port(0))]),
                    back: None,
                }),
                unplaced: Vec::new(),
            }
        );
    }

    #[test]
    fn a_laptop_16_draws_its_undriven_front_slots_and_its_bay() {
        assert_eq!(
            map(Platform::Laptop16AmdAi300, 5).outline,
            Some(Outline {
                bands: 3,
                left: Edge::Placed(vec![
                    Some(Slot::Port(3)),
                    Some(Slot::Port(2)),
                    Some(Slot::Undriven)
                ]),
                right: Edge::Placed(vec![
                    Some(Slot::Port(0)),
                    Some(Slot::Port(1)),
                    Some(Slot::Undriven)
                ]),
                back: Some(4),
            })
        );
    }

    #[test]
    fn a_board_known_by_its_sides_lists_each_sides_ports() {
        assert_eq!(
            map(Platform::Laptop13AmdAi300, 4).outline,
            Some(Outline {
                bands: 2,
                left: Edge::Listed(vec![2, 3]),
                right: Edge::Listed(vec![0, 1]),
                back: None,
            })
        );
    }

    #[test]
    fn a_board_with_nothing_placed_leaves_every_port_unplaced() {
        assert_eq!(
            map(Platform::Unknown, 4),
            Map {
                outline: None,
                unplaced: vec![0, 1, 2, 3],
            }
        );
    }

    #[test]
    fn a_port_with_no_position_is_unplaced() {
        let measured = map(Platform::Laptop13ProUltra3, 5);
        assert_eq!(
            measured.outline,
            map(Platform::Laptop13ProUltra3, 4).outline
        );
        assert_eq!(measured.unplaced, vec![4]);
    }
}
