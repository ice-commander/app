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

# The GTK stack drags in liblzo2, libjbig and libidn2, whose builds carry GPL code; setup.nsi
# excludes the real ones from the artifacts tree and packs our stand-ins.
# See src/gplstubs/README.md.
echo "--- Building the GPL stand-ins (src/gplstubs) ---"
# Through bash: a script committed from a checkout with core.fileMode off has no exec bit.
bash ./src/gplstubs/build-windows.sh

echo "--- Checking the bundle for GPL code ---"
./builder/check-bundle-licenses.sh --strict

echo "--- Compiling 64-bit Windows Installer with NSIS natively! ---"
mkdir -p ./distr

makensis src/gtk-app/setup.nsi

VERSION=$(node -p "require('./package.json').version")

SUM=$(md5sum "distr/ice-commander-gtk-${VERSION}-1-win64.exe" | awk '{print $1}')
[ -n "$SUM" ] || { echo "could not checksum the installer" >&2; exit 1; }
echo "$SUM [GTK4-EXE] ice-commander-gtk-${VERSION}-1-win64.exe" >> distr/md5sums.txt

echo "--- Done! ---"
exit 0
