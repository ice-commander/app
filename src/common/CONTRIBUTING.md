# Contributing to common

Thanks for taking the time to contribute.

This repository holds the small pieces that belong to no single domain: the
application error type, the update package and its installer, the build version
stamp, and the app-wide network timeout. It is consumed as a path dependency, so a
change here reaches every application that embeds it.

Its defining property is that it **depends on nothing else of ours**. Anything
that needs the p2p protocol, the UI toolkit or a capability implementation
belongs in another repository, not here. Keep it that way — this is the bottom
of the dependency graph, and a dependency added here is a dependency added
everywhere.

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

| Crate | What it is |
| --- | --- |
| `common` | `AppError`, `UpdatePackage`, the update installer, the generated version stamp, the network timeout |
| `client-config` | Application preferences and their persistence |
| `network` | Platform networking helpers (Unix and Windows) |
| `utils` | Small odds and ends, including restarting the application |

## Getting the source and building

This repository has no workspace manifest of its own: the consuming project
defines the workspace. Clone it directly to work on it, or work inside a
project that already includes it.

```sh
git clone https://github.com/ice-commander/common.git
```

Rust (stable) is required and nothing else — no GTK, no platform SDKs. From a
consuming workspace:

```sh
cargo check -p common -p client-config -p ic-utils
cargo test -p common
```

## A note on `version.rs`

`common/src/version.rs` is **generated** — the build stamps the application
version into it from `package.json`. Do not hand-edit it, and do not be
surprised when a build leaves it modified.

## Before you open a pull request

- Keep changes focused and prefer reusing existing abstractions over adding new
  ones.
- Comments in English, and only where the code cannot speak for itself.
- Adding a dependency here affects every consumer. Say why in the pull request.

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

## Reporting bugs

A good report includes the platform, the crate, and — most valuable — concrete
steps to reproduce.
