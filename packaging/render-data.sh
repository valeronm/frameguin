#!/bin/bash
# Stages data/ for one install prefix: *.in templates get @LIBEXECDIR@
# substituted, everything else is copied verbatim. Both install.sh (prefix
# /usr/local) and the .deb build (prefix /usr) render through this, so the
# daemon's absolute path has a single source of truth.
#
# usage: render-data.sh <libexecdir> <outdir>
set -euo pipefail
shopt -s nullglob

libexecdir="$1"
outdir="$2"
src="$(cd "$(dirname "$0")/../data" && pwd)"

rm -rf "$outdir"
cp -r "$src" "$outdir"

for tmpl in "$outdir"/*.in; do
    sed "s|@LIBEXECDIR@|$libexecdir|g" "$tmpl" >"${tmpl%.in}"
    rm -f "$tmpl"
done
