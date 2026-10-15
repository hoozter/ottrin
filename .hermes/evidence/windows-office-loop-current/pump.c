#include <stdbool.h>
#include <assert.h>
#include <stdio.h>
#include <windows.h>
#include <shlwapi.h>
#include <tlhelp32.h>
#include <LibreOfficeKit/LibreOfficeKit.h>
static char *utf8(const wchar_t *text) {
 int n=WideCharToMultiByte(CP_UTF8,0,text,-1,NULL,0,NULL,NULL);
 char *result=(char*)malloc(n); WideCharToMultiByte(CP_UTF8,0,text,-1,result,n,NULL,NULL);return result;
}
static LONG CALLBACK diagnostic_exception(EXCEPTION_POINTERS *info) {
 if(info->ExceptionRecord->ExceptionCode != EXCEPTION_STACK_OVERFLOW) return EXCEPTION_CONTINUE_SEARCH;
 fprintf(stderr,"Stack overflow at %p\n",info->ExceptionRecord->ExceptionAddress);
 void *frames[32];USHORT n=CaptureStackBackTrace(0,32,frames,NULL);
 for(USHORT i=0;i<n;i++) {
  HMODULE module=NULL;wchar_t name[1024];
  if(GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,(LPCWSTR)frames[i],&module)) {
   GetModuleFileNameW(module,name,1024);
   fprintf(stderr,"frame %hu %ls +0x%llx\n",i,name,(unsigned long long)((char*)frames[i]-(char*)module));
  }
 }
 fflush(stderr);return EXCEPTION_CONTINUE_SEARCH;
}
static BOOL CALLBACK owned_window(HWND window, LPARAM unused) {
 (void)unused;wchar_t text[2048]={0};DWORD_PTR result=0;
 SendMessageTimeoutW(window,WM_GETTEXT,2048,(LPARAM)text,SMTO_ABORTIFHUNG,200,&result);
 if(text[0]) fprintf(stderr,"Owned window text: %ls\n",text);
 return TRUE;
}
static BOOL CALLBACK owned_root(HWND window, LPARAM unused) {
 owned_window(window,unused);EnumChildWindows(window,owned_window,0);return TRUE;
}
static DWORD WINAPI owned_window_diagnostic(void *unused) {
 (void)unused;Sleep(5000);
 HANDLE snapshot=CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD,0);
 if(snapshot==INVALID_HANDLE_VALUE) {fprintf(stderr,"Owned window snapshot error=%lu\n",GetLastError());return 0;}
 THREADENTRY32 entry={0};entry.dwSize=sizeof(entry);
 if(Thread32First(snapshot,&entry)) do {
  if(entry.th32OwnerProcessID==GetCurrentProcessId()) EnumThreadWindows(entry.th32ThreadID,owned_root,0);
 } while(Thread32Next(snapshot,&entry));
 CloseHandle(snapshot);fflush(stderr);return 0;
}

