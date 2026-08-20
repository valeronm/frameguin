#!/bin/bash
# Builds the .deb into target/debian/. cargo-deb is pinned in mise.toml, so
# `mise install` provides it; without mise, `cargo install cargo-deb`.
#
# Both binaries ship in one package, so the whole workspace is built here and
# cargo-deb runs with --no-build over the result; letting it build would give
# it only the app crate.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

cargo build --release --workspace
./packaging/render-data.sh /usr/libexec target/release/data

version="$(cargo pkgid -p frameguin)"
version="${version##*@}"

# cargo-deb reads the version from Cargo.toml but ships these two verbatim, so
# a bump that misses them produces a package announcing the wrong release.
changelog_version="$(sed -n '1s/.*(\(.*\)-[0-9]*).*/\1/p' packaging/changelog)"
if [ "$changelog_version" != "$version" ]; then
    echo "packaging/changelog says $changelog_version, crate is $version" >&2
    exit 1
fi
metainfo_version="$(sed -n 's/.*<release version="\([^"]*\)".*/\1/p' \
    target/release/data/*.metainfo.xml | head -1)"
if [ "$metainfo_version" != "$version" ]; then
    echo "metainfo <release> says $metainfo_version, crate is $version" >&2
    exit 1
fi

deb="$(cargo deb -p frameguin --no-build)"

# Two mechanisms put the daemon in the package — the assets list for the
# binary, a name-keyed directory scan for the unit — and each fails quietly:
# a bad substitution leaves ExecStart pointing nowhere, and a unit the scan
# misses is only a warning. Either way the package installs but activates
# nothing, so assert both against what actually shipped.
unit_exec="$(sed -n 's/^ExecStart=//p' target/release/data/frameguin-daemon.service)"
contents="$(dpkg-deb -c "$deb")"
if ! grep -q " \.$unit_exec\$" <<<"$contents"; then
    echo "ExecStart=$unit_exec is not in the package" >&2
    exit 1
fi
if ! grep -q " \./usr/lib/systemd/system/frameguin-daemon\.service\$" <<<"$contents"; then
    echo "the systemd unit is not in the package — unit-scripts found nothing" >&2
    exit 1
fi

# Guarded so a machine without the validators can still build. --fail-on error
# only: the package carries warnings that are deliberate (see README).
if command -v lintian >/dev/null; then
    lintian --fail-on error "$deb"
fi
if command -v appstreamcli >/dev/null; then
    appstreamcli validate target/release/data/*.metainfo.xml
fi
if command -v desktop-file-validate >/dev/null; then
    desktop-file-validate target/release/data/*.desktop
fi
