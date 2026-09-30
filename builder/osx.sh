#!/bin/bash
set -e

echo "--- Starting Mac OSX Apple Silicon build ---"

VERSION=$(node -p "require('./package.json').version")

echo "Rust version: $(rustc --version)"
echo "Node.js version: $(node --version)"

# 1. Clean previous build outputs
node ./builder/gen-version.js gui dmg
rm -f ./bin/gtk-app/release/ice-commander
rm -rf ./bin/distr/dmg_stage
rm -f ./bin/distr/gtkapp-darwin/*.dmg
# Whatever dylibbundler stops copying would otherwise linger from the previous build.
rm -rf ./bin/distr/gtkapp-darwin/release/bundle

# Build the embedded web control panel before compiling (include_bytes!)
echo "=== Building web UI (web-app) ==="
npm run build-web-app

# 2. Run Cargo bundle to compile and create the .app structure
#
# The application links no mpv and no FFmpeg any more: video moved out to plugin-video,
# which carries its own LGPL libmpv. builder/build_libmpv_ffmpeg_lgpl.sh still builds that
# stack into artifacts/lgpl-media, but it is the PLUGIN's to carry, not this bundle's —
# nothing here needs it and nothing here links it.
#
# STILL OPEN, and it decides whether video works on macOS at all: the plugin opens libmpv
# with dlopen, looking beside itself first and then by bare name (plugin-video/src/
# video-view/src/mpv.rs, fn candidates). A bare name is not searched for inside an .app,
# so the dylibs have to end up beside the plugin — which is what plugin-video/build.sh
# does with IC_VIDEO_CODECS. Check that on a Mac before trusting it.

echo "=== Building application bundle ==="
cd ./src/gtk-app
CARGO_TARGET_DIR=../../bin/distr/gtkapp-darwin cargo bundle --release
cd ../..

APP_BUNDLE="./bin/distr/gtkapp-darwin/release/bundle/osx/IceCommander.app"
BREW_PREFIX=$(brew --prefix)
XCODE_DEV=$(xcode-select -p)

# 3. Copy and compile GLib schemas
echo "=== Packaging GLib settings schemas ==="
mkdir -p "$APP_BUNDLE/Contents/Resources/share/glib-2.0/schemas"
cp "$BREW_PREFIX/share/glib-2.0/schemas/org.gtk.gtk4.Settings"*.xml "$APP_BUNDLE/Contents/Resources/share/glib-2.0/schemas/"
glib-compile-schemas "$APP_BUNDLE/Contents/Resources/share/glib-2.0/schemas"

# 4. Bundle main binary dependencies (GTK and its own; no mpv or FFmpeg is linked any more)
echo "=== Bundling main binary dependencies ==="
dylibbundler -s /Library/Developer/CommandLineTools/usr/lib/swift-5.0/macosx \
             -s /Library/Developer/CommandLineTools/usr/lib/swift-5.5/macosx \
             -s "$XCODE_DEV/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.0/macosx" \
             -s "$XCODE_DEV/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx" \
             -s "$BREW_PREFIX/lib" \
             -od -b -x "$APP_BUNDLE/Contents/MacOS/ice-commander" \
             -d "$APP_BUNDLE/Contents/Libs/" \
             -p @executable_path/../Libs/

# Copy libpdfium.dylib into bundle's Libs directory
echo "=== Bundling libpdfium.dylib ==="
cp ./artifacts/libpdfium.dylib "$APP_BUNDLE/Contents/Libs/libpdfium.dylib"

# Replace liblzo2 (GPL-2.0-or-later) with fakelzo, our own two-symbol stand-in — see
# src/fakelzo/README.md for the reasoning and the measurement behind it. dylibbundler has just
# copied the real one in, because libcairo-script-interpreter links it; this overwrites it
# before signing, so the shipped bundle carries no GPL code.
echo "=== Replacing liblzo2 with fakelzo ==="
replaced=0
for lzo in "$APP_BUNDLE/Contents/Libs/"liblzo2*.dylib; do
    [ -e "$lzo" ] || continue
    ./src/fakelzo/build-macos.sh "$lzo"
    replaced=$((replaced + 1))
done
# cairo-script-interpreter always links it, so finding none means the dependency changed.
[ "$replaced" -gt 0 ] || { echo "no liblzo2 in the bundle — check what changed before shipping" >&2; exit 1; }

# libjbig (GPL-2.0-or-later) arrives the same way, through libtiff — see src/fakejbig/README.md.
echo "=== Replacing libjbig with fakejbig ==="
# Homebrew's libtiff is not built against jbigkit, so finding none here is expected.
for jbig in "$APP_BUNDLE/Contents/Libs/"libjbig*.dylib; do
    [ -e "$jbig" ] || continue
    ./src/fakejbig/build-macos.sh "$jbig"
done

# The stand-ins are verified rather than assumed: the lookups above match by name, and a
# soname bump would slip past them and leave the real library in the bundle.
echo "=== Verifying the GPL stand-ins took ==="
survivors=""
while IFS= read -r lib; do
    case "$(basename "$lib")" in
        liblzo2*) real="_lzo_version _lzo1x_1_compress _lzo1x_decompress"
                  ours="_lzo2a_decompress _lzo2a_999_compress" ;;
        libjbig*) real="_jbg_dec_getwidth _jbg_split_planes _jbg85_dec_init"
                  ours="_jbg_dec_init _jbg_newlen" ;;
        *)        continue ;;
    esac
    syms=$(nm -gU "$lib") || { echo "cannot read symbols from $lib" >&2; exit 1; }
    defined=$(printf '%s\n' "$syms" | awk '{print $NF}')
    for sym in $real; do
        if printf '%s\n' "$defined" | grep -qx -- "$sym"; then
            echo "  FAIL $(basename "$lib") defines $sym — this is the real library"
            survivors="$survivors $(basename "$lib")"
        fi
    done
    for sym in $ours; do
        if ! printf '%s\n' "$defined" | grep -qx -- "$sym"; then
            echo "  FAIL $(basename "$lib") does not define $sym — the stand-in is incomplete"
            survivors="$survivors $(basename "$lib")"
        fi
    done
    case "$survivors" in
        *"$(basename "$lib")"*) ;;
        *) echo "  ok   $(basename "$lib") is our stand-in" ;;
    esac
done < <(find "$APP_BUNDLE" -type f \( -name '*.dylib' -o -name '*.so' \))

if [ -n "$survivors" ]; then
    echo "GPL code survived into the bundle:$survivors — refusing to sign." >&2
    exit 1
fi

# 5. License texts for the bundled LGPL/GPL libraries — must land before signing,
#    or codesign will not cover them.
echo "=== Bundling license texts ==="
mkdir -p "$APP_BUNDLE/Contents/Resources/licenses"
cp ./assets/licenses/*.txt "$APP_BUNDLE/Contents/Resources/licenses/"

# 6. Codesign the bundle
echo "=== Codesigning the bundle ==="
codesign --force --deep --sign - "$APP_BUNDLE"
codesign --verify --deep --strict "$APP_BUNDLE"

# 7. Package to DMG
echo "=== Packaging DMG Installer ==="
mkdir -p ./bin/distr/dmg_stage
cp -r "$APP_BUNDLE" ./bin/distr/dmg_stage/

DMG_OUTPUT="./bin/distr/gtkapp-darwin/IceCommander.dmg"
create-dmg --volname "Ice Commander Installer" \
           --window-pos 200 120 \
           --window-size 800 400 \
           --icon-size 100 \
           --app-drop-link 600 185 \
           "$DMG_OUTPUT" \
           "./bin/distr/dmg_stage/"

rm -rf ./bin/distr/dmg_stage

DMG_NAME="ice-commander-gtk-${VERSION}-1-mac.dmg"
mv "$DMG_OUTPUT" ./distr/$DMG_NAME


SUM=$(md5 -q "distr/$DMG_NAME")
[ -n "$SUM" ] || { echo "could not checksum distr/$DMG_NAME" >&2; exit 1; }
echo "$SUM [GTK4-DMG] $DMG_NAME" >> distr/md5sums.txt
