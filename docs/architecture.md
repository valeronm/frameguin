# Architecture

How the code is arranged, and why. `CLAUDE.md` covers how to work on it;
`hardware.md` covers what the machine does. This is the shape the code is
moving to, one device at a time — the last section says how far it has got.

## Vocabulary

One meaning per word, and each word names one place in the tree.

- **Transport** — how the machine is reached: the EC, HID, a GPIO pad, the
  kernel's LED class, the firmware's SMBIOS table. `hardware/src/<name>.rs`.
- **Role** — what a device, or the mirror beneath it, needs of a transport,
  as a trait a stub can stand in for: `HapticPad`, `TouchSwitch`, `Store`. Declared beside the transport
  that fulfils it — or, where two transports can, beside the arbitration
  that picks one.
- **Device** — a thing detection finds on the machine: the haptic touchpad,
  a memory module, the battery. `hardware/src/device/<name>.rs`, one struct
  each, holding its roles and the mirror for what it cannot read back. A
  device is what is *found*; what it *offers* is said by which of the two
  facets below it implements.
- **Part** — the facet "something a person bought and can replace as a
  unit": `hardware::part::Part`, answering an `Identity`. The inventory is
  a `Vec<Identity>`. A device that is a part and no control detects into a
  list, memory being one per slot.
- **Control** — the facet "something that can be read, and usually set": one
  trait per device in `wire` — `BatteryControl`, `TouchpadControl`,
  `TouchscreenControl`, `PowerLedControl`, `PortsControl` — with one
  async fn per operation and three implementations, the device itself, the
  bus, and a stub. A trait with only getters is a control all the same:
  `PortsControl` sets nothing, what a USB-C port does being settled between
  its controller and whatever is plugged in. `DeviceError` is the one error every control and every
  detection raises.
- **Interface** — the D-Bus surface for one device's control, on
  `Served<Device>`. `daemon/src/interface/<name>.rs`. The root interface,
  for what belongs to no device, is `Daemon`'s own.
- **Bus** — the app's implementation of every control trait, each operation
  a call on the daemon. `app/src/bus.rs`, `Bus`.
- **Daemon**, on the app side — its end of the daemon: the connection and
  the controls detection registered, dialled and asked once for the run.
  `app/src/daemon.rs`, `Daemon`.
- **Client control** — the app's side of one control: its read as a
  `Snapshot`, its commands, its presets and words. `model/src/control/`,
  registered in `Controls`.
- **Group** — the window's widgets for one control. `app/src/window/`.

Nothing on the app side is called a device; "device" is reserved for the
real thing, and `GetDevices` is the inventory of devices as parts.

## One interface, three implementations

`wire` declares one control trait per device — `TouchpadControl`, and the
rest as they move — with one async fn per operation, and one error,
`DeviceError`, whose variants are the kinds a D-Bus error comes in plus
`Absent`. Everything that talks to a control talks through those traits, and
there are three implementations:

- **Device** — `frameguin-hardware`, the library that links the hardware
  libraries. Its `device::<name>` types implement the traits by touching the
  machine, argument checks included, so a caller linking the crate gets the
  same refusals the bus would give.
- **Bus** — the app's `Bus`, implementing every trait by calling the daemon,
  which runs the device.
- **Stub** — a test's, answering on the spot.

The bridge is therefore optional by construction: a process that links
`frameguin-hardware` needs no daemon. What ships keeps the bridge, because
the split is the security model: `frameguin-daemon` runs as root, links the
hardware crate and serves it over `io.github.valeronm.Frameguin`; `frameguin`
is the GTK app, links no hardware code, and is the only process a user
interacts with. The interface is private to the pair — they are built,
installed and upgraded together — so renaming, dropping or regrouping a
method is a free change.

What the daemon adds over the device is the bus's business alone: the idle
clock, and the order validate → skip → authorize → write with the polkit
prompt in the place that order puts it. A device exposes its argument check
separately (`Touchpad::check_haptic_intensity`) so the daemon can refuse
before it prompts; the setter runs the same check again for a caller that
came straight to it.

## Rows are layers, columns are devices

A device — the battery, the power button LED, the haptic touchpad, the
touch panel, the USB-C ports — is one column that crosses every layer the
same way. A layer is one row that every device crosses. What a device must
not know lives in another column; what a layer must not link lives in
another row.

