# Frameguin — working notes for Claude Code

Read README.md first for what the project is. This file covers how to work on
it and the non-obvious constraints.

## Layout and contracts

- Five crates, two binaries: `hardware/` is direct access to the machine —
  the transports, the roles, and the devices implementing the control
  traits `wire` declares — and the only crate linking `framework_lib` and
  `hidapi`; `daemon/` runs as root, links it, and serves it over the bus;
  `app/` is the GTK4/libadwaita UI and links no hardware code; `wire/` is
  the D-Bus vocabulary, the control traits and the error kind every
  implementation of them shares; `model/` is the controls as the app holds
  them, over those traits. `docs/architecture.md` opens with the vocabulary
  — transport, role, device, part, control, interface, bus, client control,
  group — and each word means one thing; "device" is the real thing on the
  machine and nothing on the app side. The
  split is the security model — the root process carries no GUI, the GUI
  process has no hardware access — and the D-Bus interface
  `io.github.valeronm.Frameguin1` is their only bridge. Nothing that touches
  hardware may enter `wire/`: the app links it, so a dependency added there
  lands in the unprivileged process too.
- `io.github.valeronm.Frameguin1` is private to those two binaries rather than
  published API. They are built, installed and upgraded as one — `install.sh`
  stops the app and the daemon and brings both back on the new build, and the
  package does the same — so an app talking to a daemon of another version is
  not a state this project has to work in, and nothing outside the pair is a
  caller it answers for. Renaming a method, dropping one or respelling a
  feature is a free change needing no deprecation window. (`busctl` against
  it stays a fine way to inspect a running daemon; it is a debugging tool, not
  a client the interface holds still for.) None of this loosens what `wire/`
  is for: the two ends still restate the interface separately, so within one
  version the vocabularies are what keep them from drifting apart.
- `wire/` holds the proxy the app calls through, the bus name and object
  path, and the vocabularies as enums serializing as `s`. The daemon's
  `#[interface]` impls cannot move there — each is an impl on a daemon-side
  type — so the method set is still two declarations, and they meet in
  `daemon/src/interface/tests.rs`, which serves every device on stub roles
  to the `wire` proxies over a socket pair: a method one end spells and the
  other does not fails there rather than in an installed pair. The devices
  it serves are `interface::Devices`, the one struct `main.rs` fills from
  detection, and the proxies it dials are `wire::Proxies`, the one struct
  the app dials too, so a device served or dialled by one end and not the
  other is a missing field. A call over the pair is awaited on the client
  connection's executor (`Peer::run`), never under `block_on` on the test
  thread: async-io's `block_on`, parked behind the thread driving its
  reactor, can miss the wakeup for a message just written, and under load
  does. What the enums buy is the other half: a feature,
  level or click force the two ends spell differently used to be a
  well-formed string that meant nothing to the receiver, and is now a
  compile error. Adding a control is one edit per layer, which
  `docs/architecture.md` lists under "Adding a control".
- `docs/architecture.md` is the shape of the code: what each layer may
  link, and what a device's column holds at each. Read it before touching a
  device.
- Inside `app/`, a module boundary is drawn where it makes a class of mistake
  impossible, not where a file got long. A control's presets, the values
  behind them and the words those values carry are its `model` control's —
  the chrome around them, group and row titles and the sentences a toast
  makes, stays with the widget that is its only site. `model` links neither
  GTK nor the bus, so the window and the tray cannot disagree about what a
  preset sends or what it is called, and a control exists in the app only
  where its device answered — the probe rule, held up by the compiler at
  this end. `tray.rs` links no GTK
  either, which matters because its menu runs on ksni's own thread; its
  fields are private, so `tray_push` is the only way that state moves.
  `ui.rs` is the largest and stays that way on purpose: `Ui`'s fields are
  private to it, which is what `Sink` and the `apply_*` writes living there
  buys. `battery.rs` is the battery report, the app's one window that only
  reads — no sync guard, no debounce, no tray push — and every field it holds
  is a descendant of the page its subscription hangs on, so nothing in it can
  reach back up the widget tree and outlive the window. `parts.rs` is the
  other read-only window, the inventory `GetDevices` answers, drawn once per
  open since the list is fixed for the daemon's run; `report.rs` is the
  shell the two share — found by name or built, destroyed on close — so the
  single-instance rule is one function rather than one copy per window.
  `reading.rs` is the
  pack's reading, taken once for however many views show it: the status row
  and the report render the same walk of the same block, and each polling for
  itself made the EC answer twice and let the two windows sit a tick apart, so
  a view subscribes and the feed does the reading; it also holds the two facts
  fixed for a daemon's run that every window wants — the bus connection and
  the detected controls — so the window, the report and the tray share one
  of each. (`about.rs` dials for itself, deliberately: its report also runs from
  `--debug-info` where no app state exists, and a bug report wants a fresh
  answer rather than one the app is already holding.) `mapped.rs` is
  the rule both timers and subscriptions obey, that nothing repeats while its
  widget is off screen: `while_mapped` takes what `acquire` returns on map and
  drops it on unmap, so stopping is a `Drop` rather than something each caller
  remembers. `board.rs` is the one read
  that bypasses the bus, `about.rs` the report and the dialog that renders it,
  `autostart.rs` the desktop entry whose path and content cannot be written
  apart. `main.rs` keeps what has to know both front-ends — the app id, the
  bus attachment, the lazily built window, the shared reading, and the tray
  event loop, exhaustive over `TrayEvent` so a new variant fails to build
  until it is handled there — along with the application-wide odds and ends
  that belong nowhere else: the command-line options and the actions no
  module of its own owns. The no-GTK rules are the ones nothing
  checks: an import is all it takes to lose one.
