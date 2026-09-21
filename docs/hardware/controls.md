# Controls

Every control the daemon sets, and what each survives. The mechanism behind
each row is in the file the row links to; [`parts.md`](parts.md) is the
matching index by part.

## Contents

Every heading in the file appears here.

- [What survives what](#what-survives-what)

## What survives what

Every control's persistence in one grid. Each row links to the file that
carries the mechanism and how it was established; nothing here is stronger
than the section it points at, so a row reading Unknown, or one whose
section marks its finding untested, means exactly that.

| Control | Suspend | Reboot | EC restart |
|---|---|---|---|
| [Charge limit](battery.md#persistence) | Kept | **Lost** | Kept |
| [Charge current limit](battery.md#persistence) | Kept | Kept | **Lost** |
| [Power button LED level](led.md#persistence) | Kept | **Lost** | Kept |
| [Power button LED darkness](led.md#persistence) | Kept | **Lost** | **Lost** |
| [Charging LED colour](led.md#persistence) | Kept | **Lost** | **Lost** |
| [Keyboard backlight](keyboard-backlight.md#persistence) | Kept | Kept | Kept |
| [Haptic touchpad](touchpad.md#persistence) | Kept | Kept | Kept |
| [Touchscreen, pad route](touchscreen.md#persistence) | **Lost** | **Lost** | not a case |
| [Touchscreen, panel route](touchscreen.md#persistence) | Unknown | Unknown | Unknown |
| [USB-C port enable](usb-c.md#persistence) | Unknown | Unknown | Unknown |

The pad route loses its setting to a fourth event the columns cannot carry —
the lid opening — and the panel route is the Laptop 12's, where none of the
pad's findings apply.
