#define _WIN32_WINNT 0x0A00
#include <windows.h>
#include <winternl.h>
#include <sddl.h>
#include <aclapi.h>
#include <stdio.h>
#include <wchar.h>
static DWORD dacl(HANDLE f,const wchar_t *s){PSECURITY_DESCRIPTOR sd=NULL;PACL acl=NULL;BOOL present=FALSE,def=FALSE;if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(s,1,&sd,NULL))return GetLastError();if(!GetSecurityDescriptorDacl(sd,&present,&acl,&def)||!present){LocalFree(sd);return ERROR_INVALID_SECURITY_DESCR;}DWORD e=SetSecurityInfo(f,SE_FILE_OBJECT,DACL_SECURITY_INFORMATION|PROTECTED_DACL_SECURITY_INFORMATION,NULL,NULL,acl,NULL);LocalFree(sd);return e;}
int wmain(int argc,wchar_t **argv){
 if(argc!=2)return 2;
 wchar_t path[1024],stream[1050];swprintf(path,1024,L"%ls\\control",argv[1]);swprintf(stream,1050,L"%ls:foreign",path);
 PSECURITY_DESCRIPTOR sd=NULL;if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(L"D:P(A;;0x00160089;;;OW)",1,&sd,NULL))return 3;
 SECURITY_ATTRIBUTES sa={sizeof(sa),sd,FALSE};HANDLE own=CreateFileW(path,GENERIC_READ|GENERIC_WRITE|DELETE|WRITE_DAC,FILE_SHARE_READ,&sa,CREATE_NEW,0,NULL);LocalFree(sd);if(own==INVALID_HANDLE_VALUE)return 4;
 HANDLE other=CreateFileW(path,WRITE_DAC,7,NULL,OPEN_EXISTING,0,NULL);BOOL held=other!=INVALID_HANDLE_VALUE;DWORD held_error=held?0:GetLastError();
 DWORD freeze=dacl(own,L"D:P(A;;FR;;;OW)");FILE_STANDARD_INFO before={0};BOOL queried=GetFileInformationByHandleEx(own,FileStandardInfo,&before,sizeof(before));
 DWORD restored=held?dacl(other,L"D:P(A;OICI;FA;;;OW)"):ERROR_INVALID_HANDLE;
 HANDLE data=CreateFileW(stream,GENERIC_WRITE,7,NULL,CREATE_NEW,0,NULL);DWORD stream_error=data==INVALID_HANDLE_VALUE?GetLastError():0,n=0;BOOL wrote=data!=INVALID_HANDLE_VALUE&&WriteFile(data,"foreign user data",17,&n,NULL)&&n==17&&FlushFileBuffers(data);
 FILE_DISPOSITION_INFO pending={TRUE};BOOL activated=SetFileInformationByHandle(own,FileDispositionInfo,&pending,sizeof(pending));DWORD activation_error=activated?0:GetLastError();
 if(data!=INVALID_HANDLE_VALUE){CloseHandle(data);}
 if(held){CloseHandle(other);}
 CloseHandle(own);
 BOOL base_exists=GetFileAttributesW(path)!=INVALID_FILE_ATTRIBUTES;HANDLE reopened=CreateFileW(stream,GENERIC_READ,7,NULL,OPEN_EXISTING,0,NULL);BOOL stream_exists=reopened!=INVALID_HANDLE_VALUE;if(stream_exists)CloseHandle(reopened);
 printf("{\"permission_handle_acquired_before_freeze\":%s,\"permission_open_error\":%lu,\"freeze_error\":%lu,\"empty_base_observed\":%s,\"held_handle_restore_error\":%lu,\"stream_open_error\":%lu,\"foreign_stream_written\":%s,\"deletion_activated\":%s,\"activation_error\":%lu,\"base_exists_after_close\":%s,\"foreign_stream_exists_after_close\":%s}\n",held?"true":"false",held_error,freeze,queried&&before.EndOfFile.QuadPart==0?"true":"false",restored,stream_error,wrote?"true":"false",activated?"true":"false",activation_error,base_exists?"true":"false",stream_exists?"true":"false");return 0;
}
