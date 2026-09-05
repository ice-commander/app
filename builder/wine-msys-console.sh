#!/bin/bash
set -e

cd /home/builder/workdir


echo "--- Starting Windows Cross-Compilation (console) ---"

export CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/exe/target"
export CARGO_HOME="/home/builder/workdir/bin/distr/cargo-home-shared"
node ./builder/gen-version.js console exe

cargo build -p console-app --release --target x86_64-pc-windows-gnu

echo "--- Compiling 64-bit Windows Installer with NSIS (console) ---"
mkdir -p ./distr

makensis src/console-app/setup.nsi

VERSION=$(node -p "require('./package.json').version")

echo "$(md5sum distr/ice-commander-console-${VERSION}-1-win64.exe | awk '{print $1}') Windows console" >> distr/md5sums.txt

echo "--- Done! ---"
exit 0
