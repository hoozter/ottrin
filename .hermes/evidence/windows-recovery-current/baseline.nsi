Unicode true
RequestExecutionLevel user

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

Var CleanupAppHandle
Var CleanupUninstallerHandle
Var IndexTransactionActive
Var IndexTransactionSnapshot
Var InstallerMutex

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
!define MUI_CUSTOMFUNCTION_ABORT RestoreIndexTransaction
!define MUI_FINISHPAGE_RUN "$INSTDIR\bin\ottrin.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Ottrin"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

!macro InstallerInitialization Prefix
Function ${Prefix}.onInit
  ; Setup and uninstall must not mutate the same payload, background task or
  ; recovery journal concurrently. Use the same lock for both entry points.
  ; Keep this handle until installer exit; Windows releases it on every exit path.
  System::Call 'kernel32::CreateMutexW(p 0, i 0, w "Global\OttrinRecoveryQAInstaller") p.r0 ?e'
  Pop $1
  ${If} $0 == 0
  ${OrIf} $1 == 183
    MessageBox MB_OK|MB_ICONSTOP "Another Ottrin setup or uninstall is running, or its installation lock could not be acquired. Finish the other operation and retry." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  StrCpy $InstallerMutex $0
FunctionEnd
!macroend

!insertmacro InstallerInitialization ""
!insertmacro InstallerInitialization "un"

Section "Ottrin" SEC_MAIN
  SetShellVarContext current
  ; This controller lives outside the payload being replaced. Its journal stays
  ; in the user data directory if restoration fails, rather than disappearing
  ; when NSIS removes $PLUGINSDIR.
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /oname=index-controller.exe "${STAGE}\bin\ottrin-indexd.exe"
  CreateDirectory "$LOCALAPPDATA\ottrin-recovery-qa"
  StrCpy $IndexTransactionSnapshot "$LOCALAPPDATA\ottrin-recovery-qa\installer-index-transaction.json"
  StrCpy $IndexTransactionActive 1
  ClearErrors
  ExecWait '"$PLUGINSDIR\index-controller.exe" --installer-transaction Prepare "$IndexTransactionSnapshot"' $0
  IfErrors index_prepare_failed
  ${If} $0 != 0
    Goto index_prepare_failed
  ${EndIf}

  SetOutPath "$INSTDIR"
  ClearErrors
  File /r "${STAGE}\*.*"

  IfErrors index_payload_failed

  WriteUninstaller "$INSTDIR\Uninstall.exe"
  CreateDirectory "$SMPROGRAMS\OttrinRecoveryQA"
  CreateShortcut "$SMPROGRAMS\OttrinRecoveryQA\Ottrin.lnk" "$INSTDIR\bin\ottrin.exe"
  CreateShortcut "$SMPROGRAMS\OttrinRecoveryQA\Uninstall Ottrin.lnk" "$INSTDIR\Uninstall.exe"

  WriteRegStr HKCU "Software\OttrinRecoveryQA" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinRecoveryQA" "DisplayName" "Ottrin"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinRecoveryQA" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinRecoveryQA" "Publisher" "hoozter"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinRecoveryQA" "DisplayIcon" "$INSTDIR\bin\ottrin.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinRecoveryQA" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinRecoveryQA" "NoModify" 1
  WriteRegDWORD HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinRecoveryQA" "NoRepair" 1

  ClearErrors
  ExecWait '"$INSTDIR\bin\ottrin.exe" index service-install' $0
  IfErrors background_integration_launch_failed
  ${If} $0 != 0
    Goto background_integration_launch_failed
  ${EndIf}
  ClearErrors
  ExecWait '"$PLUGINSDIR\index-controller.exe" --installer-transaction Commit "$IndexTransactionSnapshot"' $0
  IfErrors index_commit_failed
  ${If} $0 != 0
    Goto index_commit_failed
  ${EndIf}
  StrCpy $IndexTransactionActive 0
  Goto background_integration_done

index_prepare_failed:
  MessageBox MB_OK|MB_ICONSTOP "Ottrin could not safely stop background indexing. Application files have not been replaced. Close Ottrin and retry setup." /SD IDOK
  SetErrorLevel 1
  Abort

index_payload_failed:
  MessageBox MB_OK|MB_ICONSTOP "Ottrin could not replace all application files. Setup is incomplete; close Ottrin and retry this installer." /SD IDOK
  SetErrorLevel 1
  Abort

background_integration_launch_failed:
  MessageBox MB_OK|MB_ICONSTOP "Ottrin was installed, but its background index service could not be enabled. You can retry from Services settings. Setup will restore the previous task configuration where possible." /SD IDOK
  SetErrorLevel 1
  Abort

index_commit_failed:
  MessageBox MB_OK|MB_ICONSTOP "Ottrin could not restore the previous indexing state after setup. The transaction record is retained at $IndexTransactionSnapshot if recovery fails. Retry setup or use Services settings." /SD IDOK
  SetErrorLevel 1
  Abort

background_integration_done:
SectionEnd

Function RestoreIndexTransaction
  StrCmp $IndexTransactionActive 1 0 restore_index_done
  ClearErrors
  ExecWait '"$PLUGINSDIR\index-controller.exe" --installer-transaction Rollback "$IndexTransactionSnapshot"' $0
  IfErrors restore_index_failed
  ${If} $0 != 0
    Goto restore_index_failed
  ${EndIf}
  StrCpy $IndexTransactionActive 0
  Goto restore_index_done
restore_index_failed:
  MessageBox MB_OK|MB_ICONSTOP "Ottrin could not restore its previous background index task. The recovery record remains at $IndexTransactionSnapshot. Application file replacement may be incomplete; retry setup before opening Ottrin." /SD IDOK
restore_index_done:
FunctionEnd

Function .onInstFailed
  Call RestoreIndexTransaction
FunctionEnd

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
  Delete "$SMPROGRAMS\OttrinRecoveryQA\Ottrin.lnk"
  Delete "$SMPROGRAMS\OttrinRecoveryQA\Uninstall Ottrin.lnk"
  RMDir "$SMPROGRAMS\OttrinRecoveryQA"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\OttrinRecoveryQA"
  DeleteRegKey HKCU "Software\OttrinRecoveryQA"
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