static DWORD WINAPI owned_deadline(void *unused) {
 (void)unused; Sleep(30000); ExitProcess(124); return 0;
}
typedef struct { LibreOfficeKit *kit; wchar_t *input; wchar_t *output; } Work;
static DWORD WINAPI convert_worker(void *argument) {
 Work *work=(Work*)argument; LibreOfficeKit *kit=work->kit;
 wchar_t *input=work->input,*output=work->output;
 char *in=utf8(input),*out=utf8(output);
 LibreOfficeKitDocument *doc=kit->pClass->documentLoadWithOptions(kit,in,"Batch=true,EnableMacrosExecution=false,MacroSecurityLevel=3");
 if(!doc) {fprintf(stderr,"Load failed: %s\n",kit->pClass->getError(kit));return 7;}

 fprintf(stderr,"Saving private PDF\n");fflush(stderr);
 int ok=doc->pClass->saveAs(doc,out,"pdf",NULL);
 fprintf(stderr,"saveAs=%d\n",ok);fflush(stderr);
 doc->pClass->destroy(doc);kit->pClass->destroy(kit);free(in);free(out);
 return ok?0:8;
}
int wmain(int argc,wchar_t **argv) {
 HANDLE deadline=CreateThread(NULL,0,owned_deadline,NULL,0,NULL);
 if(!deadline) return 20;
 CloseHandle(deadline);
 HANDLE diagnostic=CreateThread(NULL,0,owned_window_diagnostic,NULL,0,NULL);
 if(diagnostic) CloseHandle(diagnostic);
 ULONG guarantee=65536;SetThreadStackGuarantee(&guarantee);
 AddVectoredExceptionHandler(1,diagnostic_exception);
 SetEnvironmentVariableA("SAL_LOG","+INFO.lok+WARN");
 if(argc!=4) return 2;
 wchar_t cwd[32768],longpath[32768],copytarget[32768];GetCurrentDirectoryW(32768,cwd);
 SetLastError(0);DWORD canonical=GetLongPathNameW(cwd,longpath,32768);
 fprintf(stderr,"Private directory canonicalization length=%lu error=%lu\n",canonical,GetLastError());
 SetLastError(0);BOOL directory=CreateDirectoryW(cwd,NULL);
 fprintf(stderr,"Existing private directory create=%d error=%lu\n",directory,GetLastError());
 swprintf(copytarget,32768,L"%ls\\preset-copy-diagnostic.bau",argv[2]);
 SetLastError(0);BOOL copied=CopyFileW(L"C:\\Program Files\\LibreOffice\\presets\\autotext\\mytexts.bau",copytarget,TRUE);
 fprintf(stderr,"Installed preset copy=%d error=%lu\n",copied,GetLastError());fflush(stderr);
 wchar_t input[32768],target[32768],output[32768]; DWORD n=32768;
 if(FAILED(UrlCreateFromPathW(argv[3],input,&n,0))) return 3;
 swprintf(target,32768,L"%ls\\kit-result.pdf",argv[2]);n=32768;
 if(FAILED(UrlCreateFromPathW(target,output,&n,0))) return 4;
 char *profile=utf8(argv[1]);const char *prefix="-env:UserInstallation=";
 if(strncmp(profile,prefix,strlen(prefix))) return 5;
 fprintf(stderr,"Initializing private LibreOfficeKit\n");fflush(stderr);
 SetDllDirectoryW(L"C:\\Program Files\\LibreOffice\\program");
 fprintf(stderr,"Loading installed DLL\n");fflush(stderr);
 HMODULE module=LoadLibraryExW(L"C:\\Program Files\\LibreOffice\\program\\mergedlo.dll",NULL,LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
 if(!module) {fprintf(stderr,"LoadLibrary failed: %lu\n",GetLastError());return 9;}
 typedef LibreOfficeKit *(*Hook)(const char*,const char*);
 Hook hook=(Hook)GetProcAddress(module,"libreofficekit_hook_2");
 if(!hook) return 10;
 fprintf(stderr,"Calling installed Kit hook\n");fflush(stderr);
 LibreOfficeKit *kit=hook("C:\\Program Files\\LibreOffice\\program",profile+strlen(prefix));
 if(!kit) {fprintf(stderr,"Kit initialization failed\n");return 6;}
 fprintf(stderr,"Loading admitted document\n");fflush(stderr);
 Work work={kit,input,output};
 HANDLE conversion=CreateThread(NULL,0,convert_worker,&work,0,NULL);
 if(!conversion) return 21;
 for (;;) {
  DWORD ready=MsgWaitForMultipleObjects(1,&conversion,FALSE,INFINITE,QS_ALLINPUT);
  if(ready==WAIT_OBJECT_0) break;
  if(ready!=WAIT_OBJECT_0+1) return 22;
  MSG message;
  while(PeekMessageW(&message,NULL,0,0,PM_REMOVE)) {TranslateMessage(&message);DispatchMessageW(&message);}
 }
 DWORD result=23;GetExitCodeThread(conversion,&result);CloseHandle(conversion);free(profile);
 return (int)result;
}
