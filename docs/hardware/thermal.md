# Temperature sensors and fans over the EC

The temperature sensors, fans and their thresholds, read and never set. The
EC calls are `hardware/src/ec.rs`; the decoding is `hardware/src/thermal.rs`,
apart from `ec.rs` so it is testable without an EC; the device is
`hardware/src/device/thermal.rs`. [Fan control](#fan-control) is what the EC
does with them on its own, read by none of that code.

Each section holds what the EC tree or this code establishes, then under
**Observed** what was read on a machine. Every observation is from the
Laptop 13 Pro (Intel Core Ultra Series 3), `sakura`.

## Contents

Every heading in the file appears here.

- [The memmap sensors](#the-memmap-sensors)
- [The fans](#the-fans)
- [Fan duty](#fan-duty)
- [Fan control](#fan-control)
  - [Fan limits by board](#fan-limits-by-board)
- [Sensor names](#sensor-names)
- [The processor die's ceiling](#the-processor-dies-ceiling)
- [Thresholds](#thresholds)
- [Open](#open)
- [Sources](#sources)

## The memmap sensors

The sensors are in the EC's memory map, a [route](ec.md#routes) that costs
no host command, at offsets `EC_MEMMAP_TEMP_SENSOR` and, past
`EC_MEMMAP_THERMAL_VERSION`, `EC_MEMMAP_TEMP_SENSOR_B`.

| Range | Entries | Valid when |
|---|---|---|
| `0x00`–`0x0f` | 16 | always |
| `0x18`–`0x1f` | 8 | `EC_MEMMAP_THERMAL_VERSION` (`0x23`) is 2 or more |

Each byte holds kelvin less 200 (`EC_TEMP_SENSOR_OFFSET`), except for four
reserved values:

| Byte | Meaning |
|---|---|
| `0xff` | sensor not present |
| `0xfe` | sensor error |
| `0xfd` | sensor not powered |
| `0xfc` | sensor not calibrated |

## The fans

`EC_MEMMAP_FAN` (`0x10`) holds four little-endian words, one per fan slot.
`0xffff` (`EC_FAN_SPEED_NOT_PRESENT`) means the slot is empty. A stalled fan
reads 0 (`EC_FAN_SPEED_STALLED`) on current firmware; `0xfffe`
(`EC_FAN_SPEED_STALLED_DEPRECATED`) is the same condition on older firmware
and is read the same way.

The EC exposes no fan names or positions. The Laptop 16's two fans are
placed as `framework_tool` names them, left and right.

## Fan duty

`EC_CMD_PWM_GET_FAN_DUTY` (`0x0027`) takes a fan's index in one byte and
answers that fan's duty as a `u32` percentage, 0 to 100. The handler is
`hc_pwm_get_fan_duty` in `common/fan.c`. `framework_lib` 0.6.6 has no
request for it, so the daemon sends it by number.

Two of the board branches declare the command: `fwk-sakura-20260429` and
`fwk-sunflower-dahlia-2026-09-30`. `sunflower` is also built from
`fwk-sunflower-26784`, which does not declare it
([`ec.md`](ec.md#which-board-the-ec-tree-calls-this-machine)).

### Observed

| Setup | Reading |
|---|---|
| Fan stopped | 0 |
| Fan at 2357 RPM | 23 |
| Fan at 2943 RPM | 32 |

## Fan control

`sakura` builds with `CONFIG_PLATFORM_EC_CUSTOM_FAN_CONTROL=n` and
`CONFIG_PLATFORM_EC_FAN_RPM_CUSTOM=n`, which is the stock fan logic:

- `thermal_fan_percent` in `common/thermal.c` gives each sensor that holds
  both `temp_fan_off` and `temp_fan_max` a percentage:
  `100 * (t - off) / (max - off)` in whole numbers, held to 0 and 100.
- The highest percentage across the sensors goes to every fan, through
  `fan_set_percent_needed`.
- `fan_percent_to_rpm` in `common/fan.c` turns it into a target speed: 0
  for 0%, otherwise `((p - 1) * rpm_max + (100 - p) * rpm_min) / 99`, so
  1% is `rpm_min` and 100% is `rpm_max`.
- `fan_set_percent_needed` raises a target below `rpm_start` to it while
  the measured speed is under nine tenths of `rpm_min`.
- `fan_adjust_duty` in `zephyr/shim/src/fan.c` moves the PWM duty toward
  the target once the measured speed is off it by more than `rpm_deviation`
  percent. The step is 20, 10, 5, 3 or 1, larger the further off it is.
- `EC_CMD_PWM_GET_FAN_TARGET_RPM` (`0x0020`) answers the target as a
  `u32`, for fan 0 only: `hc_pwm_get_fan_target_rpm` takes no index.
- Consequence: the duty is the PWM value that holds the fan at its target
  speed. It is not the percentage a sensor was given, and a running fan
  does not read below the duty that gives `rpm_min`.

### Fan limits by board

The speed limits each board gives its fans and whose fan logic it builds,
on the branches [`boards.md`](boards.md#controls) lists. Only `sakura`'s
row is observed.

| Board | `rpm_min` | `rpm_start` | `rpm_max` | From | Fan logic |
|---|---|---|---|---|---|
| `sakura` | 1800 | 1800 | 6100 | `sakura/project.overlay` | stock |
| `marigold` | 2100 | 2100 | 6100 | `marigold/fan.dtsi` | its own |
| `azalea` | 2100 | 2100 | 6800 | `azalea/fan.dtsi` | stock |
| `lilac` | 2100 | 2100 | 6200 | `azalea/fan.dtsi` on its branch | stock |
| `sunflower` | 2000 | 2100 | 5600 | `laptop12/fan.dtsi`, or `sunflower/fan.dtsi` on its older branch | stock |
| `dahlia` | 2000 | 2100 | 5400 | `laptop12/fan.dtsi`, `rpm_max` set in `dahlia/project.overlay` | stock |
| `lotus` | 1000 | 1000 | 4000 and 3700 | `lotus/fan.dtsi`, two fans | its own |
| `tulip` | 1000 | 1000 | 3400 and 3100 | `lotus/fan.dtsi`, `rpm_max` set in `tulip/project.overlay` | `lotus`'s |
| `dogwood` | 600 | 600 | 2300 | `dogwood/fan.dtsi`, three fans alike | its own percentage |
| `hx20`, `hx30` | 1800 | 1800 | 6800 | `board/hx20/board.c`, `board/hx30/board.c` | stock |

- A board's own logic is `CONFIG_PLATFORM_EC_CUSTOM_FAN_CONTROL=y` for the
  percentage and `CONFIG_PLATFORM_EC_FAN_RPM_CUSTOM=y` for its mapping to a
  speed, both from the board's `src/thermal.c`. `marigold`, `lotus` and
  `tulip` set both; `dogwood` sets the first alone.
- `lotus` and `tulip` also set
  `CONFIG_PLATFORM_EC_CUSTOM_FAN_DUTY_CONTROL=y`, which replaces
  `fan_adjust_duty` with `lotus/src/fan.c`.
- `azalea`, and `lilac` as its variant, set none of the options, which
  default to off.
- `sunflower/src/thermal.c` and `dahlia/src/thermal.c` are not built: no
  source list in `zephyr/program/framework/CMakeLists.txt` names them, on
  either `sunflower` branch.
- `hx20` and `hx30` define neither option in `board.h`.
- `baseboard/fwk/build.mk` builds `baseboard/fwk/thermal.c` only under
  `CONFIG_FAN_VIRTUAL_TEMP`, which neither `hx20` nor `hx30` defines.
- `lotus/src/thermal.c` fills `board_fan_max` and `board_fan_min` from the
  expansion bay module. Where they are nonzero they replace the devicetree
  limits.
- `marigold/src/thermal.c` declares and reads the same tables, and nothing
  in its build assigns them. Consequence: they stay zero and the devicetree
  limits apply.

### Observed

The target by `EC_CMD_PWM_GET_FAN_TARGET_RPM` and the memmap, read
together under load:

| Read | Value |
|---|---|
| Target | 2885 |
| `cpu_f75303@4d` | 50 °C, 26% of its ramp |
| `local_f75397@4c` | 41 °C, 2% of its ramp |
| Every other sensor | below its `temp_fan_off` |
| Fan | 2917 RPM |

- 2885 is `fan_percent_to_rpm` of 26% with `sakura`'s limits.
- Consequence: the target is the highest percentage put through the stock
  mapping, and the fan runs close to it.

## Sensor names

`EC_CMD_TEMP_SENSOR_GET_INFO` (`0x0070`) answers a sensor's name as a
32-byte NUL-terminated string. The name is the sensor's devicetree node — a
prefix for where it sits, the chip, then `@` and its bus address — and the
same prefixes recur across every board's devicetree.

## The processor die's ceiling

The processor die's sensor reads over PECI, which answers a margin below
the junction maximum; `peci_get_cpu_temp` subtracts that margin from a
maximum the firmware is built with, so the sensor reads no higher than it:

| Board | Junction maximum | Set by |
|---|---|---|
| `sakura` | 100 °C | `CONFIG_PLATFORM_EC_PECI_TJMAX` in its `project.conf` |
| `dahlia` | 110 °C | the same option's default in `zephyr/program/framework/Kconfig` |
| `marigold`, `sunflower` | 110 °C | `CONFIG_PECI_TJMAX`, defined beside `peci_get_cpu_temp` |

No maximum is established here for a board the table leaves out.

## Thresholds

`EC_CMD_THERMAL_GET_THRESHOLD` (`0x0051`) version 1 answers, per sensor, the
warn, high and halt levels and their release temperatures, plus
`temp_fan_off` and `temp_fan_max`, the two ends of the EC's linear fan
ramp — all in kelvin, and a zero disables the threshold it sits on. Release
temperatures are not read.

Warn asks the host to throttle, high asserts PROCHOT, and halt shuts the
machine down (`common/thermal.c`). `EC_CMD_THERMAL_SET_THRESHOLD`
(`0x0050`) can rewrite any of them at any time, and there is no command
that restores the compiled-in defaults.

### Observed

The five sensors, as high, halt, fan off and fan max in °C:

| Sensor | High | Halt | Fan off | Fan max |
|---|---|---|---|---|
| `local_f75397@4c` | 88 | 98 | 40 | 75 |
| `cpu_f75303@4d` | 88 | 98 | 40 | 78 |
| `battery_temp@b` | 50 | 60 | 40 | 50 |
| `ddr_f75303@4d` | 87 | 97 | 40 | 50 |
| `peci-temp` | 120 | 127 | 103 | 105 |

None of the five carries a warn threshold. Every threshold of `peci-temp`
sits above the 100 °C
[that sensor can read](#the-processor-dies-ceiling), so neither its fan
ramp nor its trip points engage on PECI's own reading — the thermistors
drive the fan instead.

## Open

- Whether `EC_CMD_PWM_GET_FAN_DUTY` answers on `sunflower` and `dahlia`:
  their branch declares it, and no reading from either exists.

## Sources

- [FrameworkComputer/EmbeddedController](https://github.com/FrameworkComputer/EmbeddedController)
  — `include/ec_commands.h` for the memmap offsets, the reserved sensor
  bytes, the fan words, the threshold structures and the fan duty and
  target requests and responses; `common/thermal.c` for what warn, high and
  halt each do and for `thermal_fan_percent`; `common/fan.c` for the fan
  duty and target handlers, `fan_set_percent_needed` and
  `fan_percent_to_rpm`; `zephyr/shim/src/fan.c` for `fan_adjust_duty`.
  Under `zephyr/program/framework/`, each board's `project.conf`,
  `fan.dtsi` or overlay and `src/thermal.c`, with `CMakeLists.txt`, for its
  fan limits, build options and sources. `board/hx20/` and `board/hx30/`
  with `baseboard/fwk/build.mk` for the `hx20` and `hx30` fan limits, build
  options and sources.
  `zephyr/program/framework/src/cpu_power/intel_cpu_power_interface.c` for
  `peci_get_cpu_temp`, and each board's `project.conf` with
  `zephyr/program/framework/Kconfig` for the junction maximum.
- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  0.6.6 — `framework_lib/src/chromium_ec/commands.rs` for the version 1
  threshold struct.
