/*
 * Replacement for liblzo2 in the macOS bundle.
 *
 * WHY THIS EXISTS
 * ---------------
 * The bundle's only GPL-2.0-or-later component used to be liblzo2 (LZO, by Markus Oberhumer).
 * Nothing in Ice Commander asks for it; it arrives through a chain of optional dependencies:
 *
 *     libgtk-4 -> libcairo-script-interpreter -> liblzo2
 *
 * libcairo-script-interpreter imports exactly two symbols from it, lzo2a_decompress and
 * lzo2a_999_compress, and uses them only when serialising drawing operations to a cairo
 * script — GTK debug machinery the application never drives. That was verified empirically:
 * a build with a shouting stand-in logged ZERO calls across a full session (panels, image and
 * PDF preview, video playback, editor, settings, terminal).
 *
 * Since dyld still needs the library to exist at load time (the interpreter links it), the
 * options were to rebuild GTK without the script interpreter, or to supply these two symbols
 * ourselves. This file does the latter: same names, same signatures, no LZO code, MIT-licensed
 * like the rest of the project. The GPL obligation on the bundle goes away with it.
 *
 * TRADE-OFF, STATED PLAINLY
 * -------------------------
 * These functions do not compress anything — they report failure. If some future GTK version
 * starts exercising the cairo-script path, that feature degrades instead of working. If that
 * ever matters, the honest fixes in order of preference are: build GTK with the script
 * interpreter disabled (upstream supports it, the dependency is `required: false`), or
 * implement these two entry points over zlib, which round-trips consistently because the
 * interpreter both writes and reads the data itself.
 *
 * The bundled file keeps the name liblzo2.2.dylib because the interpreter records that name;
 * assets/licenses/BUNDLED-COMPONENTS.txt says what it actually is.
 */
#include <stddef.h>

#define LZO_E_ERROR (-1)

int lzo2a_decompress(const unsigned char *src, size_t src_len,
                     unsigned char *dst, size_t *dst_len, void *wrkmem) {
    (void)src; (void)src_len; (void)dst; (void)wrkmem;
    if (dst_len) *dst_len = 0;
    return LZO_E_ERROR;
}

int lzo2a_999_compress(const unsigned char *src, size_t src_len,
                       unsigned char *dst, size_t *dst_len, void *wrkmem) {
    (void)src; (void)src_len; (void)dst; (void)wrkmem;
    if (dst_len) *dst_len = 0;
    return LZO_E_ERROR;
}
