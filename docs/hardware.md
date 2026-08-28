# Hardware notes

What the embedded controller and the devices behind it actually do, as found
while building frameguin. These are facts about the machine rather than about
this app: they hold whoever is talking to the hardware, and most of them are
not written down anywhere else, which is why they kept being rediscovered.

Where a claim comes from firmware or a datasheet it is cited by name — the
ChromiumOS EC tree Framework forks, TI's documents for the battery gauge —
rather than by line number, which rots.

Past the transport chapter the subject chapters divide by whether the machine
is being read or told: the battery is read, and every chapter between it and
the sources is a control. Each control chapter ends with a persistence section
— what survives a suspend, a reboot and an EC restart — because which of the
three a control survives does not follow from what the control does. The last
is never something the running system sees: `power_chipset_init` starts the
EC's power sequencing at G3 on every EC boot, so an EC restart takes the
machine down with it. What it asks is whether a value outlives the EC that
holds it.

Draining the pack to empty causes one. With no adapter attached the EC runs
off the battery, so a pack taken to zero takes the EC down and everything it
was holding in RAM with it. This is worth knowing before reading an EC-dated
value as evidence about a *reboot*: the EC's uptime shows that it restarted
but not what restarted it, so a boot that followed a flat battery answers
nothing about the reboot itself.

Framework is a trademark of Framework Computer Inc.; this is an independent
project and names the hardware only descriptively.

## Contents

Every heading in the file appears here.

<!-- GitHub's slugger drops the ² from "I²C"; that anchor is right as written. -->