- The two windows are reached differently, and which way is decided by whether
  the window survives being closed. The main window hides rather than closing
  wherever there is a tray to hide to, so its slot in `AppState` outlives it —
  and the slot is also where the tray finds the `Rc<Ui>` it reports presets
  into, so `window_for` builds it once and both front-ends take it from there.
  (Where the tray failed to spawn there is no second front-end to reach it,
  which is what keeps that slot honest in the session where the window really
  is destroyed on close.) The report is destroyed on close, so a
  slot would hold a dead window: it goes through a `gio` action on the
  application instead, and finds an already-open copy in the application's own
  window list, which GTK keeps accurate for free. Where a window is opened
  from more than one place, the module owning it owns the `ActionEntry` too
  and keeps its builder private, so the action is not merely the agreed way in
  but the only one that compiles — `battery.rs` does this because both
  front-ends reach the report. `about.rs` does not, and needs not: only the
  window's menu opens it, so `main.rs` holding that entry leaves nothing able
  to drift.
- Inside `hardware/`, the transport modules are drawn by how a control
  reaches the machine, so the filename answers which way: `ec.rs` the EC,
  `led.rs` the kernel's LED class, `touchpad.rs` the pad's own HID transport,
  `panel.rs` the touch panel's, `gpio.rs` a pad on the processor through the
  GPIO character device. Two of those pairs need an arbitration, and the two
  arbitrations are not alike: the power button LED has two possible drivers
  and one at a time, so what is settled is a handover and the order to make
  it in — and that lives in the device, `device/power_led.rs`, over the
  `PowerLedEc` and `LedClass` roles, because the order is what a stub has to
  be able to check; `touchscreen.rs` because the panel has two possible
  routes and a machine has only one, so what it settles is a precedence — and
  then the thing that precedence decides, which is whether the control can be
  read at all, stated once in the `TouchSwitch` role it declares over both.
  The rest divide by job: `dmi.rs`
  the SMBIOS reads — the vendor deciding whether there is an EC to open, the
  product name deciding which board's pads are which, the raw entries a
  part's identity comes from — `lifetime.rs` what
  holds a mirrored value and whether it still holds it, where `dmi.rs`
  answers for the machine, which is the difference between a fact a reboot or
  a sleep changes and one that outlives both, `state.rs`
  the keyed store for what cannot be read back, `mirror.rs` the mirror a
  device reads and writes such a value through, declared under its own key
  and the `Lifetime` of whatever holds it, `testing.rs` the stub per role
  and the store in memory, which the daemon's tests build the same devices
  from under the `testing` feature. A module's own doc says what it is
  for; the reasoning is here. `ec.rs` is every EC call: `Ec` is the only
  holder of the `CrosEc`, one method per operation, each taking the lock and
  releasing it before returning and none reaching the handle through another —
  `Mutex` does not re-enter, so a method wanting two commands under one lock
  issues both against the guard it holds. `daemon/src/main.rs` keeps the
  `Daemon` object with the root interface, polkit, the idle exit and the
  detection that registers each device's interface; `daemon/src/interface/`
  holds those interfaces, and what stays inline in each is the *order* —
  validate, skip a write already in place, authorize, write. That order is
  the policy, and splitting the write out would leave it legible at neither
  end — and would not enforce it either, since `device()` hands out the
  device to any body, so a setter that never authorizes compiles with or
  without a helper. What holds the invariant is `interface/tests.rs`, where
  every setter is refused when polkit refuses and the device left untouched,
  not a shape the bodies pass through.
