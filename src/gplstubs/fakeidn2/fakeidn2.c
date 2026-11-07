/*
 * Replacement for libidn2 in the Windows bundle.
 *
 * WHY THIS EXISTS
 * ---------------
 * libidn2 itself is LGPL-3.0-or-later OR GPL-2.0-or-later, but its build links the gnulib it
 * makes for its idn2 command-line program into the library as well, and with it gnulib's
 * progname module, which is GPL-3.0-or-later. Only the program calls progname; the MSYS2 DLL
 * this replaces carries it all the same. Nothing in Ice Commander asks for IDN; the DLL arrives
 * at the end of a chain of hard, load-time imports:
 *
 *     libadwaita -> libappstream -> libcurl -> libidn2
 *                                   libcurl -> libpsl -> libidn2
 *
 * libadwaita calls AppStream only for an About dialog built from appdata, and the application
 * builds no such dialog. Within the chain, curl imports four of the functions below and libpsl
 * one. curl converts a non-ASCII host name only after asking idn2_check_version, and turns
 * punycode back into Unicode only for curl_url_get with CURLU_PUNY2IDN, which AppStream does
 * not import. libpsl converts only the rules of a list loaded from a file; curl uses the
 * built-in one.
 *
 * WHAT IT DOES INSTEAD
 * --------------------
 * idn2_check_version accepts no requested version, so curl reports IDN as not built in. The
 * conversions report an encoding error and hand back no string, so nothing is ever allocated
 * here and idn2_free has nothing to free.
 *
 * TRADE-OFF, STATED PLAINLY
 * -------------------------
 * curl refuses a URL whose host name is not ASCII, and curl_url_get with CURLU_PUNY2IDN fails
 * for every host; libpsl keeps the non-ASCII rules of a list loaded from a file without their
 * ASCII form. If that ever matters, the honest fixes in order of preference are: rebuild curl
 * with its WinIDN backend and libpsl without an IDNA library, so that libidn2 leaves the
 * bundle; or patch libidn2's build so that the library links only the LGPL gnulib modules it
 * needs.
 *
 * The built file keeps the name libidn2-0.dll because curl and libpsl record that name in their
 * import tables; assets/licenses/BUNDLED-COMPONENTS.txt says what it actually is.
 *
 * No libidn2 code is used. Only the function signatures and one error code are shared, taken
 * from the public idn2.h interface.
 */
#include <stddef.h>
#include <stdint.h>

/* From idn2.h. */
#define IDN2_ENCODING_ERROR (-200)

/* NULL asks for the version string, which curl prints; any requested version is refused. */
const char *idn2_check_version(const char *req_version) {
    return req_version ? NULL : "fakeidn2";
}

int idn2_lookup_u8(const uint8_t *src, uint8_t **lookupname, int flags) {
    (void)src; (void)flags;
    if (lookupname) *lookupname = NULL;
    return IDN2_ENCODING_ERROR;
}

int idn2_lookup_ul(const char *src, char **lookupname, int flags) {
    (void)src; (void)flags;
    if (lookupname) *lookupname = NULL;
    return IDN2_ENCODING_ERROR;
}

int idn2_to_unicode_8z8z(const char *input, char **output, int flags) {
    (void)input; (void)flags;
    if (output) *output = NULL;
    return IDN2_ENCODING_ERROR;
}

/* Not free(): this DLL may link a different C runtime from its callers, and it never hands
   out memory, so the only pointer it can rightly be given is NULL. */
void idn2_free(void *ptr) { (void)ptr; }
