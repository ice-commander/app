#!/bin/bash
set -u

# Scans the Windows bundle for GPL code before setup.nsi packs it into the installer.
#
# Usage: ./builder/check-bundle-licenses.sh [bundle-dir] [--nsi=path] [--strict]
#            default bundle-dir: artifacts/gtk4-win32-x64
#            default nsi:        src/gtk-app/setup.nsi
#            --strict: exit non-zero if anything is found
#
# A tripwire, not a licence audit: it finds declarations, not silence. The reasoned inventory
# lives in assets/licenses/BUNDLED-COMPONENTS.txt.

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE=""
NSI="$ROOT/src/gtk-app/setup.nsi"
STRICT=0

for a in "$@"; do
    case "$a" in
        --strict) STRICT=1 ;;
        --nsi=*)  NSI="${a#--nsi=}" ;;
        *)        [ -z "$BUNDLE" ] && BUNDLE="$a" ;;
    esac
done

[ -n "$BUNDLE" ] || BUNDLE="$ROOT/artifacts/gtk4-win32-x64"
[ -d "$BUNDLE" ] || { echo "no such bundle directory: $BUNDLE" >&2; exit 1; }

findings=0

# The MSYS2 tree legitimately carries liblzo2 and libjbig; fakelzo/fakejbig replace them only
# at install time, so what has to hold is that setup.nsi still excludes the name the bundle
# actually has. A soname bump escapes an exclusion written without a wildcard.
excludes=""
if [ -f "$NSI" ]; then
    excludes=$(grep -o '/x [^ ]*' "$NSI" | cut -d' ' -f2 | tr -d '"')
else
    echo "FINDING  installer script not found: $NSI"
    findings=$((findings + 1))
fi

for pat in 'liblzo2*.dll' 'libjbig*.dll'; do
    for f in "$BUNDLE"/$pat; do
        [ -e "$f" ] || continue
        base=$(basename "$f")
        covered=0
        for x in $excludes; do
            case "$base" in $x) covered=1 ;; esac
        done
        if [ "$covered" -eq 1 ]; then
            # The wildcard exclusion survives a soname bump; the replacement's fixed name does
            # not, and the loader asks for the name the bundle had.
            if [ -f "$NSI" ] && ! grep -q "fake[a-z]*\\\\$base\"" "$NSI"; then
                echo "FINDING  $base is excluded, but no replacement is packed under that name"
                echo "         $(basename "$NSI") packs: $(grep -o 'fake[a-z]*\\[^"]*' "$NSI" | tr '\n' ' ')"
                findings=$((findings + 1))
            else
                echo "ok       $base is in the bundle, excluded and replaced by $(basename "$NSI")"
            fi
        else
            echo "FINDING  GPL library packed into the installer: $base"
            echo "         $(basename "$NSI") has no exclusion matching that name."
            findings=$((findings + 1))
        fi
    done
done

if ! command -v python3 >/dev/null 2>&1; then
    echo "skipped  byte scan for GPL declarations: python3 not available"
    report=""
else

# A bare "GPL" declaration compiled into a library. `strings` cannot find it: its
# four-character minimum hides a three-letter "GPL", hence the raw byte scan.
report=$(python3 - "$BUNDLE" <<'PY'
import glob, os, sys

KNOWN = {b"LGPL", b"GPL", b"QPL", b"GPL/QPL", b"MPL", b"MPL-2.0", b"BSD", b"MIT/X11",
         b"0BSD", b"Apache 2.0", b"Proprietary", b"unknown", b"Attribution",
         b"Attribution-NonCommercial", b"Attribution-ShareAlike"}

for path in sorted(glob.glob(os.path.join(sys.argv[1], "*.dll"))):
    data = open(path, "rb").read()
    pos = data.find(b"\x00GPL\x00")
    while pos != -1:
        window = data[max(0, pos - 120): pos + 120]
        neighbours = {s for s in window.split(b"\x00") if s and s in KNOWN}
        # A library embedding a vocabulary of licence names lists several together;
        # a declaration stands alone.
        kind = "TABLE" if len(neighbours) >= 3 else "DECLARATION"
        # gdk-pixbuf's ICNS record: the string contradicts the file's own LGPL header.
        if kind == "DECLARATION" and b"icns" in window:
            kind = "KNOWN"
        ctx = [s.decode("utf-8", "replace") for s in data[pos - 60:pos + 40].split(b"\x00")
               if 3 < len(s) < 40]
        print(f"{kind}\t{os.path.basename(path)}\t{' | '.join(ctx[:4])}")
        pos = data.find(b"\x00GPL\x00", pos + 1)
PY
)
fi

while IFS=$'\t' read -r kind lib ctx; do
    [ -n "${kind:-}" ] || continue
    if [ "$kind" = "DECLARATION" ]; then
        echo "FINDING  $lib declares GPL for a component compiled into it"
        echo "         context: $ctx"
        findings=$((findings + 1))
    elif [ "$kind" = "KNOWN" ]; then
        echo "known    $lib: gdk-pixbuf ICNS metadata says GPL, its source header says LGPL-2.0+"
        echo "         ($ctx) — see assets/licenses/BUNDLED-COMPONENTS.txt"
    else
        echo "ok       $lib carries a licence-name table, not a declaration ($ctx)"
    fi
done <<< "$report"

count=$(ls "$BUNDLE"/*.dll 2>/dev/null | wc -l)
echo
if [ "$findings" -eq 0 ]; then
    echo "bundle check: $count libraries, no GPL declarations"
    exit 0
fi

echo "bundle check: $count libraries, $findings finding(s) — do not ship this installer unreviewed."
[ "$STRICT" -eq 1 ] && exit 1
exit 0