- A control the tray can also set gets an `apply_*` function owning the whole
  write: the daemon call, the toast, the tray's copy, and moving the widget to
  match. Both the window's handler and the tray item call it; neither writes
  around it. Controls only the window sets (the touchpad) write inline
  in their handler — one caller needs no shared function, and giving it one
  would be ceremony. Writing *by* moving the widget, which the tray used to
  do, makes state the command channel: a widget already showing the requested
  value emits no change, so the write is silently dropped — which is what a
  tray click on a stale window hits. Debouncing stays with the widget that
  needs it; a tray click is discrete.
- Every window widget is already carrying the user's choice when its handler
  runs — `notify` fires after the move, for a combo as much as for a switch —
  so a refused write leaves the window asserting a state the hardware never
  took. Correct it where both hold: the prior value is recoverable without a
  read, and the wrong assertion is one a reader acts on rather than merely
  looks at. The touchscreen is the case that meets them, its prior value
  being the negation and its wrong claim being "touch is off"; everything
  else here would have to capture a value before the write or re-read after
  it, for a stale row that outlives nothing worse than the next reload.
- The tray offers one menu item per control, not per value, and every control
  takes the same shape: a submenu over the states it offers, named after the
  one in force. A value dialled in from the window is not among them, so no
  row is marked — but whether the title still names it is the control's own
  call, and they differ deliberately: the charge ones spell the raw value,
  since a menu that said nothing about a limit set from the window would be
  worse than one naming a row it cannot mark. Two-state controls go through
  the same shape rather than drawing a checkmark; `touchscreen_item` carries
  why. What a row sends is a *state*, never a gesture: a click saying only
  "toggle" would invert whatever
  the app believed by the time it landed, which is the command-channel mistake
  above in its other form — there a widget's position stood in for the
  command, here the gesture would.
- A setter skips a write already in place where a client's idea of the value
  can be stale, and skips it before the polkit call for the same reason
  argument checks come first — nobody should answer a prompt for a write that
  won't happen. The skip belongs in the daemon rather than in a caller, asked
  of the closest thing to the truth each one has: the battery's
  `set_charge_limit` asks the EC, its `set_charge_current_limit` the
  device's mirror, the touchscreen's `set_enabled` the pad — and on the
  route with no pad, nothing: it skips no write at all. A
  mirror is worth skipping on only where the event that invalidates it is the
  one its lifetime ends on, which holds for the charge current limit and not
  here: the panel's mirror catches the boot and the sleep, and a lid opening
  moves the panel with neither, so within one waking run it is no fresher
  than the client's own idea. What decides is whether a client can be stale,
  not whether the value is readable.
- **Evidence is best effort, never a gate.** Where a mirror takes evidence
  for its lifetime — the EC's boot for the charge current limit, the host's
  for the touch panel — evidence that cannot be taken costs the record and
  not the write: `Mirror::record` makes the write and then holds nothing.
  What evidence buys is knowing later whether the value still stands, and
  that is never worth refusing a write the hardware would have taken: it
  trades a control the user asked for against a label that is approximate
  anyway. The writes this applies to are state assertions rather than
  gestures, so re-asserting one already in force costs nothing either.
- D-Bus types name the value, not either end's convenience: a percentage is
  `y` (`u8`), never GTK's f64. The daemon validates every argument because
  any client can call it — an app-side clamp is UI convenience, not the check.
- In `Daemon`'s `#[interface]` impl the signature carries meaning: `async`
  means the method awaits polkit, `fdo::Result` that it can fail — neither
  implies it touches the hardware. zbus boxes sync and async alike, so never
  reach for `async` to get concurrency. A `Served<Device>` interface is
  `async` throughout, the control trait it forwards to being async for the
  bus's sake; there the meaning is carried by the order in the body instead.
- The daemon's connection runs on one executor thread and every hardware call
  blocks rather than awaits, so a slow one stalls every other task on that
  connection. Detection — the pinctrl walk for the touchscreen's pad, the EC
  waking — runs before the name is claimed, so no client waits on it; a
  slow call after that is fixed by moving it off the executor onto a
  blocking pool, not by more `async`.

## The probe rule

