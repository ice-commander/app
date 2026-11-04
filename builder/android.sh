#!/bin/bash
# Builds the Android APK into distr/, the way the other builders put their installers there.
#
# Unlike them this one runs on the host rather than in a docker image: the NDK and the GTK
# stack for Android are too large to carry in one, and cargo-ndk wants them both. A machine
# without them is told so and the rest of the build carries on.
set -e

cd "$(dirname "$0")/.."

VERSION=$(node -p "require('./package.json').version")
ABI=arm64-v8a

export ANDROID_HOME="${ANDROID_HOME:-/opt/android-sdk}"
export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$ANDROID_HOME/ndk/27.0.12077973}"
export JAVA_HOME="${JAVA_HOME:-/opt/android-studio/jbr}"

if [ ! -d "$ANDROID_NDK_HOME" ]; then
    echo "no NDK at $ANDROID_NDK_HOME — skipping the Android build" >&2
    exit 0
fi
if ! command -v cargo-ndk >/dev/null; then
    echo "cargo-ndk is not installed (cargo install cargo-ndk) — skipping the Android build" >&2
    exit 0
fi
if [ ! -x "$JAVA_HOME/bin/java" ]; then
    echo "no JDK at $JAVA_HOME — skipping the Android build" >&2
    exit 0
fi

echo "--- Starting Android build ---"
./src/gtk-app/android/build.sh release

(cd src/gtk-app/android && ./gradlew :app:assembleRelease)

BUILT="bin/android-gtk-app/outputs/apk/release/app-release.apk"
[ -f "$BUILT" ] || { echo "gradle produced no APK at $BUILT" >&2; exit 1; }

mkdir -p distr
NAMED="ice-commander-gtk-${VERSION}-${ABI}.apk"
cp "$BUILT" "distr/$NAMED"

echo "$(md5sum "distr/$NAMED" | awk '{print $1}') [GTK4-APK] $NAMED" >> distr/md5sums.txt
echo "  distr/$NAMED"

exit 0
