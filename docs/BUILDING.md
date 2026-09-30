# Building from source

Rust **stable**, edition 2021. There is no pinned toolchain and no declared MSRV.
Node.js **20+** is needed only if you change the web interface.

`Cargo.lock` is not committed, so dependencies resolve fresh on first build.

Note that `.cargo/config.toml` sets `target-dir = "bin/target"` — build output lands in
`bin/target`, not `./target`.

This repository builds the application only. The plugins are built from their own
repositories, and a release packaged from here carries none of them yet.

## Linux

**Debian / Ubuntu**

```sh
sudo apt-get install -y \
    gcc g++ make pkg-config \
    libgtk-4-dev libadwaita-1-dev libasound2-dev
```

**Fedora**

```sh
sudo dnf install -y \
    gcc gcc-c++ make pkg-config \
    gtk4-devel libadwaita-devel alsa-lib-devel
```

**Arch**

```sh
sudo pacman -S --needed base-devel pkgconf gtk4 libadwaita alsa-lib
```

Then:

```sh
cargo build --release -p ice-commander-gtk     # desktop application
cargo build --release -p console-app           # terminal UI
cargo build --release -p webserver-app         # webserver application
```

The ALSA headers are for sound, which only the desktop binary has. The two headless
binaries need neither GTK nor ALSA — build them with `-p console-app` / `-p webserver-app`
and none of it is pulled in. Nothing here is needed for video: the application does not
decode it, see [Video](#video-libmpv-for-the-plugin) below.

## macOS

[Homebrew](https://brew.sh) is required — the build reads `brew --prefix` to find the GTK
stack and the GSettings schemas. Xcode Command Line Tools are needed for the C toolchain
(`xcode-select --install`).

```sh
brew install gtk4 libadwaita pkg-config glib
cargo build --release -p ice-commander-gtk
```

For a distributable `.app` bundle, two more tools:

```sh
cargo install cargo-bundle
brew install dylibbundler
npm run build-distr-osx
```

That builds the web UI, produces the bundle, copies and compiles the GTK GSettings schemas
into it, rewrites the dynamic library paths with `dylibbundler`, adds `libpdfium.dylib`
from `artifacts/` and signs the result ad-hoc. `builder/osx.sh` wraps the same steps and
also produces a `.dmg`.

Note that `.cargo/config.toml` adds Swift runtime rpaths for both `aarch64-apple-darwin`
and `x86_64-apple-darwin`, pointing at the Command Line Tools and Xcode toolchain
directories.

Releases are currently built on **macOS Sonoma, Apple Silicon**. Other versions are
untested.

## Windows (MSYS2 / MinGW-w64)

Build from an **MSYS2 MinGW64** shell:

```sh
pacman -S --needed base-devel mingw-w64-x86_64-toolchain \
    mingw-w64-x86_64-gtk4 mingw-w64-x86_64-libadwaita mingw-w64-x86_64-pkgconf
cargo build --release -p ice-commander-gtk
```

Use the `x86_64-pc-windows-gnu` Rust target. MSVC is not what the release builds use.

## Video (libmpv for the plugin)

The application plays no video and links nothing that decodes it. Video is the video
plugin's, built from its own repository, and the LGPL libmpv it carries is the same
component on Linux, macOS and Windows.

The macOS copy of that library is built by a script that still lives **in this
repository**. It builds nothing the application itself needs — it produces
`artifacts/lgpl-media`, the macOS libmpv the plugin is shipped with:

```sh
brew install meson ninja nasm pkg-config libplacebo dav1d libass little-cms2
./builder/build_libmpv_ffmpeg_lgpl.sh      # -> artifacts/lgpl-media
```

The Linux copy is built by `docker-libmpv-multibuild`, and Windows uses a prebuilt LGPL
`libmpv-2.dll`. Both are decode-only: mpv with `-Dgpl=false`, FFmpeg without
`--enable-gpl`.

## PDF support

PDF rendering uses PDFium, which is **not** built from this repository and is **not** in
it — `artifacts/` is gitignored. Place a prebuilt library there before building the
desktop application:

| Platform | File                                    |
| -------- | --------------------------------------- |
| Linux    | `artifacts/libpdfium.so`                |
| macOS    | `artifacts/libpdfium.dylib`             |
| Windows  | `artifacts/gtk4-win32-x64/…/pdfium.dll` |

Prebuilt binaries are published by the `pdfium-binaries` project. Without it the
application still builds; opening a PDF fails at runtime.

## Web interface

The browser UI lives in `src/web-app` (React + Vite). The built bundle is **committed** at
`src/gtk-app/assets/webui/bundle.js` and embedded into the binaries, so if you change the
frontend you must rebuild it or your change will not ship:

```sh
cd src/web-app && npm install && npm run build
```

The build script copies `bundle.js` and `style.css` into `src/gtk-app/assets/webui/`.

## Packaging

`builder/` holds the scripts that produce `.deb`, `.rpm`, Arch packages, a Windows
installer and a macOS `.app`. They expect their toolchains to be present and only drive
`cargo-deb`, `cargo-generate-rpm`, `makepkg`, `makensis` and `cargo-bundle`.


## Development

```sh
cargo test --workspace          # unit tests
cargo check --workspace         # must be warning-free
npm run licenses                # regenerate THIRD-PARTY-LICENSES.md
```

Contributions are accepted under the [DCO](../DCO) — sign your commits with `git commit -s`.
See [CONTRIBUTING.md](../CONTRIBUTING.md).