Each device's `detect` and `new` apply it: one probe per exposed operation,
and a probe vouches for an operation only by a side-effect-free
exercise of that operation's own code path, or by curated knowledge — not by
an adjacent, easier check. The reason is concrete, and turned out to be worse
than first understood: the touchscreen's version read succeeds on a panel that
has no enable command at all. What stops touch is a pad on the processor,
which the controller neither knows about nor answers for — so a version-based
probe would have vouched not for a command that fails on some hardware, but
for one this panel does not implement. That the Laptop 12's panel does
implement it is the point sharpened: the read said nothing either way.

Write-only controls have no same-path probe to run, and take one of two other
forms. Asking the firmware whether it implements the exact command the setter
sends (`Ec::command_supported`) is a probe about that command and nothing
else, which is what separates it from the touchscreen trap — there a read
answered for a command the panel turned out not to have. Where even that isn't
available, the condition is hardcoded with a comment explaining why. A probe
may also require more than command support: `charge-current-limit` needs a
readable battery too, because a cap is only offered as a share of what the
pack asks for, and a feature should mean the control works rather than
merely that the write exists.

A probe decides what to *offer*, never what to *accept*. Some are proxies:
the power LED's custom levels ask for command v1, exactly right for the
percentage write but only a stand-in for the ultra-low and auto levels, which
the v0 handler takes on any firmware that has them. Narrowing an offer on a
proxy costs at worst a row nobody could have used; refusing a write on one
denies a call the EC would have honoured — and what a device offers is
settled once per daemon lifetime, so one transient read would deny it for the
whole run. So setters validate against the thing itself: `PowerLed::check_level`
looks up the LED node rather than consulting the levels it offered.

## What the hardware forces on the code

`docs/hardware.md` is what the machine does, a chapter per subsystem — naming
them here as well would be a second index to keep true. Findings belong there
rather than here, since they stay true whoever is talking to the hardware;
what belongs here is what they force on *this* code. The exception is a
finding that is the evidence for a rule stated here, like the touchscreen's
version read under the probe rule — separating those would leave the rule
asserted and its reason a file away.

- The daemon opens `Ec` behind the DMI vendor check, never constructing it
  speculatively: `CrosEc::new()` panics outright where `framework_lib`
  finds no driver. Nothing the EC answers for is detected there; the
  devices reached over HID — the haptic touchpad, the touch panel — detect
  themselves outside it.
- A value the EC is a second writer for cannot be shown from what was last
  written. A value with no readback at all is mirrored instead, through a
  `Mirror` that moves only once the hardware has taken the write, so a
  refused one leaves the last accepted value standing. The power LED's off
  needs no mirror: the kernel's record of holding it dark is the readback,
  and the one thing short of a command sent behind the driver's back that
  leaves it stale — an EC restart — takes the host down with it, so the
  reboot re-probes the driver before anything can read it. A mirror is
  declared with the `Lifetime` of whatever holds the state it claims, and
  which holder that is gets settled by evidence rather than by which life
  is shorter: `Permanent` for the touchpad, which keeps its own in flash;
  `Ec` for the charge current limit, firmware having been shown to leave it
  where the charge limit and the LED level are both re-asserted at POST, so
  it expires with the EC that took it and not with the host; `HostAwake` for
  the touch panel — the host's boot together with the time it has spent
  asleep, since the controller is expected to come up reporting from a
  reboot and from a resume alike. The evidence for `Ec` is the EC's boot
  instant, read once when the daemon starts and never again, and the
  hardware is what settles that: an EC restart takes the machine down with
  it, so no run spans one. The host's boot is read once for the same reason;
  its sleep is not, being the one thing here that moves under a running
  daemon, which is why weighing `HostAwake` reads the host afresh each time.
  The touchscreen is both, and which it is depends on the
  route: the pad carries the level, so where a pad is the control the getter
  asks the hardware, and where the panel's own command is, there is nothing
  left to ask and the device answers from its mirror. That asymmetry is the
  reason the `TouchSwitch` role answers `Option<bool>` — a device that
  reached for one account would have to know which machine it was on. Its
  second
  writer is the platform firmware rather than this daemon — a lid opening
  drives the pad back, as a resume does — and neither is something the app is
  told about, so both front ends can show a value the firmware has already
  moved. The window
  re-reads on being mapped; the tray asks when its menu opens, which is a
  request it cannot wait for, so the first menu after such a change still
  draws the old value and the one after it is right.
