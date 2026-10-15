#include <windows.h>
#include <wchar.h>
#include <stdio.h>
int wmain(int argc,wchar_t **argv) {
 if(argc==2 && !wcscmp(argv[1],L"--uninstall")) {
  wchar_t path[32768];if(!GetModuleFileNameW(NULL,path,32768))return 2;
  for(int i=0;i<3;i++){wchar_t *slash=wcsrchr(path,L'\\');if(!slash)return 3;*slash=0;}
  if(wcslen(path)+24>=32768)return 4;
  wcscat(path,L"\\cleanup-attempted.txt");FILE *f=_wfopen(path,L"ab");if(!f)return 5;
  if(fprintf(f,"%lu\n",GetCurrentProcessId())<0 || fclose(f))return 6;
  *wcsrchr(path,L'\\')=0;wcscat(path,L"\\block-cleanup");
  ULONGLONG deadline=GetTickCount64()+20000;
  while(GetFileAttributesW(path)!=INVALID_FILE_ATTRIBUTES) {
   if(GetTickCount64()>=deadline)return 7;
   Sleep(50);
  }
 }
 return 0;
}
