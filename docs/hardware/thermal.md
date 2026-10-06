# Temperature sensors and fans over the EC

The temperature sensors, fans and their thresholds, read and never set. The
EC calls are `hardware/src/ec.rs`; the decoding is `hardware/src/thermal.rs`,
apart from `ec.rs` so it is testable without an EC; the device is
`hardware/src/device/thermal.rs`.

Each section holds what the EC tree or this code establishes, then under
**Observed** what was read on a machine. Every observation is from the
Laptop 13 Pro (Intel Core Ultra Series 3).

## Contents

Every heading in the file appears here.

- [The memmap sensors](#the-memmap-sensors)
- [The fans](#the-fans)
- [Fan duty](#fan-duty)
- [Fan control](#fan-control)
- [Sensor names](#sensor-names)
- [The processor die's ceiling](#the-processor-dies-ceiling)
- [Thresholds](#thresholds)
- [Observed](#observed)
- [Open](#open)
- [Sources](#sources)

## The memmap sensors

A memmap read is not a host command: it asks the EC's shared memory
directly, at offsets `EC_MEMMAP_TEMP_SENSOR` and, past
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
answers the duty it is driven at as a `u32` percentage from 0 to 100, from
`hc_pwm_get_fan_duty` in `common/fan.c`. `framework_lib` 0.6.6 has no
request for it, so the daemon sends it by number.

Only the branches `fwk-sakura-20260429` and
`fwk-sunflower-dahlia-2026-09-30` declare the command, so `sunflower` has
it on the second of its two lines and not on `fwk-sunflower-26784`, and no
other board has it.

## Fan control

How `sakura` gets from a temperature to a duty, built with
`CONFIG_PLATFORM_EC_CUSTOM_FAN_CONTROL=n` and
`CONFIG_PLATFORM_EC_FAN_RPM_CUSTOM=n`:

- Each sensor holding both `temp_fan_off` and `temp_fan_max` asks for a
  percentage, `100 * (t - off) / (max - off)` in whole numbers, 0 below
  `temp_fan_off` and 100 above `temp_fan_max`: `thermal_fan_percent` in
  `common/thermal.c`.
- The highest percentage across the sensors goes to every fan, through
  `fan_set_percent_needed`.
- `fan_percent_to_rpm` in `common/fan.c` turns it into a target speed: 0
  for 0%, otherwise `((p - 1) * rpm_max + (100 - p) * rpm_min) / 99`, so
  1% is `rpm_min` and 100% is `rpm_max`.
- `fan_adjust_duty` in `zephyr/shim/src/fan.c` moves the PWM duty toward
  that speed in steps of 20, 10, 5, 3 or 1 by how far the measured speed
  is from it.
- `EC_CMD_PWM_GET_FAN_TARGET_RPM` (`0x0020`) answers the target as a
  `u32`, for fan 0 only: `hc_pwm_get_fan_target_rpm` takes no index.

- Consequence: the duty is what the loop settled on, with a floor wherever
  the fan turns at `rpm_min`, and is no measure of how far into its ramp a
  sensor is.

The speed limits each board's devicetree gives its fans:

| Board | `rpm_min` | `rpm_start` | `rpm_max` | From |
|---|---|---|---|---|
| `sakura` | 1800 | 1800 | 6100 | `sakura/project.overlay` |
| `marigold` | 2100 | 2100 | 6100 | `marigold/fan.dtsi` |
| `azalea` | 2100 | 2100 | 6800 | `azalea/fan.dtsi` |
| `lilac` | 2100 | 2100 | 6200 | `azalea/fan.dtsi` on `fwk-lilac-27116` |
| `sunflower` | 2000 | 2100 | 5600 | `laptop12/fan.dtsi`; `sunflower/fan.dtsi` on its older line |
| `dahlia` | 2000 | 2100 | 5400 | `laptop12/fan.dtsi`, `rpm_max` set in `dahlia/project.overlay` |
| `lotus` | 1000 | 1000 | 4000 and 3700 | `lotus/fan.dtsi`, two fans |
| `tulip` | 1000 | 1000 | 3400 and 3100 | `lotus/fan.dtsi`, `rpm_max` set in `tulip/project.overlay` |
| `dogwood` | 600 | 600 | 2300 | three fans alike |

- `marigold`, `lotus` and `tulip` build with
  `CONFIG_PLATFORM_EC_CUSTOM_FAN_CONTROL=y` and
  `CONFIG_PLATFORM_EC_FAN_RPM_CUSTOM=y`, taking the percentage and its
  mapping to a speed from `marigold/src/thermal.c` or `lotus/src/thermal.c`.
  `sunflower` and `dahlia` set both off, as `sakura` does.
- `lotus` fills `board_fan_max` and `board_fan_min` from the expansion bay
  module in `lotus/src/thermal.c`, and its fans take those limits where
  they are set.

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

## Observed

The Laptop 13 Pro's five sensors, as high, halt, fan off and fan max in °C:

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

The fan's duty, by `EC_CMD_PWM_GET_FAN_DUTY`:

| Setup | Reading |
|---|---|
| Fan stopped | 0 |
| Fan running | 19 |
| Fan at 2357 RPM | 23 |

## Open

- Whether a target read with the fan running matches `fan_percent_to_rpm`:
  `EC_CMD_PWM_GET_FAN_TARGET_RPM` was read only with the fan stopped, as 0.
- What fills `board_fan_max` and `board_fan_min` on `marigold`, which
  declares and reads them.
- Whether `sunflower/src/thermal.c` and `dahlia/src/thermal.c` are built:
  both boards set the custom options off, and no build rule naming either
  file was found.
- The fan limits of `hx20` and `hx30`.

## Sources

- [FrameworkComputer/EmbeddedController](https://github.com/FrameworkComputer/EmbeddedController)
  — `include/ec_commands.h` for the memmap offsets, the reserved sensor
  bytes, the fan words, the threshold structures and the fan duty request
  and response, and `common/thermal.c`
  for what warn, high and halt each do; `common/fan.c` for the fan duty
  and target reads and `fan_percent_to_rpm`, `zephyr/shim/src/fan.c` for
  `fan_adjust_duty`, and each board's `project.conf`, `fan.dtsi` or overlay
  under `zephyr/program/framework/` for its fan limits and build options;
  `zephyr/program/framework/src/cpu_power/intel_cpu_power_interface.c` for
  `peci_get_cpu_temp`, and each board's `project.conf` with
  `zephyr/program/framework/Kconfig` for the junction maximum.
- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  0.6.6 — `framework_lib/src/chromium_ec/commands.rs` for the version 1
  threshold struct.
