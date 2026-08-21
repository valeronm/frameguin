#!/bin/bash
# Installs (default) or removes (--uninstall) the frameguin daemon and app,
# system-wide. Idempotent: re-run after editing any file to update the
# install. Run with sudo.
set -euo pipefail

src="$(cd "$(dirname "$0")" && pwd)"

# Installed as $prefix/libexec/frameguin-uninstall.sh, where removing is all
# this can do. Its own location is the only record of the prefix it was
# installed under, so an uninstall undoes that install rather than the default.
default_prefix=/usr/local
case "${0##*/}" in
    *uninstall*)
        set -- --uninstall "$@"
        default_prefix="${src%/libexec}"
        ;;
esac

if [ "$(id -u)" -ne 0 ]; then
    echo "run with sudo" >&2
    exit 1
fi

app_id="io.github.valeronm.Frameguin"
# /usr/local is the FHS slot for software installed outside the package
# manager. polkit, D-Bus and the icon theme read from fixed system
# directories, so those files stay put wherever the rest goes.
prefix="${PREFIX:-$default_prefix}"

# A checkout has the binaries in cargo's build tree; a release tarball ships
# them beside this script.
built="$src/target/release"
[ -x "$built/frameguin" ] || built="$src"

# Staged outside the build tree so a sudo run leaves no root-owned files in it.
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
data="$staging/data"

# mode, source, destination triples — the single list both modes use. Kept as
# separate elements rather than delimited strings so a path is never parsed.
files=(
    755 "$built/frameguin-daemon"              "$prefix/libexec/frameguin-daemon"
    755 "$built/frameguin"                     "$prefix/bin/frameguin"
    644 "$data/$app_id.conf"                   "/etc/dbus-1/system.d/$app_id.conf"
    644 "$data/$app_id.service"                "/usr/share/dbus-1/system-services/$app_id.service"
    # /etc, not /usr/lib: this is a local-admin install, and a packaged one
    # must not find a competing unit shadowing its own.
    644 "$data/frameguin-daemon.service"       "/etc/systemd/system/frameguin-daemon.service"
    644 "$data/$app_id.policy"                 "/usr/share/polkit-1/actions/$app_id.policy"
    644 "$data/$app_id.desktop"                "/usr/share/applications/$app_id.desktop"
    644 "$data/$app_id.metainfo.xml"           "/usr/share/metainfo/$app_id.metainfo.xml"
    644 "$data/icons/$app_id.svg"              "/usr/share/icons/hicolor/scalable/apps/$app_id.svg"
    644 "$data/icons/$app_id-symbolic.svg"     "/usr/share/icons/hicolor/symbolic/apps/$app_id-symbolic.svg"
    644 "$data/frameguin.1"                    "$prefix/share/man/man1/frameguin.1"
    # This script, so --uninstall stays available: get.sh unpacks into a
    # temporary directory it deletes, taking the only other copy with it.
    755 "$src/install.sh"                      "$prefix/libexec/frameguin-uninstall.sh"
)

