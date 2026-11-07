# fakejbig

A ten-function stand-in for **libjbig**, so the Windows bundle carries no GPL code from it.

## Why

Ice Commander is MIT OR Apache-2.0, but the Windows installer would otherwise carry
GPL-2.0-or-later code from a library nobody asked for. The DLL identifies itself:

```
JBIG-KIT 2.1 -- (c) 1995-2014 Markus Kuhn -- Licence: GPL
```

It arrives at the end of a chain of **hard, load-time imports**:

```
libgtk-4-1 / libgdk_pixbuf-2.0-0  ->  libtiff-6  ->  libjbig-0
```

The MSYS2 build of gdk-pixbuf compiles TIFF support in (and ships the TIFF loader module as
well), so `libtiff-6.dll` is a load-time dependency, and libtiff imports ten symbols from
`libjbig-0.dll`. Neither can be dropped from the bundle — the application would not start.

**TIFF opens; JBIG inside it is what nothing uses.** The viewer
([`src/ui-common/viewer-ui/src/content.rs`](../../ui-common/viewer-ui/src/content.rs))
recognises a TIFF by its signature and hands it to gdk-pixbuf like any other image, so a TIFF
reaches libtiff, and libtiff reaches this library for one compression only.

JBIG is a compression scheme for bi-level (1-bit) images, meant for faxes and scanned
documents. libtiff calls into it only when a TIFF's Compression tag is JBIG (34661) — one of
its fifteen codecs, and one that effectively nothing produces. Bi-level scans use CCITT
G3/G4; everything else uses LZW, Deflate, PackBits or JPEG.

## Measured, not assumed

The same chain exists on Linux (`libtiff.so.6 → libjbig.so.0`), so it was tested there. An
instrumented stand-in logging every call was preloaded ahead of the real library, and eight
TIFFs were loaded through gdk-pixbuf:

| Compression | control | with the stand-in |
| --- | --- | --- |
| raw, LZW, Deflate, PackBits, JPEG | loads | loads |
| CCITT G3, CCITT G4, CCITT RLE (1-bit) | loads | loads |

All eight decoded identically, and **not one `jbg_*` function was called**. The CCITT cases
are the ones that matter: they are the closest neighbours to JBIG's purpose, so if libtiff
reached this code for bi-level images at all, it would show there.

## What it does

Exports the ten symbols libtiff imports — `jbg_dec_init`, `jbg_dec_in`, `jbg_dec_getsize`,
`jbg_dec_getimage`, `jbg_dec_free`, `jbg_enc_init`, `jbg_enc_out`, `jbg_enc_free`,
`jbg_newlen`, `jbg_strerror` — with the signatures from the public `jbig.h` interface. The
decoder entry points report `JBG_EINVAL`, the encoder emits nothing, and `jbg_strerror`
returns a static explanatory string. libtiff owns the state structs by value and this code
never writes to them.

It contains no JBIG-KIT code: the algorithm was not reimplemented. Only the function
signatures and one error code are shared, taken from the public `jbig.h` interface.

## Trade-off

A TIFF that really is JBIG-compressed fails to open instead of displaying. Nothing this
application offers depends on that. If it ever matters, the honest fixes in order of
preference are:

1. Rebuild libtiff with `--disable-jbig` — then neither library ships at all.
2. Rebuild gdk-pixbuf without TIFF support, dropping the whole branch.

Both are MSYS2 package rebuilds of `artifacts/gtk4-win32-x64/`.

## Building

**Windows** — `builder/wine-msys-gtk.sh` builds it with the other stand-ins, before NSIS runs:

```sh
./src/gplstubs/build-windows.sh          # writes bin/distr/gplstubs/libjbig-0.dll and the rest
```

Nothing is overwritten in place. `src/gtk-app/setup.nsi` excludes `libjbig-0.dll` from the
recursive `File /r` over `artifacts/gtk4-win32-x64` and packs ours instead, so the GPL
library never enters the installer archive. `artifacts/` is not in the repository, and the
MSYS2 tree it holds does contain the real DLL: the exclusion is what keeps it out.

**macOS** — nothing here, and deliberately so. Homebrew's libtiff is not built against
jbigkit, so `dylibbundler` has nothing to collect; the 0.8.144 DMG has no libjbig. If one ever
turns up, `builder/osx.sh` replaces it with `build-macos.sh` and refuses to sign if the real
library survives.

**Linux** — nothing to do. The `deb`, `rpm` and `zst` packages take the imaging stack from
the distribution and bundle no libraries at all.

## License

MIT.
