# Frameguin — working notes for Claude Code

Read README.md first for what the project is. This file covers how to work on
it and the non-obvious constraints.

## Layout and contracts

- `hardware/`, `daemon/`, `app/`, `contract/`, `wire/`, `model/` and
  `modelview/` are the crates behind the two binaries: `hardware/` is direct
  access to the machine —
  the transports, the roles, and the devices implementing the control
  traits `contract` declares — and the only crate linking `framework_lib` and
  `hidapi`, whose one `HidApi` `detect()` builds for every HID probe, since
  building one walks the bus;
  `daemon/` runs as root, links it, and serves it over the bus;
  `app/` is the GTK4/libadwaita UI and links no hardware code, nor reads
  the machine any other way — the board's own name included, which the
  daemon reports; `contract/` is
  the control traits, the values they carry with the encoding those cross
  the bus in, the error kind every implementation of them shares, and the
  strings both binaries must spell alike; `wire/` is the transport the two
  binaries talk over — the bus name and path, a proxy per interface, and
  `Bus`, every control trait answered by a call on the daemon;
  `model/` is the controls as the app holds
  them, over those traits, and the words for a part. `modelview/` is how the
  app shows them — every word for a value, the presets a control offers with
  the row each one sits on, and the names for the curated facts `model`
  holds. `docs/architecture.md` opens with the vocabulary, and each word
  means one thing; "device" is the real thing on the machine and nothing on
  the app side. The
  split is the security model — the root process carries no GUI, the GUI
  process has no hardware access — and the D-Bus interface
  `io.github.valeronm.Frameguin1` is their only bridge. Nothing that touches
  hardware may enter `contract/` or `wire/`: the app links both, so a
  dependency added there lands in the unprivileged process too. Nothing GUI
  may enter `model/`, for the reasons its manifest gives. Nothing of the bus may enter `contract/`: `hardware` and
  `model` link it, and neither reaches D-Bus by any path. Nothing GUI and
  nothing of the bus may enter `modelview/`, for the reasons its manifest
  gives.
- A string is admitted to `contract/` because a second spelling of it could
  disagree — the vendor, matched by both binaries against the string the
  same firmware gives. A value only one binary reads, or one that crosses
  the bus uncompared, stays where it is read: the board names are
  `hardware`'s, matched there into the `Platform` both binaries carry
  instead.
- A `contract/` type is shaped for the code that builds and reads it, and the
  encoding adapts to it rather than the reverse: the control traits hand
  these types to `hardware` and `model` too, so a limit of D-Bus written
  into one reaches every layer. A value that can be absent is an `Option`,
  which zvariant's `option-as-array` carries as an array of zero or one
  element, D-Bus having no maybe type; a default standing in for "not read"
  is indistinguishable from a reading. Where the codec cannot carry a
  shape, a serde adapter on that field translates it, and a separate
  transport type is only for a type no adapter can carry.
- `io.github.valeronm.Frameguin1` is private to those two binaries rather than
  published API. They are built, installed and upgraded as one — `install.sh`
  stops the app and the daemon and brings both back on the new build — so an
  app talking to a daemon of another version is not a state this project has
  to work in, and nothing outside the pair is a caller it answers for.
  Renaming a method, dropping one or respelling a feature is a free change
  needing no deprecation window. (`busctl` against
  it stays a fine way to inspect a running daemon; it is a debugging tool, not
  a client the interface holds still for.) None of this loosens what
  `contract/` and `wire/` are for: the two ends still restate the interface
  separately, so within one version the vocabularies are what keep them
  from drifting apart.