if [ "${1:-}" = "--uninstall" ]; then
    # Stop running instances first: the resident tray app would otherwise
    # linger as a broken ghost after its binary and daemon are removed.
    pkill -x frameguin 2>/dev/null || true
    systemctl stop frameguin-daemon.service 2>/dev/null || true
    # Removing this script while it runs is safe: bash reads through an open
    # descriptor, which unlink does not invalidate.
    for ((i = 0; i < ${#files[@]}; i += 3)); do
        rm -f "${files[i + 2]}"
    done
    rm -rf /var/lib/frameguin
    systemctl daemon-reload
    gtk-update-icon-cache -f /usr/share/icons/hicolor 2>/dev/null || true
    echo "uninstalled. Per-user autostart entries remain; remove yours with:"
    echo "  rm -f ~/.config/autostart/$app_id.desktop"
    exit 0
fi

if [ ! -x "$built/frameguin-daemon" ] || [ ! -x "$built/frameguin" ]; then
    echo "binaries missing from $built" >&2
    echo "in a checkout, build them first (as your user, not root):" >&2
    echo "  cargo build --release" >&2
    exit 1
fi

# The tarball carries no dependency metadata the way the .deb does, so a
# missing GTK stack surfaces here rather than as a loader error at launch.
missing="$(ldd "$built/frameguin" "$built/frameguin-daemon" |
    awk '/not found/ {print "  " $1}' | sort -u)"
if [ -n "$missing" ]; then
    # ID_LIKE carries the parent distribution, so derivatives match without
    # being listed. The names below are a best effort for distributions this
    # is not built on; the sonames above are the part that is always true.
    # shellcheck source=/dev/null
    . /etc/os-release 2>/dev/null || true
    {
        echo "these shared libraries are missing:"
        echo "$missing"
        echo "install the runtime libraries first:"
        case " ${ID:-} ${ID_LIKE:-} " in
            *" debian "*|*" ubuntu "*)
                echo "  sudo apt install libgtk-4-1 libadwaita-1-0 polkitd"
                echo "or install the .deb, which pulls them in itself." ;;
            *" fedora "*) echo "  sudo dnf install gtk4 libadwaita polkit" ;;
            *" arch "*)   echo "  sudo pacman -S gtk4 libadwaita polkit" ;;
            *" suse "*)   echo "  sudo zypper install libgtk-4-1 libadwaita-1-0 polkit" ;;
            *) echo "  GTK 4, libadwaita and polkit — your package manager can"
               echo "  name the package for a soname above" ;;
        esac
    } >&2
    exit 1
fi

# Everything that can refuse has run; what follows is advisory.

# The other prefix having an install is the mixed-install case, and here is
# where it is created — the app can only report it afterwards, from a process
# with no privilege to undo it.
for other in /usr/bin/frameguin /usr/local/bin/frameguin; do
    [ "$other" = "$prefix/bin/frameguin" ] && continue
    [ -e "$other" ] || continue
    echo "warning: $other is already installed; two installs shadow each other" >&2
    if [ "$other" = /usr/bin/frameguin ]; then
        echo "         remove it with: sudo apt purge frameguin" >&2
    else
        echo "         remove it with: sudo /usr/local/libexec/frameguin-uninstall.sh" >&2
    fi
done

# Warn (but don't refuse — containers, CI, and pre-swap installs are all
# legitimate) when this doesn't look like Framework hardware.
if [ "$(cat /sys/class/dmi/id/sys_vendor 2>/dev/null)" != "Framework" ]; then
    echo "warning: this does not look like a Framework laptop; installing anyway" >&2
    echo "         (the app will show no controls on unsupported hardware)" >&2
fi

# Stop running app instances so the update takes effect immediately, and
# remember whether the invoking user had one — only that user's instance can
# be restarted (a GUI is launched inside its owner's session).
# Matched by process name: the desktop entries launch by name, so the command
# line carries whichever path the launcher resolved, if any.
app_was_running=""
if [ -n "${SUDO_USER:-}" ] && pgrep -x -u "$SUDO_USER" frameguin >/dev/null 2>&1; then
    app_was_running=1
fi
pkill -x frameguin 2>/dev/null || true

# Asked of the binary being installed rather than tracked here: it is the one
# thing that always knows, and --version needs no display or environment.
# Expects one line ending in the version; render-data.sh rejects an empty one.
version="$("$built/frameguin" --version)"
"$src/packaging/render-data.sh" "$data" \
    LIBEXECDIR="$prefix/libexec" VERSION="${version##* }"

for ((i = 0; i < ${#files[@]}; i += 3)); do
    install -Dm"${files[i]}" "${files[i + 1]}" "${files[i + 2]}"
done
gtk-update-icon-cache -f /usr/share/icons/hicolor 2>/dev/null || true

systemctl daemon-reload
# Pick up the new bus policy without restarting the bus.
systemctl reload dbus 2>/dev/null || busctl call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig || true
# Restart the daemon if a previous version is running; activation restarts it on demand.
systemctl stop frameguin-daemon.service 2>/dev/null || true

if [ -n "$app_was_running" ] && [ -n "${SUDO_USER:-}" ]; then
    if systemd-run --machine="$SUDO_USER@.host" --user --collect \
        "$prefix/bin/frameguin" --gapplication-service >/dev/null 2>&1; then
        echo "installed and restarted (tray)."
        exit 0
    fi
    echo "note: could not restart the app; launch it from the app grid" >&2
fi

echo "installed. Launch with: frameguin (or from the app grid)"
