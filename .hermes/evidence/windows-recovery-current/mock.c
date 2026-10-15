#include <windows.h>
#include <wchar.h>
#include <stdio.h>
#ifndef PAYLOAD_VERSION
#define PAYLOAD_VERSION L"unknown"
#endif
int wmain(int argc,wchar_t **argv){
 wchar_t log[32768],fail[128];
 if(GetEnvironmentVariableW(L"OTTRIN_QA_CONTROL_LOG",log,32768)){
  FILE *f=_wfopen(log,L"ab");if(!f)return 30;
  fwprintf(f,L"%ls %ls %ls\n",PAYLOAD_VERSION,argc>1?argv[1]:L"",argc>2?argv[2]:L"");fclose(f);
 }
 if(GetEnvironmentVariableW(L"OTTRIN_QA_FAIL_PHASE",fail,128)){
  if(argc==3&&!wcscmp(argv[1],L"index")&&!wcscmp(argv[2],L"service-install")&&!wcscmp(fail,L"Service"))return 9;
  if(argc==4&&!wcscmp(argv[1],L"--installer-transaction")&&!wcscmp(argv[2],fail))return 9;
 }
 if(argc==4&&!wcscmp(argv[1],L"--installer-transaction")){
  if(!wcscmp(argv[2],L"Prepare")){
   HANDLE h=CreateFileW(argv[3],GENERIC_WRITE,0,NULL,CREATE_NEW,FILE_ATTRIBUTE_NORMAL,NULL);
   if(h==INVALID_HANDLE_VALUE)return 12;
   DWORD written=0;BOOL ok=WriteFile(h,"mock intent",11,&written,NULL)&&written==11&&FlushFileBuffers(h);CloseHandle(h);if(!ok)return 13;
  }else if(!wcscmp(argv[2],L"Activate")){
   if(GetFileAttributesW(argv[3])==INVALID_FILE_ATTRIBUTES)return 15;
  }else if(!wcscmp(argv[2],L"Rollback")||!wcscmp(argv[2],L"Commit")){
   if(!DeleteFileW(argv[3])&&GetLastError()!=ERROR_FILE_NOT_FOUND)return 14;
  }
 }
 return 0;
}
