#!/bin/bash
set -e

# Builds fakejbig and installs it OVER the libjbig that dylibbundler copied into the .app.
#
# Usage: ./src/fakejbig/build-macos.sh <path-to-bundled-libjbig.2.1.dylib>
#
# Keeps the original's install_name and version fields: libtiff records both in its load
# commands, and dyld refuses a library whose compatibility version is lower than what the
# client asked for. They are read back off the file we are about to overwrite rather than
# hard-coded, because Homebrew changes them.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TARGET="$1"

if [ -z "$TARGET" ]; then
    echo "usage: $0 <path-to-libjbig dylib>" >&2
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

clang -dynamiclib -O2 -o "$TARGET" "$SCRIPT_DIR/fakejbig.c" \
    -install_name "$INSTALL_NAME" \
    -compatibility_version "$COMPAT" \
    -current_version "${CURRENT:-$COMPAT}"

# libtiff imports all ten by name; a bundle missing one would only fail on a user's Mac.
for sym in _jbg_dec_init _jbg_dec_in _jbg_dec_getsize _jbg_dec_getimage _jbg_dec_free \
           _jbg_enc_init _jbg_enc_out _jbg_enc_free _jbg_newlen _jbg_strerror; do
    nm -gU "$TARGET" | grep -q "$sym" || { echo "fakejbig is missing $sym"; exit 1; }
done

echo "fakejbig installed at $TARGET (install_name $INSTALL_NAME, compat $COMPAT)"
