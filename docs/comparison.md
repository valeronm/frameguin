# Comparison

What frameguin and the other live Linux tools for Framework laptops can do,
row by row. Each other tool's column was read from its source, not its
README, at the version below; frameguin's describes this checkout. How each
one reaches the hardware is in the README's
[Alternatives](../README.md#alternatives).

| Tool | Version read |
|---|---|
| [framework_tool](https://github.com/FrameworkComputer/framework-system) | 0.6.7 |
| [YAFI](https://github.com/Steve-Tech/YAFI) | 1.0 |
| [framework-tool-tui](https://github.com/grouzen/framework-tool-tui) | 0.8.6 |
| [framework-control](https://github.com/ozturkkl/framework-control) | 0.5.6 |

**Set** means the tool can change the value, **Read** that it only shows it,
and a dash that it does neither; the last section's cells describe instead.
The projects on the [community's list](https://community.frame.work/t/18559)
not here are inactive, Windows-only, or for one part alone.

## Contents

Every heading in the file appears here.

- [Charging and battery](#charging-and-battery)
- [Thermals and power](#thermals-and-power)
- [Input](#input)
- [Lights](#lights)
- [USB-C ports and expansion cards](#usb-c-ports-and-expansion-cards)
- [Chassis and sensors](#chassis-and-sensors)
- [Firmware and identity](#firmware-and-identity)
- [Persistence and platforms](#persistence-and-platforms)

## Charging and battery

| | frameguin | framework_tool | YAFI | framework-tool-tui | framework-control |
|---|---|---|---|---|---|
| Charge limit | Set | Set | Set, with a minimum | Set | Set |
| Charge speed | Set | Set | — | — | Set |
| Charger and its port | Read | Read | — | Read, per port | Read, without the port |
| Battery charge and flow | Read | Read | Read | Read | Read, with a week's history |
| Battery health | Read, with cell voltages and temperature | Read, with cell voltages and temperature | Read | Read | Read |
| Battery extender | Read | — | Set | — | — |

## Thermals and power

| | frameguin | framework_tool | YAFI | framework-tool-tui | framework-control |
|---|---|---|---|---|---|
| Temperatures | Read | Read | Read | — | Read |
| Fan speed | Read | Read | Read, the first fan only | Read | Read |
| Fixed fan duty | — | Set, or a fixed RPM | Set, or a fixed RPM | — | Set |
| Fan curve | — | — | — | — | Set |
| EC thermal thresholds | Read | Set | Set, fan off and fan max only | — | — |
| Throttle status | Read | Read | — | — | — |
| Processor power | — | — | — | — | Set: governor and EPP on Linux, TDP on Windows with AMD |

## Input

| | frameguin | framework_tool | YAFI | framework-tool-tui | framework-control |
|---|---|---|---|---|---|
| Touchpad haptic intensity | Set | Set | — | — | — |
| Touchpad click force | Set | Set | — | — | — |
| Touchscreen | Set | Set | — | — | — |
| Keyboard backlight | — | Set | Set | Set | — |
| Key remapping | — | Set | — | — | — |
| Per-key RGB | — | Set | — | — | — |
| Input deck mode | Set | Set | — | — | — |
| Tablet mode override | — | Set | — | — | — |
| Stylus battery | — | Read | — | — | — |

## Lights

| | frameguin | framework_tool | YAFI | framework-tool-tui | framework-control |
|---|---|---|---|---|---|
| Power button LED | Set: levels, a percentage, off | Set: levels and a percentage, not off | Set: levels, and off among the other EC LEDs | Set: a percentage down to 5%, not off | — |
| Charging LED off | Set | — | Set, with its color | — | — |
| Other EC LEDs | — | — | Set, write only | — | — |

## USB-C ports and expansion cards

| | frameguin | framework_tool | YAFI | framework-tool-tui | framework-control |
|---|---|---|---|---|---|
| What each port carries | Read, with what is attached | Read | — | Read, power only | — |
| Cable | Read | — | — | — | — |
| Turning a port off | Set, per port | Set, the ports of one controller together, as a debug command | — | — | — |
| HDMI and DisplayPort cards | Read | Read | — | — | — |
| Audio card | — | Read | — | — | — |
| Expansion bay | — | Read | — | — | — |

## Chassis and sensors

| | frameguin | framework_tool | YAFI | framework-tool-tui | framework-control |
|---|---|---|---|---|---|
| Chassis open switch | Read | Read | Read | — | — |
| Privacy switches | Read | Read | Read | Read | — |
| Lid and power button | — | Read | Read | — | — |
| Ambient light and accelerometer | — | Read | Read, ambient light only | — | — |

## Firmware and identity

| | frameguin | framework_tool | YAFI | framework-tool-tui | framework-control |
|---|---|---|---|---|---|
| Firmware versions | Read | Read | Read, BIOS and EC only | Read, BIOS only | Read, BIOS and EC only |
| Part identities and serials | Read | Read | Read, the battery's | — | Read, the mainboard's type |
| Flashing firmware | — | Set: EC, cards, keyboard controller | — | — | — |
| EC console, panic, raw host commands | — | Read | — | — | — |

## Persistence and platforms

| | frameguin | framework_tool | YAFI | framework-tool-tui | framework-control |
|---|---|---|---|---|---|
| Settings after a restart | Reapplied at boot and resume, by choice, as [Restore settings](../README.md#what-it-does) lists | — | — | — | Reapplied continuously |
| Platforms | Linux | Linux, Windows, FreeBSD, UEFI shell | Linux, Windows | Linux, Windows, FreeBSD | Linux, Windows |
