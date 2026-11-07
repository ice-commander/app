#!/bin/bash
set -u

# Scans the Windows bundle for GPL code before setup.nsi packs it into the installer.
#
# Usage: ./builder/check-bundle-licenses.sh [bundle-dir] [--nsi=path] [--stubs=path] [--strict]
#            default bundle-dir: artifacts/gtk4-win32-x64
#            default nsi:        src/gtk-app/setup.nsi
#            default stubs:      bin/distr/gplstubs, what src/gplstubs/build-windows.sh writes
#            --strict: exit non-zero if anything is found
#
# A tripwire, not a licence audit: it finds declarations, not silence. The reasoned inventory
# lives in assets/licenses/BUNDLED-COMPONENTS.txt.

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUNDLE=""
NSI="$ROOT/src/gtk-app/setup.nsi"
STUBS="$ROOT/bin/distr/gplstubs"
STRICT=0

for a in "$@"; do
    case "$a" in
        --strict)  STRICT=1 ;;
        --nsi=*)   NSI="${a#--nsi=}" ;;
        --stubs=*) STUBS="${a#--stubs=}" ;;
        *)         [ -z "$BUNDLE" ] && BUNDLE="$a" ;;
    esac
done

[ -n "$BUNDLE" ] || BUNDLE="$ROOT/artifacts/gtk4-win32-x64"
[ -d "$BUNDLE" ] || { echo "no such bundle directory: $BUNDLE" >&2; exit 1; }

findings=0

OBJDUMP=""
for candidate in x86_64-w64-mingw32-objdump objdump; do
    if command -v "$candidate" >/dev/null 2>&1; then
        OBJDUMP="$candidate"
        break
    fi
done

