# What it does

The front page is [README.md](../README.md); this is the long version,
including how plugins fit in.

## Panels

Two independent panels, each with its own tabs, history and view mode. Details view with
sortable columns (name, size, date, permissions), grid view with thumbnails, and a
configurable row height. Sorting is remembered per panel. Tab, `Alt+F1`/`Alt+F2` and the
usual function keys work the way a two-pane manager is expected to work: `F3` view, `F4`
edit, `F5` copy, `F6` move, `F7` new folder, `F8` delete.

Copy and move run between panels regardless of what each side is — local to SFTP, archive
to WebDAV — with progress, per-file skip and retry.

## Finding and selecting

Four different things, because they answer four different questions:

|                                                |                                                                                                                                            |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| **Recursive search** (`Ctrl+S`, also `Alt+F7`) | walks the tree below the current directory and lists every match in a results panel. Works on remote filesystems too, not only local ones. |
| **Quick filter** (`Ctrl+F`)                    | narrows the current listing as you type. Nothing is hidden permanently — `Esc` restores the full list.                                     |
| **Selection by mask** (`*`)                    | opens a mask bar prefilled with `*.*`; `Enter` selects every matching entry. Supports `*` and `?`, case-insensitive.                       |
| **Type-ahead jump**                            | typing any printable character moves the cursor to the next entry starting with it.                                                        |

Search and filter both match on the file name as a case-insensitive substring — see
[Known issues](KNOWN-ISSUES.md) for what that does not cover.

## Connections

Saved connections live in one dialog (`Ctrl+N`): grouping into folders, drag to reorder,
and export and import of the whole set as one file. Which kinds exist there — FTP, SFTP,
WebDAV, or whatever else is installed — and what each one asks for is decided by the
plugins, not by this repository: the application shows the form the plugin describes and
stores the answers.

## Archives

With the `archives` plugin installed, ZIP, TAR, TAR.GZ and TAR.BZ2 open as directories —
you navigate into them, read files out of them and view their contents, including an image
inside an archive that lives on a WebDAV server. Nesting works to any depth, and each level
opens on top of whatever holds it, so a ZIP inside a TAR.GZ on a WebDAV server opens like
anything else. Packing and extracting are both the panel's own copy: copy out of an open
archive to unpack it, and copy into a destination named `work.zip` to pack — the archive is
created if it is not there yet. The same holds for a file added to an archive that already
exists, or one deleted from it.

## Viewer and editor

One window with pluggable format handlers (`src/ui-common/viewer-ui`):

- **Text** with encoding detection, and a **hex mode** with per-byte editing, cursor
  tracking and go-to-offset — the only format the application still reads itself, and
  what anything nobody claims falls back to

Everything else is a plugin, each in its own repository, and the window is the same one:

- **Images** (`plugin-images`), and camera RAW through the embedded preview
  (`plugin-cameraraw`)
- **PDF** (`plugin-pdf`) with lazy page rendering and zoom
- **Audio** (`plugin-audioplayer`) with a playlist of the current folder, cover art and
  ID3 tags
- **Video** (`plugin-video`), which draws into a canvas the application hands it and
  decodes through its own LGPL libmpv

The viewer works over every filesystem, not only the local one: a remote or in-archive
file is fetched before it is shown, with a size warning and a cancellable load.

## Terminal, processes, system

Each panel has its own embedded terminal, opened under the file list and following that
panel's current directory. `Alt+Return` expands it to fill the panel and collapses it
again, so the same window is either a file manager or a full terminal depending on what
you are doing.

On a local panel the terminal is your shell. On a panel standing in a filesystem whose
plugin carries a shell — SFTP does — it is a shell on that server instead, opened through
the same login and starting in the directory the panel shows.

The process list with kill and the system-information window are plugins
(`processes`, `sysinfo`); the registry editor is still in the application, on Windows.

## Web UI and headless mode

`ice-commander --headless --webui` starts an HTTP server and serves a React interface that
mirrors the desktop panels. `ice-webserver` is the same interface without any GTK
dependency at all — useful on a machine with no desktop session.

## Security of stored credentials

Connection passwords are encrypted at rest with XChaCha20-Poly1305 under a random data key
(`src/secret-store`). That key is itself wrapped — by default with a key derived from the
machine and the user account, or, if you set a master password, with Argon2id. Changing
the master password rewrites one small keyring file, not every stored secret. The store
fails closed: if it cannot encrypt, the secret is dropped rather than written in the clear.

Read the [Known issues](KNOWN-ISSUES.md) section before relying on this.

## Localisation

15 languages: English, Polish, Czech, Slovak, German, Spanish, Ukrainian,
Italian, French, Romanian, Hungarian, Belarusian, Russian, Bulgarian, Serbian. Catalogues are JSON
files compiled into the binaries; every language has the complete key set.

A plugin brings its own catalogue and teaches it to the application when it loads, so its
words are translated the same way and a plugin can be translated without touching this
repository.


## Plugins

A plugin is one shared library — `.so`, `.dll` or `.dylib` — that the application loads at
startup from `<data directory>/ice-commander/plugins` (`IC_PLUGIN_DIR` overrides the
folder). Nothing loads by itself: a library is only opened once it is switched on in
**Settings → Plugins**, which is the `plugins.enabled` list in the configuration, and the
change takes effect on the next start.

The boundary is plain C, with no Rust types crossing it, so a plugin built against an older
release keeps working: the table of functions the application offers only ever grows at the
end, and a plugin says which part of it it needs. Nothing draws: a plugin *describes* what
it offers — a filesystem, a connection kind and the form behind it, a window, a panel, a
column, a toolbar button, an icon — and whichever frontend is running builds that, GTK or
the browser, from the same description.

| Repository | What is in it |
| --- | --- |
| [`plugins`](https://github.com/ice-commander/plugins) | the plugins that ship with a release: archives, FTP, SFTP, WebDAV, processes, system information, developer tools |
| [`ice-commander-plugin-sdk`](https://github.com/savchenko-igor/ice-commander-plugin-sdk) | the interface itself, documented, with small worked examples to start from |

Two more are being moved out of what used to be separate builds of the whole application
and are not published yet: `node-in-net` (peer-to-peer between your own machines) and
`torrent` (downloading straight into a panel).


## Peer-to-peer

Point the second panel at another machine you own — the `node-in-net` plugin adds a
node.in.net account, finds your devices and moves files straight between them, with the
same two panels and the same keys. It used to be a separate build of the whole
application; it is a plugin now, and the forks it needed are gone.

Want it to do something else entirely? Write a plugin, or fork it. That is what the
licence is for.
