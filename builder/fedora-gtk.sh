#!/bin/bash
set -e

cd /home/builder/workdir

VERSION=$(node -p "require('./package.json').version")


echo "--- Starting Fedora build ---"
echo "Rust version: $(rustc --version)"
echo "Node.js version: $(node --version)"

export CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/rpm/target"
export CARGO_HOME="/home/builder/workdir/bin/distr/cargo-home-shared"
node ./builder/gen-version.js gui rpm

cargo build -p ice-commander-gtk --release

mkdir -p ./bin/gtk-app/release/
cp $CARGO_TARGET_DIR/release/ice-commander ./bin/gtk-app/release/ice-commander

rm -rf $CARGO_TARGET_DIR/generate-rpm
cd ./src/gtk-app
cargo generate-rpm
cd ../../

cp $CARGO_TARGET_DIR/generate-rpm/*.rpm ./distr


FILE=$(ls -t distr/*.rpm | head -n 1)
echo "$(md5sum "$FILE" | awk '{print $1}') Fedora gtk4" >> distr/md5sums.txt

exit 0