- `contract/` holds the vocabularies as enums serializing as `s`.
  `DeviceError` crosses through two functions in `wire`, one for each
  direction and called at each end, since `contract/` names nothing of
  zbus to hold a conversion in. The daemon's
  `#[interface]` impls cannot move into `wire/` beside the proxies — each
  is an impl on a daemon-side type — so the method set is still two declarations, and they meet in
  `daemon/src/interface/tests.rs`, which serves every device on stub roles
  to the `wire` proxies over a socket pair: a method one end spells and the
  other does not fails there rather than in an installed pair. The devices
  it serves are `hardware::device::Devices`, the one struct `detect()`
  fills, and the proxies it dials are `wire::Proxies`, the one struct
  the app dials too, so a device served or dialled by one end and not the
  other is a missing field. Nothing in the harness runs under async-io's
  `block_on`: two of them in one process contend for the reactor, and the
  one parked behind the other misses the wakeup for a message just
  written — rarely on an idle machine, on most runs under load — which is
  what two zbus connections with their own executor threads are. So both
  executors are ticked from one thread under `futures_lite`'s `block_on`,
  which parks on nothing but its waker, and each call is awaited on the
  client's executor (`Peer::run`) with a deadline — polled on the test
  thread it goes unanswered. What the enums buy is the other half: a
  feature,
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
  the chrome around them, the titles a widget invents and the sentences a
  toast makes, stays with the widget that is its only site. A title naming
  something else belongs to whoever knows the word: `model`'s where `model`
  curated it, group heading and list row alike, and `contract`'s where the
  hardware announced it. `model` answers for a part whether or not the
  catalogue names it, so a widget never picks a part's words by whether a
  lookup hit. Which row a reading shows is the widget's too: a value dialled
  in that happens to equal a preset is
  indistinguishable from it — the app derives one over the value, the EC's
  firmware hands back a name it deduced the same way — so the answer weighs
  where the combo sits and what moved it, and only the tray, whose menu has
  no Custom row, takes the bare preset lookup. `model` links neither
  GTK nor the bus, so the window and the tray cannot disagree about what a
  preset sends or what it is called, and a control exists in the app only
  where its device answered — the probe rule, held up by the compiler at
  this end. `tray.rs` links no GTK
  either, which matters because its menu runs on ksni's own thread; its
  fields are private, so `tray_push` is the only way that state moves.
  `window/` holds `Ui`, whose fields are private to that module tree — the
  groups under it read them, nothing outside it can — which is what `Sink`
  and the `apply_*` writes living there buys. `window/fill.rs` drives `Ui`
  from outside it — the ask, the empty page, the retry, the reload a tab
  asks for when shown — so the fan-outs
  over the groups (`gate`, `watch`, `load_values`, `connect_handlers`) stay
  on `Ui` where its fields are, and a fill asks for one rather than walking
  the groups itself — convention rather than a compiler check, `fill` being a
  child of `window` and so inside `Ui`'s privacy. Their order matters:
  `gate` first so a row this board lacks is not on screen to subscribe,
  `watch` before `load_values` so a fill reads what the fed rows want, and
  `connect_handlers` last so a loaded value cannot echo back as a write. The
  middle one is necessary and not sufficient — a subscription is taken when
  its widget is mapped, so what completes it is that a tab reloads from its
  own map — a row on a hidden tab being unmapped until then — and a row that
  is somehow unmapped when its tab shows waits a tick rather than failing;
  `window/widgets.rs` is the chrome no one group owns. `report/` holds the windows
  that only read, and the shell they share in its `mod.rs` — found by name or
  built, destroyed on close — so the single-instance rule is one private
  function rather than one copy per window, and nothing outside the tree can
  borrow it. `report/parts.rs` is the inventory `GetDevices` answers, drawn
  once per open since the list is fixed for the daemon's run.
  `report/status/` is the Readings window: no sync guard, no debounce, no tray
  push, a sidebar of sections beside the selected row's page. Its `mod.rs`
  holds the window, the one selection across every section's list, and the
  targets it can be opened on. A section is a heading over one list, and
  each page is a module of its own, adding its rows to the section `mod.rs`
  hands it where the detected controls say the board has what it shows.
  Whether a row carries a summary is its module's call, made by
  what its read costs: a summary is fed for as long as the window is on
  screen whatever page is selected, where a page is fed only while the stack
  shows it. Everything a subscription's closure holds is a descendant of the
  widget it hangs on, so nothing in it can reach back up the widget tree and
  outlive the window — which is why the sidebar holds the split view weakly.
  A port's page is fed rather than polled like everything else that
  repeats: the main window's charger row shows the same read, which is what
  makes the ports an extra on the feed below rather than this window's own
  timer. That page says everything of the thing attached, so a reading the
  EC answers for the machine — its power role, its data role — is inverted
  before it is worded: the page carries the attached device's
  own readings beside them, and one screen with two subjects reads as a
  contradiction rather than as two facts. Its groups are the connection and
  what the contract adds, headed by the contract type, because the volts and
  amps read alike whether power delivery settled them or Type-C's own
  resistors advertised them. Where a socket is on the machine is
  `model::port`'s, curated per board and answering nothing for a port nobody
  placed: the EC's port number says which controller drives a port and not
  where it is, and a wrong position reads exactly like a right one. A port
  is placed by measuring it, or where the controller its number names can
  have only one socket behind it, as the Laptop 16's bay controller does;
  its side alone is taken from Framework's own controller table in
  `framework_lib`, which names a side per controller and nothing finer.
  The layout is picked from the board the daemon reports. What a position
  is called is `modelview::port`'s, beside the other words.
  `reading.rs` is the
  machine's reading, taken once for however many views show it: the battery
  row and the Readings window render the same walk of the same block, and each
  polling for
  itself made the EC answer twice and let the two windows sit a tick apart, so
  a view subscribes and the feed does the reading. The read itself is
  `model::reading`'s, one pass over the controls with no GTK in it, so a
  second front end reads the machine the same way; what stays here is who
  is subscribed and when to read. What a view wants — the
  pack's block, its condition, the USB-C ports — is a field in its `Request` rather
  than a parameter, so a view showing none of them costs none of them, and
  each arrives as None where nothing asked, where the ask failed, and where
  the board has no such device alike. A window subscribes before it fills and
  fills through the feed's own read rather than beside it — the feed serves
  what its subscribers want, so a row that has not subscribed is one the fill
  reads nothing for, and the fill is broadcast like a tick, so opening one
  window cannot leave another showing what it saw before.
  `daemon.rs` is the app's
  end of the daemon — the bus connection, the detected controls and the
  board, the facts fixed for its run that every window wants — dialled and
  asked once,
  so the windows and the tray share one of each, and two asking
  at once wait on one answer. The board's platform is also asked for apart
  from detection, since a view needing nothing else should not wait on
  every device, so a run that asks for it first asks for the board twice.
  It is named for the real thing the way
  `device` is, and holds no state of its own: what it caches is the daemon's
  answer. (`about.rs` dials for itself, deliberately: its report also runs from
  `--debug-info` where no app state exists, and a bug report wants a fresh
  answer rather than one the app is already holding.) `mapped.rs` is
  the rule both timers and subscriptions obey, that nothing repeats while its
  widget is off screen: `while_mapped` takes what `acquire` returns on map and
  drops it on unmap, so stopping is a `Drop` rather than something each caller
  remembers. `about.rs` is the report and the dialog that renders it,
  `autostart.rs` the desktop entry whose path and content cannot be written
  apart, `failure.rs` how a failed daemon call is told — the sentence both
  tellings open with, the toast a window or dialog shows, and the notification a
  session with no window sends instead, along with the withdrawal pending
  against it. A failure with no device behind it words itself where it
  happens, the `DeviceError` those take being what drops the D-Bus error
  name and there being none to drop. `main.rs` keeps what has to know both
  front-ends — the app id, the
  bus attachment, the lazily built window, the shared reading, and the tray
  event loop, exhaustive over `TrayEvent` so a new variant fails to build
  until it is handled there — along with the application-wide odds and ends
  that belong nowhere else: the command-line options and the actions no
  module of its own owns. The no-GTK rules are the ones nothing
  checks: an import is all it takes to lose one.
