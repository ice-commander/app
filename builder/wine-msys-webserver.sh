#!/bin/bash
set -e

cd /home/builder/workdir


echo "--- Starting Windows Cross-Compilation (webserver) ---"

export CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/exe/target"
export CARGO_HOME="/home/builder/workdir/bin/distr/cargo-home-shared"
node ./builder/gen-version.js webserver exe

cargo build -p webserver-app --release --target x86_64-pc-windows-gnu

echo "--- Compiling 64-bit Windows Installer with NSIS (webserver) ---"
mkdir -p ./distr

makensis src/webserver-app/setup.nsi

VERSION=$(node -p "require('./package.json').version")

echo "$(md5sum distr/ice-commander-webserver-${VERSION}-1-win64.exe | awk '{print $1}') Windows webserver" >> distr/md5sums.txt

echo "--- Done! ---"
exit 0
