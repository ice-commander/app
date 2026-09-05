#!/bin/bash
set -e

cd /home/builder/workdir

VERSION=$(node -p "require('./package.json').version")


echo "--- Starting Fedora build (webserver) ---"
echo "Rust version: $(rustc --version)"

export CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/rpm/target"
export CARGO_HOME="/home/builder/workdir/bin/distr/cargo-home-shared"
node ./builder/gen-version.js webserver rpm

cargo build -p webserver-app --release

mkdir -p ./bin/webserver-app/release/
cp $CARGO_TARGET_DIR/release/ice-webserver ./bin/webserver-app/release/ice-webserver

rm -rf $CARGO_TARGET_DIR/generate-rpm
cd ./src/webserver-app
cargo generate-rpm
cd ../../

cp $CARGO_TARGET_DIR/generate-rpm/*.rpm ./distr


FILE=$(ls -t distr/ice-commander-webserver*.rpm | head -n 1)
echo "$(md5sum "$FILE" | awk '{print $1}') Fedora webserver" >> distr/md5sums.txt

exit 0
