# Chassis and privacy switches over the EC

Switches the EC reads on its own pins and reports by host command, and the
input deck whose power it gates on detecting it. All are read and never
set. The EC calls are `hardware/src/ec.rs`; the devices are
`hardware/src/device/chassis.rs` and `hardware/src/device/privacy_switches.rs`.

Each section holds what the EC tree or this code establishes, then under
**Observed** what was read on a machine. Every observation is from the
Laptop 13 Pro (Intel Core Ultra Series 3).

## Contents

Every heading in the file appears here.

- [The chassis open switch](#the-chassis-open-switch)
- [The intrusion record](#the-intrusion-record)
- [The privacy switches](#the-privacy-switches)
- [The input deck](#the-input-deck)
- [Open](#open)
- [Sources](#sources)

## The chassis open switch

| | |
|---|---|
| Command | `EC_CMD_CHASSIS_OPEN_CHECK`, `0x3E0F`, in `chassis.c` |
| Reads | `gpio_chassis_open_l` on the spot, active low; answers 1 for open |
| Pin | named in the Laptop 13 Intel Core Ultra board's devicetree, which the Pro's includes |

## The intrusion record

`EC_CMD_CHASSIS_INTRUSION`, `0x3E09`, reads four bytes of battery-backed RAM
when both request bytes are zero and writes when either is not:

| Request byte | Nonzero value | Effect |
|---|---|---|
| `clear_magic` | `0xCE`, `EC_PARAM_CHASSIS_INTRUSION_MAGIC` | zeroes both counts and writes `0xEC` into the marker slot |
| second byte | any | clears `chassis_ever_opened` |

| Response field | BBRAM slot | Meaning |
|---|---|---|
| `chassis_ever_opened` | `CHASSIS_WAS_OPEN` | set by any opening; cleared only by the second write, which nothing in the EC sends |
| `coin_batt_ever_remove` | `CHASSIS_MAGIC` | `0xEC` once the counts have been zeroed by command, 0 where they never were |
| `total_open_count` | `CHASSIS_TOTAL` | openings while the EC runs |
| `vtr_open_count` | `CHASSIS_VTR_OPEN` | times the EC started with the chassis already open |

- Both counts stop at 255.
- Either write answers success with an empty response, which `framework_lib`'s
  typed send reports as a size error after the write has landed.
- `framework_lib` compares the marker with 1, so its "coin cell ever
  removed" is false on every board.
- `EC_CMD_CHASSIS_COUNTER`, `0x3E15`, answers how many times the chassis
  opened while the machine was off and zeroes that count as it answers,
  `chassis_cmd_clear(0)`. The firmware keeps it for the BIOS to collect at
  POST, so a read from the host takes it away.

### Observed

| Field | Reading |
|---|---|
| `chassis_ever_opened` | 0 |
| `vtr_open_count` | 1 |

## The privacy switches

| | |
|---|---|
| Command | `EC_CMD_PRIVACY_SWITCHES_CHECK_MODE`, `0x3E14`, in `board_host_command.c` |
| Reads | `gpio_mic_sw` inverted and `gpio_cam_sw` as is: the camera's pin is low when off, the microphone's high |
| Answers | 1 for a device connected |
| Console | prints both levels on every read |
| Boards | every Framework EC branch, the 11th to 13th Gen Intel boards' included |

- The handler has no path for a board that wires something else to those
  pins. Consequence: a command that answers vouches for the command and not
  for what the pins carry, which is what `PrivacySwitches::detect` probes.

### Observed

| Setup | Reading |
|---|---|
| Microphone pin against its slider | follows the slider |
| Camera pin, slider on, camera idle | 0 |
| Camera pin, camera running, its LED lit | 1 |
| The Laptop Webcam Module (2nd Gen)'s UVC privacy control, either slider position | 0 |

- The 11th Gen Intel board's pin table names the camera pin a monitor of the
  camera's power, which fits the readings above.

## The input deck

The EC powers the input deck only once it detects it, polling every 10 ms
while the host is on: `input_module_13.c` on a Laptop 13, `input_module.c`
on a Laptop 16.

Detection on a Laptop 13 is the touchpad board's ID resistor on an ADC pin,
whose band depends on the deck's own rail:

| Rail | ID | Means |
|---|---|---|
| off | above `BOARD_VERSION_10` | no touchpad |
| on | below `BOARD_VERSION_1` | no touchpad |

- Consequence: the touchpad's board ID is a presence reading, not a
  revision, and no table in the firmware names those bands for these boards.

`EC_CMD_CHECK_DECK_STATE`, `0x3E16`, answers the state machine's position and
takes a mode:

| Mode | Effect |
|---|---|
| 0 | read only |
| 1 | return to detection, `DECK_DISCONNECTED` |
| 2 | force on, `DECK_FORCE_ON` |
| 4 | force off, `DECK_FORCE_OFF`; cuts the deck's power on a running machine |

| State | Value | `wire::DeckState` |
|---|---|---|
| `DECK_OFF` | 0 | `Off`; off with the host |
| `DECK_DISCONNECTED` | 1 | `Disconnected` |
| `DECK_TURNING_ON` | 2 | `TurningOn` |
| `DECK_ON` | 3 | `On` |
| `DECK_FORCE_OFF` | 4 | `ForceOff` |
| `DECK_FORCE_ON` | 5 | `ForceOn` |
| `DECK_NO_DETECTION` | 6 | `NoDetection`; powered with the host, no presence check |

- The mode is saved to flash, `FLASH_FLAGS_INPUT_MODULE_POWER` in
  `board_function.c`, and read back at init, so a forced deck stays forced
  across an EC restart, and a deck forced on stays powered whether or not it
  is detected.
- Every Zephyr Framework branch carries the command; `hx20` and `hx30` do
  not.
- `framework_lib`'s `InputDeckState` conversion panics on a value it does
  not know, which is why `ec.rs` decodes the byte itself and answers None.

### Observed

| Setup | Reading |
|---|---|
| Touchpad board ID, deck powered | 12; the audio board's reads 11 beside it |
| Deck state on a usable machine | on |

## Open

- Which boards, if any, carry the camera slider on the camera pin rather
  than the camera's power.

## Sources

- [FrameworkComputer/EmbeddedController](https://github.com/FrameworkComputer/EmbeddedController)
  — under `zephyr/program/framework/`, `src/chassis.c` for the three
  chassis commands and their BBRAM slots, `src/board_host_command.c` for the
  privacy switches, `src/input_module_13.c` and `include/input_module_13.h`
  for the deck's detection, states and command, `src/board_function.c` for
  the flash flag, and `include/board_host_command.h` for the command ids.
- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  — `framework_lib/src/chromium_ec/mod.rs` for the coin-cell comparison and
  `chromium_ec/input_deck.rs` for the panicking conversion.
