#include <windows.h>
__declspec(dllexport) UINT __stdcall FailAfterExecution(UINT handle) {
    (void)handle;
    return 1603;
}
BOOL WINAPI DllMain(HINSTANCE instance, DWORD reason, LPVOID reserved) {
    (void)instance;
    (void)reason;
    (void)reserved;
    return TRUE;
}
