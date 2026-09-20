# Hardware notes

What the embedded controller and the devices behind it actually do, as found
while building frameguin. These are facts about the machine rather than about
this app: they hold whoever is talking to the hardware, and most of them are
not written down anywhere else, which is why they kept being rediscovered.

Where a claim comes from firmware or a datasheet it is cited by name — the
ChromiumOS EC tree Framework forks, TI's documents for the battery gauge —
rather than by line number, which rots.

The EC transport is [`hardware/ec.md`](hardware/ec.md). The chapters here
are the controls; the battery, read and set over the EC, is
[`hardware/battery.md`](hardware/battery.md). Each control
chapter ends with a persistence section, what survives a suspend, a reboot
and an EC restart, because which of the three a control survives does not
follow from what the control does. An EC restart takes the machine down
with it ([EC restarts](hardware/ec.md#ec-restarts)), so that column asks
whether a value outlives the EC that holds it.

Framework is a trademark of Framework Computer Inc.; this is an independent
project and names the hardware only descriptively.

## Contents

Every heading in the file appears here.

<!-- GitHub's slugger drops the ² from "I²C"; that anchor is right as written. -->

- [What survives what](#what-survives-what)
- [Keyboard backlight](#keyboard-backlight)
  - [Keyboard backlight persistence](#keyboard-backlight-persistence)
- [Chassis and privacy switches](#chassis-and-privacy-switches)
  - [The chassis open switch](#the-chassis-open-switch)
  - [The privacy switches](#the-privacy-switches)
  - [The input deck](#the-input-deck)
- [Sources](#sources)

## What survives what

Every control chapter's persistence section in one grid. Each row links to the
section that carries the mechanism and how it was established; nothing here is
stronger than the section it points at, so a row reading Unknown, or one whose
section marks its finding untested, means exactly that.

| Control | Suspend | Reboot | EC restart |
|---|---|---|---|
| [Charge limit](hardware/battery.md#persistence) | Kept | **Lost** | Kept |
| [Charge current limit](hardware/battery.md#persistence) | Kept | Kept | **Lost** |
| [Power button LED level](hardware/led.md#persistence) | Kept | **Lost** | Kept |
| [Power button LED darkness](hardware/led.md#persistence) | Kept | **Lost** | **Lost** |
| [Charging LED colour](hardware/led.md#persistence) | Kept | **Lost** | **Lost** |
| [Keyboard backlight](#keyboard-backlight-persistence) | Kept | Kept | Kept |
| [Haptic touchpad](hardware/touchpad.md#persistence) | Kept | Kept | Kept |
| [Touchscreen, pad route](hardware/touchscreen.md#persistence) | **Lost** | **Lost** | not a case |
| [Touchscreen, panel route](hardware/touchscreen.md#persistence) | Unknown | Unknown | Unknown |
| [USB-C port enable](hardware/usb-c.md#persistence) | Unknown | Unknown | Unknown |

The pad route loses its setting to a fourth event the columns cannot carry —
the lid opening — and the panel route is the Laptop 12's, where none of the
pad's findings apply.

## Keyboard backlight

Read it with `EcRequestPwmGetKeyboardBacklight`, which returns the stored
percentage exactly. `framework_lib::get_keyboard_backlight()` goes through PWM
duty instead and floors twice — percent to duty in the EC, then duty back to
percent in the library — so most values come back one low: 5% reads as 4%.

The EC is a **second writer**. Fn+Space changes it, and newer boards have a
firmware auto mode. Anything showing the value has to re-read rather than
trusting what it last wrote.

### Keyboard backlight persistence

The EC saves this one rather than resetting it. On the way to shutdown it
writes the current brightness into BBRAM — or a marker standing for auto,
where the firmware auto mode is on — and restores it when it next initializes,
so a level set from the OS is still there after a reboot and after an EC
restart alike. A suspend never reaches the save at all, the EC staying up. The
Fn-lock state shares that same byte.

**Whether BIOS setup re-sends it is untested**, where the charge limit and the
power button LED level demonstrably are. The one keyboard-backlight write seen
at boot lands after the kernel's own EC probe, so it is the host restoring a
saved level rather than firmware, and POST itself cannot be watched here — see
[the charge current limit](hardware/battery.md#persistence) for why. This control has a
getter, so the test is the one that settled those two: set a distinctive
level, reboot, and read it back.

## Chassis and privacy switches

Switches the EC reads on its own pins and reports by host command, and the
input deck whose power it gates on detecting it.

### The chassis open switch

`EC_CMD_CHASSIS_OPEN_CHECK` (`0x3E0F`) reads the switch's pin on the spot,
active low, and answers 1 for open. The Laptop 13 Pro's firmware reads it on
the pin the Laptop 13 Intel Core Ultra board's devicetree names, which the
Pro's includes.

`EC_CMD_CHASSIS_INTRUSION` (`0x3E09`) reads four bytes of battery-backed RAM
when both of its request bytes are zero, and writes instead when either is
not: `0xCE` in the first zeroes both counts and stamps a marker, anything
nonzero in the second clears the opened flag. Either write answers success
with an empty response, which `framework_lib`'s typed send reports as a size
error after the write has landed.

- `chassis_ever_opened` is set by any opening and cleared only by that second
  write, which nothing in the EC sends: a Laptop 13 Pro reads it 0 beside a
  `vtr_open_count` of 1.
- `coin_batt_ever_remove` is the marker's slot, `0xEC` once the counts have
  been zeroed by command and 0 where they never were. `framework_lib` compares
  it with 1, so its "coin cell ever removed" is false on every board.
- `total_open_count` counts openings while the EC runs.
- `vtr_open_count` counts the times the EC started with the chassis already
  open.

Both counts stop at 255.

`EC_CMD_CHASSIS_COUNTER` (`0x3E15`) answers how many times the chassis opened
while the machine was off and zeroes that count as it answers. The firmware
keeps it for the BIOS to collect at POST, so a read from the host takes it
away.

### The privacy switches

`EC_CMD_PRIVACY_SWITCHES_CHECK_MODE` (`0x3E14`) reports the levels of two
pins as the camera and microphone switches, whose polarities are opposite —
the camera's low when off, the microphone's high — and answers 1 for a device
connected. The handler has no path for a board that wires something else to
those pins, so a command that answers says nothing about what they carry.
Every read prints two lines to the EC console, and every Framework EC branch
carries the command, the 11th to 13th generation Intel boards' included.

On the Laptop 13 Pro the microphone's pin follows its slider and the camera's
does not: it reads 1 only while the camera is running, its LED lit, and 0 with
the slider on and the camera idle. The Laptop Webcam Module (2nd Gen) on that
machine reports no slider either, its UVC privacy control reading 0 in both
positions. The 11th generation Intel board's pin table names the camera pin a
monitor of the camera's power, which fits what the Pro shows; which boards, if
any, carry the camera slider on that pin is untested.

### The input deck

The EC powers the input deck only once it detects it, polling every 10 ms
while the host is on (`input_module_13.c`, `input_module.c` on the
Laptop 16). On a Laptop 13 the detection is the touchpad board's ID resistor
read on an ADC pin, and its band depends on the deck's own rail: with the rail
off an ID above 10 is no touchpad, with it on an ID below 1 is none. So the
touchpad's board ID, 12 on the Laptop 13 Pro with the deck powered and the
audio board's 11 beside it, is a presence reading rather than a revision, and
no table in the firmware names those bands for these boards.

`EC_CMD_CHECK_DECK_STATE` (`0x3E16`) answers the state machine's position:
off with the host, disconnected, turning on, on, forced off, forced on, or
powered without detection. Its mode byte reads at 0 and writes otherwise: 1
returns to detection, 2 forces the deck on, and 4 forces it off, cutting the
deck's power on a running machine. The mode is saved to flash
(`FLASH_FLAGS_INPUT_MODULE_POWER` in `board_function.c`), so a forced deck
stays forced across an EC restart, and a deck forced on stays powered whether
or not it is detected. A usable machine reads on unless forced. Every Zephyr Framework
branch carries the command; `hx20` and `hx30` do not.

## Sources

- [FrameworkComputer/EmbeddedController](https://github.com/FrameworkComputer/EmbeddedController)
  — the ChromiumOS EC fork these boards run. Under
  `zephyr/program/framework/src/`, `chassis.c` holds the chassis switch's
  host commands and its counts, `board_host_command.c` the privacy
  switches', and `input_module_13.c` the Laptop 13's input deck detection.
  The keyboard backlight is `common/keyboard_backlight.c` and `common/pwm.c`.
