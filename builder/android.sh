#!/bin/bash
# Builds the Android APK into distr/, the way the other builders put their installers there.
# Runs in the debian13-builder image (service apk of ice-commander-builder.yml), which carries the SDK,
# the NDK, the JDK and cargo-ndk.
set -e

cd "$(dirname "$0")/.."

VERSION=$(node -p "require('./package.json').version")
ABI=arm64-v8a

export ANDROID_HOME=/opt/android-sdk
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/27.0.12077973"
export JAVA_HOME=/opt/android-studio/jbr
export PATH="$JAVA_HOME/bin:/opt/rust/bin:$PATH"

# In the mounted tree, so the next container reuses the crates, the GTK stack and Gradle fetched before.
export CARGO_HOME="$PWD/bin/distr/cargo-home-shared"
export CARGO_TARGET_DIR="$PWD/bin/distr/apk/target"
export XDG_CACHE_HOME="$PWD/bin/distr/apk/cache"
export GRADLE_USER_HOME="$PWD/bin/distr/apk/gradle-home"
# The release is signed with the debug key; a new key per container would make every APK refuse to update the last.
export ANDROID_USER_HOME="$PWD/bin/distr/apk/android-user-home"

[ -d "$ANDROID_NDK_HOME" ] || { echo "no NDK at $ANDROID_NDK_HOME in this image" >&2; exit 1; }
command -v cargo-ndk >/dev/null || { echo "cargo-ndk is not in this image" >&2; exit 1; }
[ -x "$JAVA_HOME/bin/java" ] || { echo "no JDK at $JAVA_HOME in this image" >&2; exit 1; }

echo "--- Starting Android build ---"
echo "Rust version: $(rustc --version)"
echo "Java version: $(java -version 2>&1 | head -n 1)"

node ./builder/gen-version.js gui apk

# The git modes of these lose their exec bit, and build.sh runs fetch-gtk-stack.sh directly.
chmod +x src/gtk-app/android/fetch-gtk-stack.sh src/gtk-app/android/gradlew
bash ./src/gtk-app/android/build.sh release

(cd src/gtk-app/android && ./gradlew --no-daemon :app:assembleRelease)

BUILT="bin/android-gtk-app/outputs/apk/release/app-release.apk"
[ -f "$BUILT" ] || { echo "gradle produced no APK at $BUILT" >&2; exit 1; }

mkdir -p distr
NAMED="ice-commander-gtk-${VERSION}-${ABI}.apk"
cp "$BUILT" "distr/$NAMED"

echo "$(md5sum "distr/$NAMED" | awk '{print $1}') [GTK4-APK] $NAMED" >> distr/md5sums.txt
echo "  distr/$NAMED"

exit 0