- Which window a thing goes in is decided by what kind of fact it is: the main
  window holds what can be set, Hardware what the hardware is — fixed for the
  daemon's run and read once — and Readings what the hardware is doing now,
  read only while it is on screen. A new reading is a page in Readings
  rather than a window of its own or a row in the main window.
- The main window and the reports are reached differently, and which way is
  decided by whether the window survives being closed. The main window hides rather than closing
  wherever there is a tray to hide to, so its slot in `AppState` outlives it —
  and the slot is also where the tray finds the `Rc<Ui>` it reports presets
  into, so `window_for` builds it once and both front-ends take it from there.
  (Where the tray failed to spawn there is no second front-end to reach it,
  which is what keeps that slot honest in the session where the window really
  is destroyed on close.) A report is destroyed on close, so a
  slot would hold a dead window: it goes through a `gio` action on the
  application instead, and finds an already-open copy in the application's own
  window list, which GTK keeps accurate for free. Where a window is opened
  from more than one place, the module owning it owns the `ActionEntry` too
  and keeps its builder private, so the action is not merely the agreed way in
  but the only one that compiles — `report/status/` does this because both
  front-ends reach it. Its action carries the page to open on, forwarded to
  an action on the window it found or built, since only that window holds
  the rows a target is settled against. `about.rs` does not, and needs not:
  only the window's menu opens it, so `main.rs` holding that entry leaves
  nothing able to drift.
