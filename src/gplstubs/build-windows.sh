#!/bin/bash
set -e

# Builds every stand-in in this folder for the Windows bundle, into one folder that
# src/gtk-app/setup.nsi packs in place of the excluded originals.
#
# Usage: ./src/gplstubs/build-windows.sh [output-dir]      (default: bin/distr/gplstubs)
#
# builder/wine-msys-gtk.sh calls it before builder/check-bundle-licenses.sh, which checks that
# each stand-in exports everything the bundle imports from it.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OUT_DIR="${1:-$(cd "$SCRIPT_DIR/../.." && pwd)/bin/distr/gplstubs}"

for build in "$SCRIPT_DIR"/*/build-windows.sh; do
    echo "--- $(basename "$(dirname "$build")") ---"
    # Through bash: a script committed from a checkout with core.fileMode off has no exec bit.
    bash "$build" "$OUT_DIR"
done
