#!/bin/bash
# Builds the .deb into target/debian/. cargo-deb is pinned in mise.toml, so
# `mise install` provides it; without mise, `cargo install cargo-deb`.
#
# usage: build-deb.sh [--expect <version>]
#
# --expect names the version this build is supposed to produce, so the whole
# release gate can be run before a tag exists rather than only by CI after one.
#
# Both binaries ship in one package, so the whole workspace is built here and
# cargo-deb runs with --no-build over the result; letting it build would give
# it only the app crate.
set -euo pipefail

expected=""
case "${1:-}" in
    --expect) expected="${2:?--expect needs a version}" ;;
    "") ;;
    *) echo "usage: $(basename "$0") [--expect <version>]" >&2; exit 2 ;;
esac

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

version="$(./packaging/check-version.sh "$expected")"

cargo build --release --workspace
./packaging/render-data.sh /usr/libexec target/release/data

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

# Guarded so a machine without the validators can still build.
# initial-upload-closes-no-bugs is suppressed rather than overridden: it stops
# firing once a second changelog entry exists, so an override for it would
# ship to every user and quietly go dead.
if command -v lintian >/dev/null; then
    lintian --fail-on error,warning \
        --suppress-tags initial-upload-closes-no-bugs "$deb"
fi
if command -v appstreamcli >/dev/null; then
    appstreamcli validate target/release/data/*.metainfo.xml
fi
if command -v desktop-file-validate >/dev/null; then
    desktop-file-validate target/release/data/*.desktop
fi

echo "built frameguin $version"
