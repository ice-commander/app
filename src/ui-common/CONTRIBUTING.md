# Contributing to ui-common

Thanks for taking the time to contribute.

This directory holds the GTK4/libadwaita components [Ice
Commander](https://icecommander.com) is built from: the file manager, the
terminal, the process and registry views, system info, the viewer, the updater
and the translation catalogue. They are separate crates so that each stays
independently testable.

The components are deliberately **thin**: they render and emit, and the host
application owns the transport. A file manager view does not know whether the
entries it shows came from the local disk, an FTP server or a WebDAV share — the host
answers that.

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

To never forget it, install a hook — once per clone. Note that
`git config format.signoff` does **not** do this; it only affects
`git format-patch`:

```sh
printf '%s\n' '#!/bin/sh' 'git interpret-trailers --in-place --if-exists doNothing --trailer "Signed-off-by: $(git config user.name) <$(git config user.email)>" "$1"' > .git/hooks/prepare-commit-msg
chmod +x .git/hooks/prepare-commit-msg
```

It reads `user.name` and `user.email` from git's config, runs for `git commit`
from any editor or GUI, and does not add a second line when you already
passed `-s`.

Missing a sign-off on an existing commit? `git commit --amend -s` fixes the
last one; `git rebase --signoff <base>` fixes a whole branch.

## Licensing of contributions

Unless you state otherwise, any contribution you submit is licensed under the
same terms as the project — **MIT OR Apache-2.0**, at the user's option. See
[LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).

## Crates

`fm-core` carries the `FileSystemRpc` trait every panel talks through, plus the
entry and path types; `fm-ui` is the file-manager view built on it, and
`client-archives` opens zip and tar archives as if they were folders.
`terminal-ui`, `process-ui`, `registry-ui`, `sysinfo-ui`, `graph-ui`,
`clipboard-ui`, `power-ui`, `os-services-ui`, `net-ui`, `sync-ui` and
`rdesk-ui` are one view each. `node-auth` handles signing in, `updater-ui` the
update prompt, and `i18n` holds the translation catalogue.

Not every consumer builds every crate — an application lists only the ones it
uses as workspace members, so an unused view costs it nothing.

## Getting the source and building

This repository has no workspace manifest of its own: the consuming project
defines the workspace. Clone it directly to work on it, or work inside a
project that already includes it.

```sh
git clone https://github.com/ice-commander/ui-common.git
```

Rust (stable) is required, plus the GTK4 stack — `gtk4`, `libadwaita` and their
development headers. The crates target relm4. From a consuming workspace, name
the packages you touched:

```sh
cargo check -p fm-core -p gtk-fm-ui
cargo test -p fm-core
```

## Before you open a pull request

- Keep changes focused and prefer reusing existing abstractions over adding new
  ones.
- Comments in English, and only where the code cannot speak for itself.
- **Keep the components thin.** If a view starts deciding where its data comes
  from, that decision belongs in the host application instead.
- User-visible strings go through `i18n`, not inline literals.

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Visual changes cannot be judged by the test suite. Say in the pull request what
you looked at and on which platform.

## Reporting bugs

A good report includes the platform, the crate, and — most valuable — concrete
steps to reproduce. For a freeze in a GTK application, a backtrace of the main
thread is the fastest evidence:

```sh
gdb -p <PID> -batch -ex "thread 1" -ex "bt 25"
```
