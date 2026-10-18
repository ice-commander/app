#!/usr/bin/env bash
# Fetches the GTK stack release (android-rust-gtk4-adwaita) that gtk-stack.lock pins into the cache all
# apps share, ${XDG_CACHE_HOME:-~/.cache}/android-rust-gtk4-adwaita/<version>/, and prints that folder.
#
#   fetch-gtk-stack.sh [--abi ABI]... [--print-dir] [LOCK]
#
# LOCK: default ./gtk-stack.lock. --abi: only this ABI (repeatable); default: every ABI the lock pins.
# --print-dir: no download; fails when an ABI is not there yet.
# stdout: the folder, the only line. stderr: what is downloaded, nothing when it is all there.
# The folder has the paths of a built checkout (GTK_STACK). .fetched-<abi> in it holds the SHA-256 of
# the archive the ABI came from; app/build.gradle.kts checks it against the lock.
set -euo pipefail

die() { echo "fetch-gtk-stack: $*" >&2; exit 1; }
usage() { sed -n '5,9p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }

LOCK=""
ABIS=()
FETCH=1
while [ $# -gt 0 ]; do
	case "$1" in
		--abi) [ $# -ge 2 ] || usage; ABIS+=("$2"); shift 2 ;;
		--abi=*) ABIS+=("${1#--abi=}"); shift ;;
		--print-dir) FETCH=0; shift ;;
		-*) usage ;;
		*) [ -z "$LOCK" ] || usage; LOCK="$1"; shift ;;
	esac
done
LOCK="${LOCK:-./gtk-stack.lock}"
[ -f "$LOCK" ] || die "no lock file $LOCK"

KNOWN_ABIS=(arm64-v8a armeabi-v7a)
VERSION=""
REPO=https://github.com/savchenko-igor/android-rust-gtk4-adwaita
declare -A SHA=()
while IFS= read -r line || [ -n "$line" ]; do
	line="${line%%#*}"
	[[ "$line" =~ ^[[:space:]]*$ ]] && continue
	[[ "$line" =~ ^[[:space:]]*([A-Za-z0-9._-]+)[[:space:]]*=[[:space:]]*([^[:space:]]+)[[:space:]]*$ ]] ||
		die "$LOCK: not key=value: $line"
	key="${BASH_REMATCH[1]}" value="${BASH_REMATCH[2]}"
	case "$key" in
		version) VERSION="$value" ;;
		repo) REPO="${value%/}" ;;
		sha256-*)
			[[ " ${KNOWN_ABIS[*]} " == *" ${key#sha256-} "* ]] || die "$LOCK: unknown ABI in $key"
			[[ "$value" =~ ^[0-9a-f]{64}$ ]] || die "$LOCK: $key is not a SHA-256"
			SHA["${key#sha256-}"]="$value"
			;;
		*) die "$LOCK: unknown key $key" ;;
	esac
