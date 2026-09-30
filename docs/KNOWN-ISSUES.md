# Known issues

This section is deliberately blunt. These are current limitations, not a roadmap.

## Security

- **A plugin runs with the application's own privileges.** It is native code loaded into
  the process: there is no sandbox, no permission prompt and nothing to limit it to the
  capability it declares. Install only plugins you trust.
- **What a connection does on the wire is the plugin's business, and the ones that ship
  are not careful.** SFTP never consults `known_hosts` and shows no fingerprint, FTP is
  plaintext with no FTPS, and WebDAV authenticates with HTTP Basic only. The passwords
  themselves are stored by the application, as below.
- **The web API has no authentication.** Every route of the headless server — including a
  live terminal WebSocket and read/write access to any path the process can reach — is
  open to whoever can reach the port. The only protection is the default `127.0.0.1` bind.
  `--host 0.0.0.0` warns and then serves everyone.
- **The default at-rest protection is a machine key**, derived from the machine id and the
  user name. It protects a config file copied to another machine; it does not protect
  against other software running as the same user. Set a master password if that matters.
- **Connection export is plaintext unless you give it a password.**

## Functionality

- **Delete is permanent** — there is no trash, and a multi-file delete stops at the first
  error rather than skipping.
- **Search and filter match filenames only** — a case-insensitive substring. No content
  search, no size or date filters, and no globs there: `*` and `?` work in the selection
  mask, not in search.
- **Drag and drop is inbound only.** Files can be dropped into a panel; they cannot be
  dragged out to another application.
- **Permissions are editable on local Unix and over SFTP only.** On Windows the dialog
  appears to work, reports invented modes and changes nothing.
- **Stars on saved connections did not survive the move to plugins.** Favourites were keyed
  by an FTP-shaped address and are now keyed by connection kind and name, so a star saved
  by an older version no longer matches and has to be set again.
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

## Platform

- **The registry editor is Windows-only**; there is no entry point elsewhere.
- **Video is a plugin, and the same one on every system.** The application decodes no
  video itself and no longer offers to install codecs for it. The video plugin plays
  through the LGPL libmpv it carries, on Linux, macOS and Windows alike. Without that
  plugin installed there is no video at all.
- **No GPL code ships on any platform.** The one GPL library GTK used to drag in,
  `liblzo2`, is replaced by our own MIT stub ([`src/fakelzo/`](../src/fakelzo/)) in both
  desktop bundles; Linux links nothing of the sort. The libmpv and FFmpeg that travel with
  the video plugin are built LGPL and decode-only, so none of the GPL parts (x264, x265,
  libpostproc) are present there either. This source tree stays MIT OR Apache-2.0 — see
  [THIRD-PARTY-LICENSES.md](../THIRD-PARTY-LICENSES.md).

## Incomplete

- **No release package carries a plugin yet.** The installers build the application alone,
  and the application looks for plugins in one folder under the user's data directory, so
  for now they are put there by hand. The settings page lists what a catalogue offers but
  cannot fetch anything — there is no download side to it yet.
- **The plugin interface has no terminal.** A plugin can bring a filesystem, a connection
  kind, a window, a panel, a toolbar and a viewer for a format — video is one — but the
  terminal is still inside the application.
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
