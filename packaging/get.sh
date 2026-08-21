#!/bin/sh
# Downloads the latest Frameguin release tarball and installs it.
#
#   curl -fsSL https://raw.githubusercontent.com/valeronm/frameguin/main/packaging/get.sh | sh
#
# On Debian and Ubuntu prefer the .deb from the same release: it declares its
# dependencies, so apt installs the GTK and polkit runtime for you and can
# remove the package again as a unit.
#
# The download runs as you; only install.sh is run with sudo. Piping this into
# `sudo sh` instead would fetch and unpack as root for no benefit.
set -eu

repo=valeronm/frameguin
latest="https://github.com/$repo/releases/latest"

for tool in curl tar sha256sum; do
    command -v "$tool" >/dev/null || { echo "$tool is required" >&2; exit 1; }
done

# The redirect target names the tag, so no API call and no rate limit — an
# unauthenticated api.github.com allows 60 requests an hour per address, which
# a shared or corporate egress can exhaust without the user doing anything.
tag="$(curl -fsSL -o /dev/null -w '%{url_effective}' "$latest")"
tag="${tag##*/}"
case "$tag" in
    v*) ;;
    *) echo "could not determine the latest release" >&2; exit 1 ;;
esac

name="frameguin-${tag#v}-$(uname -m)-linux"
base="https://github.com/$repo/releases/download/$tag"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

# Which architectures exist is the release's business, not this script's, so
# a missing asset is what reports an unsupported one.
echo "downloading $name"
if ! curl -fsSL -o "$tmp/$name.tar.gz" "$base/$name.tar.gz"; then
    echo "$tag has no build for $(uname -m) — build from source instead:" >&2
    echo "  https://github.com/$repo#build-from-source" >&2
    exit 1
fi
curl -fsSL -o "$tmp/$name.tar.gz.sha256" "$base/$name.tar.gz.sha256"

# The checksum ships beside the tarball, so this catches a truncated download
# or a half-replaced release asset rather than a hostile one — that is what
# HTTPS is for.
( cd "$tmp" && sha256sum -c "$name.tar.gz.sha256" >/dev/null ) ||
    { echo "checksum mismatch — download corrupted" >&2; exit 1; }

tar -xzf "$tmp/$name.tar.gz" -C "$tmp"

echo "installing with sudo; you may be prompted for your password"
sudo "$tmp/$name/install.sh"