done <"$LOCK"
[[ "$VERSION" =~ ^[0-9A-Za-z][0-9A-Za-z.+_-]*$ ]] || die "$LOCK: no version"
if [ ${#ABIS[@]} -eq 0 ]; then
	for abi in "${KNOWN_ABIS[@]}"; do
		[ -z "${SHA[$abi]:-}" ] || ABIS+=("$abi")
	done
	[ ${#ABIS[@]} -gt 0 ] || die "$LOCK pins no ABI (sha256-<abi>=…)"
fi
for abi in "${ABIS[@]}"; do
	[ -n "${SHA[$abi]:-}" ] || die "$LOCK pins no $abi (sha256-$abi=…)"
done

CACHE="${XDG_CACHE_HOME:-}"
[[ "$CACHE" == /* ]] || CACHE="$HOME/.cache"
CACHE="$CACHE/android-rust-gtk4-adwaita"
DIR="$CACHE/$VERSION"
TOP="android-rust-gtk4-adwaita-$VERSION"

# What an ABI needs in a stack folder: the pkg-config root to build against and the stage to package.
has_abi() {
	local dir="$1" abi="$2" arch stage got=""
	case "$abi" in
		arm64-v8a) arch=aarch64 stage=apk ;;
		armeabi-v7a) arch=armv7 stage=apk-armv7 ;;
	esac
	[ -f "$dir/bin/gtk-stack/$stage/jniLibs/$abi/libgtk-4.so" ] &&
		[ -d "$dir/bin/gtk-stack/root-$arch/lib/$abi/pkgconfig" ] || return 1
	[ "$dir" != "$DIR" ] || { read -r got <"$dir/.fetched-$abi" && [ "$got" = "${SHA[$abi]}" ]; } 2>/dev/null
}

missing() {
	MISSING=()
	local abi
	for abi in "${ABIS[@]}"; do
		has_abi "$DIR" "$abi" || MISSING+=("$abi")
	done
}

missing
if [ ${#MISSING[@]} -eq 0 ]; then
	echo "$DIR"
	exit 0
fi
[ "$FETCH" = 1 ] || die "no GTK stack $VERSION for ${MISSING[*]} in $DIR: run npm run fetch-gtk-stack"
for tool in curl sha256sum tar xz; do
	command -v "$tool" >/dev/null || die "needs $tool"
done

mkdir -p "$CACHE"
# One fetch at a time, so that two apps fetching different ABIs of a version keep both.
exec 9>"$CACHE/.lock"
if command -v flock >/dev/null; then
	flock 9
	rm -rf "$CACHE"/.tmp-*
	missing
	if [ ${#MISSING[@]} -eq 0 ]; then
		echo "$DIR"
		exit 0
	fi
fi

TMP="$(mktemp -d "$CACHE/.tmp-$VERSION.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT
trap 'exit 130' INT TERM HUP
PROGRESS=(-sS)
[ ! -t 2 ] || PROGRESS=(-#)

# Every archive is checked before anything is unpacked.
for abi in "${MISSING[@]}"; do
	asset="$TOP-$abi.tar.xz"
	url="$REPO/releases/download/v$VERSION/$asset"
	echo "fetch-gtk-stack: $url" >&2
	curl -fL --retry 3 --proto '=https' --proto-redir '=https' "${PROGRESS[@]}" -o "$TMP/$asset" "$url" ||
		die "cannot download $url"
	got="$(sha256sum "$TMP/$asset")"
	got="${got%% *}"
	[ "$got" = "${SHA[$abi]}" ] ||
		die "SHA-256 mismatch for $asset: $LOCK pins ${SHA[$abi]}, the download is $got"
done

# The new folder is built beside the old one (the ABIs already there as hard links) and swapped in.
NEW="$TMP/stack"
mkdir "$NEW"
[ ! -d "$DIR" ] || cp -al "$DIR/." "$NEW/"
for abi in "${MISSING[@]}"; do
	asset="$TOP-$abi.tar.xz"
	# tar replaces a file both archives have (it unlinks it first), so the old folder stays as it was.
	tar -xJf "$TMP/$asset" -C "$NEW" --strip-components=1 --no-same-owner "$TOP/" ||
		die "cannot unpack $asset"
	rm -f "$TMP/$asset"
	has_abi "$NEW" "$abi" || die "$asset has no $abi stack under $TOP/bin/gtk-stack"
	# A new file: the old one is a hard link into the live folder.
	rm -f "$NEW/.fetched-$abi"
	echo "${SHA[$abi]}" >"$NEW/.fetched-$abi"
done
if [ ! -d "$DIR" ]; then
	mv -T "$NEW" "$DIR"
elif ! mv -T --exchange "$NEW" "$DIR" 2>/dev/null; then
	# coreutils before 9.5: a moment without the folder.
	mv -T "$DIR" "$TMP/old"
	mv -T "$NEW" "$DIR"
fi
echo "fetch-gtk-stack: GTK stack $VERSION (${MISSING[*]}) in $DIR" >&2
echo "$DIR"
