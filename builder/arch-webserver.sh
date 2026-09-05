#!/bin/bash
set -e

cd /home/builder/workdir

VERSION=$(node -p "require('./package.json').version")


echo "--- Starting Arch build (webserver) ---"

export CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/zst/target"
export CARGO_HOME="/home/builder/workdir/bin/distr/cargo-home-shared"
node ./builder/gen-version.js webserver zst

cargo build -p webserver-app --release

# Disable LTO, debug packages, and stripping from makepkg to preserve exact binary MD5
echo "OPTIONS+=(!strip !lto !debug)" > ~/.makepkg.conf

mkdir -p /tmp/ice-commander-webserver-arch-build

# No gtk/adwaita/alsa — a single GTK-free web-server binary.
cat <<EOF > /tmp/ice-commander-webserver-arch-build/PKGBUILD
pkgname=ice-commander-webserver
pkgver=$VERSION
pkgrel=1
pkgdesc="Ice Commander — headless web-server (dual-pane file manager, browser is the client)"
arch=('x86_64')
url="https://icecommander.com"
license=('MIT')
depends=()
source=()
sha256sums=()

package() {
    install -Dm755 "/home/builder/workdir/bin/distr/zst/target/release/ice-webserver" "\$pkgdir/usr/bin/ice-webserver"
}
EOF

cd /tmp/ice-commander-webserver-arch-build
PKGDEST="/home/builder/workdir/distr/" CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/zst/target" makepkg -cf
cd /home/builder/workdir


FILE=$(ls -t distr/ice-commander-webserver*.pkg.tar.zst | head -n 1)
echo "$(md5sum "$FILE" | awk '{print $1}') Arch Linux webserver" >> distr/md5sums.txt