- [What survives what](#what-survives-what)
- [Reaching the EC](#reaching-the-ec)
  - [The EC's uptime clock](#the-ecs-uptime-clock)
- [Battery](#battery)
  - [The EC's battery block](#the-ecs-battery-block)
  - [What the flag byte means, and does not](#what-the-flag-byte-means-and-does-not)
  - [The pack itself, over I²C](#the-pack-itself-over-ic)
  - [Cycle count goes stale in the EC](#cycle-count-goes-stale-in-the-ec)
  - [Battery temperature](#battery-temperature)
  - [Which status bits actually mean a fault](#which-status-bits-actually-mean-a-fault)
  - [Reading a health verdict with care](#reading-a-health-verdict-with-care)
- [Charging](#charging)
  - [Charge limit](#charge-limit)
  - [Charge current limit](#charge-current-limit)
  - [Charging persistence](#charging-persistence)
- [Power button LED](#power-button-led)
  - [Power button LED persistence](#power-button-led-persistence)
- [Charging LED](#charging-led)
  - [Charging LED persistence](#charging-led-persistence)
- [Keyboard backlight](#keyboard-backlight)
  - [Keyboard backlight persistence](#keyboard-backlight-persistence)
- [Haptic touchpad](#haptic-touchpad)
  - [Haptic touchpad persistence](#haptic-touchpad-persistence)
- [Touchscreen](#touchscreen)
  - [Touchscreen persistence](#touchscreen-persistence)
- [Sources](#sources)

## What survives what

Every control chapter's persistence section in one grid. Each row links to the
section that carries the mechanism and how it was established; nothing here is
stronger than the section it points at, so a row reading Unknown, or one whose
section marks its finding untested, means exactly that.

| Control | Suspend | Reboot | EC restart |
|---|---|---|---|
| [Charge limit](#charging-persistence) | Kept | **Lost** | Kept |
| [Charge current limit](#charging-persistence) | Kept | Kept | **Lost** |
| [Power button LED level](#power-button-led-persistence) | Kept | **Lost** | Kept |
| [Power button LED darkness](#power-button-led-persistence) | Kept | **Lost** | **Lost** |
| [Charging LED colour](#charging-led-persistence) | Kept | **Lost** | **Lost** |
| [Keyboard backlight](#keyboard-backlight-persistence) | Kept | Kept | Kept |
| [Haptic touchpad](#haptic-touchpad-persistence) | Kept | Kept | Kept |
| [Touchscreen, pad route](#touchscreen-persistence) | **Lost** | **Lost** | not a case |
| [Touchscreen, panel route](#touchscreen-persistence) | Unknown | Unknown | Unknown |

The pad route loses its setting to a fourth event the columns cannot carry —
the lid opening — and the panel route is the Laptop 12's, where none of the
pad's findings apply.

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

### The EC's uptime clock

`EC_CMD_GET_UPTIME_INFO` answers with `time_since_ec_boot_ms`, and it is the
only thing the EC says about its own life: there is no boot id, no restart
counter, nothing with an identity. So the only way to ask whether the EC is
still the one that took a write is to compare how far its clock has advanced
against how far the host's has, and that comparison has two properties worth
knowing before trusting it.

**The counter is 32 bits of milliseconds**, so it wraps at 49.7 days of EC
uptime and starts again from zero. An EC that has been up longer than that
reads as one that restarted.

**The EC keeps its own time, and keeps it badly**: its firmware documents 1%
or worse frequency error against the host clock, so the two disagree by
minutes over a week of uptime even with nothing wrong. Any comparison needs
slack on that order, which is what stops a long-standing write from reading
as expired.

## Battery

What the pack and the EC report about it, and how to read it. What can be set
lives under [Charging](#charging).

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

## Charging

The names invite confusion, so take them apart first.

### Charge limit

A ceiling on state of charge: a percentage the EC's battery sustainer holds the
pack at. Sitting at that ceiling is what produces the direction the EC's flags
cannot express — see
[what the flag byte means, and does not](#what-the-flag-byte-means-and-does-not).

### Charge current limit

A ceiling on the current drawn while charging, which says nothing about where
charging stops.

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

### Charging persistence

**The charge limit** is kept in BBRAM, so it outlives an EC restart. But UEFI
setup re-sends its own stored value at every POST, so a limit set from the OS
lasts until the next reboot and the standing value lives in BIOS setup.

**The charge current limit** is not stored anywhere the EC could restore it
from: `user_current_limit` and its pending value are plain statics in the
charger task, written only by the host command and by the threshold applier,
and `charger_init` — the hook every EC boot runs — leaves them alone. So an EC
restart drops it, by nothing more than those statics being initialized again.

**A host reboot does not drop it**, and it is the one control here that
firmware leaves alone. The EC runs straight through a reboot, so nothing on
its side clears the value, and UEFI setup does not re-send its own the way it
does for the charge limit above and for
[the power button LED's level](#power-button-led-persistence). That is what
separates it from those two: setup has an option for each of them and none for
a charge current, so there is nothing stored for POST to re-assert.

The evidence is a contrast rather than a reading, since this control has no
readback in any command version. Across one reboot the EC survived, with a
limit standing from before it: the charge limit came back at the value held in
setup and the LED level came back at setup's, while the current limit was
still the one written from the OS and the pack still charged at it. The same
POST overwrote the two controls firmware owns and left this one untouched.

**Watching for the command itself does not work on this machine.** It would be
better evidence, and `framework_tool --console recent` prints the EC's console
ring in which a write appears as `HC 0x00a1`, the command's own number, with
the charger target it produces as `charge_request(<mV>, <mA>)`. But the ring is
about 4 KB, and through POST the EC fills it with paired `event set` and
`PORT80:` lines at roughly fifty a second — so it holds some two seconds of
boot, against a POST that ended ten seconds before the earliest moment a
userspace unit can read it. Boot destroys its own record. Reaching it wants an
EC UART or a firmware build with a larger buffer; the console is still good
for watching a write land while the machine is up, and its timestamps are EC
uptime, so a dump spanning two host boots is itself proof the EC did not
restart.

A suspend costs neither of them anything, the EC staying up across one.

## Power button LED

The EC's host commands for it are spelled `FP_LED`, the fingerprint reader
sharing the button; the EC's own id for it is `EC_LED_ID_POWER_LED`.

Levels are 1–100. **Zero is rejected**: the EC will not let the host
extinguish the machine's power indicator.

The **percentage write** needs command **v1**, which older EC firmware lacks
([framework-system #211](https://github.com/FrameworkComputer/framework-system/issues/211)).
The ultra-low and auto levels do not need it — the v0 handler takes them on any
firmware that has them — but they arrived with the same firmware generation, so
asking whether v1 exists is a serviceable stand-in for asking whether they do.
It is a proxy, not a requirement: worth knowing if you are deciding what to
*refuse* rather than what to offer.

**Auto is the ambient light sensor**, not a policy the EC runs on its own.
Setting it raises a bit in the BIOS-function BBRAM slot, and the LED's duty
then follows the sensor on each tick, on boards built with a dedicated ALS. It
is a writer of the brightness rather than a level among the others.

**The level a read reports is deduced, not remembered.** Only the percentage is
stored, and the getter maps it back to whichever named level shares its value,
answering custom for anything unmapped. A custom percentage that happens to
equal a named level's therefore reads back as that level, with nothing to tell
the two apart. Auto is the exception, being a flag of its own, and it replaces
the deduced answer rather than being read out of the percentage.

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

**A brightness write is the handover**, not a step taken before one: the host
command's handler sets the colour and clears the LED's auto flag in the same
call, so there is no order a caller can choose and no way to write a colour
without also taking the LED. That flag is what the EC's own policy consults
before touching the LED on its tick.

That account is readable, but there is no *hardware* read behind it: the driver
implements no `brightness_get`, and the EC's LED command answers only with
which colours exist. So an EC restart hands every LED back to the EC without
the kernel noticing, and the record silently becomes wrong — though never
under a running host, the restart taking the machine down and the reboot
re-probing the driver (both below), so nothing running can read it stale.

### Power button LED persistence

Nothing set from the OS survives a reboot, and each mechanism below sees to
that on its own, so fixing any one of them would change nothing.

**BIOS setup re-sends its level at every POST**, exactly as it does
[the charge limit](#charging-persistence). The option is under Advanced,
"Power Button Brightness Level", and its
value replaces whatever the OS last set. Observed with the option left on
Auto: a level set from the OS read back as auto after a reboot, with the EC's
uptime counting straight through and its reset flags unchanged, so no EC
restart could account for it — leaving the host command the option sends as
the only thing that can have turned auto back on.

**The EC also resets the stored percentage on the way down**, independently:
reaching S5 writes the BBRAM slot back to the high level, 55%, whether auto is
on or not. A suspend does not: the reset hangs off the chipset being off, not
merely asleep.

So a discrete level chosen in BIOS setup is what holds, setup asserting it
again each boot — observed for Auto, with the fixed levels being the same
option sending the same command. A custom percentage has nowhere in setup to
be chosen from, so it cannot survive at all.

An **EC restart** leaves the stored percentage where it is, the slot being
battery-backed, but takes the LED back from anything holding it: the level
outlives the restart and darkness does not.

Darkness does not survive a reboot either, and for a reason of its own again:
the reboot re-probes the kernel's LED driver and re-attaches the EC's trigger,
so the kernel's record reads as lit and nothing re-sends the write.

## Charging LED

`EC_LED_ID_BATTERY_LED`, reached by `EC_CMD_LED_CONTROL` — the command that
darkens the power LED, here doing its ordinary job. The kernel exposes it the
same way, at `/sys/class/leds/chromeos:multicolor:charging`, and the same
`chromeos-auto` trigger is the handover.

These boards answer for this LED and the power LED and no others. Which ids are
supported is computed from the devicetree's pin nodes rather than declared, so
the ids the protocol also defines — adapter, left, right, recovery, sysrq — are
unsupported by having no pins rather than by being turned off. What
`/sys/class/leds` lists under `chromeos:` is therefore the whole set of LEDs
the EC offers, not a subset the kernel happened to bind.

**Six colours, and not a mixer.** The board's devicetree gives the LED a pin
node per colour — red, green, blue, yellow, white, amber — and the query
reports exactly those. The pins beneath them are RGB, and the board retunes
what white means between chipset startup and shutdown, but that mixer is the
EC's alone: `led_set_brightness` walks the nodes and lights the colour of the
last one whose slot is nonzero, so two colours asked for at once do not blend.
One wins.

**The brightness value carries no brightness.** Nonzero means the colour is on
at its devicetree duty, zero in every slot means off, and nothing between is
expressible. The range the query advertises is a flat 100 for every colour
present — `led_get_brightness_range` writes that constant without consulting
the hardware — so `max_brightness` promises a scale the firmware does not
implement. It holds for the power LED too, where a single white channel makes
it easy to miss.

**Two LEDs answer to the one id.** There is a charge indicator on each side of
the chassis, each behind its own enable. Under auto the EC lights the side of
the active charge port — a port at the back indicates on the right, the
firmware's stated reason being Lot 6 — and darkens both while discharging. A
host that takes the LED gets both: the EC's tick raises both enables whenever
auto is off, and which side is lit is not something a host command can steer.

**It is not only a charge indicator.** The EC pre-empts the charge pattern to
raise faults on this LED, each a blink pattern with no other channel to reach
anyone by: diagnostics running, the battery cut off, no battery present
outside standalone mode, the C cover open. Boards with a GPU bay add its cover
being open, a module fault, and an input deck not fully populated. A host
holding the LED silences all of them.

### Charging LED persistence

Ownership is a RAM flag and nothing else: `led_auto_control_flags` starts with
every LED on auto and is never written down, so an EC restart hands the LED
back. A suspend does not, the EC staying up through one, so a colour set from
the OS holds across it.

A reboot ends it by the host's route rather than the EC's. The EC keeps
holding the colour across the reset, and it is the kernel re-probing its LED
driver and re-attaching the auto trigger that gives the LED up. Nothing
re-sends the colour afterwards.

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
[the charge current limit](#charging-persistence) for why. This control has a
getter, so the test is the one that settled those two: set a distinctive
level, reboot, and read it back.

## Haptic touchpad

Reached over its own HID transport, not the EC.

**Write-only**: the firmware acknowledges `GET_FEATURE` with zeros rather than
the current setting, so there is no readback. Anything reporting what is set
has to remember it.

The firmware implements **five intensity steps** rather than the 0–100 its HID
descriptor advertises.

### Haptic touchpad persistence

Settings live in the touchpad's own flash, so they survive a suspend, a reboot
and an EC restart alike — the EC is not on the path and has nothing to reset.
Nothing needs re-applying after a resume. That independence is no help to
anything that forgot what it set, though: the write-only interface above means
the device will not say.

## Touchscreen

Two panels, two unrelated mechanisms — and on the Laptop 13, a control
reached through neither the EC nor the panel itself, which is what makes it
unlike every other one here.

**The Laptop 13's Himax panel has no off command**, and this is where it
parts company with the Laptop 12. The Himax HID interface answers version
reads and carries a vendor collection of config and firmware-staging reports,
but nothing that stops it reporting. What gates it is a board signal reaching
the display connector, driven by a pad on the processor — `GPP_B_18` on the
Laptop 13 Pro, driven low to cut touch. So the control is a level on a line,
and the controller is never addressed at all.

The Laptop 12's Ilitek controller is the opposite: it takes a vendor HID
command to switch touch off, which is what `framework_lib`'s `enable_touch`
sends: that path opens only the Ilitek vendor ID, so `--touchscreen-enable`
works on the Laptop 12 and nowhere else, whatever `--help` implies by listing
it unconditionally. A control for one panel is not a control for the other,
and a probe that found the Ilitek would be vouching for a command this pad
knows nothing about.

**That command answers nothing.** It is sent with no read length and the
controller volunteers no report of its own, so the panel's state is knowable
only to whoever wrote it last. This is the one way the two routes differ for
anyone using them: the pad holds the level it is driving and reads back, and
the panel holds the setting and will not say so.

**The enable is a pin on the display connector.** Framework's published
mainboard pinout gives that connector a touch group beside the video pairs: a
`3V_TS` supply on 29 and 30, a USB 2.0 pair on 31 and 32, then `TS_EN`,
`TS_RST`, `TS_INT_N`, `TS_SDA` and `TS_SCL` on 33 through 37. The partial
schematics show the I²C half fitted — series resistors, clamp diodes, a shared
ESD array — on every Laptop 13 mainboard back to the first, from both silicon
vendors. The USB half is not universal: the AMD boards omit it, and where a
board has no use for the pair it goes elsewhere, to Bluetooth on the Laptop 13
Pro and to camera power on the Chromebook Edition. So touch arrives over an
I²C controller belonging to the processor, which is the other half of why the
EC has nothing to say about it.

**Those pins predate touch by several mainboard generations**, which is why a
touch panel works in front of a board that shipped long before one was sold.
Nothing about the board changes; the cable does. The panel's own connector is
an ordinary 40-pin eDP panel pinout — backlight power on 36 through 39 — so
the eDP cable is a rewiring harness rather than a straight-through, and it is
the part that carries the touch group across.

**Panels and mainboards are sold apart** and the chassis takes any pairing, so
neither answers for the other. Which pad carries the enable is a fact about
the mainboard; whether anything is behind it is a fact about the panel. A
board of the right generation behind a panel with no touch has the pad and
nothing on the end of it.

**A switched supply marks a board designed for touch.** Where a touchscreen
shipped with the machine, the supply at pins 29 and 30 comes from a load
switch with a named enable — `gpio_ec_ts_pwr_en` into a switch shared with the
eDP logic rail on the Laptop 13 Pro, `EN_PP3300_TCHSCR` on the Chromebook
Edition. Where touch was only a reserved possibility, that supply reaches the
connector from a system rail through a fuse: protection, with no enable
anywhere on the path. On those boards the panel's power follows whatever the
system rail does and nothing can address it.

**What drives the enable pin is published for one board only.** On the Laptop
13 Pro it is `SOC_TS_0_EN_LS`, the level-shifted pad this control drives.
Elsewhere the net leaves the connector page for a sheet the partial schematics
do not include, so the far end is unknown — and since it carries no pull-up or
pull-down at the connector on any board, its resting level cannot be read off
the published pages either. Whether an older board can gate touch at all is
undecided from the documents: a pad driving that line would be controllable
the same way, a tie to a rail would not. The names lean toward a driver — both
Core Ultra generations put a level shifter in the path, which is done to a
driven signal and not to a tie, and the Chromebook Edition calls its
equivalent `USI_REPORT_EN`. Settling it needs the machine, and pinctrl's
debugfs pin dump is the way: it gives every pad's mode and level, and a driven
enable shows up there as an output already holding a level. Asking ACPI
instead does not work — see below.

**The pad keeps its level once the line is released.** Intel's pinctrl leaves
`PADCFG` as the last requester set it, so a process can drive the pad and exit
without the setting going with it.

**The enable is not an ACPI resource of the touch device, but firmware drives
it anyway.** The controller's `_CRS` declares two things and no more: the I²C
connection, and a `GpioInt` on pin 44 — its interrupt, the one in
`/proc/interrupts`. No `GpioIo`, for the enable or for `TS_RST`. So the pad
cannot be discovered from the device that depends on it, on a board where the
pad is *known* to gate touch. Anyone surveying another board should skip this
test: it answers "no processor pad" where there demonstrably is one. The
pinctrl pin dump is what finds it — an unclaimed pad in GPIO mode, output
driver enabled, no `[LOCKED`.

Firmware reaches it through a helper instead. `STSP(on, delay, pad)` calls
`\_SB.SGOV` on pad `0x001A1012`, and the EC's lid queries call it: `_Q01` on
lid close with 0, `_Q02` on lid open with 1 after a 250 ms settle. The
platform's screen-on notification calls it with 1 as well. The pad constant is
not decoded here beyond its low byte, 18, matching `GPP_B_18` — what
identifies the pad is the observation rather than the arithmetic: **with the
pad driven low and the line released, opening the lid drove it high**, watched
directly in the pin dump with `systemd-inhibit` holding the lid switch so no
suspend could account for it. The pad had held low for twenty seconds before
that, which is the control case for the same run. Lid close was invisible
because it drives low and the pad was already there. The screen-on call site
did **not** fire in the same test: blanking and waking the display through
GNOME's screensaver left the pad low throughout. So that call exists in the
tables without being reached by an ordinary blank — which is worth knowing
mostly as a warning that the tables list more callers than a session will
exercise.

**The state reads back**, which nothing else off the EC manages: Intel's
pinctrl answers a get from the output latch whenever the output driver is
enabled, so the pad reports the level being driven. The caveat is narrow — a
pad restored in another mode would answer from the input instead, which on
this pad is disabled and therefore meaningless.

### Touchscreen persistence

The pad holds whatever was last driven into it, as above, so nothing here is
the pad forgetting — it is something else overwriting it.

**Off does not survive a suspend** — observed, and explicable from two
directions at once: the pad returns to its firmware default on resume, and the
EC brings the panel's own rail up independently — `gpio_ec_ts_pwr_en` is
driven in `POWER_S3S0` and `POWER_S0S3`, grouped with the SSD and speaker-amp
rails. That is power sequencing rather than a control, and there is no
touchscreen host command anywhere in the EC's custom set. Moving the control
into the EC would need both a command and a flag its power sequence honours,
or the first resume would undo it.

**Nor does it survive the lid opening**, which is the same loss with no
suspend to explain it — the lid query above drives the pad back high.

**Nor a reboot** — with the pad driven low and touch confirmed dead, a reboot
with the lid left open throughout brought the pad back high and touch with it.
Since the pad's own `PADCFG` would have carried that low across a warm reset,
what undoes it is platform firmware configuring its pads at POST: the one
writer the published documents do not cover, and the only candidate left with
the lid ruled out.

**An EC restart is not a case this control has.** The enable is a processor pad
the EC cannot reach, so nothing about it turns on the EC being up. The panel's
own rail is a path the EC does drive — `gpio_ec_ts_pwr_en`, low until
`POWER_S3S0` — but a restart reaches it only through the boot that follows,
which is the case above.

**None of the above is known to hold for the Laptop 12**, and none of it can
be carried over: every finding here is about a pad that route does not touch.
Whether the Ilitek keeps its setting across a suspend, a lid opening or a boot
is unestablished, and settling it needs the machine. The one thing that can be
said from the documents is where to expect the answer to come from — a
controller that loses its supply cannot be keeping anything, and the supply is
switched on boards designed for touch, so a boot is the likeliest of the three
to clear it. That is an expectation and not a finding.

## Sources

- [FrameworkComputer/EmbeddedController](https://github.com/FrameworkComputer/EmbeddedController)
  — the ChromiumOS EC fork these boards run. `common/battery_v2.c`,
  `common/charge_state.c` and `include/battery_smart.h` cover the battery
  block, the static/dynamic split and the status bits; `common/led_common.c`
  is the LED host command, with the policy, the pin walk and the fault
  patterns in `led.c`, `led_pwm.c` and `laptop_led.c` under
  `zephyr/program/framework/src/`. The per-board devicetree under
  `zephyr/program/framework/` names each pack, the battery temperature sensor
  node and each LED's colours — often by including a sibling board's file
  rather than carrying its own.
- [FrameworkComputer/Framework-Laptop-13](https://github.com/FrameworkComputer/Framework-Laptop-13)
  — the mainboard connector pinouts and a partial schematic per generation,
  which is where the display connector's touch group and the circuits around
  it are readable. Full schematics are not published: any sheet these
  reference for the far end of a signal is outside the set, which is the limit
  every unresolved question above runs into.
- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  — `framework_tool` and `framework_lib`, and the issue tracker where the
  command-version and readback limitations above are recorded. Its
  `laptop13pro-touchscreen-disable` branch (unmerged, head `f3a4cbb4`) is
  where the touchscreen enable pad was first named; the pairing recorded
  above was confirmed on the machine rather than taken from it, since a topic
  branch is not something a reader can rely on finding.
- TI **bq40z50** technical reference manual — the register map and the set
  conditions for every status bit. SLUUA43A covers the R2 revision and
  SLUUBU5A the R3, which differ in their ManufacturerAccess status bits.
  The bq40z50 *datasheet* (SLUSBS8) is the electrical specification and
  contains no register map; it defers to the TRM throughout.
- Smart Battery Data Specification — the generic register set the gauge
  implements.
