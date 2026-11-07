# gplstubs

Stand-ins for the libraries that the GTK stack drags into the desktop bundles and whose builds
carry GPL code. Each one keeps the original's file name, exports exactly the functions the
bundle imports from it, reports failure instead of doing the work, and contains none of the
original's code. Each README says when, if ever, the application reaches it, and how that was
established.

| Stand-in | Replaces | Why it is GPL | Arrives through | Bundles |
| --- | --- | --- | --- | --- |
| [fakelzo](fakelzo/) | liblzo2 | LZO is GPL-2.0-or-later | libgtk-4 → libcairo-script-interpreter | Windows, macOS |
| [fakejbig](fakejbig/) | libjbig | JBIG-KIT is GPL-2.0-or-later | libgtk-4 / libgdk_pixbuf → libtiff | Windows |
| [fakeidn2](fakeidn2/) | libidn2 | its build links gnulib's progname, GPL-3.0-or-later, into the library | libadwaita → libappstream → libcurl (→ libpsl) | Windows |

## How the build uses them

**Windows.** `builder/wine-msys-gtk.sh` runs `build-windows.sh` here, which builds every
stand-in into `bin/distr/gplstubs`, with no C runtime: each DLL holds its own source and
`dllmain.c` alone and imports nothing. `src/gtk-app/setup.nsi` excludes the originals from the
`File /r` over `artifacts/gtk4-win32-x64` and packs the stand-ins under the same names.
`builder/check-bundle-licenses.sh --strict` runs before NSIS and fails the build if an original
is not excluded, if a stand-in is missing, is not a 64-bit DLL or lacks a function the bundle
imports from it, or if any packed DLL declares GPL or carries gnulib's progname.

**macOS.** `builder/osx.sh` overwrites the originals that `dylibbundler` copied into the `.app`
with each stand-in's `build-macos.sh`, then refuses to sign if a real one survived. Only
liblzo2 arrives there today: Homebrew's libtiff links no libjbig, and AppStream uses macOS's
own curl.

**Linux and Android.** The Linux packages bundle no libraries, and the Android stack links
none of the three.

## What GPL code still ships

None without an exception that lets it ship on other terms. The Windows installer carries the
GCC runtime, `libgcc_s_seh-1.dll` and `libstdc++-6.dll`, and `ice-commander.exe` has parts of
it linked in, as every program GCC links does: GPL-3.0-or-later with the GCC Runtime Library
Exception, which lets programs compiled with GCC be distributed on their own terms. It cannot
be stubbed, because GTK, cairo, HarfBuzz and others really call it. GStreamer's pipeline
parser and gettext's plural-form parser in libintl are Bison output, under the Bison
exception, which lets each library carry them under its own licence; libintl brings one to
macOS as well. The GPL and GCC exception texts are in `assets/licenses/`.

## Adding one

1. Add the library's name to the list in `builder/check-bundle-licenses.sh`; in
   `src/gtk-app/setup.nsi` exclude it and pack `bin\distr\gplstubs\` under its name, next to
   the others. The check then reports the stand-in as missing and prints what the bundle
   imports from the library.
2. Copy a folder here and export exactly those functions, with the signatures from the
   library's public header and its failure codes.
3. If it reaches the macOS bundle too, add it to the replace loops and to the verification in
   `builder/osx.sh`.
4. Establish when, if ever, the application calls it, and write down how.
