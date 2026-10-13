Unicode true
RequestExecutionLevel user

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

Var CleanupAppHandle
Var CleanupUninstallerHandle

!ifndef VERSION
  !define VERSION "0.1.0"
!endif
!ifndef STAGE
  !error "STAGE must point to an extracted Ottrin Windows portable directory"
!endif
!ifndef OUTPUT
  !define OUTPUT "Ottrin-${VERSION}-windows-x64-setup.exe"
!endif

Name "Ottrin"
OutFile "${OUTPUT}"
SetCompressor /SOLID lzma
InstallDir "$LOCALAPPDATA\Programs\Ottrin"

!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\bin\ottrin.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Ottrin"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

Section "Ottrin" SEC_MAIN
  SetShellVarContext current
  SetOutPath "$INSTDIR"
  File /r "${STAGE}\*.*"

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\Ottrin"
  CreateShortcut "$SMPROGRAMS\Ottrin\Ottrin.lnk" "$INSTDIR\bin\ottrin.exe"
  CreateShortcut "$SMPROGRAMS\Ottrin\Uninstall Ottrin.lnk" "$INSTDIR\Uninstall.exe"

  WriteRegStr HKCU "Software\Ottrin" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin" "DisplayName" "Ottrin"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin" "Publisher" "hoozter"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin" "DisplayIcon" "$INSTDIR\bin\ottrin.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin" "NoRepair" 1

  ClearErrors
  ExecWait '"$INSTDIR\bin\ottrin.exe" index service-install' $0
  IfErrors background_integration_launch_failed
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONEXCLAMATION "Ottrin was installed, but its background search refresh could not be enabled. You can retry from Search settings." /SD IDOK
  ${EndIf}
  Goto background_integration_done

background_integration_launch_failed:
  MessageBox MB_OK|MB_ICONEXCLAMATION "Ottrin was installed, but its background search refresh could not be enabled. You can retry from Search settings." /SD IDOK

background_integration_done:
SectionEnd

Section "Uninstall"
  SetShellVarContext current
  StrCpy $CleanupAppHandle 0
  StrCpy $CleanupUninstallerHandle 0

  IfFileExists "$INSTDIR\bin\ottrin.exe" 0 cleanup_unavailable
  ClearErrors
  ExecWait '"$INSTDIR\bin\ottrin.exe" --uninstall' $0
  IfErrors cleanup_launch_failed
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONSTOP "Ottrin could not remove its background integration and generated data. The application has been left installed so cleanup can be retried." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  Goto cleanup_files

cleanup_launch_failed:
  MessageBox MB_OK|MB_ICONSTOP "Ottrin could not start its cleanup process. The application has been left installed so cleanup can be retried." /SD IDOK
  SetErrorLevel 1
  Abort

cleanup_unavailable:
  MessageBox MB_OK|MB_ICONSTOP "Ottrin's cleanup executable is missing. The application has been left installed to avoid an incomplete uninstall." /SD IDOK
  SetErrorLevel 1
  Abort

cleanup_files:
  ; Keep the retry executables until every other payload file has gone. Holding
  ; DELETE-access handles also prevents a new reader without delete sharing
  ; from locking those executables halfway through removal.
  System::Call 'kernel32::GetFileAttributesW(w "$INSTDIR\bin\ottrin.exe") i.r0'
  IntCmp $0 -1 cleanup_delete_failed
  IntOp $0 $0 & 1
  IntCmp $0 0 +2
  Goto cleanup_delete_failed
  System::Call 'kernel32::CreateFileW(w "$INSTDIR\bin\ottrin.exe", i 0x10000, i 7, p 0, i 3, i 0, p 0) p.r0'
  IntCmp $0 -1 cleanup_delete_failed
  StrCpy $CleanupAppHandle $0
  System::Call 'kernel32::GetFileAttributesW(w "$INSTDIR\Uninstall.exe") i.r0'
  IntCmp $0 -1 cleanup_delete_failed
  IntOp $0 $0 & 1
  IntCmp $0 0 +2
  Goto cleanup_delete_failed
  System::Call 'kernel32::CreateFileW(w "$INSTDIR\Uninstall.exe", i 0x10000, i 7, p 0, i 3, i 0, p 0) p.r0'
  IntCmp $0 -1 cleanup_delete_failed
  StrCpy $CleanupUninstallerHandle $0
  StrCpy $R0 0
  ClearErrors
  ${un.Locate} "$INSTDIR" "/L=F" "un.RemovePayloadFile"
  IfErrors cleanup_delete_failed
  IntCmp $R0 0 +2
  Goto cleanup_delete_failed
  Delete "$INSTDIR\bin\ottrin.exe"
  IfErrors cleanup_delete_failed
  Delete "$INSTDIR\Uninstall.exe"
  IfErrors cleanup_delete_failed
  Call un.ReleaseCleanupHandles
  ClearErrors
  RMDir /r "$INSTDIR"
  IfErrors cleanup_delete_failed
  Delete "$SMPROGRAMS\Ottrin\Ottrin.lnk"
  Delete "$SMPROGRAMS\Ottrin\Uninstall Ottrin.lnk"
  RMDir "$SMPROGRAMS\Ottrin"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\Ottrin"
  DeleteRegKey HKCU "Software\Ottrin"
  Goto uninstall_complete

cleanup_delete_failed:
  Call un.ReleaseCleanupHandles
  MessageBox MB_OK|MB_ICONSTOP "Ottrin could not remove all application files. Installed Apps registration has been retained so uninstall can be retried." /SD IDOK
  SetErrorLevel 1
  Abort

uninstall_complete:
SectionEnd

Function un.RemovePayloadFile
  StrCmp $R9 "$INSTDIR\bin\ottrin.exe" payload_file_done
  StrCmp $R9 "$INSTDIR\Uninstall.exe" payload_file_done
  ClearErrors
  Delete "$R9"
  IfErrors 0 payload_file_done
  StrCpy $R0 1
payload_file_done:
  Push ""
FunctionEnd

Function un.ReleaseCleanupHandles
  StrCmp $CleanupAppHandle 0 +3
  System::Call 'kernel32::CloseHandle(p $CleanupAppHandle)'
  StrCpy $CleanupAppHandle 0
  StrCmp $CleanupUninstallerHandle 0 +3
  System::Call 'kernel32::CloseHandle(p $CleanupUninstallerHandle)'
  StrCpy $CleanupUninstallerHandle 0
FunctionEnd
