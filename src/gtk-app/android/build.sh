#!/usr/bin/env bash
# Builds the client (src/gtk-app) as libice_commander_gtk.so against the GTK stack cross-built by
# android-rust-gtk4-adwaita, into bin/android-gtk-jniLibs/<abi>/, where the Gradle module picks it up.
#
#   src/gtk-app/android/build.sh [release|dev|check|clippy]
#
# release: the release profile (fat LTO); dev: android-dev (thin LTO), for iterating on a device.
# GTK_STACK: that repository, relative to ice-commander/app (default: a sibling of ice-commander/).
set -euo pipefail

CMD="${1:-release}"
ABI=arm64-v8a
TRIPLE=aarch64-linux-android
HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$(cd "$HERE/../../.." && pwd)"
GTK_STACK="$(cd "$APP" && cd "${GTK_STACK:-../../android-rust-gtk4-adwaita}" && pwd)"
ROOT="$GTK_STACK/bin/gtk-stack/root-aarch64"
[ -d "$ROOT/lib/$ABI/pkgconfig" ] || { echo "No $ROOT — build the GTK stack in $GTK_STACK first." >&2; exit 1; }

# Scoped to the target: the build scripts (glib-build-tools) run on the host with the host's pkg-config.
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_SYSROOT_DIR_aarch64_linux_android="$ROOT"
export PKG_CONFIG_LIBDIR_aarch64_linux_android="$ROOT/lib/$ABI/pkgconfig"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-${ANDROID_HOME:-/opt/android-sdk}/ndk/27.0.12077973}"
export PATH="$HOME/.cargo/bin:$PATH"

cd "$APP"
case "$CMD" in
	release|dev)
		PROFILE=$([ "$CMD" = release ] && echo release || echo android-dev)
		# The crate is a library and the desktop binary; here only the library, as a cdylib. Its audio
		# (rodio → cpal → oboe) is C++, so libc++_shared.so goes beside it.
		cargo ndk --platform 31 -t "$ABI" --link-libcxx-shared rustc -p ice-commander-gtk --lib --crate-type cdylib --profile "$PROFILE"
		OUT="$APP/bin/android-gtk-jniLibs/$ABI"
		mkdir -p "$OUT"
		cp "$APP/bin/target/$TRIPLE/$PROFILE/libice_commander_gtk.so" "$OUT/"
		cp "$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/sysroot/usr/lib/$TRIPLE/libc++_shared.so" "$OUT/"
		;;
	*) cargo ndk --platform 31 -t "$ABI" "$CMD" -p ice-commander-gtk --lib ;;
esac
