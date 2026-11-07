/*
 * Replacement for libjbig in the Windows bundle.
 *
 * WHY THIS EXISTS
 * ---------------
 * libjbig is JBIG-KIT 2.1, by Markus Kuhn, under the GPL; the DLL says "Licence: GPL" in its
 * own strings. Nothing in Ice Commander asks for it. It arrives at the end of a chain of hard,
 * load-time imports:
 *
 *     libgtk-4 / libgdk_pixbuf -> libtiff -> libjbig
 *
 * The MSYS2 build of gdk-pixbuf compiles TIFF support in, so libtiff is a load-time
 * dependency, and libtiff in turn imports ten symbols from libjbig. Neither DLL can simply
 * be dropped — the application would fail to start.
 *
 * libtiff calls these ten only for TIFF files whose Compression tag is JBIG (34661). That is
 * one of its fifteen codecs, and effectively nothing produces it: bi-level scans and faxes
 * use CCITT G3/G4, everything else uses LZW, Deflate, PackBits or JPEG. Our own code asks for
 * no codec: the viewer recognises a TIFF by its signature and hands it to gdk-pixbuf like any
 * other image.
 *
 * MEASURED, NOT ASSUMED
 * ---------------------
 * The same chain exists on Linux (libtiff.so.6 -> libjbig.so.0), so it was tested there: an
 * instrumented stand-in logging every call was preloaded ahead of the real library, and eight
 * TIFFs covering raw, LZW, Deflate, PackBits, JPEG and the three CCITT bi-level variants were
 * loaded through gdk-pixbuf. All eight decoded identically to the control run, and not one
 * jbg_* function was called. The CCITT cases matter most: they are the closest neighbours to
 * JBIG's purpose, and if libtiff touched this code for bi-level images it would show there.
 *
 * TRADE-OFF, STATED PLAINLY
 * -------------------------
 * A TIFF that really is JBIG-compressed will fail to open instead of displaying. If that ever
 * matters, the honest fixes in order of preference are: rebuild libtiff with --disable-jbig,
 * so neither library ships; or rebuild gdk-pixbuf without TIFF support, which drops the whole
 * branch. Both are MSYS2 package rebuilds.
 *
 * The built file keeps the name libjbig-0.dll because libtiff records that name in its import
 * table; assets/licenses/BUNDLED-COMPONENTS.txt says what it actually is.
 *
 * No JBIG-KIT code is used. Only the function signatures and one error code are shared, taken
 * from the public jbig.h interface.
 */
#include <stddef.h>

/* From jbig.h: error codes are shifted left by four bits. */
#define JBG_EINVAL (6 << 4)

/* Decoder. libtiff owns the state struct by value and this file never touches it. */
void jbg_dec_init(void *s) { (void)s; }
void jbg_dec_free(void *s) { (void)s; }

int jbg_dec_in(void *s, unsigned char *data, size_t len, size_t *cnt) {
    (void)s; (void)data; (void)len;
    if (cnt) *cnt = 0;
    return JBG_EINVAL;
}

unsigned long jbg_dec_getsize(const void *s) { (void)s; return 0; }

unsigned char *jbg_dec_getimage(const void *s, int plane) {
    (void)s; (void)plane;
    return NULL;
}

/* Encoder. jbg_enc_out is what would emit data; producing none leaves libtiff with an
   empty strip, and it reports the failure through its own error path. */
void jbg_enc_init(void *s, unsigned long x, unsigned long y, int planes,
                  unsigned char **p,
                  void (*data_out)(unsigned char *start, size_t len, void *file),
                  void *file) {
    (void)s; (void)x; (void)y; (void)planes; (void)p; (void)data_out; (void)file;
}

void jbg_enc_out(void *s) { (void)s; }
void jbg_enc_free(void *s) { (void)s; }

int jbg_newlen(unsigned char *bie, size_t len) {
    (void)bie; (void)len;
    return JBG_EINVAL;
}

/* libtiff prints this, so it must be a valid, permanently-live string. */
const char *jbg_strerror(int errnum) {
    (void)errnum;
    return "JBIG is not supported in this build";
}
