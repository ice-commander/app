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

echo "--- Compiling 64-bit Windows Installer with NSIS natively! ---"
mkdir -p ./distr

makensis src/gtk-app/setup.nsi

VERSION=$(node -p "require('./package.json').version")

echo "Installer Windows md5: $(md5sum distr/ice-commander-${VERSION}-1-win64.exe | awk '{print $1}')" >> distr/md5sums.txt

echo "--- Done! ---"
exit 0