- A device never sees evidence, only its value or its absence. What
  evidence is and how it is weighed is `lifetime.rs`, keyed on the one
  `Lifetime` the device declared; *when* — witnessed before the write, kept
  beside the value, weighed on every read — is `mirror.rs`, which names no
  holder. So a device cannot weigh a write against the wrong holder, and a
  new holder is one variant in one file. Evidence that cannot be
  weighed is never believed — a holder that will not answer withdraws every
  record of its lifetime — and what a mirror holds after its holder's life
  ended is a state both know, the current cap lifted, the panel reporting,
  so no device keeps a rule of its own for it.
- What the pack is asked directly falls into two groups, and the split is why
  one is a feature the battery offers and the other is not. The temperature,
  cell voltages and alarms have no fallback, so they are one operation behind
  `BatteryFeature::Condition`, probed by the getter's own read and only on a
  device whose pack answered — a mainboard running standalone must not spend
  transfers asking a battery that is not there what it thinks. The cycle
  count and the manufacturing date each fall back to the EC's answer or to
  nothing, so they need no feature and are read inside the `Pack` role's
  `info`, once per run and then remembered, absence included.
- A direction is never taken from the EC's flags alone; `charge_flow` weighs
  them against the charger and the rate, because the flags do not mean what
  their names suggest and the charge limiter produces a state they cannot
  express.

## Build, run, verify

- `cargo build --release`, then `sudo ./install.sh` installs system-wide
  (it kills and restarts a running app). Install and uninstall change system
  files and need sudo, so the user runs them.
- `clippy::pedantic` is on workspace-wide and CI gates on `-D warnings`, so
  both crates build warning-free. CI lints only the binaries, not test code.
- CI also gates on `cargo fmt --all --check`, with the style edition pinned
  in `rustfmt.toml` rather than inferred from each crate's own edition — so
  an edition bump cannot reformat the tree as a side effect. Run `cargo fmt`
  before pushing; nothing local enforces it.
- A change that moves the window or the tray menu re-shoots `screenshot.png`
  or `screenshot-tray.png` in the same commit: the metainfo serves both from
  `main`, so between the two commits `main` is wrong.
- `packaging/changelog` is written at release time, not accumulated per
  commit, so a change that will deserve a bullet owes nothing when it lands.
- Cutting a release is `docs/release.md`.
- Smoke test: run `target/debug/frameguin`. The app is single-instance, so a
  second launch only activates the resident one — kill it first to exercise
  a fresh build.
- Daemon logs: `sudo journalctl -u frameguin-daemon.service`. Direct calls:
  `busctl call io.github.valeronm.Frameguin /io/github/valeronm/Frameguin
  io.github.valeronm.Frameguin1 GetDevices`; `busctl introspect` on the
  same path lists the control interfaces detection registered.
- Non-Framework hardware is a test case in its own right: expected behavior
  is "No Framework hardware detected" in the header, a status page naming the
  vendor where the controls would be, fast, no error toast. Both real
  regressions so far (port-I/O probe stalls, the aarch64 panic) showed up only
  there.
- A window with no controls says which of its three reasons it is — no
  Framework hardware, a daemon it could not reach, a Framework board that
  answered with no controls. They look identical as a bare empty window,
  and only one of the three is a bug worth a report, so the page carries the
  distinction rather than a toast that is gone by the time anyone asks.

## Conventions

- Comments explain why, not what, and carry no references to sessions,
  dates, or private context — the repo is public and must read standalone.
  They open on the constraint rather than on a restatement of the code
  beside them, name things the way the code names them, and stop where the
  fact stops: one sentence, unless a second independent constraint earns
  another. A docstring on a cross-module interface may run longer, for the
  behavioural contract and the caller-side caveats only. Tests carry no
  commentary — a fixture that needs explaining wants named values instead.
- **A why that needs a paragraph is architecture, and belongs in this file**
  or in `docs/`, not in a module doc. The module doc says what the module is
  for; the reasoning behind a mechanism — why a boundary sits where it does,
  what a rule is protecting against — goes here, where one statement covers
  every module that obeys it. The shape of the whole — the layers, what each
  links, what a device is at each — goes to `docs/architecture.md`, and a
  fact about the machine to `docs/hardware.md`.
- Clippy suppressions live at the site with a `reason`, never in a manifest:
  a manifest allow is invisible where the code is read and blankets the whole
  workspace. `#[expect]` when the suppression is situational, so a stale one
  fails the build; `#[allow]` only when it is permanent by design.
- History is public, so it moves by normal commits; pushed commits are not
  amended.
- "Framework" is Framework Computer Inc.'s trademark. The project name avoids
  using it as a product name, the README carries a non-affiliation
  disclaimer, and "Framework" appears only descriptively ("for Framework
  laptops").
