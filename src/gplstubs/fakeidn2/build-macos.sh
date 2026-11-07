#!/bin/bash
set -e

# Builds fakeidn2 and installs it OVER the libidn2 that dylibbundler copied into the .app.
#
# Usage: ./src/gplstubs/fakeidn2/build-macos.sh <path-to-bundled-libidn2.0.dylib>
#
# Keeps the original's install_name and version fields: curl and libpsl record both in their
# load commands, and dyld refuses a library whose compatibility version is lower than what the
# client asked for. They are read back off the file we are about to overwrite rather than
# hard-coded, because Homebrew changes them.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TARGET="$1"

if [ -z "$TARGET" ]; then
    echo "usage: $0 <path-to-libidn2 dylib>" >&2
    exit 1
fi
if [ ! -f "$TARGET" ]; then
    echo "$TARGET does not exist — nothing to replace" >&2
    exit 1
fi

INSTALL_NAME=$(otool -D "$TARGET" | tail -1)
COMPAT=$(otool -l "$TARGET" | awk '/cmd LC_ID_DYLIB/{f=1} f&&/compatibility version/{print $3; exit}')
CURRENT=$(otool -l "$TARGET" | awk '/cmd LC_ID_DYLIB/{f=1} f&&/current version/{print $3; exit}')

[ -n "$INSTALL_NAME" ] && [ -n "$COMPAT" ] || { echo "could not read install_name/version off $TARGET" >&2; exit 1; }

clang -dynamiclib -O2 -o "$TARGET" "$SCRIPT_DIR/fakeidn2.c" \
    -install_name "$INSTALL_NAME" \
    -compatibility_version "$COMPAT" \
    -current_version "${CURRENT:-$COMPAT}"

# curl and libpsl import all five by name; a bundle missing one would only fail on a Mac.
for sym in _idn2_check_version _idn2_free _idn2_lookup_u8 _idn2_lookup_ul _idn2_to_unicode_8z8z; do
    nm -gU "$TARGET" | grep -q "$sym" || { echo "fakeidn2 is missing $sym"; exit 1; }
done

echo "fakeidn2 installed at $TARGET (install_name $INSTALL_NAME, compat $COMPAT)"
