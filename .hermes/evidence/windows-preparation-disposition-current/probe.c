#define _WIN32_WINNT 0x0A00
#include <windows.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <wchar.h>

/* MinGW gates this SDK enum behind a malformed version threshold. The
 * Windows FILE_INFO_BY_HANDLE_CLASS value is also used by windows-sys. */
#define DISPOSITION_EX_CLASS ((FILE_INFO_BY_HANDLE_CLASS)21)

static BOOL rename_held(HANDLE file, const wchar_t *path) {
    size_t bytes = wcslen(path) * sizeof(wchar_t);
    FILE_RENAME_INFO *info = calloc(1, offsetof(FILE_RENAME_INFO, FileName) + bytes);
    if (!info) return FALSE;
    info->FileNameLength = (DWORD)bytes;
    memcpy(info->FileName, path, bytes);
    BOOL ok = SetFileInformationByHandle(file, FileRenameInfo, info,
        (DWORD)(offsetof(FILE_RENAME_INFO, FileName) + bytes));
    DWORD error = ok ? 0 : GetLastError();
    free(info);
    SetLastError(error);
    return ok;
}
static int exists(const wchar_t *path) { return GetFileAttributesW(path) != INVALID_FILE_ATTRIBUTES; }
int wmain(int argc, wchar_t **argv) {
    if (argc != 2) return 2;
    for (int mode = 0; mode < 5; mode++) {
        wchar_t source[1024], target[1024];
        if (swprintf(source, 1024, L"%ls\\source-%d", argv[1], mode) < 0 ||
            swprintf(target, 1024, L"%ls\\promoted-%d", argv[1], mode) < 0) return 3;
        HANDLE file = CreateFileW(source, GENERIC_READ | GENERIC_WRITE | DELETE,
            FILE_SHARE_READ, NULL, CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | (mode ? FILE_FLAG_DELETE_ON_CLOSE : 0), NULL);
        if (file == INVALID_HANDLE_VALUE) { fprintf(stderr,"create mode %d error %lu\n",mode,GetLastError()); return 4; }
        DWORD wrote;
        if (!WriteFile(file,"claim",5,&wrote,NULL) || wrote != 5 || !FlushFileBuffers(file)) { CloseHandle(file); return 5; }
        BOOL clear_ok = TRUE; DWORD clear_error = 0;
        if (mode == 2) {
            FILE_DISPOSITION_INFO info = {FALSE};
            clear_ok = SetFileInformationByHandle(file,FileDispositionInfo,&info,sizeof(info));
        } else if (mode == 3) {
            FILE_DISPOSITION_INFO_EX info = {FILE_DISPOSITION_FLAG_ON_CLOSE};
            clear_ok = SetFileInformationByHandle(file,DISPOSITION_EX_CLASS,&info,sizeof(info));
        }
        if (!clear_ok) clear_error = GetLastError();
        BOOL renamed = rename_held(file,target);
        DWORD rename_error = renamed ? 0 : GetLastError();
        if (mode == 4) {
            FILE_DISPOSITION_INFO_EX info = {FILE_DISPOSITION_FLAG_ON_CLOSE};
            clear_ok = SetFileInformationByHandle(file,DISPOSITION_EX_CLASS,&info,sizeof(info));
            if (!clear_ok) clear_error = GetLastError();
        }
        const wchar_t *bound = renamed ? target : source;
        HANDLE other = CreateFileW(bound,GENERIC_READ | GENERIC_WRITE | DELETE,FILE_SHARE_READ,NULL,OPEN_EXISTING,0,NULL);
        DWORD contender_error = other == INVALID_HANDLE_VALUE ? GetLastError() : 0;
        if (other != INVALID_HANDLE_VALUE) CloseHandle(other);
        BOOL closed = CloseHandle(file);
        printf("{\"mode\":%d,\"clear_ok\":%s,\"clear_error\":%lu,\"renamed\":%s,\"rename_error\":%lu,\"contender_error\":%lu,\"closed\":%s,\"source_exists\":%s,\"target_exists\":%s}\n",
            mode,clear_ok?"true":"false",clear_error,renamed?"true":"false",rename_error,contender_error,closed?"true":"false",exists(source)?"true":"false",exists(target)?"true":"false");
        fflush(stdout);
    }
    return 0;
}
