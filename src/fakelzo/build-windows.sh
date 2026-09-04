#!/bin/bash
set -e

# Builds fakelzo as liblzo2-2.dll for the Windows bundle.
#
# Usage: ./fakelzo/build-windows.sh [output-dir]      (default: bin/distr/fakelzo)
#
# Unlike macOS, nothing is overwritten in place here: artifacts/gtk4-win32-x64 keeps the real
# (GPL) liblzo2-2.dll untouched, and src/gtk-app/setup.nsi excludes that one file from the
# recursive File directive and picks ours up instead. That way the GPL library is never packed
# into the installer at all — excluding it matters, because shipping it inside the archive would
# be distribution regardless of which copy wins on disk.
#
# Run inside the msys-builder container (builder/wine-msys-gtk.sh does this automatically), or
# anywhere a mingw-w64 cross compiler is available.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
OUT_DIR="${1:-$(cd "$SCRIPT_DIR/../.." && pwd)/bin/distr/fakelzo}"
OUT="$OUT_DIR/liblzo2-2.dll"

CC=""
for candidate in x86_64-w64-mingw32-gcc x86_64-w64-mingw32-gcc-posix gcc; do
    if command -v "$candidate" >/dev/null 2>&1; then
        # Plain `gcc` only qualifies if it actually targets mingw (i.e. inside MSYS2).
        if [ "$candidate" = "gcc" ] && ! gcc -dumpmachine 2>/dev/null | grep -q mingw; then
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

# -shared makes the DLL; the two functions are exported by default because they are
# non-static externals and no .def / __declspec(dllexport) narrowing is applied.
"$CC" -shared -O2 -o "$OUT" "$SCRIPT_DIR/fakelzo.c" -Wl,--out-implib,"$OUT_DIR/liblzo2.dll.a"

# Verify the exports rather than trusting the compiler: a DLL missing these would make the
# installed application fail to start, and that failure would only show up on Windows.
if command -v x86_64-w64-mingw32-objdump >/dev/null 2>&1; then
    OBJDUMP=x86_64-w64-mingw32-objdump
elif command -v objdump >/dev/null 2>&1; then
    OBJDUMP=objdump
else
    OBJDUMP=""
fi

if [ -n "$OBJDUMP" ]; then
    # The export NAMES live in the "[Ordinal/Name Pointer] Table" section, not in the
    # "Export Address Table" one (which only carries ordinals and RVAs) — grep the whole
    # -p output rather than a slice of it.
    EXPORTS=$("$OBJDUMP" -p "$OUT")
    for sym in lzo2a_decompress lzo2a_999_compress; do
        echo "$EXPORTS" | grep -qw "$sym" || { echo "fakelzo.dll is missing export $sym" >&2; exit 1; }
    done
    echo "exports verified: lzo2a_decompress, lzo2a_999_compress"
else
    echo "objdump unavailable — exports not verified" >&2
fi

echo "fakelzo built at $OUT ($CC)"
