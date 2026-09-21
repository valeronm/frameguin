# Touchscreen over a processor pad or the panel's HID

One control, reached two unrelated ways, and a machine has one or the
other: a pad on the processor where the panel takes no command, or the
panel's own vendor command where it does. The pad route is
`hardware/src/gpio.rs` through the kernel's GPIO character device; the
panel route is `hardware/src/panel.rs` over HID; which a machine has, and
the role over either, is `hardware/src/touchscreen.rs`; the device and its
mirror are `hardware/src/device/touchscreen.rs`.

Each section holds what the EC tree, `framework_lib`, Framework's published
schematics and pinouts, ACPI tables or this code establish, then under
**Observed** what was read on a machine. Every observation is from the
Laptop 13 Pro (Intel Core Ultra Series 3) unless its row names another
board.

## Contents

Every heading in the file appears here.

- [The two routes](#the-two-routes)
- [The panel command](#the-panel-command)
- [The display connector](#the-display-connector)
- [The enable pad](#the-enable-pad)
- [What else drives the pad](#what-else-drives-the-pad)
- [Persistence](#persistence)
- [Open](#open)
- [Sources](#sources)

## The two routes

| Panel | Controller | HID ids | Route | Boards |
|---|---|---|---|---|
| Laptop 13 Pro touchscreen | Himax | `3558:14fd` | a processor pad, `GPP_B_18`, driven low to cut touch | `sakura`; `TOUCHSCREEN_PLATFORM` in `gpio.rs` |
| Laptop 12 touchscreen | Ilitek | `222a:5539` | the panel's own vendor command | `sunflower` |

- Which pad carries the enable is a fact about the mainboard; whether a
  command is implemented is a fact about the panel. Panels and mainboards
  are sold apart and the chassis takes any pairing, so neither answers for
  the other, and the pairings that exist put exactly one route within reach.
  `touchscreen::find` settles which, a precedence rather than a handover.
- The two routes differ in readback: the pad holds the level it drives and
  answers a get; the panel holds the setting and answers nothing. That is the
  one difference `TouchSwitch::reading` carries, and why the device's mirror
  exists for the panel route alone, declared `Lifetime::HostAwake`.
- The Himax interface answers version reads and carries a vendor collection
  of config and firmware-staging reports, and nothing that stops it
  reporting. Consequence: a probe that found the Ilitek's command would vouch
  for a command the Himax lacks, so each route is probed on its own path.
- The EC has no touchscreen host command in its custom set. What it drives
  is the panel's rail, `gpio_ec_ts_pwr_en`, raised in `POWER_S3S0` and
  dropped in `POWER_S0S3` beside the SSD and speaker-amplifier rails: power
  sequencing, not a control.

## The panel command

- `framework_lib::enable_touch` opens the vendor usage page `0xFF00` and
  accepts only `ILI_VID`, so `framework_tool --touchscreen-enable` works on
  the Laptop 12 and nowhere else, whatever `--help` implies by listing it
  unconditionally.
- The command is sent with no read length and the controller volunteers no
  report, so the panel's state is knowable only to whoever wrote it last.

## The display connector

Framework's published mainboard pinout gives the connector a touch group
beside the video pairs:

| Pins | Signal |
|---|---|
| 29, 30 | `3V_TS` supply |
| 31, 32 | a USB 2.0 pair |
| 33 | `TS_EN` |
| 34 | `TS_RST` |
| 35 | `TS_INT_N` |
| 36, 37 | `TS_SDA`, `TS_SCL` |

- The partial schematics show the I²C half fitted, series resistors, clamp
  diodes and a shared ESD array, on every Laptop 13 mainboard back to the
  first, from both silicon vendors. The USB half is not universal: the AMD
  boards omit it, and where a board has no use for the pair it goes
  elsewhere, to Bluetooth on the Laptop 13 Pro and to camera power on the
  Chromebook Edition. Consequence: touch arrives over an I²C controller
  belonging to the processor, which is why the EC takes no part in it.
- Those pins predate touch by several mainboard generations, so a touch
  panel works in front of a board that shipped long before one was sold.
  The panel's own connector is an ordinary 40-pin eDP pinout, backlight
  power on 36 through 39, so the eDP cable is a rewiring harness and is the
  part that carries the touch group across.
- A switched supply marks a board designed for touch. Where a touchscreen
  shipped with the machine, the supply at 29 and 30 comes from a load
  switch with a named enable: `gpio_ec_ts_pwr_en` into a switch shared with
  the eDP logic rail on the Laptop 13 Pro, `EN_PP3300_TCHSCR` on the
  Chromebook Edition. Where touch was only a reserved possibility, the
  supply reaches the connector from a system rail through a fuse, with no
  enable on the path.
- What drives `TS_EN` is published for one board only: `SOC_TS_0_EN_LS` on
  the Laptop 13 Pro, the level-shifted pad this control drives. Elsewhere
  the net leaves the connector page for a sheet the partial schematics do
  not include, and it carries no pull-up or pull-down at the connector on
  any board, so its resting level cannot be read off the published pages.

## The enable pad

- `GPP_B_18` is found by the name pinctrl gives it in its debugfs pin dump,
  not by chip and offset: which `/dev/gpiochipN` the controller becomes
  depends on what else registered a chip first, the EC registers one of its
  own, and the pinctrl device's ACPI name changes with each generation.
  Intel's pinctrl leaves the character device's own line names empty, so
  the stable interface cannot answer.
- Consequence: a kernel built without debugfs, or running without it
  mounted, has no touchscreen control.
- Intel's pinctrl leaves `PADCFG` as the last requester set it, so a
  process can drive the pad and exit without the setting going with it.
- Intel's pinctrl answers a get from the output latch whenever the output
  driver is enabled, so the pad reports the level being driven. A pad
  restored in another mode would answer from the input instead, which on
  this pad is disabled.
- The pad is not an ACPI resource of the touch device: the controller's
  `_CRS` declares the I²C connection and a `GpioInt` on pin 44, its
  interrupt, and no `GpioIo` for the enable or for `TS_RST`. Consequence:
  the pad cannot be discovered from the device that depends on it, on a
  board where the pad is known to gate touch, and a survey of another board
  should not use that test. The pin dump finds it as an unclaimed pad in
  GPIO mode with its output driver enabled and no `[LOCKED`.

### Observed

| Setup | Reading |
|---|---|
| Pad driven low, then the line released | level holds low; touch dead |

## What else drives the pad

Firmware reaches the pad through an ACPI helper, `STSP(on, delay, pad)`,
which calls `\_SB.SGOV` on pad `0x001A1012`, whose low byte is 18:

| Caller | Argument | Fires on |
|---|---|---|
| `_Q01`, the EC's lid-close query | 0 | lid close |
| `_Q02`, the EC's lid-open query | 1, after a 250 ms settle | lid open |
| the platform's screen-on notification | 1 | not an ordinary blank; see below |

### Observed

| Setup | Reading |
|---|---|
| Pad driven low and released, `systemd-inhibit` holding the lid switch, lid opened | the pad went high in the pin dump, having held low for the twenty seconds before; no suspend could account for it |
| Lid closed in the same run | invisible: it drives low and the pad was already there |
| Display blanked and woken through GNOME's screensaver, same run | the pad stayed low; the screen-on caller did not fire |

- Consequence: the pad's identity rests on the lid observation rather than
  on decoding the constant, and the ACPI tables list more callers than a
  session exercises.

## Persistence

| Route | Suspend | Reboot | EC restart | Lid open |
|---|---|---|---|---|
| Pad, Laptop 13 Pro | lost | lost | not a case | lost |
| Panel, Laptop 12 | unknown | unknown | unknown | unknown |

- The pad holds whatever was last driven, so every loss is another writer:
  on resume the pad returns to its firmware default and the EC raises the
  panel's rail independently; on lid open `_Q02` drives it high; at POST
  platform firmware configures its pads, the one writer the published
  documents do not cover and the only candidate left with the lid ruled
  out, since `PADCFG` would have carried a low across a warm reset.
- An EC restart reaches the pad only through the boot that follows.
- Consequence: moving the control into the EC would need both a command
  and a flag its power sequence honors, or the first resume would undo it.
- Nothing above is known to hold for the Laptop 12: every finding is about
  a pad that route does not touch. A controller that loses its supply keeps
  nothing, and the supply is switched on boards designed for touch, so a
  boot is the likeliest of the three to clear it; that is an expectation.

### Observed

| Setup | Reading |
|---|---|
| Pad low, touch dead, one suspend and resume | touch back |
| Pad low, touch dead, lid opened | touch back |
| Pad low, touch dead, one reboot with the lid open throughout | pad high, touch back |

## Open

- Whether an older Laptop 13 board can gate touch at all: a pad driving
  `TS_EN` would be controllable the same way, a tie to a rail would not.
  The names favor a driver, both Core Ultra generations putting a
  level shifter in the path and the Chromebook Edition calling its
  equivalent `USI_REPORT_EN`. Settling it takes an older Laptop 13 with a
  touch panel fitted and its pin dump; Core Ultra Series 1 first, its net
  being the Pro's minus the `SOC_` prefix.
- Every persistence column for the Laptop 12, and whether its Ilitek keeps
  the setting across a suspend, which the mirror's `HostAwake` lifetime
  assumes it does not.
- The pad's `PADCFG` constant, `0x001A1012`, decoded beyond its low byte.

## Sources

- [FrameworkComputer/Framework-Laptop-13](https://github.com/FrameworkComputer/Framework-Laptop-13)
  — the mainboard connector pinouts and the partial schematics per
  generation, where the display connector's touch group and the circuits
  around it are readable. Any sheet these reference for the far end of a
  signal is outside the published set.
- [FrameworkComputer/EmbeddedController](https://github.com/FrameworkComputer/EmbeddedController)
  — `zephyr/program/framework/sakura/src/power_sequence.c` for
  `gpio_ec_ts_pwr_en`, and `include/board_host_command.h` for the absence
  of a touch command.
- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  — `framework_lib/src/touchscreen.rs` for the controller ids and
  `enable_touch`. Its `laptop13pro-touchscreen-disable` branch is where the
  enable pad was first named; the pairing here was confirmed on the machine
  rather than taken from it.
- The machine's ACPI tables, `STSP`, `\_SB.SGOV`, `_Q01`, `_Q02` and the
  touch controller's `_CRS`, and Intel's `pinctrl-intel` driver's debugfs
  pin dump.
