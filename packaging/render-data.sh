#!/bin/bash
# Stages data/ for one install: *.in templates get their @TOKEN@ placeholders
# substituted, everything else is copied verbatim. Both install.sh and the
# .deb build render through this, so the values a template needs have a single
# source rather than one per artifact.
#
# usage: render-data.sh <outdir> TOKEN=VALUE...
set -euo pipefail

outdir="$1"
shift
# With no pairs the sed below would take the template's name as its script and
# read stdin, writing every file out empty.
[ $# -gt 0 ] || { echo "usage: $(basename "$0") <outdir> TOKEN=VALUE..." >&2; exit 1; }

src="$(cd "$(dirname "$0")/../data" && pwd)"

rm -rf "$outdir"
cp -r "$src" "$outdir"

subst=()
for pair in "$@"; do
    [ -n "${pair#*=}" ] || { echo "${pair%%=*} has no value" >&2; exit 1; }
    subst+=(-e "s|@${pair%%=*}@|${pair#*=}|g")
done

for tmpl in "$outdir"/*.in; do
    rendered="${tmpl%.in}"
    sed "${subst[@]}" "$tmpl" >"$rendered"
    rm -f "$tmpl"
    # A token nobody passed would otherwise ship as literal @TEXT@, which the
    # consumers of these files read as a path or a version.
    if leftover="$(grep -o '@[A-Z_]\+@' "$rendered" | sort -u)"; then
        echo "${rendered##*/} still has unsubstituted tokens:" >&2
        echo "  ${leftover//$'\n'/$'\n'  }" >&2
        exit 1
    fi
done
