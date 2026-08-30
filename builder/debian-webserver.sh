#!/bin/bash
set -e

cd /home/builder/workdir

VERSION=$(node -p "require('./package.json').version")


echo "--- Starting Debian build (webserver) ---"
echo "Rust version: $(rustc --version)"

# Same target dir as the GTK deb build: the shared crates are already compiled, so only
# the GTK-free graph (virtualfs no-gtk, panel-core/server, webserver-app) links here.
export CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/deb/target"
export CARGO_HOME="/home/builder/workdir/bin/distr/cargo-home-shared"
node ./builder/gen-version.js webserver deb

cargo build -p webserver-app --release

mkdir -p ./bin/webserver-app/release/
cp $CARGO_TARGET_DIR/release/ice-webserver ./bin/webserver-app/release/ice-webserver

cargo deb -p webserver-app --no-build

cp $CARGO_TARGET_DIR/debian/ice-commander-webserver*.deb ./distr


FILE=$(ls -t distr/ice-commander-webserver*.deb | head -n 1)
echo "Installer Debian webserver md5: $(md5sum "$FILE" | awk '{print $1}')" >> distr/md5sums.txt

exit 0
