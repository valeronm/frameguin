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
//! number cannot mislead anyone. Beside a measured position sits the
//! socket's wiring — the two USB root ports it reaches — which the kernel
//! links to no connector on these boards, so a device is placed only where
//! that was measured too.
//!
//! Positions are as seen from the keyboard with the lid open, which is the
//! only viewpoint a window on that screen can mean. Turning the machine over
//! to read its underside mirrors every one of them, so a position measured
//! that way is entered here flipped.

use frameguin_contract::{Attached, Platform};

/// Declared in the order ports are listed, which the derived `Ord` follows.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    Left,
    Right,
}

/// Declared rear to front, the order ports along one side are listed in.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Depth {
    Rear,
    Middle,
    Front,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Position {
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
}

/// A root port as the kernel names it: its controller's PCI address and its
/// number on that controller's root hub.
#[derive(Clone, Copy)]
struct RootPort {
    controller: &'static str,
    port: u8,
}

/// The two root ports one socket reaches, on separate controllers on Intel
/// boards whose Type-C lanes run to the processor.
#[derive(Clone, Copy)]
struct Wiring {
    superspeed: RootPort,
    usb2: RootPort,
}

/// The chipset's USB controller on the Core Ultra Series 3.
const PCH: &str = "0000:00:14.0";
/// The processor's Type-C controller on the Core Ultra Series 3.
const TCSS: &str = "0000:00:0d.0";

const fn pro_ultra_3(superspeed: u8, usb2: u8) -> Wiring {
    Wiring {
        superspeed: RootPort {
            controller: TCSS,
            port: superspeed,
        },
        usb2: RootPort {
            controller: PCH,
            port: usb2,
        },
    }
}

/// The sockets of the boards named, in the EC's port order, None for one
/// nobody has placed.
struct Layout {
    platforms: &'static [Platform],
    positions: &'static [Option<Position>],
    /// Each port's root ports, in the EC's port order, None for one nobody
    /// measured.
    wiring: &'static [Option<Wiring>],
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
        platforms: &[
            Platform::Laptop13Gen11,
            Platform::Laptop13Gen12,
            Platform::Laptop13Gen13,
            Platform::Laptop13Ultra1,
            Platform::Laptop13Amd7040,
            Platform::Laptop13AmdAi300,
            Platform::Laptop12Gen13,
            // framework-system gives this board the 13th-gen Laptop 12's PD
            // layout.
            Platform::Laptop12Core3,
        ],
        positions: &[RIGHT, RIGHT, LEFT, LEFT],
        wiring: &[],
    },
    Layout {
        platforms: &[Platform::Laptop13ProUltra3],
        positions: &[
            Some(Position::at(Side::Right, Depth::Front)),
            Some(Position::at(Side::Right, Depth::Rear)),
            Some(Position::at(Side::Left, Depth::Rear)),
            Some(Position::at(Side::Left, Depth::Front)),
        ],
        wiring: &[
            Some(pro_ultra_3(4, 3)),
            Some(pro_ultra_3(3, 2)),
            Some(pro_ultra_3(2, 5)),
            Some(pro_ultra_3(1, 4)),
        ],
    },
    // The same controller table's sides, and the EC declaring the bay's
    // controller third, driving one port.
    Layout {
        platforms: &[Platform::Laptop16Amd7040, Platform::Laptop16AmdAi300],
        positions: &[RIGHT, RIGHT, LEFT, LEFT, Some(Position::Back)],
        wiring: &[],
    },
];

/// Where one board's ports are. The default places nothing, as a board no
/// layout names does.
#[derive(Clone, Copy, Default)]
pub struct Placement {
    layout: Option<&'static Layout>,
}

impl Placement {
    #[must_use]
    pub fn of(platform: Platform) -> Self {
        let layout = LAYOUTS
            .iter()
            .find(|layout| layout.platforms.contains(&platform));
        Self { layout }
    }

    /// None for a port past the layout, and for one the layout leaves
    /// unplaced.
    #[must_use]
    pub fn position(self, index: u8) -> Option<Position> {
        self.layout?
            .positions
            .get(usize::from(index))
            .copied()
            .flatten()
    }

    /// Whether any socket on the board has its root ports measured.
    #[must_use]
    pub fn wired(self) -> bool {
        self.layout
            .is_some_and(|layout| layout.wiring.iter().any(Option::is_some))
    }

