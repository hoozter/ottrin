#define _WIN32_WINNT 0x0A00
#include <windows.h>
#include <winternl.h>
#include <sddl.h>
#include <aclapi.h>
#include <stdio.h>
#include <wchar.h>
#define EX_CLASS ((FILE_INFO_BY_HANDLE_CLASS)21)
int wmain(int argc,wchar_t **argv) {
 if(argc!=2)return 2;
 HANDLE parent=CreateFileW(argv[1],GENERIC_READ|GENERIC_WRITE,7,NULL,OPEN_EXISTING,FILE_FLAG_BACKUP_SEMANTICS,NULL);if(parent==INVALID_HANDLE_VALUE)return 3;
 PSECURITY_DESCRIPTOR sd=NULL;if(!ConvertStringSecurityDescriptorToSecurityDescriptorW(L"D:P(A;;FR;;;OW)",1,&sd,NULL))return 4;
 UNICODE_STRING u={0};u.Buffer=L"restricted";u.Length=(USHORT)(wcslen(u.Buffer)*2);u.MaximumLength=u.Length;
 OBJECT_ATTRIBUTES a={0};a.Length=sizeof(a);a.RootDirectory=parent;a.ObjectName=&u;a.SecurityDescriptor=sd;
 IO_STATUS_BLOCK io={0};HANDLE f=NULL;NTSTATUS status=NtCreateFile(&f,GENERIC_READ|GENERIC_WRITE|DELETE|WRITE_DAC|SYNCHRONIZE,&a,&io,NULL,FILE_ATTRIBUTE_NORMAL,FILE_SHARE_READ,2,0x200000|0x20|0x40|0x1000,NULL,0);LocalFree(sd);
 if(status<0){printf("{\"mode\":0,\"created\":false,\"native_status\":%ld}\n",(long)status);CloseHandle(parent);return 0;}
 DWORD wrote=0;BOOL own=WriteFile(f,"record",6,&wrote,NULL)&&wrote==6&&FlushFileBuffers(f);
 wchar_t path[1024],ads[1050];swprintf(path,1024,L"%ls\\restricted",argv[1]);swprintf(ads,1050,L"%ls:foreign",path);
 HANDLE other=CreateFileW(ads,GENERIC_READ|GENERIC_WRITE,7,NULL,CREATE_NEW,0,NULL);int before=other!=INVALID_HANDLE_VALUE;DWORD err=before?0:GetLastError();if(before)CloseHandle(other);
 FILE_DISPOSITION_INFO_EX clear={FILE_DISPOSITION_FLAG_ON_CLOSE};BOOL cleared=SetFileInformationByHandle(f,EX_CLASS,&clear,sizeof(clear));
 PSECURITY_DESCRIPTOR full=NULL;PACL dacl=NULL;BOOL present=FALSE,def=FALSE;DWORD restored=ERROR_INVALID_SECURITY_DESCR;
 if(cleared&&ConvertStringSecurityDescriptorToSecurityDescriptorW(L"D:P(A;OICI;FA;;;OW)",1,&full,NULL)&&GetSecurityDescriptorDacl(full,&present,&dacl,&def)&&present){restored=SetSecurityInfo(f,SE_FILE_OBJECT,DACL_SECURITY_INFORMATION|PROTECTED_DACL_SECURITY_INFORMATION,NULL,NULL,dacl,NULL);}if(full)LocalFree(full);
 other=CreateFileW(ads,GENERIC_READ|GENERIC_WRITE,7,NULL,CREATE_NEW,0,NULL);int after=other!=INVALID_HANDLE_VALUE;BOOL saved=FALSE;if(after){saved=WriteFile(other,"keep",4,&wrote,NULL)&&wrote==4&&FlushFileBuffers(other);CloseHandle(other);}
 CloseHandle(f);int file_after=GetFileAttributesW(path)!=INVALID_FILE_ATTRIBUTES;HANDLE stream=CreateFileW(ads,GENERIC_READ,7,NULL,OPEN_EXISTING,0,NULL);char bytes[8]={0};DWORD got=0;BOOL intact=stream!=INVALID_HANDLE_VALUE&&ReadFile(stream,bytes,8,&got,NULL)&&got==4&&!memcmp(bytes,"keep",4);if(stream!=INVALID_HANDLE_VALUE)CloseHandle(stream);
 printf("{\"mode\":0,\"created\":true,\"owner_write\":%s,\"stream_before\":%d,\"stream_before_error\":%lu,\"cleared\":%s,\"restore_error\":%lu,\"stream_after\":%d,\"saved\":%s,\"file_after\":%d,\"stream_intact\":%s}\n",own?"true":"false",before,err,cleared?"true":"false",restored,after,saved?"true":"false",file_after,intact?"true":"false");
 CloseHandle(parent);return 0;
}
