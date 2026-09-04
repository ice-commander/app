#!/bin/bash
set -e

# Builds fakelzo and installs it OVER the liblzo2 that dylibbundler copied into the .app.
#
# Usage: ./fakelzo/build-macos.sh <path-to-bundled-liblzo2.2.dylib>
#
# The replacement has to keep the original's install_name and version fields: the cairo script
# interpreter records both in its load commands, and dyld refuses a library whose compatibility
# version is lower than what the client asked for. Rather than hard-coding them (they change
# whenever Homebrew bumps lzo), read them back off the file we are about to overwrite.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
TARGET="$1"

if [ -z "$TARGET" ]; then
    echo "usage: $0 <path-to-liblzo2.2.dylib>" >&2
    exit 1
fi
if [ ! -f "$TARGET" ]; then
    echo "$TARGET does not exist — nothing to replace" >&2
    exit 1
fi

INSTALL_NAME=$(otool -D "$TARGET" | tail -1)
COMPAT=$(otool -l "$TARGET" | awk '/cmd LC_ID_DYLIB/{f=1} f&&/compatibility version/{print $3; exit}')
CURRENT=$(otool -l "$TARGET" | awk '/cmd LC_ID_DYLIB/{f=1} f&&/current version/{print $3; exit}')

clang -dynamiclib -O2 -o "$TARGET" "$SCRIPT_DIR/fakelzo.c" \
    -install_name "$INSTALL_NAME" \
    -compatibility_version "$COMPAT" \
    -current_version "${CURRENT:-$COMPAT}"

# Fail loudly rather than shipping something dyld will reject at launch.
for sym in _lzo2a_decompress _lzo2a_999_compress; do
    nm -gU "$TARGET" | grep -q "$sym" || { echo "fakelzo is missing $sym"; exit 1; }
done

echo "fakelzo installed at $TARGET (install_name $INSTALL_NAME, compat $COMPAT)"
