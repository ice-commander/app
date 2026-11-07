# fakeidn2

A five-function stand-in for **libidn2**, so the Windows bundle carries no GPL code from it.

## Why

libidn2 is LGPL-3.0-or-later OR GPL-2.0-or-later, and the LGPL would do. But libidn2 builds
one gnulib for its `idn2` command-line program and links that same convenience library
(`gl/libgnu.la`, through `lib/Makefile.am`) into the library too, and with it gnulib's
**progname** module, which is GPL-3.0-or-later. That is upstream's build, so other
distributions' libidn2 carry it as well. Only the program calls progname; its message is in
MSYS2's `libidn2-0.dll` all the same:

```
A NULL argv[0] was passed through an exec system call.
```

Nothing in Ice Commander asks for IDN. The DLL arrives at the end of a chain of **hard,
load-time imports**:

```
libadwaita-1-0  ->  libappstream-5  ->  libcurl-4  ->  libidn2-0
                                        libcurl-4  ->  libpsl-5  ->  libidn2-0
```

libcurl imports four functions from it and libpsl one. None of these DLLs can be dropped from
the bundle — the application would not start.

## Traced and measured, not assumed

The import tables of the bundle show a single way in: `libcurl-4.dll` is imported only by
`libappstream-5.dll`, and `libappstream-5.dll` only by `libadwaita-1-0.dll`, which uses
AppStream for its About dialog built from appdata. The application builds no such dialog: the
0.8.144 `ice-commander.exe` imports no `adw_about_*` function.

Within that chain, curl converts a non-ASCII host name only after asking `idn2_check_version`,
and turns punycode back into Unicode only for `curl_url_get` with `CURLU_PUNY2IDN`, which
AppStream does not import. libpsl converts only the rules of a list loaded from a file, and
curl uses the built-in one.

Then it was run. Under Wine, with the 0.8.144 Windows bundle:

| | real libidn2 | fakeidn2 |
| --- | --- | --- |
| `curl_version()` | `libidn2/2.3.8` | `libidn2/fakeidn2` |
| curl's IDN feature | yes | no |
| ASCII URL: parse, then fetch from a closed port | host parsed; error 7, connection refused | the same |
| `http://bücher.invalid/` | error 6, host not resolved | error 4, not built in |
| `xn--bcher-kva.example` through `curl_url_get(…, CURLU_PUNY2IDN)` | `bücher.example` | error 21, as for any host |
| libpsl: `co.uk`, `example.co.uk`, `рф`, cookie domains | 1, 0, 1, as expected | identical |
| `LoadLibrary` of libappstream, libadwaita, libgtk-4 | ok | ok |

And the 0.8.144 application itself, started under Wine with a stand-in that writes every call
to a file, opened its window and ran for 75 seconds: **no call**. The same stand-in wrote its calls
when a test program drove curl directly, so the log was working.

## What it does

Exports the five functions the bundle imports — `idn2_check_version`, `idn2_free`,
`idn2_lookup_u8`, `idn2_lookup_ul`, `idn2_to_unicode_8z8z` — with the signatures from the
public `idn2.h` interface. `idn2_check_version` accepts no requested version, so curl reports
IDN as not built in. The conversions return `IDN2_ENCODING_ERROR` and hand back no string, so
nothing is ever allocated and `idn2_free` has nothing to free.

It contains no libidn2 code; only the function signatures and one error code are shared.

## Trade-off

curl refuses a URL whose host name is not ASCII, `curl_url_get` with `CURLU_PUNY2IDN` fails
for every host, and libpsl keeps the non-ASCII rules of a list loaded from a file without
their ASCII form. Nothing this application offers depends on any of them. If it ever matters,
the honest fixes in order of preference are:

1. Rebuild curl with its WinIDN backend and libpsl without an IDNA library: libidn2 then
   leaves the bundle. Both are MSYS2 package rebuilds of `artifacts/gtk4-win32-x64/`.
2. Patch libidn2's build so that the library links only the LGPL gnulib modules it needs.

## Building

**Windows** — `builder/wine-msys-gtk.sh` builds it with the other stand-ins, before NSIS runs:

```sh
./src/gplstubs/build-windows.sh          # writes bin/distr/gplstubs/libidn2-0.dll and the rest
```

Nothing is overwritten in place. `src/gtk-app/setup.nsi` excludes `libidn2-0.dll` from the
recursive `File /r` over `artifacts/gtk4-win32-x64` and packs ours instead, so the GPL code
never enters the installer archive.

**macOS** — nothing to replace today: AppStream uses macOS's own curl, so no libidn2 reaches
the bundle. `builder/osx.sh` would replace one with `build-macos.sh` and refuses to sign if a
real one survives.

**Linux** — nothing to do. The packages bundle no libraries.

## License

MIT.
