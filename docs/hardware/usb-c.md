# USB-C ports

Every USB-C port is driven by a Cypress CCG controller that the EC reaches
over I²C. The host reaches the controller's own registers by
[I²C passthrough](../hardware.md#reaching-the-ec) and the EC's copy of each
port's state by host command. The controller, not the port, is the unit the
hardware is organized around, and everything the host reads about a port is
the EC's cache of it.

Each section holds what the EC tree, `framework_lib` or the kernel
establishes, then under **Observed** what was read on a machine. Every observation is
from a Laptop 13 Pro (Intel Core Ultra Series 3) with two CCG8 controllers,
silicon ID `0x3E81`, unless its row names another board. A board driving its
ports with a CCG5 or CCG6 may answer differently.

## Contents

Every heading in the file appears here.

- [Topology](#topology)
- [Port index](#port-index)
- [Port map, observed](#port-map-observed)
- [Host commands](#host-commands)
- [Port state fields](#port-state-fields)
- [Controller registers](#controller-registers)
- [Attached devices](#attached-devices)
  - [Slot halves](#slot-halves)
  - [Expansion cards](#expansion-cards)
  - [Network adapters](#network-adapters)
  - [Storage bridges](#storage-bridges)
  - [DisplayPort](#displayport)
- [Port enable](#port-enable)
- [Persistence](#persistence)
- [Open](#open)
- [Sources](#sources)

## Topology

| Board | Controllers | Ports | Source |
|---|---|---|---|
| Laptop 12, Laptop 13 | 2 | 2 + 2 | `CONFIG_PLATFORM_EC_PD_CHIP_MAX_COUNT` default |
| Laptop 16 | 3 | 2 + 2 + 1 | `lotus/project.conf` raises the count; the third controller is on the expansion bay module |
| Desktop | 3 | | `tulip/project.conf` raises the count; its ports are not covered here |

- A controller answers at its own I²C address on its own EC bus, and both
  its ports come with it. Nothing addresses a port on its own.
- The Laptop 16's third controller is declared in `usbc_5port_config.c`
  with address `0xFF` and a `CCG_STATE_NO_POWER` initial state;
  `ccg8s_init` fills the address in when the module powers up. Its
  controller count is a fact about what is installed, not about the machine.
- Controller I²C addresses, from `framework_lib`'s `PdPort` table:
  controller 0 at `0x42` (CCG8 boards) or `0x08` (CCG5 and CCG6 boards),
  controller 1 at `0x40`.

How many controllers a machine has, two ways to ask:

| Method | Cost | Answers |
|---|---|---|
| `EC_CMD_READ_PD_VERSION` v1 | no I²C transfer | a count and that many version blobs; `read_pd_version` stops at the first all-zero cached version |
| Silicon ID read per controller | one I²C read each, needs an address table | which part each controller is |

### Observed

| Setup | Reading |
|---|---|
| Silicon ID, both controllers | `0x3E81` |

## Port index

`EC_CMD_GET_PD_PORT_STATE` takes a port number that indexes the EC's
`pd_port_states` array directly. The array is controller-major, as the
charge-port code in `cypd_ccg6.c` writes it:

```
index = controller × 2 + connector
```

| Index | Controller | Connector |
|---|---|---|
| 0 | 0 | 0 |
| 1 | 0 | 1 |
| 2 | 1 | 0 |
| 3 | 1 | 1 |

- The index encodes which controller and which of its connectors, and
  nothing about where the socket is on the chassis.
- Socket position is a per-board table, the UCSI connector maps.
  Consequence: no index-to-position table is right for a whole family.
- `cmd_get_pd_port_state` answers `EC_RES_INVALID_PARAM` for any port at
  or past `PD_PORT_COUNT`, and the Linux driver passes the refusal on. The
  guard was `>` until
  [EC commit `a999906`](https://github.com/FrameworkComputer/EmbeddedController/commit/a999906f300ada3e14ecded27ab44564a5e30624)
  made it `>=`, so firmware built before it accepts a port number equal to
  the count and reads past the array
  ([framework-system #253](https://github.com/FrameworkComputer/framework-system/issues/253)).
- The bound on the port count is controllers × 2, from
  `EC_CMD_READ_PD_VERSION`. It over-counts by one on a Laptop 16, whose
  third controller drives one port.

UCSI connector to controller.connector, per map file:

| Map | Boards | UCSI 1 | UCSI 2 | UCSI 3 | UCSI 4 | UCSI 5 |
|---|---|---|---|---|---|---|
| `ucsi_port_12.c` | `sunflower` | 0.1 | 0.2 | 1.2 | 1.1 | |
| `ucsi_port_13.c` | `azalea`, `marigold` | 0.1 | 0.2 | 1.1 | 1.2 | |
| `sakura/src/ucsi_port.c` | `sakura` | 0.2 | 0.1 | 1.2 | 1.1 | |
| `ucsi_port_16.c` | `lotus` | 0.1 | 0.2 | 1.1 | 1.2 | GPU.1 |

- The shared Laptop 13 map is commented as covering most Laptop 13
  mainboards, with a board that differs carrying its own file. The Laptop
  13 Pro is that board, and its map swaps the two connectors on both
  controllers.

What `framework_lib` claims about position:

| Claim | Where |
|---|---|
| Index to side and front/rear, one Laptop 16 special case | hardcoded match in `get_and_print_pd_info`, printed only by `--pdports-chromebook` |
| Controller 0's pair is the right side, controller 1's the left, no front or rear | `PdPort::Right01`, `PdPort::Left23`, on every Laptop board it knows |

### Observed

| Setup | Reading |
|---|---|
| `EC_CMD_GET_PD_PORT_STATE` for port 4 on this 4-port board, EC `sakura-3.0.2-cf48815` | success, every field set, 65535 mV at 65535 mA: the off-by-one above |
| `framework_tool --pdports-chromebook` against the measured map below | sides right, front and rear reversed on both |
| `PdPort::Right01` and `Left23` against the measured map | right |

- Consequence of the first row: whether the EC objects to port 4 depends
  on the firmware version, so the port count cannot be found by walking
  until it does.

## Port map, observed

Positions are as seen from the keyboard with the lid open. The machine
turned over shows the mirror image of every one.

| EC port | Controller | Slot | USB 2.0 root port (`0000:00:14.0`) | SuperSpeed root port (`0000:00:0d.0`) |
|---|---|---|---|---|
| 0 | I²C `0x42` | right front | 3 | 4 |
| 1 | I²C `0x42` | right rear | 2 | 3 |
| 2 | I²C `0x40` | left rear | 5 | 2 |
| 3 | I²C `0x40` | left front | 4 | 1 |

- Method: a source attached to one slot at a time, reading which index
  reported the contract. Ports 1, 2 and 3 measured; port 0 is the remainder.
- EC port 0 is UCSI's second connector, `ucsi-source-psy-USBC000:002`,
  connectors numbered from 1. Only this pair was measured, and it matches
  the `sakura` map above.
- None of this table transfers to another board.

## Host commands

| Command | Id | Answers | Caveat |
|---|---|---|---|
| `EC_CMD_READ_PD_VERSION` | | a version per controller, cached while bringing them up; v1 adds the count | |
| `EC_CMD_GET_PD_PORT_STATE` | | the EC's `pd_port_states` entry for one index, plus `pd_alt_mode_status` read live from the controller | the cache, not the port; see [Port state fields](#port-state-fields) |
| `EC_CMD_USB_PD_POWER_INFO` | `0x0103` | `charge_manager_fill_power_info` for one port | restates what `cypress_pd_common.c` fed the charge manager, the same voltage and current it stores in `pd_port_states`; measures nothing |

`EC_CMD_USB_PD_POWER_INFO` fields on a build with
`CONFIG_PLATFORM_EC_USB_PD_VBUS_MEASURE_NOT_PRESENT` (`sakura`, `marigold`,
`azalea`, `sunflower`, `lotus`):

| Field | From source | Why |
|---|---|---|
| `voltage_now` | 5000 on a port sourcing; 0 on a port sinking | `get_vbus_voltage` returns 5000 for a source and, with no VBUS ADC, 0 otherwise. `tulip` alone measures it, through its charger |
| `dualrole` | false on every port | `cypress_pd_common.c` registers each port `CAP_DEDICATED` |
| `current_max` | the current the machine offers a device | |
| voltage, current | the negotiated values and their product | |

### Observed

| Setup | Reading |
|---|---|
| `voltage_now`, port charging | 0 |
| `voltage_now`, port sourcing a peripheral | a constant 5 V |
| `dualrole`, every port | false |
| `current_max`, empty port | 1.5 A |
| `current_max`, port sourcing a Type-C peripheral | 1.5 A |

## Port state fields

Everything the EC serves about a port is the contract, the PDO offered and
the RDO requested, or Type-C's own advertisement where no contract was made.
None of it is a measurement.

| Field | From source |
|---|---|
| voltage, current | the contract; with no contract, `TYPE_C_VOLTAGE` and the Type-C current for a sink; 0 for nothing attached |
| `pd_state` | a flag, zero for no PD contract; `framework_lib` prints it Yes or No |
| `c_state`, `power_role`, `data_role` | Cypress enums, which `framework_lib` prints by name |
| `vconn` | whether the machine powers the cable's or accessory's chips |
| `pd_alt_mode_status` | the controller's `DP_ALT_MODE_CONFIG` register, read live and passed on untouched; bits below |
| `active_port` | whether this port is the active charge port |
| `epr_active`, `epr_support` | whether an extended-power-range contract is active, and whether the partner supports one |
| `cc_polarity` | which CC line the connection is on |

`pd_alt_mode_status` bits, as `framework_lib` decodes them:

| Bit | Meaning | Note |
|---|---|---|
| 0 | DFP_D connected | the EC treats it as the DisplayPort bit shifted down on a port without Thunderbolt mode, so its check and this app's `video` mask bits 0 and 1 together |
| 1 | UFP_D connected | |
| 2 | power low | |
| 3 | enabled | |
| 4 | multi-function | |
| 5 | USB config | |
| 6 | exit request | |
| 7 | HPD high | |

Cache staleness, from `cypress_pd_common.c`: `pd_port_states` is filled from
the controllers' interrupts. A controller whose ports are disabled raises
none, and the EC neither clears nor marks the entry.

### Observed

- A port disabled with a 90 W display attached kept reporting the contract,
  its voltage and current, and alternate mode for as long as it stayed
  disabled, display dark; a source attached during that window appeared
  only on re-enable. Consequence: a port reading is meaningless without its
  controller's [port mask](#port-enable), and a disabled controller's ports
  are unreported, not stale.
- Voltage and current read alike under a contract and under a Type-C
  advertisement, and `pd_state` alone separates them.
- `data_role`, `vconn`, `cc_polarity` and `epr_active` read as the partner
  is: two chargers gave opposite `data_role` and `vconn`, the polarity is
  the cable's orientation, and EPR is the charger's contract. None is a
  fact about the port itself.

## Controller registers

HPI addresses, from Infineon's open-source host library; the EC's names for
the same offsets are in `cypress_pd_common.h`. A connector's register block
is `0x1000` for a controller's first connector and `0x2000` for its second.

| HPI name | EC name | Address | Units |
|---|---|---|---|
| `BUS_VOLTAGE` | `CCG_TYPE_C_VOLTAGE_REG` | block + `0x0D` | 100 mV |
| `DP_ALT_MODE_CONFIG` | `CCG_DP_ALT_MODE_CONFIG_REG` | block + `0x2B` | bits above |
| `BUS_CURRENT` | `CCG_PORT_CURRENT_REG`, printed `TYPE_C_CURRENT` | block + `0x58` | 50 mA |
| `PORT_HOST_CAP` | `CCG_PORT_HOST_CAP_REG` | block + `0x5C` | |
| sink PDO EPR mask | `SELECT_SINK_PDO_EPR_MASK` | block + `0x65` | |
| `PDPORT_ENABLE` | `CCG_PDPORT_ENABLE_REG` | `0x2C` | bitmask, one bit per port |

- The EC reads `BUS_VOLTAGE` and `BUS_CURRENT` only in its console dump.
- No port current reading exists anywhere: the four ports pass through load
  switches into one adapter node before the charger, so no charger-side
  reading can be attributed to a port, and
  [the charger](../hardware.md#the-charger-itself) reports none anyway.
  Over-current protection is a comparator against a threshold and yields no
  number.

### Observed

| Register | Setup | Reading |
|---|---|---|
| `BUS_VOLTAGE` | port under a 20 V contract | 19.7 to 20.1 V, drifting per sample, beside a negotiated value of exactly 20000 mV |
| `BUS_VOLTAGE` | port sourcing a peripheral | 4.9 to 5.1 V |
| `BUS_VOLTAGE` | empty port | 0; a few hundred mV during an attach |
| `BUS_CURRENT` | both controllers, sinking 20 V, sourcing 5 V, attached, detached | `0xFF` every time |
| `PORT_HOST_CAP`, sink PDO EPR mask | same reads | data, so `0xFF` above is the register's own answer |
| block + `0x6C` onward | same reads | NAK |
| `PDPORT_ENABLE` | after each write | reads back what was written |

- Consequence: `BUS_VOLTAGE` is an ADC and not the contract restated, and
  `BUS_CURRENT` reports nothing on a CCG8.

## Attached devices

### Slot halves

From the kernel:

- A slot's two halves are on different controllers: USB 2.0 on the
  chipset, `0000:00:14.0`; SuperSpeed on the processor's Type-C controller,
  `0000:00:0d.0`.
- The kernel's port mapper links a root port to a Type-C connector by ACPI
  `physical_location`, so a `connector` or `peer` link appears only where
  the locations differ.
- A root port's `location` attribute reads the same on both halves of one
  slot.
- A USB bus number is enumeration order and names no controller.

#### Observed

| Setup | Reading |
|---|---|
| ACPI `physical_location`, all four Type-C connectors | identical, `unknown`/`upper`/`left`, so no root port carries a `connector` or `peer` link |
| `location`, `usb3-port5` and `usb2-port2` | both `0x80000004`; the pairing predicted three SuperSpeed ports after one was measured, all held |
| Kernel Type-C connector numbers against the EC's order and the USB order | follow neither |
| A hub behind a card | appears on both halves of one slot |
| HDMI expansion card as partner | sink, 5 V / 680 mA, VCONN on |
| USB 3 drive as partner | sink, 5 V / 1500 mA, no PD contract |

- Consequence: a slot is keyed by controller PCI address and root port.

### Expansion cards

- Framework's cards announce the names they are sold under: the `product`
  string, `Framework` as manufacturer, vendor id `32ac`.
- `framework_lib`'s version check sends `magic_unlock`, reads feature
  report `0xE0`, then calls `flashing_mode`, so the check as a whole leaves
  the card in flashing mode.
- `decode_fw_info` asserts the unlocked report's signature is `CY`.

#### Observed

| Product id | `product` string |
|---|---|
| `0002` | `HDMI Expansion Card` |
| `0003` | `DisplayPort Expansion Card` |
| `0009` | `SD Expansion Card` |

| Setup | Reading |
|---|---|
| Feature report `0xE0` from an HDMI or DisplayPort card, no unlock sent | answered; signature `AA` instead of `CY`, otherwise the same layout: silicon id, a UID matching the USB serial, and a version matching `framework_tool --dp-hdmi-info` |

- Consequence: a card is named by what it says about itself. An id-to-name
  table buys nothing and risks a wrong name for a third-party device sharing
  a bridge chip's ids.

### Network adapters

From the kernel:

- The interface sits in the net class under the USB interface,
  `2-2:1.0/net/<interface>`.
- `carrier` is refused on an interface set down with `ip link set … down`
  and reads `0` with the cable out; `operstate` reads `down` in both cases,
  so only `carrier` tells them apart.
- `speed` is refused on a driver that keeps no rate.
- `speed` and `duplex` are each the driver's own call. `r8152` and some
  other USB Ethernet drivers read PHY registers over USB control transfers
  on every call; the rest answer from the kernel's record.

#### Observed

| Setup | Reading |
|---|---|
| `speed` on `iwlwifi`, link up | refused |

### Storage bridges

From the kernel:

- Disks sit as SCSI devices under the USB interface,
  `2-3:1.0/host1/target1:0:0/1:0:0:N`.
- `size` is the block layer's count of 512-byte sectors, kept by the
  kernel, so reading it does not reach the drive.

#### Observed

| Setup | Reading |
|---|---|
| A bridge's devices beside the drive | a CD emulation, peripheral type `5`, with a size of its own; an image slot, type `0`, size `0` while empty, as a card reader's slot is |

- Consequence: a capacity is a device of type `0` whose `size` is not `0`.
  Type alone counts the empty slot, size alone the CD emulation.

### DisplayPort

- DRM connectors are the processor's Type-C outputs, each naming its own
  in its AUX channel: `card0-DP-1` is `AUX USBC1/DDI TC1/PHY TC1`, through
  TC4.
- Nothing in sysfs records the slot-to-connector pairing: the connectors'
  ACPI nodes carry no position, and the kernel's Type-C port has no
  DisplayPort device or connector link.
- The EC takes no part in it. `sakura`, `marigold` and `sunflower` build with
  `CONFIG_USB_PD_ALTMODE_INTEL=n`, so the controllers enter the mode
  themselves and the TC output is settled past the EC.
- What the EC adds is a 30 s timeout: a partner whose VDM carries a
  Framework vendor and product id from `cypd_altmode_ids` (the HDMI and
  DisplayPort cards and five Framework power adapters) and has not entered
  DisplayPort mode by then has its port's mux set to safe.

#### Observed

| Setup | Reading |
|---|---|
| One USB-C DisplayPort monitor in the left rear, left front and right front slots | `DP-1` every time |

- Consequence: a connector does not name a slot; the board hands an output
  to whichever slot asks for one.

## Port enable

| | |
|---|---|
| Register | `PDPORT_ENABLE`, HPI address `0x2C`, one bit per port |
| Readback | the register itself |

Who writes it:

| Writer | Value | When |
|---|---|---|
| `framework_lib` `enable_ports` | `0b11` or `0b00` behind a boolean | on request |
| The EC, `cypd_reset_pd_chip` | `0` only | the start of a controller reset: up to 650 ms for the ports to discharge, interrupts cleared, 1 s, then a chip reset that restarts the controller's firmware with its ports enabled |

- The EC never writes the mask back.
- The EC defines `CCG_PDPORT_DISABLE` (`0x00`) and `CCG_PDPORT_ENABLE`
  (`0x01`) and uses neither.

### Observed

| Setup | Reading |
|---|---|
| Write `0` | power, data and alternate mode stop; an attached display goes dark |
| Write `0b11` after that | the mask reads back, alternate mode returns, a source attached while off is negotiated without being unplugged |

## Persistence

| Event | Expected | Source |
|---|---|---|
| Suspend | mask stands | nothing in the EC writes the register outside a controller reset |
| Reboot, Laptop 13 Pro | mask cleared, ports back | `sakura/project.conf` sets `CONFIG_PLATFORM_EC_PD_RESET_BEFORE_EC_REBOOT`, the one board in the tree that does; `board_reset_pd_mcu` then resets every controller on the EC's way into a reboot |
| Reboot, other boards | mask stands | `board_reset_pd_mcu` compiles to a return |
| EC restart | mask stands while the controller keeps its supply | nothing in the EC touches the register |

### Observed

Nothing. Every row is an expectation from the source, and the mask
[written back](#port-enable) is the only recovery demonstrated.

## Open

- Whether a controller honors a per-port mask, `0b01`.
- Every row of [Persistence](#persistence).
- The reset hook's path on the Laptop 13 Pro.
- Every observation above on a CCG5 or CCG6 controller.

## Sources

- [FrameworkComputer/EmbeddedController](https://github.com/FrameworkComputer/EmbeddedController)
  — the ChromiumOS EC fork these boards run. Under
  `zephyr/program/framework/`, `src/cypress_pd_common.c` and
  `include/cypress_pd_common.h` carry the controller registers, the
  port-enable write and the reset path; `src/board_host_command.c` the
  port-state and version host commands; `src/ucsi_port_*.c` and
  `sakura/src/ucsi_port.c` the per-board connector maps, selected in
  `CMakeLists.txt`; `src/usbc_5port_config.c` the Laptop 16's third
  controller; and each board's `project.conf` its controller count and
  whether the controllers are reset before an EC reboot.
  `common/charge_manager.c` at the top level is `EC_CMD_USB_PD_POWER_INFO`.
- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  — `framework_lib/src/ccgx/device.rs` for the controller addresses and the
  port-enable write, `framework_lib/src/power.rs` for the port-state
  printout and the alternate-mode bits, `framework_lib/src/ccgx/hid.rs` for
  the expansion card's firmware report.
- [Infineon/hpi](https://github.com/Infineon/hpi) — the vendor's own host-side
  HPI library, whose `cy_hpi_defines_default.h` publishes the register map in
  the clear: every port register's offset, name and units, including those the
  EC only prints. The specification the register map belongs to is under NDA.
