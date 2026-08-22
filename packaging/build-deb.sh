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

# --locked: a committed lock that disagrees with the manifest is a release
# defect, and cargo would otherwise rewrite it here without saying so.
cargo build --release --workspace --locked
./packaging/render-data.sh target/release/data \
    LIBEXECDIR=/usr/libexec VERSION="$version"
# Debian requires compressed manual pages and cargo-deb ships assets as-is.
# -n so the same source produces the same bytes.
gzip -9n target/release/data/frameguin.1

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

# Guarded so a machine without the validators can still build. The package
# carries no lintian overrides: warnings are fatal and there are none.
if command -v lintian >/dev/null; then
    lintian --fail-on error,warning "$deb"
fi
# --no-net: without it the validator fetches every URL in the file, so the
# gate answers for GitHub's reachability rather than this metadata, and a
# filtered network stalls it with the whole workspace already built. It also
# ties a local check to the repo's published state, since the screenshots
# name a branch.
if command -v appstreamcli >/dev/null; then
    appstreamcli validate --no-net target/release/data/*.metainfo.xml
fi
if command -v desktop-file-validate >/dev/null; then
    desktop-file-validate target/release/data/*.desktop
fi

echo "built frameguin $version"
