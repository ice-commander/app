#!/bin/bash
set -e

cd /home/builder/workdir

VERSION=$(node -p "require('./package.json').version")


echo "--- Starting Debian build ---"
echo "Rust version: $(rustc --version)"
echo "Node.js version: $(node --version)"

export CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/deb/target"
export CARGO_HOME="/home/builder/workdir/bin/distr/cargo-home-shared"
node ./builder/gen-version.js gui deb

cargo build -p ice-commander-gtk --release

mkdir -p ./bin/gtk-app/release/
cp $CARGO_TARGET_DIR/release/ice-commander ./bin/gtk-app/release/ice-commander

cargo deb -p ice-commander-gtk

cp $CARGO_TARGET_DIR/debian/*.deb ./distr


FILE=$(ls -t distr/*.deb | head -n 1)
echo "Installer Debian md5: $(md5sum "$FILE" | awk '{print $1}')" >> distr/md5sums.txt

exit 0