- Inside `hardware/`, the transport modules are drawn by how a control
  reaches the machine, so the filename answers which way; which module
  reaches the machine which way is the crate's own module doc, so a module
  added there cannot leave a list here stale. Two pairs of them need an
  arbitration, and the two arbitrations are not alike: the power
  button LED has two possible drivers and one at a time, so what is settled
  is a handover and the order to make it in — and that lives in
  the device, `device/power_led.rs`, over the `PowerLedEc` and
  `LedClass` roles, because the order is what a stub has to
  be able to check; `touchscreen.rs` because the panel has two possible
  routes and a machine has only one, so what it settles is a precedence — and
  then the thing that precedence decides, which is whether the control can be
  read at all, stated once in the `TouchSwitch` role it declares over both.
  The rest divide by job: `dmi.rs`
  the SMBIOS reads — the board, read once and handed to every device that
  asks what machine it is on, the raw entries a
  part's identity comes from — `sbs.rs` the pack's own registers and what
  their words mean, apart from `ec.rs` so the decoding is testable without
  an EC and `ec.rs` stays every EC call and nothing else —
  `pd_controller.rs` what a PD controller reads of a port's cable, contract
  and bus and where each board's controllers sit, apart from `ec.rs` for the
  same reason — `edid.rs` what a
  panel's own block says it is, apart from `drm.rs` for the same reason —
  `udev.rs` the words udev's database holds for an id sysfs gives bare — the
  record it cached for a device it processed, and the vendor list read
  directly for a part with no record of its own, a panel over DRM having the
  GPU's — `ccg3.rs` what an HDMI or DisplayPort card's firmware report
  means, apart from `usb.rs` so the decoding is testable without a card —
  `lifetime.rs` what
  holds a mirrored value and whether it still holds it, where `dmi.rs`
  answers for the machine, which is the difference between a fact a reboot or
  a sleep changes and one that outlives both, `state.rs`
  the keyed store for what cannot be read back and what was asked for, and
  the spelling a value takes in it, `mirror.rs` the mirror a
  device reads and writes such a value through, declared under its own key
  and the `Lifetime` of whatever holds it, `restore.rs` the `Wanted` beside
  a mirror for what firmware moves back and the switch that has it written
  again, `testing.rs` the stub per role
  and the store in memory, which the daemon's tests build the same devices
  from under the `testing` feature. A module's own doc says what it is
  for; the reasoning is here. `device::detect()` is the whole way in — it
  opens every transport — so a module is public only where a path outside
  the crate names it, the daemon's harness counting as one; an item inside
  a closed module keeps `pub` only where a public signature elsewhere
  carries it, which is the role traits and what `testing` spells in its
  own. `ec.rs` is every EC call, its lock discipline stated in its own
  module doc. `daemon/src/main.rs` keeps the
  `Daemon` object with the root interface, polkit, the idle exit and the
  serving of every device `detect()` found; `daemon/src/interface/`
  holds those interfaces, and every setter in them authorizes before it
  does anything else — before its argument is checked or the value in
  place is read — because authorization is for the attempt, not for the
  write it turns out to be. A setter takes its device from
  `Served::authorized`, but `device()` hands the same device to any body,
  so what holds the order is `interface/tests.rs`, where every setter, a
  bad argument and a write already in place included, is refused when
  polkit refuses and the device left untouched.