A column need not reach every row: the USB-C ports are read and never set,
so they have no group of their own, no tray item and no words for a command —
a control trait with only getters is still a column, and stops where it runs
out of things to be. The one row they put in a window sits in the Power
group, that being the question it answers: what is coming in, beside what the
pack is doing about it — which is why that group is named for the subject
rather than for the battery whose control it otherwise holds.

| Layer | Crate | Links | Owns | Must not know | Tested against |
|---|---|---|---|---|---|
| Groups, tray | `app` | GTK, libadwaita, ksni, `model` | Widgets, toasts, the sync guard, timers, the tray thread's copy of each value | Which daemon operation a command becomes; any preset's value | Kept thin; not tested |
| Client controls | `model` | `wire` | One object per control: its read, its commands, its presets and words | GTK, the bus, another control's trait | A stub of the control trait |
| Control traits | `wire` | zbus, serde | One trait per device, one async fn per operation; `DeviceError` | How an operation is reached | — |
| Bus | `wire`, `app`, `daemon` | zbus, polkit | One proxy per interface and the vocabularies (`wire`); `Bus` implementing the traits over them (`app`); `Served<Device>` with the validate → skip → authorize → write order (`daemon`) | Anything that touches hardware (`wire`, `app`); which EC command a role sends (`daemon`) | Its own `wire` proxies over a socket pair, the devices on the same stubs |
| Devices | `hardware` | `wire` | `detect()`, the control impl with its argument checks, the `Part` impl, mirrors under a declared lifetime, arbitrations | The bus, polkit | The stub per role and the store in memory, in `hardware::testing` |
| Roles | `hardware` | — | One trait per hardware need: `Charger`, `Pack`, `PowerLedEc`, `LedClass`, `HapticPad`, `TouchSwitch`, `PdPorts`, `Store` | Who calls them | — |
| Transports | `hardware` | `framework_lib`, hidapi, libc | `Ec` and its lock, the sysfs LED node, the GPIO pad, the panel and touchpad HID, the SMBIOS table, the state file | Devices, policy, the bus | The machine |

The two trait rows are the seams. A stub replaces the real thing at either,
which is what makes the logic on both sides testable: the skip rule, a mirror's
lifetime and the power LED's release order on the device side, the
snapshot's movement under a refused write on the app's.

### One device, top to bottom

- **`hardware/src/device/<name>.rs`** — `detect()` answering whether the
  device is there and which of its operations work (the probe rule, beside
  the operations it vouches for), keeping the identity detection saw; the
  mirror for what cannot be read back, cut from `Mirrors` under the
  device's own key and the lifetime of whatever holds the value;
  `impl <Name>Control` with the argument checks inside it; `impl Part`. The
  device holds its transport as a `dyn` role, and `Mirror`s whose store is a
  `dyn` role beneath them — so it is constructible without hardware — and
  nothing of the
  bus, so it is constructible without a connection.
- **`daemon/src/interface/<name>.rs`** — the
  `#[interface(name = "io.github.valeronm.Frameguin1.<Name>")]` impl on
  `Served<Device>`, forwarding through the control trait with the bus's
  order around it.
- **`wire`** — `<Name>Control`, `<Name>Proxy` for that interface, and the
  vocabulary its values travel in, beside the strings both binaries must
  spell alike.
- **`model/src/control/<name>.rs`**, or `<name>/` where the words outgrow
  one file — `<Name><H: <Name>Control>` holding an
  `Rc<H>`; `detect()` by its own first read; a `read()` answering what the
  device reports — a `Snapshot` (`Copy`, `Send`, so the tray can hold one)
  where that is several values, the `wire` type itself where it is one;
  commands that call the hardware; the presets, rows and labels both
  front-ends draw from; its defaults.
- **`app/src/window/<name>.rs`** — the `PreferencesGroup`, `gate(control)`
  showing it where the device is, `show(snapshot)` moving the widgets under
  the sync guard, and handlers dispatching to the control's commands.
- **`app/src/tray.rs`** — one item per control, drawn from its snapshot and
  labels.

### The daemon's side

Every interface is registered at the one object path, and only where
`detect()` found the device. The interfaces present at the path are the
inventory of controls: `busctl introspect` shows exactly what was detected,
and a call to an absent device fails at the bus rather than in a
`NotSupported` every method spells. Detection runs at startup, before the
name is claimed, since registration needs its answer.

Presence therefore gates acceptance, where the probe rule used to gate only
the offer. A device whose detection fails transiently is off the bus until
the daemon's next start. The rule's stronger case is kept: a *feature* — the
power LED's custom levels, the pack's condition — is offer-only, and a
setter never refuses a write on the strength of one.

