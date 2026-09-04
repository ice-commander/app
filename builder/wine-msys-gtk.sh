#!/bin/bash
set -e

cd /home/builder/workdir


echo "--- Starting Native Linux Cross-Compilation (Fast!) ---"

export CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/exe/target"
export CARGO_HOME="/home/builder/workdir/bin/distr/cargo-home-shared"
node ./builder/gen-version.js gui exe

cargo build --release \
    --manifest-path /home/builder/workdir/src/gtk-app/Cargo.toml \
    --target x86_64-pc-windows-gnu

# fakelzo replaces the GPL liblzo2-2.dll that the GTK stack drags in; setup.nsi excludes the
# real one from the artifacts tree and packs this instead. See src/fakelzo/README.md.
echo "--- Building fakelzo (LGPL-clean liblzo2 replacement) ---"
./src/fakelzo/build-windows.sh

echo "--- Compiling 64-bit Windows Installer with NSIS natively! ---"
mkdir -p ./distr

makensis src/gtk-app/setup.nsi

VERSION=$(node -p "require('./package.json').version")

echo "Installer Windows md5: $(md5sum distr/ice-commander-${VERSION}-1-win64.exe | awk '{print $1}')" >> distr/md5sums.txt

echo "--- Done! ---"
exit 0