# A stand-in missing a function that a library in the bundle imports from it would stop the
# installed application from starting, and only on Windows: compare what is imported from the
# name with what the stand-in exports.
check_stub() {
    local base="$1" stub="$STUBS/$1" wanted="" exported missing
    if [ -n "$OBJDUMP" ]; then
        wanted=$(find "$BUNDLE" -type f \( -iname '*.dll' -o -iname '*.exe' \) -print0 |
            xargs -0 "$OBJDUMP" -p 2>/dev/null |
            awk -v dll="$(printf '%s' "$base" | tr '[:upper:]' '[:lower:]')" '
                /DLL Name:/ { cur = tolower($3); next }
                /^[[:space:]]*$/ { cur = ""; next }
                cur == dll && $1 ~ /^[0-9a-f]+$/ { print ($NF == "<none>" ? "ordinal " $2 : $NF) }' | sort -u)
    fi
    if [ ! -f "$stub" ]; then
        echo "FINDING  $base: its stand-in was not built ($stub)"
        [ -n "$wanted" ] && echo "         the bundle imports from it: $(printf '%s\n' "$wanted" | tr '\n' ' ')"
        findings=$((findings + 1))
        return
    fi
    if [ -z "$OBJDUMP" ]; then
        echo "FINDING  what the bundle imports from $base cannot be checked: no objdump"
        findings=$((findings + 1))
        return
    fi
    if [ -z "$wanted" ]; then
        echo "FINDING  nothing in the bundle imports from $base, or $OBJDUMP cannot read PE files"
        findings=$((findings + 1))
        return
    fi
    if ! "$OBJDUMP" -f "$stub" 2>/dev/null | grep -q 'pei-x86-64'; then
        echo "FINDING  the stand-in $base is not a 64-bit Windows DLL"
        findings=$((findings + 1))
        return
    fi
    exported=$("$OBJDUMP" -p "$stub" 2>/dev/null |
        awk '/\[Ordinal\/Name Pointer\] Table/ { t = 1; next }
             t && /^[[:space:]]*$/ { t = 0 }
             t && /\[/ { print $NF }' | sort -u)
    missing=$(comm -23 <(printf '%s\n' "$wanted") <(printf '%s\n' "$exported") | tr '\n' ' ')
    if [ -n "${missing// /}" ]; then
        echo "FINDING  the stand-in $base does not export what the bundle imports from it: $missing"
        findings=$((findings + 1))
    else
        echo "ok       the stand-in $base exports all $(printf '%s\n' "$wanted" | grep -c .) functions the bundle imports from it"
    fi
}

# The MSYS2 tree legitimately carries liblzo2, libjbig and libidn2; the stand-ins in
# src/gplstubs replace them only at install time, so what has to hold is that setup.nsi still
# excludes the name the bundle actually has. A soname bump escapes an exclusion written without
# a wildcard, and only the lines NSIS reads count: a commented-out exclusion or File line does
# not.
excludes=""
nsi_live=""
if [ -f "$NSI" ]; then
    nsi_live=$(grep -v '^[[:space:]]*[;#]' "$NSI")
    excludes=$(printf '%s\n' "$nsi_live" | grep -o '/x [^ ]*' | cut -d' ' -f2 | tr -d '"')
else
    echo "FINDING  installer script not found: $NSI"
    findings=$((findings + 1))
fi

for pat in 'liblzo2*.dll' 'libjbig*.dll' 'libidn2*.dll'; do
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
            if [ -f "$NSI" ] && ! printf '%s\n' "$nsi_live" | grep -q "gplstubs\\\\$base\""; then
                echo "FINDING  $base is excluded, but no replacement is packed under that name"
                echo "         $(basename "$NSI") packs: $(printf '%s\n' "$nsi_live" | grep -o 'gplstubs\\[^"]*' | tr '\n' ' ')"
                findings=$((findings + 1))
            else
                echo "ok       $base is in the bundle, excluded and replaced by $(basename "$NSI")"
                check_stub "$base"
            fi
        else
            echo "FINDING  GPL library packed into the installer: $base"
            echo "         $(basename "$NSI") has no exclusion matching that name."
            findings=$((findings + 1))
        fi
    done
done

if ! command -v python3 >/dev/null 2>&1; then
    echo "FINDING  the byte scan for GPL declarations cannot run: no python3"
    findings=$((findings + 1))
    report=""
else

# A bare "GPL" declaration compiled into a library. `strings` cannot find it: its
# four-character minimum hides a three-letter "GPL", hence the raw byte scan.
report=$(python3 - "$BUNDLE" "$STUBS" "$excludes" <<'PY'
import fnmatch, glob, os, sys

KNOWN = {b"LGPL", b"GPL", b"QPL", b"GPL/QPL", b"MPL", b"MPL-2.0", b"BSD", b"MIT/X11",
         b"0BSD", b"Apache 2.0", b"Proprietary", b"unknown", b"Attribution",
         b"Attribution-NonCommercial", b"Attribution-ShareAlike"}

# gnulib's progname module is GPL-3.0-or-later and meant for programs. A library built with a
# program's gnulib carries it, as MSYS2's libidn2 did, and this message is how it shows.
PROGNAME = b"A NULL argv[0] was passed through an exec system call."

bundle, stubs, excludes = sys.argv[1], sys.argv[2], sys.argv[3].split()
# What gets packed: the bundle, subfolders included, without what setup.nsi excludes, and the
# stand-ins.
packed = {p: os.path.relpath(p, bundle)
          for p in glob.glob(os.path.join(bundle, "**", "*.dll"), recursive=True)
          if os.path.isfile(p) and not any(fnmatch.fnmatch(os.path.basename(p).lower(), x.lower()) for x in excludes)}
packed.update({p: os.path.join(os.path.basename(stubs.rstrip("/")), os.path.basename(p))
               for p in glob.glob(os.path.join(stubs, "*.dll")) if os.path.isfile(p)})

for path, name in sorted(packed.items(), key=lambda item: item[1]):
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
        print(f"{kind}\t{name}\t{' | '.join(ctx[:4])}")
        pos = data.find(b"\x00GPL\x00", pos + 1)
    if PROGNAME in data:
        print(f"PROGNAME\t{name}\t{PROGNAME.decode()}")
print(f"SCANNED\t{len(packed)}\t")
PY
) || { echo "FINDING  the byte scan for GPL declarations failed"; findings=$((findings + 1)); }
fi

count=0
while IFS=$'\t' read -r kind lib ctx; do
    [ -n "${kind:-}" ] || continue
    if [ "$kind" = "SCANNED" ]; then
        count="$lib"
    elif [ "$kind" = "DECLARATION" ]; then
        echo "FINDING  $lib declares GPL for a component compiled into it"
        echo "         context: $ctx"
        findings=$((findings + 1))
    elif [ "$kind" = "PROGNAME" ]; then
        echo "FINDING  $lib contains gnulib's progname module, which is GPL-3.0-or-later"
        echo "         context: $ctx"
        findings=$((findings + 1))
    elif [ "$kind" = "KNOWN" ]; then
        echo "known    $lib: gdk-pixbuf ICNS metadata says GPL, its source header says LGPL-2.0+"
        echo "         ($ctx) — see assets/licenses/BUNDLED-COMPONENTS.txt"
    else
        echo "ok       $lib carries a licence-name table, not a declaration ($ctx)"
    fi
done <<< "$report"

if [ "$count" -eq 0 ] && command -v python3 >/dev/null 2>&1; then
    echo "FINDING  the byte scan found no libraries to scan"
    findings=$((findings + 1))
fi

echo
if [ "$findings" -eq 0 ]; then
    echo "bundle check: $count libraries, no GPL declarations"
    exit 0
fi

echo "bundle check: $count libraries, $findings finding(s) — do not ship this installer unreviewed."
[ "$STRICT" -eq 1 ] && exit 1
exit 0
