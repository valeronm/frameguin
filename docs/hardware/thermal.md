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
- [Sensor names](#sensor-names)
- [The processor die's ceiling](#the-processor-dies-ceiling)
- [Thresholds](#thresholds)
- [Observed](#observed)
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

## Sources

- [FrameworkComputer/EmbeddedController](https://github.com/FrameworkComputer/EmbeddedController)
  — `include/ec_commands.h` for the memmap offsets, the reserved sensor
  bytes, the fan words and the threshold structures, and `common/thermal.c`
  for what warn, high and halt each do;
  `zephyr/program/framework/src/cpu_power/intel_cpu_power_interface.c` for
  `peci_get_cpu_temp`, and each board's `project.conf` with
  `zephyr/program/framework/Kconfig` for the junction maximum.
- [FrameworkComputer/framework-system](https://github.com/FrameworkComputer/framework-system)
  0.6.6 — `framework_lib/src/chromium_ec/commands.rs` for the version 1
  threshold struct.
