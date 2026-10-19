#define _WIN32_WINNT 0x0A00
#include <windows.h>
#include <winternl.h>
#include <sddl.h>
#include <stdio.h>
#include <stdlib.h>
#include <wchar.h>

#define DISPOSITION_EX_CLASS ((FILE_INFO_BY_HANDLE_CLASS)21)
static HANDLE create_under(HANDLE parent,const wchar_t *name,int directory,int temporary) {
    UNICODE_STRING u={0};u.Buffer=(PWSTR)name;u.Length=(USHORT)(wcslen(name)*sizeof(wchar_t));u.MaximumLength=u.Length;
    PSECURITY_DESCRIPTOR security=NULL;
    if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(L"D:P(A;OICI;FA;;;OW)",SDDL_REVISION_1,&security,NULL))return INVALID_HANDLE_VALUE;
    OBJECT_ATTRIBUTES a={0};a.Length=sizeof(a);a.RootDirectory=parent;a.ObjectName=&u;a.SecurityDescriptor=security;
    IO_STATUS_BLOCK io={0};HANDLE h=NULL;
    NTSTATUS status=NtCreateFile(&h,GENERIC_READ|GENERIC_WRITE|DELETE|WRITE_DAC|SYNCHRONIZE,&a,&io,NULL,FILE_ATTRIBUTE_NORMAL,FILE_SHARE_READ,2,
        0x00200000|0x20|(directory?1:0x40)|(temporary?0x1000:0),NULL,0);
    LocalFree(security);
    if(status<0){fprintf(stderr,"NtCreateFile %ls status %08lx\n",name,(unsigned long)status);return INVALID_HANDLE_VALUE;}
    return h;
}
static int present(const wchar_t *path) {
    if(GetFileAttributesW(path)!=INVALID_FILE_ATTRIBUTES)return 1;
    DWORD e=GetLastError();if(e==ERROR_FILE_NOT_FOUND||e==ERROR_PATH_NOT_FOUND)return 0;
    fprintf(stderr,"observe %ls error %lu\n",path,e);return -1;
}
static int put(HANDLE f,const char *s) {DWORD n=0;return WriteFile(f,s,(DWORD)strlen(s),&n,NULL)&&n==strlen(s)&&FlushFileBuffers(f);}
static int matches(const wchar_t *path,const char *expected) {
    HANDLE f=CreateFileW(path,GENERIC_READ,7,NULL,OPEN_EXISTING,0,NULL);if(f==INVALID_HANDLE_VALUE)return 0;
    char b[64]={0};DWORD n=0;BOOL ok=ReadFile(f,b,sizeof(b),&n,NULL);CloseHandle(f);
    return ok&&n==strlen(expected)&&memcmp(b,expected,n)==0;
}
static int hold(const wchar_t *root) {
    HANDLE parent=CreateFileW(root,GENERIC_READ|GENERIC_WRITE,7,NULL,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,NULL);
    if(parent==INVALID_HANDLE_VALUE)return 21;
    HANDLE d=create_under(parent,L"mode-6",1,1);if(d==INVALID_HANDLE_VALUE)return 22;
    HANDLE f=create_under(d,L"child",0,1);if(f==INVALID_HANDLE_VALUE)return 23;
    if(!put(f,"temporary child"))return 24;
    HANDLE ready=create_under(parent,L"ready",0,0);if(ready==INVALID_HANDLE_VALUE||!put(ready,"ready"))return 25;
    CloseHandle(ready);Sleep(90000);return 26;
}
int wmain(int argc,wchar_t **argv) {
    if(argc==3&&wcscmp(argv[1],L"--hold")==0)return hold(argv[2]);
    if(argc!=2)return 2;
    HANDLE parent=CreateFileW(argv[1],GENERIC_READ|GENERIC_WRITE,7,NULL,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,NULL);
    if(parent==INVALID_HANDLE_VALUE)return 3;
    for(int mode=0;mode<6;mode++) {
        wchar_t name[32],path[1024],child[1024];swprintf(name,32,L"mode-%d",mode);swprintf(path,1024,L"%ls\\%ls",argv[1],name);swprintf(child,1024,L"%ls\\child",path);
        HANDLE d=create_under(parent,name,1,1);if(d==INVALID_HANDLE_VALUE)return 4;
        HANDLE f=INVALID_HANDLE_VALUE;int child_created=0,clear_ok=-1,stream_created=-1;
        if(mode==1||mode==2||mode==3||mode==4) {
            f=create_under(d,L"child",0,mode!=3);if(f==INVALID_HANDLE_VALUE)return 5;
            if(!put(f,mode==3?"preserve child":"temporary child"))return 6;
            child_created=1;
        }
        if(mode==4){FILE_DISPOSITION_INFO_EX info={FILE_DISPOSITION_FLAG_ON_CLOSE};clear_ok=SetFileInformationByHandle(d,DISPOSITION_EX_CLASS,&info,sizeof(info))?1:0;if(!clear_ok)return 7;}
        if(mode==5) {
            wchar_t stream[1050];swprintf(stream,1050,L"%ls:foreign",path);
            HANDLE ads=CreateFileW(stream,GENERIC_READ|GENERIC_WRITE,7,NULL,CREATE_NEW,0,NULL);
            stream_created=ads!=INVALID_HANDLE_VALUE;
            if(stream_created){if(!put(ads,"preserve stream"))return 8;CloseHandle(ads);}
            else {DWORD e=GetLastError();if(e!=ERROR_SHARING_VIOLATION&&e!=ERROR_ACCESS_DENIED)return 9;}
        }
        if(mode==2){CloseHandle(d);CloseHandle(f);}else {if(f!=INVALID_HANDLE_VALUE)CloseHandle(f);CloseHandle(d);}
        int dir_after=present(path),child_after=present(child);if(dir_after<0||child_after<0)return 10;
        int child_intact=mode==3?matches(child,"preserve child"):-1;
        int stream_intact=-1;if(mode==5&&stream_created){wchar_t stream[1050];swprintf(stream,1050,L"%ls:foreign",path);stream_intact=matches(stream,"preserve stream");}
        printf("{\"mode\":%d,\"child_created\":%d,\"clear_ok\":%d,\"stream_created\":%d,\"directory_after\":%d,\"child_after\":%d,\"child_content_intact\":%d,\"stream_content_intact\":%d}\n",mode,child_created,clear_ok,stream_created,dir_after,child_after,child_intact,stream_intact);fflush(stdout);
    }
    wchar_t exe[32768],cmd[34000],ready[1024],dir[1024],child[1024];
    if(!GetModuleFileNameW(NULL,exe,32768))return 11;
    swprintf(cmd,34000,L"\"%ls\" --hold \"%ls\"",exe,argv[1]);
    STARTUPINFOW si={0};si.cb=sizeof(si);PROCESS_INFORMATION pi={0};
    if(!CreateProcessW(exe,cmd,NULL,NULL,FALSE,CREATE_NO_WINDOW,NULL,NULL,&si,&pi))return 12;
    swprintf(ready,1024,L"%ls\\ready",argv[1]);swprintf(dir,1024,L"%ls\\mode-6",argv[1]);swprintf(child,1024,L"%ls\\child",dir);
    DWORD start=GetTickCount();
    while(present(ready)!=1) {if(GetTickCount()-start>10000||WaitForSingleObject(pi.hProcess,0)==WAIT_OBJECT_0){TerminateProcess(pi.hProcess,90);WaitForSingleObject(pi.hProcess,10000);CloseHandle(pi.hThread);CloseHandle(pi.hProcess);return 13;}Sleep(20);}
    if(!TerminateProcess(pi.hProcess,86)||WaitForSingleObject(pi.hProcess,10000)!=WAIT_OBJECT_0)return 14;
    DWORD code=0;GetExitCodeProcess(pi.hProcess,&code);CloseHandle(pi.hThread);CloseHandle(pi.hProcess);
    int d=present(dir),c=present(child);if(d<0||c<0||code!=86)return 15;
    printf("{\"mode\":6,\"hard_kill_exit\":%lu,\"directory_after\":%d,\"child_after\":%d}\n",code,d,c);
    HANDLE regular=create_under(parent,L"mode-7",0,1);if(regular==INVALID_HANDLE_VALUE||!put(regular,"temporary file"))return 16;
    wchar_t base[1024],stream[1050];swprintf(base,1024,L"%ls\\mode-7",argv[1]);swprintf(stream,1050,L"%ls:foreign",base);
    HANDLE ads=CreateFileW(stream,GENERIC_READ|GENERIC_WRITE,7,NULL,CREATE_NEW,0,NULL);int created=ads!=INVALID_HANDLE_VALUE;
    if(created){if(!put(ads,"preserve stream"))return 17;CloseHandle(ads);}else {DWORD e=GetLastError();if(e!=ERROR_SHARING_VIOLATION&&e!=ERROR_ACCESS_DENIED)return 18;}
    CloseHandle(regular);int regular_after=present(base);if(regular_after<0)return 19;
    printf("{\"mode\":7,\"stream_created\":%d,\"file_after\":%d,\"stream_content_intact\":%d}\n",created,regular_after,created?matches(stream,"preserve stream"):-1);
    CloseHandle(parent);return 0;
}
