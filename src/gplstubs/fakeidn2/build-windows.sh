#!/bin/bash
set -e

# Builds fakeidn2 as libidn2-0.dll for the Windows bundle.
#
# Usage: ./src/gplstubs/fakeidn2/build-windows.sh [output-dir]      (default: bin/distr/gplstubs)
#
# Nothing is overwritten in place: src/gtk-app/setup.nsi excludes libidn2-0.dll from the
# recursive File directive over artifacts/gtk4-win32-x64 and picks ours up instead. That way
# the GPL code is never packed into the installer at all — excluding it matters, because
# shipping it inside the archive would be distribution regardless of which copy wins on disk.
#
# Run anywhere a mingw-w64 cross compiler is available; src/gplstubs/build-windows.sh calls it.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OUT_DIR="${1:-$(cd "$SCRIPT_DIR/../../.." && pwd)/bin/distr/gplstubs}"
OUT="$OUT_DIR/libidn2-0.dll"

CC=""
for candidate in x86_64-w64-mingw32-gcc x86_64-w64-mingw32-gcc-posix gcc; do
    if command -v "$candidate" >/dev/null 2>&1; then
        # Plain `gcc` only qualifies if it actually targets mingw (i.e. inside MSYS2).
        if [ "$candidate" = "gcc" ] && ! gcc -dumpmachine 2>/dev/null | grep -q "^x86_64-.*mingw"; then
            continue
        fi
        CC="$candidate"
        break
    fi
done

if [ -z "$CC" ]; then
    echo "no mingw-w64 compiler found (looked for x86_64-w64-mingw32-gcc)" >&2
    exit 1
fi

mkdir -p "$OUT_DIR"

# -nostdlib leaves out the C runtime and GCC's start-up code, so the DLL holds this file and
# ../dllmain.c alone and imports nothing; -s drops the symbol table.
"$CC" -shared -O2 -s -nostdlib -Wl,--entry,DllMain -o "$OUT" "$SCRIPT_DIR/fakeidn2.c" \
    "$SCRIPT_DIR/../dllmain.c" -Wl,--out-implib,"$OUT_DIR/libidn2.dll.a"

# Verify the exports rather than trusting the compiler. curl and libpsl import these five by
# name, and a DLL missing any of them would make the installed application fail to start — a
# failure that would only show up on Windows.
if command -v x86_64-w64-mingw32-objdump >/dev/null 2>&1; then
    OBJDUMP=x86_64-w64-mingw32-objdump
elif command -v objdump >/dev/null 2>&1; then
    OBJDUMP=objdump
else
    OBJDUMP=""
fi

if [ -n "$OBJDUMP" ]; then
    # Not every host objdump can read PE. Degrade to "not verified" instead of
    # killing the build under set -e.
    EXPORTS=$("$OBJDUMP" -p "$OUT" 2>/dev/null) || EXPORTS=""
fi

if [ -n "$EXPORTS" ]; then
    for sym in idn2_check_version idn2_free idn2_lookup_u8 idn2_lookup_ul idn2_to_unicode_8z8z; do
        echo "$EXPORTS" | grep -qw "$sym" || { echo "fakeidn2.dll is missing export $sym" >&2; exit 1; }
    done
    echo "exports verified: all five idn2_* symbols"
else
    echo "objdump unavailable — exports not verified" >&2
fi

echo "fakeidn2 built at $OUT ($CC)"
