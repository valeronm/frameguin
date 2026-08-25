# Hardware notes

What the embedded controller and the devices behind it actually do, as found
while building frameguin. These are facts about the machine rather than about
this app: they hold whoever is talking to the hardware, and most of them are
not written down anywhere else, which is why they kept being rediscovered.

Where a claim comes from firmware or a datasheet it is cited by name — the
ChromiumOS EC tree Framework forks, TI's documents for the battery gauge —
rather than by line number, which rots.

Framework is a trademark of Framework Computer Inc.; this is an independent
project and names the hardware only descriptively.

## Reaching the EC

Three routes, and which one a value comes from decides what it costs and how
fresh it is.

**The memory map.** A region the EC keeps updated and the host reads without a
command round trip. Cheap. Carries the battery block, thermal sensors, fan
speeds. On Linux these are `CROS_EC_DEV_IOCRDMEM` ioctls against
`/dev/cros_ec`.

**Host commands.** A request/response over the same device. Everything that
sets something, and the reads the memory map has no room for.

**I²C passthrough** (`EC_CMD_I2C_PASSTHRU`). A host command carrying an I²C
transaction the EC performs on the host's behalf. This is how you reach a
device the EC is itself driving — notably the battery gauge. Much slower than
either of the above: an EC round trip *plus* a real bus transaction.

`framework_lib`'s `CrosEc::new()` panics outright when it finds no driver
(an empty driver list — for example aarch64 with no `/dev/cros_ec`), so it
must be constructed behind a check that the machine is the right one, not
called speculatively.

## Battery and charging

### The EC's battery block

One memory-map region carrying voltage, present rate, remaining and last-full
capacity, design capacity and voltage, cycle count, a flag byte, and four
8-byte strings (manufacturer, model, serial, chemistry).

The 8-byte string fields are a genuine limit — but on the packs seen so far
nothing is being truncated by them. The model reads `FRANEDA`, and the pack's
own Smart Battery `DeviceName` register also returns `FRANEDA`, so the longer
`FRANEDAC00` printed on the physical label exists only on the label. Worth
knowing before writing code to chase a fuller name over I²C; there isn't one.

Capacities are in mAh and voltages in mV. `framework_lib` computes the charge
percentage as `100 * remaining / last_full`, which divides by a value the pack
supplies — a pack reporting zero there panics inside the library.

### What the flag byte means, and does not

The EC's **discharging** flag means *not being charged*, not *supplying the
machine*. A full pack sitting on a connected charger sets it, because the smart
battery is reporting zero charge current. `framework_tool --power` prints
"Battery discharging" in that state too.

So the flag alone never settles the direction. Weigh it against whether a
charger is present and against the rate, which reads a clean 0 mA at rest.

**Neither flag set is a real state**, and a charge limit produces it. The limit
arms the EC's battery sustainer, which switches to `CHARGE_CONTROL_IDLE` on
reaching the ceiling and clears both flags there — ACPI's charge-limiting
convention asks that the host stop claiming a direction. The charge current
then decays for as long as a minute, so there is a window with a substantial
rate and no direction at all. A pack whose charge is not moving is what
distinguishes that from a pack running the machine.

### Charge limit

The EC persists it in BBRAM. But UEFI setup re-sends its own stored value at
every POST, so a limit set from the OS lasts until the next reboot and the
standing value lives in BIOS setup.

### Charge current limit

