#!/bin/bash
set -e

cd /home/builder/workdir

VERSION=$(node -p "require('./package.json').version")


echo "--- Starting Arch build (console) ---"

export CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/zst/target"
export CARGO_HOME="/home/builder/workdir/bin/distr/cargo-home-shared"
node ./builder/gen-version.js console zst

cargo build -p console-app --release

# Disable LTO, debug packages, and stripping from makepkg to preserve exact binary MD5
echo "OPTIONS+=(!strip !lto !debug)" > ~/.makepkg.conf

mkdir -p /tmp/ice-commander-console-arch-build

# No gtk/adwaita/alsa — a single GTK-free terminal binary.
cat <<EOF > /tmp/ice-commander-console-arch-build/PKGBUILD
pkgname=ice-commander-console
pkgver=$VERSION
pkgrel=1
pkgdesc="Ice Commander — terminal file manager (dual-pane TUI, à la Midnight Commander)"
arch=('x86_64')
url="https://icecommander.com"
license=('MIT')
depends=()
source=()
sha256sums=()

package() {
    install -Dm755 "/home/builder/workdir/bin/distr/zst/target/release/ice-console" "\$pkgdir/usr/bin/ice-console"
}
EOF

cd /tmp/ice-commander-console-arch-build
PKGDEST="/home/builder/workdir/distr/" CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/zst/target" makepkg -cf
cd /home/builder/workdir


FILE=$(ls -t distr/ice-commander-console*.pkg.tar.zst | head -n 1)
echo "Installer Arch Linux console md5: $(md5sum "$FILE" | awk '{print $1}')" >> distr/md5sums.txt
