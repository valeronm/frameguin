# The EC's LEDs

The power button LED and the charging LED are the two LEDs the EC drives,
and each has two possible holders: the EC's own policy, and the host
holding it dark through the kernel's LED class. The EC's level command for
the power LED is `hardware/src/ec.rs`; the kernel's class is
`hardware/src/led.rs`; the handover between the two, per LED, is
`hardware/src/device/power_led.rs` and `hardware/src/device/charging_led.rs`.

Each section holds what the EC tree, the kernel or this code establishes,
then under **Observed** what was read on a machine. Every observation is
from the Laptop 13 Pro (Intel Core Ultra Series 3).

## Contents

Every heading in the file appears here.

- [Which LEDs](#which-leds)
- [The LED command](#the-led-command)
- [The kernel's class](#the-kernels-class)
- [Power LED levels](#power-led-levels)
- [Power LED off](#power-led-off)
- [Charging LED colors](#charging-led-colors)
- [Charging LED sides](#charging-led-sides)
- [Charging LED faults](#charging-led-faults)
- [Persistence](#persistence)
- [Open](#open)
- [Sources](#sources)

## Which LEDs

| EC id | Kernel node | Colors in the devicetree |
|---|---|---|
| `EC_LED_ID_POWER_LED` | `/sys/class/leds/chromeos:*:power` | white |
| `EC_LED_ID_BATTERY_LED` | `/sys/class/leds/chromeos:multicolor:charging` | red, green, blue, yellow, white, amber |

- `led_is_supported` computes the supported ids from the devicetree's pin
  nodes, so the ids the protocol also defines, adapter, left, right,
  recovery, sysrq, are unsupported by having no pins. Consequence: what the
  kernel lists under `chromeos:` is the whole set the EC offers.
- The EC's host commands for the power LED are spelled `FP_LED`, the
  fingerprint reader sharing the button.
- The power LED is white alone on every Zephyr laptop board. `hx20` and
  `hx30`, the old EC, drive it on white, green and red channels, compose
  six colors from them in `pwr_led_color_map`, and answer a range for each,
  so the kernel names it `chromeos:multicolor:power`; that EC's
  `led_set_brightness` lights the first nonzero slot in the order red,
  green, blue, yellow, white, amber. The same boards expose the side LEDs as
  separate `left` and `right` ids rather than one charging LED.
- `dogwood`, the desktop, has a white `EC_LED_ID_POWER_LED` and an amber
  `EC_LED_ID_SECOND_POWER_LED`, which its policy lights together.

## The LED command

`EC_CMD_LED_CONTROL` sets colors and hands the LED between holders:

| Flag or value | Effect |
|---|---|
| `EC_LED_FLAGS_QUERY` | answers the brightness range per color and writes nothing |
| `EC_LED_FLAGS_AUTO` | `led_auto_control(id, 1)`, the EC's policy takes the LED back |
| a brightness array | `led_auto_control(id, 0)` and `led_set_brightness` in the same call |

- The range answered is a flat 100 for every color present:
  `led_get_brightness_range` writes the constant without consulting the
  hardware. Consequence: `max_brightness` promises a scale the firmware does
  not implement.
- `led_set_brightness` walks the LED's pin nodes and calls `led_set_color`
  for every color whose slot is nonzero, so the last nonzero color wins and
  nothing blends; all slots zero is off. The value carries no brightness:
  nonzero means the color at its devicetree duty.
- A brightness write is the handover, not a step before one: the same call
  clears the auto flag and sets the color, so no color can be written
  without taking the LED, and the flag is what the EC's tick consults
  before touching the LED.
- The EC keeps no readable record of the holder: `led_auto_control_flags` is
  a RAM word starting with every LED on auto, and the command answers only
  the range.

## The kernel's class

- The cros_ec LED driver registers `chromeos-auto` as a hardware-controlled
  trigger. The trigger active is the EC holding the LED; `none` with a
  brightness written is the host holding it.
- The class answers a `brightness` read with `ENODATA` while a
  hardware-controlled trigger is active, and with the value last written
  otherwise. The driver implements no `brightness_get`, so no hardware read
  stands behind the value.
- Consequence: the class is the only account of who holds a LED, and it is
  right only while every write goes through it. A command sent to the EC
  behind the driver leaves the record describing a policy the EC has
  stopped following, which is why `led.rs` darkens through the class and
  `ec.rs` carries no LED off.
- Consequence: the file answers whether the host holds the LED dark, and
  never whether the EC's policy has it lit.
- Writing `none` runs `led_trigger_set` with no trigger, which sets the
  LED off only when a trigger was attached. Consequence: an LED already on
  `none` and left lit stays lit until a brightness of 0 is written, which
  is why `darken` writes one after the trigger.
- The driver registers a multicolor device with one subled per color the
  EC gives a range for, and at probe sets `multi_intensity` to 100 for the
  first of them in the EC's color order and 0 for the rest. Consequence: at
  those defaults a nonzero `brightness` lights red on any LED that has red,
  the charging LED and the old boards' power LED alike.

## Power LED levels

`EC_CMD_FP_LED_LEVEL_CONTROL`, `0x3E0E`, in `led.c`:

| Version | Set takes | Get answers |
|---|---|---|
| v0 | a level: high, medium, low, ultra-low, auto | the stored percentage |
| v1 | a percentage, 1 to 100 | the percentage and a level deduced from it |

| Level | Percentage |
|---|---|
| high | 55 |
| medium | 40 |
| medium-low | 28; auto's step only, no level names it |
| low | 15 |
| ultra-low | 8 |

- Zero is refused in v1 with `EC_RES_INVALID_PARAM`. The EC will not let
  the host extinguish the power indicator through this command.
- `hx20` and `hx30` declare the command `EC_VER_MASK(0)` alone and take
  high, medium and low, refusing the rest
  ([framework-system #211](https://github.com/FrameworkComputer/framework-system/issues/211)).
  Ultra-low and auto need no v1, but arrived with the same firmware
  generation, so `Ec::custom_power_led_levels` asks for v1 as a stand-in.
  It is a proxy for what to offer, not a rule for what to refuse.
- Their get answers the raw BBRAM byte, 0 where no level was ever set,
  while `led_configure` lights the LED at high.
- `marigold` on `fwk-marigold-*`, and `azalea` and `lotus`'s 3.x line on
  `fwk-lotus-azalea-*`, also declare the command for v0 alone, taking
  high, medium and low.
- `sunflower` takes v1 and ultra-low but has no `FP_LED_BRIGHTNESS_AUTO`,
  refusing it with `EC_RES_INVALID_PARAM`, and builds without
  `CONFIG_PLATFORM_EC_DEDICATED_ALS`. The v1 stand-in would offer auto
  there, so `takes_power_led_auto` in `ec.rs` names the board.
- `dogwood` declares the command for v0, but its `bbram.dtsi` names no
  `fp_led_level` region: a set succeeds and changes nothing, and a get
  leaves the response's level unwritten. `keeps_power_led_level` in
  `ec.rs` names the board, and `Ec::power_led_level` answers
  `NotSupported` there.
- Storage is two BBRAM slots: `SYSTEM_BBRAM_IDX_FP_LED_LEVEL` for the
  percentage, and the `ALS_AUTO_FP` bit of `SYSTEM_BBRAM_IDX_BIOS_FUNCTION`
  for auto. Setting any level or percentage clears the bit; setting auto
  sets it and leaves the percentage alone.
- Auto is the ambient light sensor: `auto_als_led_brightness` steps the
  percentage through the five values above by lux thresholds, on boards
  built with an ALS. It is a writer of the percentage, not a level among
  the others.
- A v1 get deduces the level from the percentage, 55, 40, 15 and 8 mapping
  to a name and anything else to custom, then overwrites the answer with
  auto where the bit is set. Consequence: a custom percentage equal to a
  named level's reads back as that level, and auto's medium-low step would
  read as custom were auto not overriding it. A v0 get hands back the
  percentage alone; the same deduction is reproducible from it.
- The set is acknowledged at once and applied 100 ms later:
  `fp_led_level_control` stores the level and defers
  `change_pwm_led_maximum_duty`, which moves the PWM duty. Until then the LED
  carries the previous level, and `led_set_brightness` lights a color at
  whatever duty stands, so a write that lights the LED inside that window
  shows the old brightness. `power_led.rs` waits `LEVEL_SETTLE`, 150 ms.
- The BBRAM slot reads a 0 back as full brightness, 0 being its
  uninitialized value.

## Power LED off

- The level command has no off. Darkness is `EC_CMD_LED_CONTROL` with every
  slot zero, sent through the kernel's class so the class's record stays
  right, and undone by handing the LED back to `chromeos-auto`.
- Consequence: the power LED's off needs no mirror; the class's record is
  the readback, and the one event that leaves it stale behind a running
  host does not exist: an EC restart takes the host down and the reboot
  re-probes the driver.

## Charging LED colors

- Six colors, from the devicetree, and not a mixer: the pins beneath them
  are RGB and the board retunes what white means between chipset startup
  and shutdown, but that mixing is the EC's alone.
- The color has no read: the LED command answers only the range,
  `EC_CMD_PWM_GET_DUTY` answers only for the keyboard and display
  backlights, and the policy's choice lives in the EC's RAM.
- It can be derived from what the policy weighs, the charge state, the
  active port, the charge level and the chipset state, but a derivation
  cannot see a fault pattern pre-empting them.

### Observed

| Setup | Reading |
|---|---|
| The derivation against the LED, idle and charging | white and amber, matching |

## Charging LED sides

- Two LEDs answer to the one id, one on each side of the chassis, each
  behind its own enable: the EC GPIOs `left_side` and `right_side` in the
  devicetree.
- Under auto the policy lights the side of the active charge port, and a
  port at the back indicates on the right, the code's stated reason being
  Lot 6; both are darkened while discharging.
- Whenever auto is off the tick raises both enables, so a host that takes
  the LED gets both sides and cannot steer which is lit. That includes the
  handback: a nonzero brightness written just before the auto trigger lights
  both sides until the policy's next tick, 200 ms at most, where a zero one
  leaves them dark.
- `sunflower`'s devicetree declares both `left_side` and `right_side`; how
  many side LEDs the Laptop 12 fits is not in the source.
- `EC_CMD_GPIO_GET` reads a pin by name on a locked EC; only
  `EC_CMD_GPIO_SET` is refused there with `EC_RES_ACCESS_DENIED`. So which
  side is lit is readable, `Ec::side_enables`, and reads "both" for a LED
  the host holds dark.

## Charging LED faults

The EC pre-empts the charge pattern to raise faults on this LED, each a
blink pattern with no other channel to reach anyone by, in `laptop_led.c`:

| Fault | Boards |
|---|---|
| diagnostics running | all |
| battery cut off | all |
| no battery present, outside standalone mode | all |
| C cover open | all |
| GPU bay cover open | boards with a GPU bay |
| GPU module fault, on external power | boards with a GPU bay |
| input deck not fully populated | boards with a GPU bay |

- Consequence: a host holding the LED silences all of them.

## Persistence

| Value | Suspend | Reboot | EC restart | Source |
|---|---|---|---|---|
| Power LED level | kept | lost | kept | BBRAM survives the EC. UEFI setup re-sends its "Power Button Brightness Level" at every POST, and the EC itself writes the slot back to high, 55, when the chipset enters any off state after the ALS had stabilized; a suspend is not an off state |
| Power LED darkness | kept | lost | lost | the reboot re-probes the kernel's LED driver and re-attaches the auto trigger; an EC restart hands every LED back, `led_auto_control_flags` being RAM |
| Charging LED color | kept | lost | lost | the same two mechanisms; the EC holds the color across the host's reset and the kernel's re-probe gives it up |

- A custom percentage has nowhere in setup to be chosen from, so it cannot
  survive a reboot at all; a discrete level chosen in setup is what holds.
- Nothing re-sends a color or a darkness after a reboot.

### Observed

| Setup | Reading |
|---|---|
| A level set from the OS, setup's option on Auto, one reboot | read back as auto, with the EC's uptime counting through and its reset flags unchanged, so setup's command is the only writer that fits |

## Open

- The EC's shutdown write of 55 was inferred from `led.c`; the reboot
  observation cannot separate it from setup's re-send, both landing before
  the OS can read.

## Sources

- [FrameworkComputer/EmbeddedController](https://github.com/FrameworkComputer/EmbeddedController)
  — `common/led_common.c` for `EC_CMD_LED_CONTROL`, `led_auto_control_flags`
  and `led_is_supported`; under `zephyr/program/framework/`, `src/led.c` for
  `EC_CMD_FP_LED_LEVEL_CONTROL`, the ALS stepping, the shutdown reset and
  the side enables, `src/led_pwm.c` for `led_set_brightness`,
  `led_get_brightness_range` and `change_pwm_led_maximum_duty`,
  `src/laptop_led.c` for the fault patterns, `include/led.h` for the level
  percentages, `marigold/led_pins.dtsi` and `gpio.dtsi` for the colors and
  the enables; `common/pwm.c` and `common/gpio_commands.c` for the two
  commands' scope; `dogwood/led_pins.dtsi` on the `fwk-dogwood-*` branch,
  and `board/hx20/led.c` and `board/hx30/led.c` on `hx20-hx30`.
- The Linux kernel's `drivers/leds/leds-cros_ec.c`, the driver that
  registers the `chromeos-auto` trigger and sets the default intensities,
  `drivers/leds/led-class.c` for the `ENODATA` read under a hardware-controlled trigger,
  and `drivers/leds/led-triggers.c` for `led_trigger_set`.
- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  — `framework_lib/src/chromium_ec/commands.rs` for `FpLedBrightnessLevel`,
  and issue #211.
