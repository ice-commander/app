/* The entry point of the Windows builds, which link no C runtime: see build-windows.sh. */
#include <windows.h>

BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved) {
    (void)instance; (void)reason; (void)reserved;
    return TRUE;
}
