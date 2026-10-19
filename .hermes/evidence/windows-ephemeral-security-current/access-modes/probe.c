#define _WIN32_WINNT 0x0A00
#include <windows.h>
#include <winternl.h>
#include <sddl.h>
#include <aclapi.h>
#include <stdio.h>
#include <wchar.h>
NTSYSAPI NTSTATUS NTAPI NtSetSecurityObject(HANDLE,SECURITY_INFORMATION,PSECURITY_DESCRIPTOR);
#define EX_CLASS ((FILE_INFO_BY_HANDLE_CLASS)21)
static int content(const wchar_t *p) {HANDLE f=CreateFileW(p,GENERIC_READ,7,NULL,OPEN_EXISTING,0,NULL);char b[8]={0};DWORD n=0;BOOL ok=f!=INVALID_HANDLE_VALUE&&ReadFile(f,b,8,&n,NULL)&&n==4&&!memcmp(b,"keep",4);if(f!=INVALID_HANDLE_VALUE)CloseHandle(f);return ok;}
int wmain(int argc,wchar_t **argv) {
 if(argc!=2)return 2;
 HANDLE parent=CreateFileW(argv[1],GENERIC_READ|GENERIC_WRITE,7,NULL,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS|FILE_FLAG_OPEN_REPARSE_POINT,NULL);if(parent==INVALID_HANDLE_VALUE)return 3;
 for(int mode=0;mode<7;mode++) {
  PSECURITY_DESCRIPTOR sd=NULL;if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(L"D:P(A;;FR;;;OW)",1,&sd,NULL))return 4;
  wchar_t name[32],path[1024],ads[1050];swprintf(name,32,L"mode-%d",mode);swprintf(path,1024,L"%ls\\%ls",argv[1],name);swprintf(ads,1050,L"%ls:foreign",path);
  UNICODE_STRING u={0};u.Buffer=name;u.Length=(USHORT)(wcslen(name)*2);u.MaximumLength=u.Length;
  OBJECT_ATTRIBUTES a={0};a.Length=sizeof(a);a.RootDirectory=parent;a.ObjectName=&u;a.SecurityDescriptor=sd;
  IO_STATUS_BLOCK io={0};HANDLE f=NULL;NTSTATUS created=NtCreateFile(&f,mode==4?0x02100000:mode==5?0x0217019f:mode==6?0x001f01ff:0x0017019f,&a,&io,NULL,FILE_ATTRIBUTE_NORMAL,FILE_SHARE_READ,2,0x200000|0x20|0x40|0x1000,NULL,0);LocalFree(sd);
  if(created<0){printf("{\"mode\":%d,\"created\":false,\"status\":%ld}\n",mode,(long)created);continue;}
  PUBLIC_OBJECT_BASIC_INFORMATION basic={0};ULONG length=0;NTSTATUS query=NtQueryObject(f,ObjectBasicInformation,&basic,sizeof(basic),&length);
  DWORD wrote=0;BOOL own=WriteFile(f,"record",6,&wrote,NULL)&&wrote==6&&FlushFileBuffers(f);
  HANDLE other=CreateFileW(ads,GENERIC_READ|GENERIC_WRITE,7,NULL,CREATE_NEW,0,NULL);int before=other!=INVALID_HANDLE_VALUE;DWORD before_error=before?0:GetLastError();if(before)CloseHandle(other);
  FILE_DISPOSITION_INFO_EX clear={FILE_DISPOSITION_FLAG_ON_CLOSE};BOOL cleared=SetFileInformationByHandle(f,EX_CLASS,&clear,sizeof(clear));
  PSECURITY_DESCRIPTOR full=NULL;PACL dacl=NULL;BOOL present=FALSE,def=FALSE;DWORD win_error=ERROR_INVALID_SECURITY_DESCR;NTSTATUS native_status=0;int restored=0;
  if(cleared&&ConvertStringSecurityDescriptorToSecurityDescriptorW(mode==1?L"D:P(A;;FA;;;OW)":L"D:P(A;OICI;FA;;;OW)",1,&full,NULL)) {
   if((mode<2||mode>=4)&&GetSecurityDescriptorDacl(full,&present,&dacl,&def)&&present){win_error=SetSecurityInfo(f,SE_FILE_OBJECT,DACL_SECURITY_INFORMATION|PROTECTED_DACL_SECURITY_INFORMATION,NULL,NULL,dacl,NULL);restored=win_error==0;}
   if(mode==2||mode==3){native_status=NtSetSecurityObject(f,DACL_SECURITY_INFORMATION|(mode==3?PROTECTED_DACL_SECURITY_INFORMATION:0),full);restored=native_status>=0;}
  }
  if(full)LocalFree(full);
  other=CreateFileW(ads,GENERIC_READ|GENERIC_WRITE,7,NULL,CREATE_NEW,0,NULL);int after=other!=INVALID_HANDLE_VALUE;BOOL saved=FALSE;if(after){saved=WriteFile(other,"keep",4,&wrote,NULL)&&wrote==4&&FlushFileBuffers(other);CloseHandle(other);}
  CloseHandle(f);int file_after=GetFileAttributesW(path)!=INVALID_FILE_ATTRIBUTES;int stream_intact=content(ads);int reject_cleanup=restored?-1:(DeleteFileW(path)?1:0);
  printf("{\"mode\":%d,\"created\":true,\"query_status\":%ld,\"granted_access\":%lu,\"owner_write\":%s,\"stream_before\":%d,\"stream_before_error\":%lu,\"cleared\":%s,\"win_error\":%lu,\"native_status\":%ld,\"restored\":%s,\"stream_after\":%d,\"saved\":%s,\"file_after\":%d,\"stream_intact\":%d,\"rejected_fixture_cleanup\":%d}\n",mode,(long)query,(unsigned long)basic.GrantedAccess,own?"true":"false",before,before_error,cleared?"true":"false",win_error,(long)native_status,restored?"true":"false",after,saved?"true":"false",file_after,stream_intact,reject_cleanup);fflush(stdout);
 }
 CloseHandle(parent);return 0;
}
