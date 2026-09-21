# Hardware notes

What the embedded controller and the devices behind it actually do, as found
while building frameguin. These are facts about the machine rather than about
this app: they hold whoever is talking to the hardware, and most of them are
not written down anywhere else, which is why they kept being rediscovered.

Where a claim comes from firmware or a datasheet it is cited by name — the
ChromiumOS EC tree Framework forks, TI's documents for the battery gauge —
rather than by line number, which rots.

Every subject has its file under [`hardware/`](hardware/): the EC
transport and what is read or set over it, the ports, and each part's
transport, with [`hardware/parts.md`](hardware/parts.md) as the index by
part. What stays here is the one table that spans them.

Framework is a trademark of Framework Computer Inc.; this is an independent
project and names the hardware only descriptively.

## What survives what

Every control's persistence in one grid. Each row links to the file that
carries the mechanism and how it was established; nothing here is stronger
than the section it points at, so a row reading Unknown, or one whose
section marks its finding untested, means exactly that.

| Control | Suspend | Reboot | EC restart |
|---|---|---|---|
| [Charge limit](hardware/battery.md#persistence) | Kept | **Lost** | Kept |
| [Charge current limit](hardware/battery.md#persistence) | Kept | Kept | **Lost** |
| [Power button LED level](hardware/led.md#persistence) | Kept | **Lost** | Kept |
| [Power button LED darkness](hardware/led.md#persistence) | Kept | **Lost** | **Lost** |
| [Charging LED colour](hardware/led.md#persistence) | Kept | **Lost** | **Lost** |
| [Keyboard backlight](hardware/keyboard-backlight.md#persistence) | Kept | Kept | Kept |
| [Haptic touchpad](hardware/touchpad.md#persistence) | Kept | Kept | Kept |
| [Touchscreen, pad route](hardware/touchscreen.md#persistence) | **Lost** | **Lost** | not a case |
| [Touchscreen, panel route](hardware/touchscreen.md#persistence) | Unknown | Unknown | Unknown |
| [USB-C port enable](hardware/usb-c.md#persistence) | Unknown | Unknown | Unknown |

The pad route loses its setting to a fourth event the columns cannot carry —
the lid opening — and the panel route is the Laptop 12's, where none of the
pad's findings apply.
