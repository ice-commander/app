#!/bin/bash
set -e

# Builds the macOS media stack — FFmpeg + libmpv — as LGPL, decode-only, into
# artifacts/lgpl-media. Run once per machine (or after bumping the versions below);
# builder/osx.sh then links the app against it instead of Homebrew's.
#
# WHY: Homebrew's ffmpeg is configured with
#     --enable-gpl --enable-version3 --enable-libx264 --enable-libx265
# so a bundle that picks it up (dylibbundler pulls whatever the binary links) must be
# distributed under GPL-3.0. Ice Commander only ever DECODES video, and every decoder it
# needs — H.264, HEVC, AV1, VP8/VP9, MPEG-2/4, AAC, MP3, Opus, Vorbis, FLAC, AC3 — is LGPL.
# The GPL parts of FFmpeg are x264/x265 (ENCODERS) and libpostproc, none of which a viewer uses.
#
# Result: FFmpeg reports "License: LGPL version 2.1 or later", mpv's config.h has HAVE_GPL 0.
#
# REQUIREMENTS (checked below, so a missing one fails immediately rather than mid-build):
#   brew install meson ninja nasm pkg-config libplacebo dav1d libass little-cms2
# plus the Xcode command line tools for clang. The mpv and FFmpeg sources are downloaded by
# this script; everything they link against comes from Homebrew.

FFMPEG_VERSION="8.1.2"          # matches Homebrew's, so mpv is happy with the ABI
MPV_TAG="v0.41.0"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
APP_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
PREFIX="$APP_DIR/artifacts/lgpl-media"
WORK="${TMPDIR:-/tmp}/ice-commander-lgpl-media"

# This script is NOT self-contained: it fetches the mpv and FFmpeg sources itself, but the
# libraries they link against come from Homebrew. Check for everything up front — otherwise the
# failure surfaces halfway through a long build, as a meson or configure error that does not
# obviously name the missing package.
MISSING_TOOLS=""
for tool in meson ninja nasm pkg-config; do
    command -v "$tool" >/dev/null || MISSING_TOOLS="$MISSING_TOOLS $tool"
done
if [ -n "$MISSING_TOOLS" ]; then
    echo "missing build tools:$MISSING_TOOLS" >&2
    echo "  brew install$MISSING_TOOLS" >&2
    exit 1
fi

# libplacebo is mpv's renderer and is mandatory; dav1d is the AV1 decoder FFmpeg is configured
# with below; libass draws embedded subtitles; lcms2 does colour management. The pkg-config
# names differ from the formula names, hence the pairs.
MISSING_LIBS=""
for pair in "libplacebo:libplacebo" "dav1d:dav1d" "libass:libass" "lcms2:little-cms2"; do
    pc="${pair%%:*}"; formula="${pair##*:}"
    PKG_CONFIG_PATH="$(brew --prefix)/lib/pkgconfig:$(brew --prefix)/share/pkgconfig" \
        pkg-config --exists "$pc" 2>/dev/null || MISSING_LIBS="$MISSING_LIBS $formula"
done
if [ -n "$MISSING_LIBS" ]; then
    echo "missing libraries:$MISSING_LIBS" >&2
    echo "  brew install$MISSING_LIBS" >&2
    exit 1
fi

command -v clang >/dev/null || { echo "clang not found — install the Xcode command line tools"; exit 1; }

mkdir -p "$WORK"
rm -rf "$PREFIX"

# ── FFmpeg ───────────────────────────────────────────────────────────────────────────────
# No --enable-gpl and no --enable-version3: those are what pull in the GPL components.
# --disable-encoders/--disable-muxers drop the write side entirely; videotoolbox keeps
# hardware decoding, which is part of FFmpeg itself and not GPL.
# NOTE: there is no --disable-postproc in FFmpeg 8.x — libpostproc simply is not built
# without --enable-gpl.
#
# --enable-libdav1d is NOT optional: FFmpeg's built-in "av1" decoder is hardware-accelerated only
# (av1dec.c ships hw_configs and no software path), so without dav1d an AV1 file plays only where
# VideoToolbox can decode it in hardware and shows nothing anywhere else. dav1d is BSD-2-Clause,
# ~0.8 MB, and is the DECODER — not to be confused with libSvtAv1Enc, the encoder, which we do
# want gone.
echo "=== Building LGPL FFmpeg $FFMPEG_VERSION ==="
cd "$WORK"
[ -d "ffmpeg-$FFMPEG_VERSION" ] || {
    curl -sL -O "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz"
    tar xf "ffmpeg-$FFMPEG_VERSION.tar.xz"
}
cd "ffmpeg-$FFMPEG_VERSION"
./configure \
    --prefix="$PREFIX" \
    --enable-shared --disable-static \
    --disable-programs --disable-doc --disable-debug \
    --disable-encoders --disable-muxers \
    --disable-devices --disable-avdevice \
    --enable-videotoolbox --enable-audiotoolbox \
    --enable-libdav1d \
    --enable-pic
make -j"$(sysctl -n hw.ncpu)"
make install

# ── mpv ──────────────────────────────────────────────────────────────────────────────────
# -Dlibavdevice=disabled matters: mpv would otherwise find Homebrew's libavdevice (there is
# none in our prefix) and drag the GPL FFmpeg back in alongside our LGPL one.
echo "=== Building LGPL libmpv $MPV_TAG ==="
cd "$WORK"
[ -d mpv ] || git clone --depth 1 --branch "$MPV_TAG" https://github.com/mpv-player/mpv.git mpv
cd mpv
rm -rf build
# Everything below the first four options is mpv's optional feature set, which meson enables by
# "auto" detection — that quietly linked our libmpv against eight extra Homebrew libraries
# (luajit, mujs, libbluray, rubberband, uchardet, zimg, vulkan, …) that a file manager's video
# preview never uses. Disabling them explicitly keeps the dependency surface to libplacebo (a hard
# requirement of mpv's renderer) and libass (embedded subtitles), and shrinks the bundle.
PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:$(brew --prefix)/lib/pkgconfig:$(brew --prefix)/share/pkgconfig" \
    meson setup build \
        --prefix="$PREFIX" \
        -Dgpl=false \
        -Dlibmpv=true \
        -Dcplayer=false \
        -Dbuild-date=false \
        -Dlibavdevice=disabled \
        -Dlua=disabled \
        -Djavascript=disabled \
        -Dlibbluray=disabled \
        -Drubberband=disabled \
        -Duchardet=disabled \
        -Dzimg=disabled \
        -Dvulkan=disabled \
        -Dcdda=disabled \
        -Ddvdnav=disabled \
        -Dsdl2-audio=disabled \
        -Dsdl2-video=disabled \
        -Dsdl2-gamepad=disabled \
        -Dx11=disabled \
        -Dgl-x11=disabled \
        -Dx11-clipboard=disabled \
        -Djpeg=disabled
ninja -C build
ninja -C build install

# ── verify ───────────────────────────────────────────────────────────────────────────────
echo "=== Verifying ==="
grep -q "#define HAVE_GPL 0" "$WORK/mpv/build/config.h" \
    || { echo "mpv was NOT built in LGPL mode"; exit 1; }
if otool -L "$PREFIX/lib/libmpv.2.dylib" | grep -qE "$(brew --prefix)/.*libav"; then
    echo "libmpv links Homebrew's FFmpeg — the GPL build leaked in"; exit 1
fi
echo "OK: libmpv is LGPL and links only $PREFIX"
ls -lh "$PREFIX/lib/"*.dylib | awk '{printf "  %6s  %s\n", $5, $NF}'