- A control the tray can also set gets an `apply_*` function owning the whole
  write: the daemon call, the toast, the tray's copy, and moving the widget to
  match. Both the window's handler and the tray item call it; neither writes
  around it. The `Sink` it reports into decides the report's form: a toast
  where a window is built, a desktop notification for a refusal where none
  is. Controls only the window sets (the touchpad) write inline
  in their handler — one caller needs no shared function, and giving it one
  would be ceremony. Either way a handler answers `window/widgets.rs` with
  its write as a future: the helper owns the sync guard and the spawn, and a
  handler spawning for itself is caught by nothing but the `glib` import it
  would need. Writing *by* moving the widget, which the tray used to
  do, makes state the command channel: a widget already showing the requested
  value emits no change, so the write is silently dropped — which is what a
  tray click on a stale window hits. Debouncing stays with the widget that
  needs it; a tray click is discrete.
- Every window widget is already carrying the user's choice when its handler
  runs — `notify` fires after the move, for a combo as much as for a switch —
  so a refused write leaves the window asserting a state the hardware never
  took. Correct it where both hold: the prior value is recoverable without a
  read, and the wrong assertion is one a reader acts on rather than merely
  looks at. The touchscreen and the restore switch are the cases that meet
  them, a switch's prior value being its negation — the touchscreen's wrong
  claim is "touch is off", the restore switch's that a setting will come
  back after a restart. The two EC LEDs
  meet the second and not the first — a level the kernel would not hand
  the power LED back for reads as Off, a charging LED the kernel took half
  a handover for is lit on neither account, and "lit" is a claim of the
  same kind — so each re-reads after every write rather than capturing
  anything. Everything
  else here would have to capture a value before the write or re-read after
  it, for a stale row that outlives nothing worse than the next reload.
- The tray holds what gets changed on the move or several times a day — the
  charging, the touchscreen — and leaves a setting picked once and kept, such
  as an LED's level, to the window. It offers one item per control, not
  per value, and every item takes the same shape: a submenu over the states
  it offers, named after the one in force. A value dialled in from the window
  is not among them, so no row is marked, and the title spells the raw value
  instead — a menu that said nothing about a limit set from the window would
  be worse than one naming a row it cannot mark. Two-state controls go through
  the same shape rather than drawing a checkmark; `touchscreen_item` carries
  why. What a row sends is a *state*, never a gesture: a click saying only
  "toggle" would invert whatever
  the app believed by the time it landed, which is the command-channel mistake
  above in its other form — there a widget's position stood in for the
  command, here the gesture would.
- A setter skips a write already in place where a client's idea of the value
  can be stale. The skip belongs in the device's setter, after its argument
  check, rather than in a caller, asked of the closest thing to the truth
  each one has: the battery's `set_charge_limit` asks the EC, its
  `set_charge_current_limit` its mirror, and only for a cap, since the
  mirror holds nothing for a cap written without evidence as much as for
  no cap at all — a lift is always written; the charging LED's `set_enabled`
  the kernel's hold, the touchscreen's `set_enabled` the pad — and on the
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
  bus's sake; there a setter is told by taking its device from
  `Served::authorized`.
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
sends (`Ec::offers`) is a probe about that command and nothing
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
whole run. So setters validate against the thing itself: `PowerLed::set_level`
looks up the LED node rather than consulting the levels it offered.

