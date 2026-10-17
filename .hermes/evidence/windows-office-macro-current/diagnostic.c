#include <assert.h>
#include <stdbool.h>
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
static DWORD WINAPI owned_deadline(void *unused) {
 (void)unused; Sleep(30000); ExitProcess(124); return 0;
}

typedef struct { LibreOfficeKit *kit; char *input; char *output; HANDLE done; UINT message; LONG started; int result; } Work;
static Work *pending;
static DWORD backend_thread;
static int conflicting_threads;
static HMODULE backend_module;
static BOOL CALLBACK identify_backend_window(HWND window, LPARAM unused) {
 (void)unused;DWORD process=0;DWORD thread=GetWindowThreadProcessId(window,&process);
 if(process!=GetCurrentProcessId()) return TRUE;
 LONG_PTR procedure=GetWindowLongPtrW(window,GWLP_WNDPROC);HMODULE module=NULL;
 if(procedure && GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,(LPCWSTR)procedure,&module) && module==backend_module) {
  if(backend_thread && backend_thread!=thread) conflicting_threads=1;
  backend_thread=thread;
 }
 return TRUE;
}
static DWORD find_backend_thread(void) {
 backend_module=GetModuleHandleW(L"vclplug_winlo.dll");
 if(!backend_module) return 0;
 HANDLE snapshot=CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD,0);
 if(snapshot==INVALID_HANDLE_VALUE) return 0;
 THREADENTRY32 entry={0};entry.dwSize=sizeof(entry);
 if(Thread32First(snapshot,&entry)) do {
  if(entry.th32OwnerProcessID==GetCurrentProcessId()) EnumThreadWindows(entry.th32ThreadID,identify_backend_window,0);
 } while(Thread32Next(snapshot,&entry));
 CloseHandle(snapshot);
 HWND window=NULL;
 while((window=FindWindowExW(HWND_MESSAGE,window,NULL,NULL))) identify_backend_window(window,0);
 return conflicting_threads?0:backend_thread;
}
static LRESULT CALLBACK dispatch_conversion(int code,WPARAM reason,LPARAM data) {
 MSG *message=(MSG*)data;Work *work=pending;
 if(code>=0 && reason==PM_REMOVE && work && message->message==work->message && message->wParam==(WPARAM)work && InterlockedCompareExchange(&work->started,1,0)==0) {
  message->message=WM_NULL;
  fprintf(stderr,"Converting on backend thread %lu\n",GetCurrentThreadId());fflush(stderr);
  LibreOfficeKitDocument *doc=work->kit->pClass->documentLoadWithOptions(work->kit,work->input,"Batch=true,EnableMacrosExecution=true,MacroSecurityLevel=0");
  if(!doc) {fprintf(stderr,"Load failed: %s\n",work->kit->pClass->getError(work->kit));work->result=7;}
  else {
   int macro=work->kit->pClass->runMacro(work->kit,"macro://./Standard.Module1.OnOpen()");
   char *error=work->kit->pClass->getError(work->kit);
   fprintf(stderr,"Explicit diagnostic macro result=%d error=%s\n",macro,error?error:"");fflush(stderr);
   if(error)work->kit->pClass->freeError(error);
   int saved=doc->pClass->saveAs(doc,work->output,"pdf",NULL);
   fprintf(stderr,"saveAs=%d\n",saved);doc->pClass->destroy(doc);work->result=saved?0:8;
  }
  fflush(stderr);SetEvent(work->done);
 }
 return CallNextHookEx(NULL,code,reason,data);
}
int wmain(int argc,wchar_t **argv) {
 HANDLE deadline=CreateThread(NULL,0,owned_deadline,NULL,0,NULL);
 if(!deadline) return 20;
 CloseHandle(deadline);
 ULONG guarantee=65536;SetThreadStackGuarantee(&guarantee);
 AddVectoredExceptionHandler(1,diagnostic_exception);
 SetEnvironmentVariableA("SAL_LOG","+INFO.lok+WARN");
 if(argc!=5) return 2;
 wchar_t cwd[32768],longpath[32768],copytarget[32768];GetCurrentDirectoryW(32768,cwd);
 SetLastError(0);DWORD canonical=GetLongPathNameW(cwd,longpath,32768);
 fprintf(stderr,"Private directory canonicalization length=%lu error=%lu\n",canonical,GetLastError());
 SetLastError(0);BOOL directory=CreateDirectoryW(cwd,NULL);
 fprintf(stderr,"Existing private directory create=%d error=%lu\n",directory,GetLastError());
 swprintf(copytarget,32768,L"%ls\\preset-copy-diagnostic.bau",argv[4]);
 SetLastError(0);BOOL copied=CopyFileW(L"C:\\Program Files\\LibreOffice\\presets\\autotext\\mytexts.bau",copytarget,TRUE);
 fprintf(stderr,"Installed preset copy=%d error=%lu\n",copied,GetLastError());fflush(stderr);
 wchar_t input[32768],target[32768],output[32768]; DWORD n=32768;
 if(FAILED(UrlCreateFromPathW(argv[3],input,&n,0))) return 3;
 swprintf(target,32768,L"%ls\\kit-result.pdf",argv[4]);n=32768;
 if(FAILED(UrlCreateFromPathW(target,output,&n,0))) return 4;
 char *profile=utf8(argv[2]);const char *prefix="-env:UserInstallation=";
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

 DWORD thread=find_backend_thread();fprintf(stderr,"Caller=%lu backend=%lu\n",GetCurrentThreadId(),thread);fflush(stderr);
 if(!thread || thread==GetCurrentThreadId()) return 31;
 char *in=utf8(input),*out=utf8(output);
 Work work={kit,in,out,CreateEventW(NULL,TRUE,FALSE,NULL),RegisterWindowMessageW(L"OttrinOwnedOfficeConversionProbe"),0,32};
 if(!work.done || !work.message) return 33;
 pending=&work;HHOOK hook_dispatch=SetWindowsHookExW(WH_GETMESSAGE,dispatch_conversion,NULL,thread);
 if(!hook_dispatch) {fprintf(stderr,"Hook failed %lu\n",GetLastError());return 34;}
 if(!PostThreadMessageW(thread,work.message,(WPARAM)&work,0)) return 35;
 if(WaitForSingleObject(work.done,25000)!=WAIT_OBJECT_0) {fprintf(stderr,"Owned dispatch timeout\n");return 36;}
 if(!UnhookWindowsHookEx(hook_dispatch)) return 37;
 pending=NULL;CloseHandle(work.done);kit->pClass->destroy(kit);free(profile);free(in);free(out);
 return work.result;
}
