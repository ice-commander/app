#!/bin/bash
set -e

cd /home/builder/workdir

VERSION=$(node -p "require('./package.json').version")


echo "--- Starting Debian build (console) ---"
echo "Rust version: $(rustc --version)"

# Same target dir as the GTK/webserver deb builds: the shared crates are already compiled, so
# only the GTK-free graph (virtualfs no-gtk, panel-core, console-app) links here.
export CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/deb/target"
export CARGO_HOME="/home/builder/workdir/bin/distr/cargo-home-shared"
node ./builder/gen-version.js console deb

cargo build -p console-app --release

mkdir -p ./bin/console-app/release/
cp $CARGO_TARGET_DIR/release/ice-console ./bin/console-app/release/ice-console

cargo deb -p console-app --no-build

cp $CARGO_TARGET_DIR/debian/ice-commander-console*.deb ./distr


FILE=$(ls -t distr/ice-commander-console*.deb | head -n 1)
echo "Installer Debian console md5: $(md5sum "$FILE" | awk '{print $1}')" >> distr/md5sums.txt

exit 0
