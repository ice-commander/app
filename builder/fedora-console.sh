#!/bin/bash
set -e

cd /home/builder/workdir

VERSION=$(node -p "require('./package.json').version")


echo "--- Starting Fedora build (console) ---"
echo "Rust version: $(rustc --version)"

export CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/rpm/target"
export CARGO_HOME="/home/builder/workdir/bin/distr/cargo-home-shared"
node ./builder/gen-version.js console rpm

cargo build -p console-app --release

mkdir -p ./bin/console-app/release/
cp $CARGO_TARGET_DIR/release/ice-console ./bin/console-app/release/ice-console

rm -rf $CARGO_TARGET_DIR/generate-rpm
cd ./src/console-app
cargo generate-rpm
cd ../../

cp $CARGO_TARGET_DIR/generate-rpm/*.rpm ./distr


FILE=$(ls -t distr/ice-commander-console*.rpm | head -n 1)
echo "$(md5sum "$FILE" | awk '{print $1}') Fedora console" >> distr/md5sums.txt

exit 0