Write-only: no readback exists in any command version
([framework-system #180](https://github.com/FrameworkComputer/framework-system/issues/180)).
Anything wanting to report it has to remember what it wrote.

The command has a variant that applies the limit above a state-of-charge
threshold. It **latches inside the EC**: once applied it is never re-evaluated,
so a later threshold cannot lift it
([framework-system #342](https://github.com/FrameworkComputer/framework-system/issues/342)).
The unconditional form is the one to send unless you want that behaviour.

A charge rate expressed in C is converted against **design capacity** — the
design capacity in mAh is numerically the 1C current in mA.
`framework_lib::set_charge_rate_limit` does exactly this and prints the result
as "Design Current".

### The pack itself, over I²C

Every Framework battery in the EC's devicetree declares `battery-smart`, and
they share a gauge IC, so the address is the same on every board: **port 3,
address 0x0b** (the 7-bit form of the 8-bit `0x16` the datasheet names).

The gauge is a **TI bq40z50**. Its Smart Battery registers are generic, but the
ManufacturerAccess map — safety status, permanent-failure status, state of
health, the lifetime data blocks — is specific to that part, so anything built
on those stops working if a pack ever ships with a different gauge.

Useful registers, all plain reads:

| Register | What |
|---|---|
| `0x08` Temperature | Tenths of a Kelvin |
| `0x16` BatteryStatus | Alarm and state bits, see below |
| `0x17` CycleCount | The pack's own count |
| `0x1B` ManufactureDate | Packed: day in bits 0–4, month in 5–8, years since 1980 above |
| `0x3C`–`0x3F` CellVoltage | mV per cell — note the registers run *backwards* against cell numbering, `0x3F` being cell 1 |

Reading the gauge's **firmware version** is the exception: it is a
ManufacturerAccess block command, which needs a *write* of the subcommand to
register `0x00` before the block read from `0x44`. Everything else above needs
no write.

Sealed packs answer the generic registers but return zeros or empty blocks for
safety status, permanent-failure status and the lifetime data. Those need an
unseal key, and unsealing is itself a write.

### Cycle count goes stale in the EC

The EC publishes a cycle count in its memory map, and it can be **weeks
behind**. On one pack the EC said 3 where the gauge said 8.

The value lives in the EC's *static* battery block. `update_static_battery_info`
fills that block only while the charger task's `need_static` flag is set, and
clears the flag as soon as one read succeeds. The flag is set on a battery
presence change and on the paths that revive an unresponsive or deeply
discharged pack — nothing else. Since the EC outlives host reboots, the
published count is whatever was true when the EC last initialized the battery.

Everything else in that static block either genuinely cannot change (design
capacity, the strings) or is separately refreshed by the *dynamic* block on
every charger pass (voltage, rate, remaining capacity, last-full capacity,
flags). Cycle count is the one value that both moves and is published as
static. Read it from the gauge instead.

### Battery temperature

The EC's thermal sensor array carries a battery entry on some boards, but it is
not a second sensor: its devicetree node is `cros-ec,temp-sensor-battery` at
the pack's own I²C address, and the binding describes it as "the last polled
battery temperature". It is the gauge's sensor, relayed.

Reading the gauge directly is better on three counts: tenths of a degree rather
than whole degrees, current rather than last-polled, and it works on the boards
whose EC does not relay it at all — the array's entry sits at a different index
per board, and the AMD and Desktop variants have no battery entry in it.

The array's own encoding, if you do use it: Kelvin offset by 200, with the top
four byte values reserved for a sensor that cannot answer (not present, error,
not powered, not calibrated). Freezing is therefore 73, so decode signed —
`framework_lib`'s own `t - 73` underflows below 0 °C.

### Which status bits actually mean a fault

`BatteryStatus` (`0x16`) splits into states (bits 4–7: fully discharged, fully
charged, discharging, initialized) and alarms (bits 8–15). The EC's own console
prints them as two separate groups.

`INIT` is a *good* state: it means the gauge has finished its power-on
self-test and calibration, so its readings can be trusted. It is not "starting
up".

Of the alarms, only two mean something is wrong on their own. The bq40z50
technical reference (SLUUA43A, "Terminate Charge and Discharge Alarms") gives
every set condition:

- **`OCA`** (overcharged) and **`OTA`** (overtemperature) have only safety and
  permanent-failure conditions. A healthy pack cannot raise them.
- **`TCA`** (terminate charge) and **`TDA`** (terminate discharge) each also
  have a `GaugingStatus()` condition, which fires at every ordinary full charge
  and every ordinary empty one. Treating these as faults puts a warning on a
  battery that has merely finished charging. The datasheet counts "valid charge
  terminations" as a lifetime statistic, which is the same point from the other
  direction. (`FD`, the fully-discharged *state* at bit 4, has a
  `GaugingStatus()` condition for the same reason.)
- **`RCA`** and **`RTA`** fire against thresholds the *host* sets, so on a
  laptop they duplicate what the OS already warns about.

`TCA` and `TDA` **together** are worth catching. Their gauging conditions are
mutually exclusive — one requires charge mode, the other discharge mode — so
both at once can only come from a safety alert, a permanent failure, or the
pack reporting itself absent. That combination is the only visibility into
over-current and cell-undervoltage faults without unsealing.

### Reading a health verdict with care

`framework_tool --smartbattery` ends with a health analysis. On a **sealed**
pack its safety-status and permanent-failure checks read through
`.unwrap_or(0)` and its lifetime blocks come back empty, so those checks are
silently skipped — and the code cannot distinguish "nothing wrong" from "could
not look". A sealed "Status: HEALTHY" rests only on the alarm bits, capacity
retention and cell balance.

Capacity retention — last-full against design capacity — is what most tools
call health. It can exceed 100% on a new pack. It says nothing about internal
resistance or cell balance, so a pack can show excellent retention while a cell
drifts. Cell spread is the independent signal, and the EC publishes only the
pack total, so it has to come from the gauge.

## Fingerprint LED

Levels are 1–100. **Zero is rejected**, because the LED doubles as the power
indicator.

The **percentage write** needs command **v1**, which older EC firmware lacks
([framework-system #211](https://github.com/FrameworkComputer/framework-system/issues/211)).
The ultra-low and auto levels do not need it — the v0 handler takes them on any
firmware that has them — but they arrived with the same firmware generation, so
asking whether v1 exists is a serviceable stand-in for asking whether they do.
It is a proxy, not a requirement: worth knowing if you are deciding what to
*refuse* rather than what to offer.

**A level is acknowledged at once and applied 100 ms later.** The EC's
`fp_led_level_control` stores the level in BBRAM and defers
`change_pwm_led_maximum_duty`, which is what actually moves the PWM duty the
brightness is. Until that hook fires the LED still carries the previous level,
and `led_set_brightness` treats any nonzero value as "colour on" at whatever
duty currently stands — so lighting the LED inside that window shows the *old*
brightness, whichever write does the lighting. Wait the hook out.

**The level command has no off.** It rejects 0, and the BBRAM slot reads a 0
back as full brightness, 0 being the uninitialized value there.

Darkening is still possible — `EC_CMD_LED_CONTROL` will do it — but the EC
keeps no readable record of who owns the LED. The kernel's LED class
(`/sys/class/leds/chromeos:*:power`) does keep one, so a command sent to the EC
behind the driver's back leaves that record describing a policy the EC has
already stopped following. Going through the kernel instead keeps the only
account there is truthful.

That account is readable, but there is no *hardware* read behind it: the driver
implements no `brightness_get`, and the EC's LED command answers only with
which colours exist. So an EC restart hands every LED back to the EC without
the kernel noticing, and the record silently becomes wrong. Anything relying on
it has to date its own write against the EC's uptime — which can only ever
withdraw the kernel's account, never supply one.

## Keyboard backlight

Read it with `EcRequestPwmGetKeyboardBacklight`, which returns the stored
percentage exactly. `framework_lib::get_keyboard_backlight()` goes through PWM
duty instead and floors twice — percent to duty in the EC, then duty back to
percent in the library — so most values come back one low: 5% reads as 4%.

The EC is a **second writer**. Fn+Space changes it, and newer boards have a
firmware auto mode. Anything showing the value has to re-read rather than
trusting what it last wrote.

## Haptic touchpad

Reached over its own HID transport, not the EC.

**Write-only**: the firmware acknowledges `GET_FEATURE` with zeros rather than
the current setting, so there is no readback. Anything reporting what is set
has to remember it.

Settings persist in the touchpad's own flash across suspend and reboot, so
nothing needs re-applying after a resume — the hardware keeps its own state.

The firmware implements **five intensity steps** rather than the 0–100 its HID
descriptor advertises.

## Sources

- [FrameworkComputer/EmbeddedController](https://github.com/FrameworkComputer/EmbeddedController)
  — the ChromiumOS EC fork these boards run. `common/battery_v2.c`,
  `common/charge_state.c` and `include/battery_smart.h` cover the battery
  block, the static/dynamic split and the status bits; the per-board
  devicetree under `zephyr/program/framework/` names each pack and the
  battery temperature sensor node.
- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  — `framework_tool` and `framework_lib`, and the issue tracker where the
  command-version and readback limitations above are recorded.
- TI **bq40z50** technical reference manual — the register map and the set
  conditions for every status bit. SLUUA43A covers the R2 revision and
  SLUUBU5A the R3, which differ in their ManufacturerAccess status bits.
  The bq40z50 *datasheet* (SLUSBS8) is the electrical specification and
  contains no register map; it defers to the TRM throughout.
- Smart Battery Data Specification — the generic register set the gauge
  implements.
