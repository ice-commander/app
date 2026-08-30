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

# Build the embedded web control panel before compiling (include_bytes!)
echo "=== Building web UI (web-app) ==="
npm run build-web-app

# 2. Run Cargo bundle to compile and create the .app structure
echo "=== Building application bundle ==="
cd ./src/gtk-app
CARGO_TARGET_DIR=../../bin/distr/gtkapp-darwin cargo bundle --release
cd ../..

APP_BUNDLE="./bin/distr/gtkapp-darwin/release/bundle/osx/IceCommander.app"
BREW_PREFIX=$(brew --prefix)

# 3. Copy and compile GLib schemas
echo "=== Packaging GLib settings schemas ==="
mkdir -p "$APP_BUNDLE/Contents/Resources/share/glib-2.0/schemas"
cp "$BREW_PREFIX/share/glib-2.0/schemas/org.gtk.gtk4.Settings"*.xml "$APP_BUNDLE/Contents/Resources/share/glib-2.0/schemas/"
glib-compile-schemas "$APP_BUNDLE/Contents/Resources/share/glib-2.0/schemas"

# 4. Bundle main binary dependencies (dylibbundler will automatically bundle libmpv and FFmpeg dylibs)
echo "=== Bundling main binary dependencies ==="
dylibbundler -s /Library/Developer/CommandLineTools/usr/lib/swift-5.0/macosx \
             -s /Library/Developer/CommandLineTools/usr/lib/swift-5.5/macosx \
             -s "$(xcode-select -p)/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.0/macosx" \
             -s "$(xcode-select -p)/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx" \
             -s "$BREW_PREFIX/lib" \
             -od -b -x "$APP_BUNDLE/Contents/MacOS/ice-commander" \
             -d "$APP_BUNDLE/Contents/Libs/" \
             -p @executable_path/../Libs/

# Copy libpdfium.dylib into bundle's Libs directory
echo "=== Bundling libpdfium.dylib ==="
cp ./artifacts/libpdfium.dylib "$APP_BUNDLE/Contents/Libs/libpdfium.dylib"

# 5. License texts for the bundled LGPL/GPL libraries — must land before signing,
#    or codesign will not cover them.
echo "=== Bundling license texts ==="
mkdir -p "$APP_BUNDLE/Contents/Resources/licenses"
cp ./assets/licenses/*.txt "$APP_BUNDLE/Contents/Resources/licenses/"

# 6. Codesign the bundle
echo "=== Codesigning the bundle ==="
codesign --force --deep --sign - "$APP_BUNDLE"

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

DMG_NAME="ice-commander-${VERSION}-1-mac.dmg"
mv "$DMG_OUTPUT" ./distr/$DMG_NAME


echo "Installer macOS md5: $(md5 -q distr/$DMG_NAME)" >> distr/md5sums.txt
