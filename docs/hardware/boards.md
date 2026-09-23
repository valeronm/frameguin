# Boards

What each Framework board is expected to offer: the main window's controls
and the Readings window's sections, a column per board. The mechanism behind
each row is in the file the row links to; [`controls.md`](controls.md) is
the index by control and [`parts.md`](parts.md) the index by part.

## Contents

Every heading in the file appears here.

- [Controls](#controls)
- [Readings](#readings)

## Controls

Only the `sakura` column is observed. Every other cell is read from the EC
firmware of the board its column names, in
[`ec.md`'s table](ec.md#which-board-the-ec-tree-calls-this-machine), on
the branches `fwk-marigold-22606`, `fwk-lotus-azalea-19573`,
`fwk-lilac-27116` (building `lilac` as an `azalea` variant),
`fwk-sunflower-26784`, `fwk-tulip-29169` (`lotus` on its 4.x line, and
`tulip`), `fwk-hx20-hx30-4410` and `fwk-dogwood-27111`. A cell holds for
firmware built from that branch, and the section a row links to carries
why.

| Cell | Means |
|---|---|
| Tested | observed working on the board |
| Expected | the firmware implements what the probe and the setter send |
| -- | the app shows no row |

| Control | sakura | marigold | azalea | lilac | sunflower | lotus | tulip | hx20, hx30 | dogwood |
|---|---|---|---|---|---|---|---|---|---|
| [Charge limit](battery.md#charge-limit) | Tested | Expected | Expected | Expected | Expected | Expected | Expected | Expected | -- |
| [Charge speed](battery.md#charge-current-limit) | Tested | Expected | Expected | Expected | Expected | Expected | Expected | Expected | -- |
| [Power LED high, medium, low](led.md#power-led-levels) | Tested | Expected | Expected | Expected | Expected | Expected | Expected | Expected | -- |
| [Power LED percentage, ultra-low](led.md#power-led-levels) | Tested | -- | -- | Expected | Expected | Expected | Expected | -- | -- |
| [Power LED auto](led.md#power-led-levels) | Tested | -- | -- | Expected | -- | Expected | Expected | -- | -- |
| [Power LED off](led.md#power-led-off) | Tested | Expected | Expected | Expected | Expected | Expected | Expected | Expected | -- |
| [Charging LED off](led.md#which-leds) | Tested | Expected | Expected | Expected | Expected | Expected | Expected | -- | -- |
| [Charging LED side](led.md#charging-led-sides) | Tested | Expected | Expected | Expected | Expected | Expected | Expected | -- | -- |
| [Touchscreen off](touchscreen.md) | Tested | -- | -- | -- | Expected | -- | -- | -- | -- |

- The haptic touchpad is found by its own USB ids on any board, so its
  controls follow the pad rather than the mainboard.
- The Laptop 12 with Intel Core Series 3 has no firmware branch to read,
  and is left out.

## Readings

The Readings window's sections, on the same branches and in the same cells
as the controls above.

| Reading | sakura | marigold | azalea | lilac | sunflower | lotus | tulip | hx20, hx30 | dogwood |
|---|---|---|---|---|---|---|---|---|---|
| [Battery charge and flow](battery.md#the-ecs-battery-block) | Tested | Expected | Expected | Expected | Expected | Expected | Expected | Expected | -- |
| [Battery condition](battery.md#the-pack-over-ic) | Tested | Expected | Expected | Expected | Expected | Expected | Expected | Expected | -- |
| [Battery extender](battery.md#battery-extender) | Tested | Expected | Expected | Expected | Expected | Expected | Expected | Expected | -- |
| [USB-C ports](usb-c.md#host-commands) | Tested | Expected | -- | Expected | Expected | Expected | Expected | -- | Expected |
| [Chassis open](chassis.md#the-chassis-open-switch) | Tested | Expected | Expected | Expected | Expected | Expected | Expected | Expected | -- |
| [Input deck](chassis.md#the-input-deck) | Tested | -- | -- | -- | -- | Expected | Expected | -- | -- |
| [Privacy switches](chassis.md#the-privacy-switches) | Tested | Expected | Expected | Expected | Expected | Expected | Expected | Expected | -- |
