/* Read-only registry transaction comparison for an owned Windows guest. */
#include <windows.h>
#include <appmodel.h>
#include <ktmw32.h>
#include <sddl.h>
#include <stdio.h>
#include <wchar.h>

static int print_caller(void)
{
    HANDLE token;
    union {
        TOKEN_USER user;
        BYTE bytes[sizeof(TOKEN_USER) + SECURITY_MAX_SID_SIZE];
    } user_buffer;
    TOKEN_ELEVATION elevation;
    DWORD size;
    WCHAR *sid = NULL;

    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &token))
        return 0;
    BOOL ok = GetTokenInformation(token, TokenUser, &user_buffer, sizeof(user_buffer), &size)
        && GetTokenInformation(token, TokenElevation, &elevation, sizeof(elevation), &size);
    CloseHandle(token);
    if (!ok || !ConvertSidToStringSidW(user_buffer.user.User.Sid, &sid))
        return 0;
    wprintf(L"user_sid=%ls elevated=%lu\n", sid, (unsigned long)elevation.TokenIsElevated);
    LocalFree(sid);
    return !elevation.TokenIsElevated;
}

static void probe_key(const WCHAR *path, REGSAM access)
{
    HKEY key;
    LONG ordinary = RegOpenKeyExW(HKEY_CURRENT_USER, path, 0, access, &key);
    if (ordinary == ERROR_SUCCESS)
        RegCloseKey(key);
    HANDLE transaction = CreateTransaction(NULL, NULL, 0, 0, 0, 0, NULL);
    if (transaction == INVALID_HANDLE_VALUE) {
        DWORD error = GetLastError();
        wprintf(L"HKCU\\%ls access=0x%lx ordinary=%ld create_transaction_error=%lu\n",
                path, (unsigned long)access, ordinary, (unsigned long)error);
        return;
    }
    LONG transacted = RegOpenKeyTransactedW(HKEY_CURRENT_USER, path, 0, access,
                                            &key, transaction, NULL);
    if (transacted == ERROR_SUCCESS)
        RegCloseKey(key);
    CloseHandle(transaction);
    wprintf(L"HKCU\\%ls access=0x%lx ordinary=%ld transacted=%ld\n",
            path, (unsigned long)access, ordinary, transacted);
}

int wmain(int argc, WCHAR **argv)
{
    WCHAR package[256];
    UINT32 size = ARRAYSIZE(package);
    LONG result = GetCurrentPackageFullName(&size, package);
    if (argc != 2 || (wcscmp(argv[1], L"packaged") != 0 &&
                      wcscmp(argv[1], L"unpackaged") != 0))
        return 2;
    LONG expected = wcscmp(argv[1], L"packaged") == 0 ? ERROR_SUCCESS : APPMODEL_ERROR_NO_PACKAGE;
    if (result != expected) {
        fwprintf(stderr, L"Unexpected package identity result: %ld\n", result);
        return 3;
    }
    wprintf(L"package=%ls\n", result == ERROR_SUCCESS ? package : L"(none)");
    if (!print_caller()) {
        fwprintf(stderr, L"Expected an ordinary user token\n");
        return 4;
    }

    const WCHAR *keys[] = {
        L"Software",
        L"Software\\Classes",
        L"Software\\Ottrin\\DesktopIntegration",
        L"Software\\Microsoft\\Windows\\CurrentVersion\\Run",
    };
    for (size_t i = 0; i < ARRAYSIZE(keys); ++i) {
        probe_key(keys[i], KEY_QUERY_VALUE);
        probe_key(keys[i], KEY_QUERY_VALUE | KEY_WOW64_64KEY);
    }
    return 0;
}
