#!/bin/bash
set -e

cd /home/builder/workdir

VERSION=$(node -p "require('./package.json').version")

export CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/zst/target"
export CARGO_HOME="/home/builder/workdir/bin/distr/cargo-home-shared"
node ./builder/gen-version.js gui zst

cargo build -p ice-commander-gtk --release
cd ./src/gtk-app
# Disable LTO, debug packages, and stripping from makepkg to preserve exact binary MD5
echo "OPTIONS+=(!strip !lto !debug)" > ~/.makepkg.conf

# Extract version from package.json
mkdir -p /tmp/ice-commander-arch-build

cat <<EOF > /tmp/ice-commander-arch-build/PKGBUILD
pkgname=ice-commander
pkgver=$VERSION
pkgrel=1
pkgdesc="Ice Commander - Dual-Pane P2P File Manager"
arch=('x86_64')
url="https://icecommander.com"
license=('MIT')
depends=('gtk4' 'libadwaita' 'alsa-lib')
source=()
sha256sums=()

package() {
    install -Dm755 "/home/builder/workdir/bin/distr/zst/target/release/ice-commander" "\$pkgdir/usr/bin/ice-commander"
    install -Dm644 "/home/builder/workdir/src/gtk-app/assets/com.icecommander.gtkapp.desktop" "\$pkgdir/usr/share/applications/com.icecommander.gtkapp.desktop"
    install -Dm644 "/home/builder/workdir/src/gtk-app/assets/app-logo-512.png" "\$pkgdir/usr/share/icons/hicolor/512x512/apps/com.icecommander.gtkapp.png"
    install -Dm644 "/home/builder/workdir/artifacts/libpdfium.so" "\$pkgdir/usr/lib/ice-commander/libpdfium.so"
    for lic in /home/builder/workdir/assets/licenses/*.txt; do
        install -Dm644 "\$lic" "\$pkgdir/usr/share/doc/ice-commander/licenses/\$(basename "\$lic")"
    done
}
EOF

cd /tmp/ice-commander-arch-build
PKGDEST="/home/builder/workdir/distr/" CARGO_TARGET_DIR="/home/builder/workdir/bin/distr/zst/target" makepkg -cf
cd /home/builder/workdir


FILE=$(ls -t distr/*.pkg.tar.zst | head -n 1)
echo "$(md5sum "$FILE" | awk '{print $1}') Arch Linux gtk4" >> distr/md5sums.txt