    /// The devices on port `index`'s socket, its `SuperSpeed` half first, and
    /// none where the socket's wiring was not measured.
    #[must_use]
    pub fn attached(self, index: u8, devices: &[Attached]) -> Vec<&Attached> {
        let Some(wiring) = self
            .layout
            .and_then(|layout| layout.wiring.get(usize::from(index)))
            .copied()
            .flatten()
        else {
            return Vec::new();
        };
        let on = |root: RootPort| {
            devices.iter().filter(move |device| {
                device.controller == root.controller && device.root_port == root.port
            })
        };
        on(wiring.superspeed).chain(on(wiring.usb2)).collect()
    }

    /// The key ports are listed by: left side then right, each rear to front
    /// where measured and by number where only the side is known, then by
    /// number where no position is known, and the back last — the side slots
    /// are the ports every such machine has, the bay's only the module fitted
    /// in it.
    #[must_use]
    pub fn order(self, index: u8) -> impl Ord {
        let position = self.position(index);
        let group = match position {
            Some(Position::Side { .. }) => 0,
            None => 1,
            Some(Position::Back) => 2,
        };
        (group, position, index)
    }
}

#[cfg(test)]
mod tests {
    use super::Placement;

    use frameguin_contract::Platform::Laptop13AmdAi300 as SIDED;
    use frameguin_contract::Platform::Laptop13ProUltra3 as MEASURED;
    use frameguin_contract::{Attached, Platform, UsbSpeed};

    fn products(placement: Placement, index: u8, devices: &[Attached]) -> Vec<&str> {
        placement
            .attached(index, devices)
            .iter()
            .map(|d| d.product.as_str())
            .collect()
    }

    fn listed(platform: Platform, indices: std::ops::Range<u8>) -> Vec<u8> {
        let placement = Placement::of(platform);
        let mut indices: Vec<u8> = indices.collect();
        indices.sort_by_key(|index| placement.order(*index));
        indices
    }

    #[test]
    fn the_default_places_nothing() {
        let placement = Placement::default();
        assert!(placement.position(2).is_none());
        assert!(!placement.wired());
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
        assert_eq!(listed(Platform::Unknown, 0..4), [0, 1, 2, 3]);
    }

    #[test]
    fn a_board_known_by_its_sides_lists_the_left_side_then_the_right_by_number() {
        assert_eq!(listed(SIDED, 0..4), [2, 3, 0, 1]);
    }

    #[test]
    fn a_port_past_the_measured_ones_gets_no_position() {
        assert!(Placement::of(MEASURED).position(4).is_none());
    }

    #[test]
    fn the_back_lists_after_the_sides() {
        assert_eq!(listed(Platform::Laptop16Amd7040, 0..5), [2, 3, 0, 1, 4]);
    }

    fn on(controller: &str, root_port: u8, product: &str) -> Attached {
        Attached {
            controller: controller.to_owned(),
            root_port,
            vendor_id: 0x32ac,
            product_id: 0x0002,
            manufacturer: "Framework".to_owned(),
            product: product.to_owned(),
            speed: UsbSpeed::Full,
            firmware: String::new(),
            network: Vec::new(),
            storage: Vec::new(),
        }
    }

    #[test]
    fn only_a_measured_board_is_wired() {
        assert!(Placement::of(MEASURED).wired());
        assert!(!Placement::of(SIDED).wired());
        assert!(!Placement::of(Platform::Unknown).wired());
    }

    #[test]
    fn a_device_on_either_half_of_a_socket_lands_on_its_port() {
        let devices = [
            on("0000:00:14.0", 5, "card"),
            on("0000:00:0d.0", 1, "drive"),
        ];
        assert_eq!(products(Placement::of(MEASURED), 2, &devices), ["card"]);
        assert_eq!(products(Placement::of(MEASURED), 3, &devices), ["drive"]);
    }

    #[test]
    fn a_hub_on_both_halves_lists_its_superspeed_half_first() {
        let devices = [
            on("0000:00:14.0", 3, "usb2"),
            on("0000:00:0d.0", 4, "superspeed"),
        ];
        assert_eq!(
            products(Placement::of(MEASURED), 0, &devices),
            ["superspeed", "usb2"]
        );
    }

    #[test]
    fn a_device_on_a_root_port_no_socket_lists_lands_nowhere() {
        let devices = [on("0000:00:14.0", 6, "webcam")];
        assert!((0..4).all(|index| Placement::of(MEASURED).attached(index, &devices).is_empty()));
    }

    #[test]
    fn an_unwired_board_attaches_nothing() {
        let devices = [on("0000:00:14.0", 5, "card")];
        assert!(Placement::of(SIDED).attached(2, &devices).is_empty());
    }
}