The logic that protects hardware stays on the root side, in the device:
validating arguments, ordering a level write before the LED is handed back,
believing a mirror only for the lifetime of whatever holds the state it
claims. Any client gets
that, which is why it cannot live in the app. Skipping a write already in
place is the daemon's, because what it protects is the polkit prompt.

### The app's side

The app implements every control trait once, over the bus, on one
connection dialled once per run. A client control is one object shared by
every window and the tray, and the pack's reading is taken once and fed to
every view showing it — a window and a report showing one pack must show one
reading. A command returns what happened, and one place in the window turns
that into a toast, a push to the tray and a move of the group; the tray
holds an `Option` per value in `TrayValues` and merges value-wise, a push
carrying only what the write moved.

`model` links neither GTK nor a bus connection, and Cargo enforces it — the
tray draws from it on ksni's own thread, and it is the one no-GTK rule here
the compiler checks. It is single-threaded by design: `Rc`, `Cell`,
`async fn` in traits without `Send`, because the app has one thread and a
stub answers on the spot.

## Parts and controls

A machine built from modules has two things to say about a device, and they
are asked differently.

A **control** is asked through its own trait — `TouchpadControl` and the
rest — because a caller of a control has to know which device it is talking
to. There is no common trait over controls, and none is wanted: nothing
loops over controls without knowing which one it holds.

A **part** is asked what it is through one common trait,
`hardware::part::Part`, answering an `Identity` — kind, vendor, model,
serial, the identifier it announces itself by, prefixed with its space
(`hid:093a:1343`, `dmi-slot:LPCAMM2_0`, `dmi-board:FRANMJCP07`), and every
firmware it would report — because its caller iterates the machine's bill
of materials without caring what any entry does. `Identity` lives in `wire`,
being what that caller receives: the daemon collects one per part at
startup, `GetDevices` answers with the list, and the app's parts window
draws it with the words `model::part` gives. Detection sees the identity
anyway, so a device keeps it rather than reducing it to a bool, and a device
that is a part and nothing else — the mainboard, a memory module — is a
device all the same.

The two facets are not one list. A memory module or an expansion card is a
part with no control; the power button LED is a control that is no part; the
mainboard is a part the daemon reads and never sets, and its BIOS, its EC and
its USB-C power delivery controllers are firmware it runs rather than parts of
their own — what is soldered to the board is not a part, there being no such
thing to order. So the inventory is its own
list, a control's device is on it only where it happens to be a part, and
the bus carries it as one method on the root interface —
`GetDevices -> Vec<Identity>` — beside the per-device control interfaces. Where a part maps to
something purchasable — the pad's descriptor names nothing, the part a
person buys is Framework's — that is `model::part::catalogue`, a curated
table keyed on the words a part announces for itself: words about values,
beside the labels; the device keeps what detection saw, not the word.

## Adding a control

One edit per row: a variant or method in `wire`, and for a new interface
its proxy in `wire::Proxies`; the device module in `hardware`, or a method
in one that exists, with a stub in `hardware::testing` for any role it
adds; its interface in the daemon, and its field in `interface::Devices`,
which is what puts it on the bus and in front of the proxies in
`interface/tests.rs`; the client control in `model`; the group; the tray
item. What another device shares is a line in a struct or a fan-out — the
`Devices`, `Proxies` and `Controls` fields, the window's `gate`, `watch`,
`load_values` and `connect_handlers` arms — never a body. Adding a part with
no control is one device module
implementing `Part`, and its line where the daemon collects the inventory
at startup.

## Migration

Devices move one at a time, each as one commit carrying that device through
every layer. The first carried the scaffolding the rest reuse: the keyed
`Store`, the shared `Service`, `Served`, the `model` crate and its traits.

Order: touchpad, touchscreen, power LED, battery. Smallest column first, the
one with the most shared state last.

Every control has moved: **touchpad, touchscreen, power LED, battery**.
Parts with no control: **mainboard, memory**. The keyboard backlight, which
the app never showed — the desktop already carries it on its own keys — was
dropped rather than moved.

A device detects itself at both ends — in `hardware` by its own probe, in
`model` by its own first read, which an unregistered interface answers with
`DeviceError::Absent`, the one kind only the bus raises, so a present
device's own `NotSupported` cannot read as absence. There is no capability
list: presence is the interface being on the bus, and the features a device
offers beyond presence travel on its own interface. The root interface
carries only what belongs to no device — the inventory and the daemon's
build.
