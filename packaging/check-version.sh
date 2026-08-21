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

version="$(cargo pkgid -p frameguin)"
version="${version##*@}"

if [ -n "$expected" ] && [ "$expected" != "$version" ]; then
    echo "asked for $expected, crate is $version" >&2
    exit 1
fi

# cargo-deb reads the version from Cargo.toml but ships the changelog and the
# metainfo verbatim, so a bump that misses one announces the wrong release.
# dpkg-parsechangelog rather than a regex: it is the parser dpkg itself uses,
# and it comes with build-essential, which building this already needs.
changelog_version="$(dpkg-parsechangelog -l packaging/changelog -SVersion)"
if [ "${changelog_version%-*}" != "$version" ]; then
    echo "packaging/changelog says $changelog_version, crate is $version" >&2
    exit 1
fi
if [ "$(dpkg-parsechangelog -l packaging/changelog -SDistribution)" = "UNRELEASED" ]; then
    echo "packaging/changelog is still UNRELEASED — finalize it first" >&2
    exit 1
fi

metainfo_version="$(sed -n 's/.*<release version="\([^"]*\)".*/\1/p' \
    data/*.metainfo.xml | head -1)"
if [ "$metainfo_version" != "$version" ]; then
    echo "metainfo <release> says $metainfo_version, crate is $version" >&2
    exit 1
fi

echo "$version"
