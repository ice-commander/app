# Contributing to Ice Commander

Thanks for taking the time to contribute.

## Developer Certificate of Origin (sign-off required)

This project does **not** use a CLA. Instead, every commit must carry a
`Signed-off-by` line, certifying the [Developer Certificate of Origin](DCO)
(the full text is in the `DCO` file at the repository root).

Git adds the line for you:

```sh
git commit -s -m "your message"
```

It looks like this, and the name and e-mail must match the commit author:

```
Signed-off-by: Jane Doe <jane@example.com>
```

To never forget it:

```sh
git config format.signoff true
```

Missing a sign-off on an existing commit? `git commit --amend -s` fixes the
last one; `git rebase --signoff <base>` fixes a whole branch. A CI check
enforces this on every pull request.

## Licensing of contributions

Unless you state otherwise, any contribution you submit is licensed under the
same terms as the project — **MIT OR Apache-2.0**, at the user's option. See
[LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

Note on binaries: the Windows and macOS release bundles link `libmpv`, which is
built under GPL-2.0-or-later, so those *bundles* are distributed under GPL-2+
terms even though this source tree is permissive. The Linux build does not use
libmpv at all. See [THIRD-PARTY-LICENSES.md](THIRD-PARTY-LICENSES.md).

The name "Ice Commander" and the project logo are not covered by the code
license.

## Getting the source

```sh
git clone https://github.com/ice-commander/app.git
```

## Building

Rust (stable) and Node.js are required, plus the GTK4 stack — `gtk4`,
`libadwaita`, and their development headers — for the desktop app.

```sh
npm run build-web-app   # the embedded web UI; must run before any cargo build
npm run build-gtk       # GTK desktop app
npm run build-console   # terminal client
npm run build-webserver # headless web server
```

Release packages (deb / rpm / zst / exe / dmg) are produced by the container
recipes in [builder/](builder/) — see `builder/build-all.sh`. The Windows and
macOS bundles additionally need prebuilt native libraries (GTK4 for MinGW,
PDFium, libmpv) which are not kept in this repository; Linux builds need
nothing extra.

## The one rule about background work

Most of the UI freezes in this project's history came from the same mistake, so
it is worth stating up front:

**When the GTK app performs many background operations, the whole loop goes
into one `tokio::task::spawn_blocking`, and the main thread awaits that single
`JoinHandle` exactly once.** Never `await` once per item on the GLib
main-thread executor — a single lost wakeup leaves the main thread asleep in
`ppoll` forever, and the app looks frozen with no error anywhere.

A "batched" API is not enough on its own: check that the implementation does
not await per item internally. If you hit a freeze, get evidence before
guessing — a backtrace of thread 1 (see below) says immediately whether the
main thread is stuck.

## Before you open a pull request

- Keep changes focused and prefer reusing existing abstractions over adding
  new ones.
- Run the tests that cover what you touched:

```sh
npm run unit-tests
```

  The integration and end-to-end suites live in a separate repository and are
  not part of this checkout.

## Reporting bugs

A good report includes the platform, the app version, and — most valuable —
concrete steps to reproduce. For freezes, a backtrace of the main thread helps
enormously:

```sh
gdb -p <PID> -batch -ex "thread 1" -ex "bt 25"
```
