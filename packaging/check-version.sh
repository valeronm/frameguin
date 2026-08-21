#!/bin/bash
# Prints the crate version, having checked that everything else carrying a
# version agrees with it. Both builders run this first, so "releasable as X"
# has one definition rather than one per artifact.
#
# usage: check-version.sh [expected]
set -euo pipefail

expected="${1:-}"

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

# Read the manifest, not `cargo pkgid`: that resolves through Cargo.lock, so a
# bumped manifest with a stale lock would report the old version here while
# CARGO_PKG_VERSION, cargo-deb and the metainfo all followed the new one.
version="$(sed -n '/^\[workspace\.package\]/,/^\[/ s/^version = "\(.*\)"/\1/p' Cargo.toml)"
if [ -z "$version" ]; then
    echo "no version in [workspace.package] of Cargo.toml" >&2
    exit 1
fi

fail() {
    echo "$1 says $2, crate is $version" >&2
    exit 1
}

[ -z "$expected" ] || [ "$expected" = "$version" ] || fail "the caller" "$expected"

# cargo-deb reads the version from Cargo.toml but ships the changelog and the
# metainfo verbatim, so a bump that misses one announces the wrong release.
# dpkg-parsechangelog rather than a regex: it is the parser dpkg itself uses,
# and it comes with build-essential, which building this already needs.
changelog="$(dpkg-parsechangelog -l packaging/changelog -SVersion)"
[ "${changelog%-*}" = "$version" ] || fail "packaging/changelog" "$changelog"
if [ "$(dpkg-parsechangelog -l packaging/changelog -SDistribution)" = "UNRELEASED" ]; then
    echo "packaging/changelog is still UNRELEASED — finalize it first" >&2
    exit 1
fi

metainfo="$(sed -n 's/.*<release version="\([^"]*\)".*/\1/p' data/*.metainfo.xml | head -1)"
[ "$metainfo" = "$version" ] || fail "the metainfo <release>" "$metainfo"

echo "$version"
