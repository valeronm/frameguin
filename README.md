# Frameguin

An app for managing the hardware settings of a Framework laptop, from a
window or a tray icon. Written in Rust and split into an unprivileged GUI
and a root D-Bus daemon.

This is a community project, not affiliated with or endorsed by Framework
Computer Inc. "Framework" and the gear logo are trademarks of Framework
Computer Inc. Licensed under the [MIT License](LICENSE).

[**Latest release**](https://github.com/valeronm/frameguin/releases/latest)
· [Install](#install) · [Report a bug](https://github.com/valeronm/frameguin/issues)

<img src="screenshot.png" alt="The Frameguin window, one group per piece of hardware" width="420">

## What it does

- **Power** — what the charger is supplying, the current flowing in or out,
  a ceiling on how full the battery charges, and a cap on the charging rate.
- **Input** — the touchpad's haptic click intensity and click force, and
  switching the touchscreen off.
- **Lights** — the power button LED's brightness as a level or a percentage,
  switching the charging LED off, and which side's charging LED is lit.
- **Readings** — what is attached to each USB-C port, what it negotiated,
  and which one is powering the machine.

A charge limit set here lasts until reboot: UEFI setup re-sends its own
stored value at every POST, so the standing limit lives in BIOS setup. The
same goes for the power button LED's level, and a touchscreen switched off
comes back when the lid is opened, after a suspend, and at the next restart —
the panel's enable is a line the firmware re-asserts rather than a setting
anything stores. **Restore settings**, in Preferences, has the daemon put each of them back
after a restart or a resume, to whatever they were when it was switched on
or last set here since; switching it off forgets them, and the next restart
is the firmware's again. An LED switched off is the exception: either one
lights again at the next restart whatever the switch says.

The power button LED and the fingerprint reader share one button. What the
app reaches is the LED's brightness; nothing here touches the reader.

Closing the window hides it to the tray; **Quit Frameguin** in the tray menu
is the real exit. **Start at login**, in Preferences, brings up the tray
icon only.

<img src="screenshot-tray.png" alt="The Frameguin tray menu, with one control expanded to its presets" width="280">

## Alternatives

Other community projects reach the same hardware, and one of them may suit
you better. Frameguin exists for a particular shape: the GUI holds no
privileged access of its own, and the controls live in the tray, a menu away
rather than an app to launch.

The others are shaped differently:

- **[framework_tool](https://github.com/FrameworkComputer/framework-system)** —
  Framework's own CLI, on the `framework_lib` this app also links, run under
  `sudo` per invocation.
- **[YAFI](https://github.com/Steve-Tech/YAFI)** — also a GTK4/libadwaita app,
  with the GUI reaching the EC itself rather than through a privileged
  service.
- **[framework-tool-tui](https://github.com/grouzen/framework-tool-tui)** — a
  terminal dashboard over the same `framework_lib`, run entirely under `sudo`.
- **[framework-control](https://github.com/ozturkkl/framework-control)** — a
  background service with a browser UI.

## Requirements

- **A Framework laptop with a Framework mainboard.** `framework_lib`, which
  every control goes through, does not speak to third-party boards built for
  the same chassis. Developed and tested on the Laptop 13 Pro (Intel Core
  Ultra Series 3), BIOS 03.02. Other Framework boards should work, and
  reports from them are welcome.
- **GTK 4 with libadwaita 1.5 or newer** — Ubuntu 24.04, Debian 13, Fedora 40
  or their equivalents.
- **A tray implementation, for the tray icon only.** The window works
  anywhere. KDE and Xfce have one natively; stock GNOME needs the
  [AppIndicator extension](https://extensions.gnome.org/extension/615/appindicator-support/)
  (preinstalled on Ubuntu). Without it there is no tray menu, and **Start at
  login** — which starts the tray alone — leaves nothing visible.

## Install

Neither way needs a Rust toolchain or the `-dev` packages.

A tarball carrying the same installer the source build uses:

```sh
curl -fsSL https://raw.githubusercontent.com/valeronm/frameguin/main/packaging/get.sh | sh
```

That downloads and unpacks as your user and runs only the installer under
`sudo`. Read it first if you'd rather — it is
[packaging/get.sh](packaging/get.sh).

By hand, from the
[latest release](https://github.com/valeronm/frameguin/releases/latest): download
`frameguin-<version>-x86_64-linux.tar.xz` and the `.sha256` beside it, then

```sh
sha256sum -c frameguin-*-x86_64-linux.tar.xz.sha256
tar -xJf frameguin-*-x86_64-linux.tar.xz
sudo ./frameguin-*-x86_64-linux/install.sh
```

A tarball declares no dependencies, so GTK 4 and libadwaita must already be
present; the installer checks before writing anything and names the package
to install if they are not.

Either way, launch with `frameguin` or from the app grid.

## Updating

- **Tarball** — re-run the same `curl … | sh`.
- **Source** — `git pull && cargo build --release && sudo ./install.sh`.

Both are idempotent: they stop the daemon, replace the files, and
restart the tray app if it was running.

## Uninstall

```sh
sudo /usr/local/libexec/frameguin-uninstall.sh
```

That path follows the install prefix: `install.sh` puts a copy of itself
there, so a `curl … | sh` install can be removed without re-downloading
anything and a `PREFIX` install undoes itself. It also removes
`/var/lib/frameguin`, the daemon's record of the settings the hardware cannot
report back and of the ones it restores.

**Start at login** writes a desktop entry per user, and no uninstaller can
reach another user's home directory, so each user who turned it on removes
their own:

```sh
rm -f ~/.config/autostart/io.github.valeronm.Frameguin.desktop
```

A leftover entry is harmless: it carries `TryExec`, so a session skips it once
the binary is gone.

## Troubleshooting

**No controls, "No Framework hardware detected"** — the DMI vendor is not
`Framework`. Expected on other machines.

**Some controls missing** — the daemon probes each operation and shows only
what the board answers to. `frameguin --debug-info` lists what it found.

**No tray icon on GNOME** — install the AppIndicator extension (see
Requirements).

**A password prompt for every change** — polkit allows an active local
session without one. Over SSH, or from an inactive session, admin
authentication is required by design.

```sh
# both versions with the paths they ran from, every part with its firmware
frameguin --debug-info
# daemon logs
sudo journalctl -u frameguin-daemon.service
```

## How it works

Controls are detected, not assumed: each device probes itself once when the
daemon starts and is served on the bus only where it was found, and the app
shows only the groups whose device answered — so new boards work without
code changes. Frameguin currently covers the everyday controls, with more
of `framework_tool`'s surface planned.

What the hardware itself does — how each subsystem is reached, what it will
and will not report, and the quirks that shape any code talking to it — is
written up under [`docs/hardware/`](docs/hardware/), a file per transport
or control, indexed by part in `parts.md` and by control in `controls.md`.
Much of it is not documented elsewhere, so it may be useful whatever you are
building against these machines.

Cargo workspace:

- `daemon/` (`frameguin-daemon`) — owns
  `io.github.valeronm.Frameguin` on the **system bus**, runs as root, and
  links `hardware/`, the one crate holding `framework_lib` (default features
  off plus `hidapi`) and the devices over it.
  D-Bus-activated via systemd (`Type=dbus`), exits after 5 minutes idle.
  Setters are polkit-gated.
- `app/` (`frameguin`) — gtk4-rs + libadwaita GUI talking to the
  daemon over zbus.
- `data/` — D-Bus system bus policy + activation file, systemd unit, polkit
  policy, desktop entry, icons. `*.in` files carry the daemon's absolute path
  and are rendered per prefix by `install.sh`.
- `packaging/` — the tarball build (`build-tarball.sh`) and its downloader
  (`get.sh`).

### Security model

The daemon runs as root (required for `/dev/cros_ec` and hidraw access),
started only by systemd via D-Bus activation, with `ProtectHome`,
`PrivateTmp`, and a systemd-owned `StateDirectory`. Authorization is
layered:

- **Name ownership** — the bus policy allows only root to own
  `io.github.valeronm.Frameguin`, so the daemon can't be impersonated.
- **Reach** — any local user or process may call the daemon; filtering
  happens per method, not per connection.
- **Setters** — every state-changing method asks polkit to authorize the
  message sender (kernel-verified identity) against
  `io.github.valeronm.frameguin.manage`: an **active local session**
  is allowed without a password (the same trust GNOME grants screen
  brightness — someone at the console has the hardware keys anyway);
  inactive sessions are denied; everything else (SSH, daemons) needs admin
  authentication via a polkit agent.
- **Getters** are unauthenticated: they expose only hardware metadata
  (current levels, features, the parts and their firmware).
- **Surface** — a fixed set of operations with all inputs validated against
  hardware-accepted values; no raw command passthrough.

## Build from source

`mise install` provides the pinned Rust toolchain; without
[mise](https://mise.jdx.dev), install Rust 1.97+ yourself. Then the system
libraries:

```sh
# Debian and Ubuntu
sudo apt install libgtk-4-dev libadwaita-1-dev libudev-dev pkg-config build-essential
# Fedora
sudo dnf install gtk4-devel libadwaita-devel systemd-devel pkgconf-pkg-config gcc
```

```sh
cargo build --release
sudo ./install.sh
```

This installs under `/usr/local`, the FHS slot for software outside the
package manager. `PREFIX` moves the two binaries; polkit, D-Bus and the icon
theme only read from fixed system directories, so those files stay put.

### Building the tarball

```sh
./packaging/build-tarball.sh    # target/dist/
```

Every push to `main` and every pull request runs the same workflow a release
does, without the release step. Cutting a release is
[`docs/release.md`](docs/release.md).

## Contributing

Issues and pull requests welcome:
<https://github.com/valeronm/frameguin/issues>

Reports from boards other than the Laptop 13 Pro are especially useful.
Include the output of `frameguin --debug-info` — the same report the main
menu → **About Frameguin** → **Troubleshooting** page offers behind a copy
button.