## What the hardware forces on the code

`docs/hardware/` is what the machine does, a file per transport or control,
with `parts.md`, `controls.md` and `boards.md` as its indexes — naming the
files here as well would be another index to keep true. Findings belong there
rather than here, since they stay true whoever is talking to the hardware;
what belongs here is what they force on *this* code. The exception is a
finding that is the evidence for a rule stated here, like the touchscreen's
version read under the probe rule — separating those would leave the rule
asserted and its reason a file away. A finding about a peripheral plugged
into the machine is stated for the kind of device and what it does, not for
the product it was read from.

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
  it expires with the EC that took it and not with the host, on the boards
  whose firmware keeps it that long — the holder is the firmware's, so it
  can differ by board; `HostAwake` for
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
  request it cannot wait for, so the menu opens on the old value and the
  host redraws it, still open, when the fresh one is pushed.
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
- A wanted value is the other record a setter keeps, and it is not a mirror:
  a mirror claims what the hardware holds and is withdrawn with its
  holder's life, a `Wanted` claims what was asked for and is withdrawn only
  by the next ask, a write skipped as already in place included, or by the
  restore switch going off. The writes it is
  written back through are the ordinary setters, so the mirror moves with
  it. The one trigger is `frameguin-restore.service`, wanted by
  `multi-user.target` and the sleep targets and calling `Restore` on the
  root interface — a call rather than a restart, so a daemon still up from
  before the sleep is the one told, and one activated by the call restores
  in the same method. The daemon never restores on its own start: it is
  activated by any client and exits idle, so a restore on every start would
  switch the touchscreen off hours into a run because a lid opening had
  switched it on, and the unit already covers the boot — a session starts
  after `multi-user.target`, which waits on the oneshot. Re-asserting a
  value in force costs nothing, so the call needs no record of having run.
  Recording happens only while the switch is on and switching it off drops
  every wanted value, so the store holds a wanted value exactly while the
  switch is on. Switching it on records what each device finds in force,
  a value set before the switch being what a user turning it on means to
  keep; where that is what firmware would re-send anyway, pinning it costs
  nothing. The same call writes the touchpad's mirrored settings back
  whether or not the switch is on: Windows sends the pad its own at boot,
  and keeping a mirror true is not the switch's to decide, a mirror being
  a claim about the hardware rather than a wanted value. A pad nothing set
  here is left as the other system left it.
- What the pack is asked directly falls into two groups, and the split is why
  one is a feature the battery offers and the other is not. The temperature,
  cell voltages and alarms have no fallback, so they are one operation behind
  `BatteryFeature::Condition`, probed by the getter's own read and only on a
  device whose pack answered — a mainboard running standalone must not spend
  transfers asking a battery that is not there what it thinks. The cycle
  count and the manufacturing date each fall back to the EC's answer or to
  nothing, so they need no feature: the count is read inside the `Pack`
  role's `info`, once per run and then remembered, absence included, and
  the date inside its `identity`, which detection asks once.
- A pack is catalogued by the seven characters the EC's memory map holds and
  never by the fuller name `EC_CMD_BATTERY_GET_STATIC` would answer with,
  which firmware on battery API v1 does not implement at all — so a pack
  carried between two machines would key two ways.
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
- Release notes are written on the draft release, not accumulated per
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
  is a "No Framework hardware detected" page naming the vendor where the
  controls would be, fast, no error toast. Both real
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
  every module that obeys it. A mechanism confined to one module keeps its
  reasoning in that module: there is no second obeyer for a statement here
  to cover, and the rule binds whoever is editing that file. `ec.rs`'s lock
  discipline is the case. The shape of the whole — the layers, what each
  links, what a device is at each — goes to `docs/architecture.md`, and a
  fact about the machine to its file under `docs/hardware/`.
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
