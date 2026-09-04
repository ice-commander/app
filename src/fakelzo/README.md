# fakelzo

A two-function stand-in for **liblzo2**, so the release bundles carry no GPL code.

## Why

Ice Commander is MIT OR Apache-2.0, but the desktop bundles used to be distributed under
GPL-2.0-or-later because of a single library nobody asked for. It arrives at the end of a
chain of optional dependencies:

```
libgtk-4  ->  libcairo-script-interpreter  ->  liblzo2   (GPL-2.0-or-later)
```

The interpreter imports exactly two symbols from it — `lzo2a_decompress` and
`lzo2a_999_compress` — and uses them only when serialising drawing operations to a cairo
script, which is GTK debugging machinery. The application never drives it.

That last sentence was **measured, not assumed**. A build was made with an instrumented
stand-in that printed to stderr on every call, then the application was exercised through a
full session: both panels, image preview, PDF preview, video playback, the editor, settings
and the terminal. Across ~2000 lines of log there were **zero calls**.

dyld still needs the library to exist, because the interpreter links it. So instead of
shipping someone else's GPL code for a code path that never runs, we ship these 40 lines.

## What it does

Exports the two symbols with the LZO signatures and reports failure (`LZO_E_ERROR`, output
length zero). It does not compress anything, and it contains no LZO code — the algorithm was
not reimplemented and no LZO source was consulted. Function signatures are not copyrightable;
this is the same approach that lets libedit stand in for readline.

The built file keeps the name `liblzo2.2.dylib` because the interpreter records that name in
its load commands. `assets/licenses/BUNDLED-COMPONENTS.txt` states plainly what the file
really is, so nobody inspecting the bundle is misled.

## Trade-off

Cairo-script serialisation does not work in a build that uses this. Nothing the application
offers depends on it. If that ever changes, there are two honest fixes, in order of
preference:

1. Build GTK with the script interpreter disabled — upstream supports it, the dependency is
   declared `required: false`, and then neither library ships at all.
2. Implement these two entry points over zlib. The interpreter both writes and reads the data
   itself, so any algorithm round-trips consistently; only the on-disk format would differ
   from real cairo scripts.

## Building

macOS — called automatically by `builder/osx.sh` after `dylibbundler` has populated
`Contents/Libs`, and before code signing:

```sh
./fakelzo/build-macos.sh <path-to-bundle>/Contents/Libs/liblzo2.2.dylib
```

It reads the install_name and version fields off the real library before overwriting it, so
the replacement keeps whatever Homebrew's current lzo advertises, then verifies both symbols
are present.

Windows — not done yet. The same treatment applies to
`artifacts/gtk4-win32-x64/liblzo2-2.dll`; it needs a mingw build of this source producing a
DLL that exports the same two symbols. Until then the Windows bundle remains GPL-2.0-or-later.

## License

MIT, like the rest of the project.
