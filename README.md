# Ice Commander

A dual-pane file manager for the desktop, written in Rust with GTK4 and libadwaita.

Two panels side by side, keyboard-first, with the local disk, FTP, SFTP, WebDAV and
archives all behaving the same way — the same navigation, the same copy, the same viewer.

![Ice Commander dual-pane file manager ](docs/main-window.png)

The repository builds three binaries from one workspace:

| Binary          | What it is                                                 |
| --------------- | ---------------------------------------------------------- |
| `ice-commander` | the GTK desktop application                                |
| `ice-console`   | a terminal UI, no GTK linked                               |
| `ice-webserver` | a headless server that serves the same panels to a browser |

Licensed under **MIT OR Apache-2.0**.

### Looking for peer-to-peer?

Point the second panel at another machine you own. **[Ice Commander Node.In.Net
mod](https://github.com/ice-commander/app-nn)** adds a node.in.net account, finds your
devices and moves files straight between them — same two panels, same keys, nothing new to
learn. It installs alongside this build instead of replacing it, so you can keep both.

Want it to do something else entirely? Fork it. That is what the licence is for.

---

## Features

### Panels

Two independent panels, each with its own tabs, history and view mode. Details view with
sortable columns (name, size, date, permissions), grid view with thumbnails, and a
configurable row height. Sorting is remembered per panel. Tab, `Alt+F1`/`Alt+F2` and the
usual function keys work the way a two-pane manager is expected to work: `F3` view, `F4`
edit, `F5` copy, `F6` move, `F7` new folder, `F8` delete.

Copy and move run between panels regardless of what each side is — local to SFTP, archive
to WebDAV — with progress, per-file skip and retry.

### Finding and selecting

Four different things, because they answer four different questions:

|                                                |                                                                                                                                            |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| **Recursive search** (`Ctrl+S`, also `Alt+F7`) | walks the tree below the current directory and lists every match in a results panel. Works on remote filesystems too, not only local ones. |
| **Quick filter** (`Ctrl+F`)                    | narrows the current listing as you type. Nothing is hidden permanently — `Esc` restores the full list.                                     |
| **Selection by mask** (`*`)                    | opens a mask bar prefilled with `*.*`; `Enter` selects every matching entry. Supports `*` and `?`, case-insensitive.                       |
| **Type-ahead jump**                            | typing any printable character moves the cursor to the next entry starting with it.                                                        |

Search and filter both match on the file name as a case-insensitive substring — see
[Known issues](#known-issues) for what that does not cover.

### Remote filesystems

| Protocol | Library                    |
| -------- | -------------------------- |
| FTP      | `suppaftp`                 |
| SFTP     | `ssh2` (libssh2, vendored) |
| WebDAV   | `reqwest`                  |

Connections are managed in one dialog (`Ctrl+N`): grouping into folders, drag to reorder,
per-connection remote path, SFTP password or key file with passphrase, and an optional SSH
tunnel with its own credentials. The whole set can be exported and imported as one file.

SFTP and FTP sessions are kept alive and reconnected on failure rather than dialled per
operation.

### Archives

ZIP, TAR, TAR.GZ and TAR.BZ2 open as directories — you navigate into them, read files out
of them and view their contents, including an image inside an archive that lives on a
WebDAV server. Nesting works to any depth, and each level opens on top of whatever holds
it, so a ZIP inside a TAR.GZ on a WebDAV server opens like anything else. The contents of
an open archive are read-only; the context menu creates new ZIP, TAR.GZ and TAR.BZ2
archives from the selection.

### Viewer and editor

One window with pluggable format handlers (`src/ui-common/viewer-ui`):

- **Text** with encoding detection, and a **hex mode** with per-byte editing, cursor
  tracking and go-to-offset
- **Images**, including camera RAW through the embedded preview
- **PDF** with lazy page rendering and zoom
- **Audio** with a playlist of the current folder, cover art and ID3 tags
- **Video** in its own window

The viewer works over every filesystem, not only the local one: a remote or in-archive
file is fetched before it is shown, with a size warning and a cancellable load.

### Terminal, processes, system

Each panel has its own embedded terminal, opened under the file list and following that
panel's current directory. `Alt+Return` expands it to fill the panel and collapses it
again, so the same window is either a file manager or a full terminal depending on what
you are doing.

On a panel that is connected over **SFTP the terminal is a shell on that server** — it
launches `ssh`, changes into the directory the panel is showing and starts your login
shell there. If the connection is configured with an SSH tunnel, the already-authenticated
tunnel is reused instead of opening a second one. On a local panel it is your normal shell.

Also a process list with kill, system information, and on Windows a registry editor.

### Web UI and headless mode

`ice-commander --headless --webui` starts an HTTP server and serves a React interface that
mirrors the desktop panels. `ice-webserver` is the same interface without any GTK
dependency at all — useful on a machine with no desktop session.

### Security of stored credentials

Connection passwords are encrypted at rest with XChaCha20-Poly1305 under a random data key
(`src/secret-store`). That key is itself wrapped — by default with a key derived from the
machine and the user account, or, if you set a master password, with Argon2id. Changing
the master password rewrites one small keyring file, not every stored secret. The store
fails closed: if it cannot encrypt, the secret is dropped rather than written in the clear.

Read the [Known issues](#known-issues) section before relying on this.

### Localisation

15 languages: English, Polish, Czech, Slovak, German, Spanish, Ukrainian,
Italian, French, Romanian, Hungarian, Belarusian, Russian, Bulgarian, Serbian. Catalogues are JSON
files compiled into the binaries; every language has the complete key set.

---

## Building

Rust **stable**, edition 2021. There is no pinned toolchain and no declared MSRV.
Node.js **20+** is needed only if you change the web interface.

`Cargo.lock` is not committed, so dependencies resolve fresh on first build.

Note that `.cargo/config.toml` sets `target-dir = "bin/target"` — build output lands in
`bin/target`, not `./target`. It also sets `LIBSSH2_SYS_USE_PKG_CONFIG = "0"`, so libssh2
is compiled from bundled sources and you do not need a system libssh2; you do need a C
toolchain.

### Linux

**Debian / Ubuntu**

```sh
sudo apt-get install -y \
    gcc g++ make cmake pkg-config \
    libgtk-4-dev libadwaita-1-dev libdbus-1-dev libssl-dev \
    libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev
```

**Fedora**

```sh
sudo dnf install -y \
    gcc gcc-c++ make cmake pkg-config \
    gtk4-devel libadwaita-devel dbus-devel openssl-devel \
    gstreamer1-devel gstreamer1-plugins-base-devel
```

**Arch**

```sh
sudo pacman -S --needed base-devel cmake pkgconf gtk4 libadwaita dbus openssl
```

Then:

```sh
cargo build --release -p ice-commander-gtk     # desktop application
cargo build --release -p console-app           # terminal UI
cargo build --release -p webserver-app         # webserver application
```

The GStreamer packages are for video playback and for the codec check the viewer does
before opening a video. The two headless binaries need neither GTK nor GStreamer — build
them with `-p console-app` / `-p webserver-app` and none of it is pulled in.

### macOS

[Homebrew](https://brew.sh) is required — the build reads `brew --prefix` to find the GTK
stack and the GSettings schemas. Xcode Command Line Tools are needed for the C toolchain
(`xcode-select --install`).

```sh
brew install gtk4 libadwaita pkg-config glib
cargo build --release -p ice-commander-gtk
```

Video playback uses libmpv here rather than GStreamer:

```sh
brew install mpv
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

### Windows (MSYS2 / MinGW-w64)

Build from an **MSYS2 MinGW64** shell:

```sh
pacman -S --needed base-devel mingw-w64-x86_64-toolchain \
    mingw-w64-x86_64-gtk4 mingw-w64-x86_64-libadwaita mingw-w64-x86_64-pkgconf
cargo build --release -p ice-commander-gtk
```

Use the `x86_64-pc-windows-gnu` Rust target. MSVC is not what the release builds use.

### PDF support

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

### Web interface

The browser UI lives in `src/web-app` (React + Vite). The built bundle is **committed** at
`src/gtk-app/assets/webui/bundle.js` and embedded into the binaries, so if you change the
frontend you must rebuild it or your change will not ship:

```sh
cd src/web-app && npm install && npm run build
```

The build script copies `bundle.js` and `style.css` into `src/gtk-app/assets/webui/`.

### Packaging

`builder/` holds the scripts that produce `.deb`, `.rpm`, Arch packages, a Windows
installer and a macOS `.app`. They expect their toolchains to be present and only drive
`cargo-deb`, `cargo-generate-rpm`, `makepkg`, `makensis` and `cargo-bundle`.

---

## Development

```sh
cargo test --workspace          # unit tests
cargo check --workspace         # must be warning-free
npm run licenses                # regenerate THIRD-PARTY-LICENSES.md
```

Contributions are accepted under the [DCO](DCO) — sign your commits with `git commit -s`.
See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## Known issues

This section is deliberately blunt. These are current limitations, not a roadmap.

### Security

- **No SSH host-key verification.** The SFTP provider and the SSH tunnel authenticate
  immediately after the handshake; `known_hosts` is never consulted and no fingerprint is
  shown. SFTP connections are trivially interceptable on a hostile network.
- **The web API has no authentication.** Every route of the headless server — including a
  live terminal WebSocket and read/write access to any path the process can reach — is
  open to whoever can reach the port. The only protection is the default `127.0.0.1` bind.
  `--host 0.0.0.0` warns and then serves everyone.
- **FTP is plaintext only.** FTPS is not enabled, so credentials and data cross the wire in
  the clear. WebDAV authenticates with HTTP Basic only.
- **The default at-rest protection is a machine key**, derived from the machine id and the
  user name. It protects a config file copied to another machine; it does not protect
  against other software running as the same user. Set a master password if that matters.
- **Connection export is plaintext unless you give it a password.**

### Functionality

- **Delete is permanent** — there is no trash, and a multi-file delete stops at the first
  error rather than skipping.
- **Search and filter match filenames only** — a case-insensitive substring. No content
  search, no size or date filters, and no globs there: `*` and `?` work in the selection
  mask, not in search.
- **Drag and drop is inbound only.** Files can be dropped into a panel; they cannot be
  dragged out to another application.
- **Compress and extract work on local files only.** The menu entries appear everywhere but
  fail with "not implemented" on FTP, SFTP, WebDAV and inside archives.
- **Permissions are editable on local Unix and SFTP only.** On Windows the dialog appears
  to work, reports invented modes and changes nothing.
- **File associations apply to Enter and double-click, not to F3/F4.** `F3` always opens
  the built-in viewer.
- **Handing a remote file to an external application does not write changes back**, and for
  the system default handler the temporary copy is intentionally leaked, so temp files
  accumulate.
- **Session restore keeps one local path per panel.** A session that ended inside a remote
  connection or an archive reopens at the nearest local ancestor; extra tabs are not
  restored.
- **The process panel takes a full system snapshot on every refresh**, which is visible as
  a hitch on machines with many processes.
- **Terminal colours have no settings UI** — they are config-file keys only.
- **The application checks for updates at every launch** unless `--no-check-update` is
  passed. There is no setting for it.

### Platform

- **The registry editor is Windows-only**; there is no entry point elsewhere.
- **Video plays through GStreamer on Linux and libmpv on Windows and macOS.** On Linux,
  missing codecs are detected and the application offers to install them.
- **libmpv is GPL-licensed.** The Windows and macOS bundles that ship it are distributed
  under GPL-2.0-or-later as a whole. The Linux build links no GPL code. This source tree
  stays MIT OR Apache-2.0 — see [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md).

### Incomplete

- **The console UI copies files only** — directory copy and move are not implemented. Its
  viewer refuses files over 8 MB, its editor over 4 MB, and it is English-only regardless
  of the language setting.
- **The headless server is a first version**: no tab model (tab operations are accepted and
  ignored), copy and move read the whole file into memory and do not recurse into
  directories, and it always binds `127.0.0.1`.
- **In GTK mode the web UI is a remote control, not an independent client.** Every browser
  and the desktop window show the same directory; you cannot browse elsewhere in the tab.
- **Several dialogs are hard-coded English** despite the 15 locales: permissions/chmod, the
  overwrite prompt, the transfer-error dialog, the terminal context menu and part of the
  help. They are marked `TODO(i18n)` in the source.
- **Integration tests are not in this repository.** Only in-crate unit tests are here; the
  suites that drive the built binaries live in a separate repository.
