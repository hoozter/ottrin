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
Var PayloadTransactionActive
Var PayloadPublished
Var PayloadTransactionPhase
Var PayloadTransactionResult

!ifndef VERSION
  !define VERSION "0.1.0"
!endif
!ifndef STAGE
  !error "STAGE must point to an extracted Ottrin Windows portable directory"
!endif
!ifndef PAYLOAD_TRANSACTION_SCRIPT
  !define PAYLOAD_TRANSACTION_SCRIPT "payload-transaction.ps1"
!endif
!ifndef OUTPUT
  !define OUTPUT "Ottrin-${VERSION}-windows-x64-setup.exe"
!endif

Name "Ottrin"
OutFile "${OUTPUT}"
SetCompressor /SOLID lzma
InstallDir "$LOCALAPPDATA\Programs\Ottrin"

!define MUI_ABORTWARNING
!define MUI_CUSTOMFUNCTION_ABORT RestoreInstallationTransaction
!define MUI_FINISHPAGE_RUN "$INSTDIR\bin\ottrin.exe"
!define MUI_FINISHPAGE_RUN_TEXT "Launch Ottrin"

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

!macro AcquireInstallationLock
  ; Setup and uninstall must not mutate the same payload, background task or
  ; recovery journal concurrently. Use the same lock for both entry points.
  ; Keep this handle until installer exit; Windows releases it on every exit path.
  System::Call 'kernel32::CreateMutexW(p 0, i 0, w "Global\OttrinInstaller") p.r0 ?e'
  Pop $1
  ${If} $0 == 0
  ${OrIf} $1 == 183
    MessageBox MB_OK|MB_ICONSTOP "Another Ottrin setup or uninstall is running, or its installation lock could not be acquired. Finish the other operation and retry." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
  StrCpy $InstallerMutex $0
  ; Never bypass an unresolved snapshot left by the preceding installer format.
  System::Call 'kernel32::GetFileAttributesW(w "$LOCALAPPDATA\ottrin\installer-index-transaction.json") i.r0 ?e'
  Pop $1
  ${If} $0 == -1
    ${If} $1 == 2
    ${OrIf} $1 == 3
      StrCpy $0 0
    ${Else}
      StrCpy $0 1
    ${EndIf}
  ${Else}
    StrCpy $0 1
  ${EndIf}
  ${If} $0 == 1
    MessageBox MB_OK|MB_ICONSTOP "A previous background-index installation transaction needs recovery at $LOCALAPPDATA\ottrin\installer-index-transaction.json. Its records have not been changed." /SD IDOK
    SetErrorLevel 1
    Abort
  ${EndIf}
!macroend

Function .onInit
  !insertmacro AcquireInstallationLock
FunctionEnd

Function un.onInit
  !insertmacro AcquireInstallationLock
  ; A crashed setup still owns its retained payload and task snapshot. Do not
  ; let uninstall remove the evidence needed to recover that installation.
  System::Call 'kernel32::GetFileAttributesW(w "$INSTDIR.ottrin-setup") i.r0 ?e'
  Pop $1
  ${If} $0 == -1
    ${If} $1 == 2
    ${OrIf} $1 == 3
      Return
    ${EndIf}
  ${EndIf}
  MessageBox MB_OK|MB_ICONSTOP "An installation transaction is retained at $INSTDIR.ottrin-setup. Recover that installation before uninstalling Ottrin." /SD IDOK
  SetErrorLevel 1
  Abort
FunctionEnd

Section "Ottrin" SEC_MAIN
  SetShellVarContext current
  ; This controller lives outside the payload being replaced. Its journal stays
  ; in the user data directory if restoration fails, rather than disappearing
  ; when NSIS removes $PLUGINSDIR.
  InitPluginsDir
  SetOutPath "$PLUGINSDIR"
  File /oname=index-controller.exe "${STAGE}\bin\ottrin-indexd.exe"
  File /oname=payload-transaction.ps1 "${PAYLOAD_TRANSACTION_SCRIPT}"
  System::Call 'kernel32::SetEnvironmentVariableW(w "OTTRIN_INSTALLER_TARGET", w "$INSTDIR") i.r0'
  StrCmp $0 0 payload_transaction_failed
  System::Call 'kernel32::SetEnvironmentVariableW(w "OTTRIN_INSTALLER_PAYLOAD_SCRIPT", w "$PLUGINSDIR\payload-transaction.ps1") i.r0'
  StrCmp $0 0 payload_transaction_failed
  System::Call 'kernel32::SetEnvironmentVariableW(w "OTTRIN_INSTALLER_PAYLOAD_NOTICE", w "$PLUGINSDIR\payload-notice.txt") i.r0'
  StrCmp $0 0 payload_transaction_failed
  StrCpy $PayloadTransactionPhase Initialize
  Call RunPayloadTransaction
  StrCmp $0 0 +2
  Goto payload_transaction_failed
  StrCpy $PayloadTransactionActive 1
  ; Complete extraction and uninstaller construction before stopping indexing
  ; or moving the old installation. Publication never overlays existing files.
  ClearErrors
  SetOutPath "$INSTDIR.ottrin-setup\new"
  IfErrors index_payload_failed
  File /r "${STAGE}\*.*"
  WriteUninstaller "$INSTDIR.ottrin-setup\new\Uninstall.exe"
  IfErrors index_payload_failed
  SetOutPath "$PLUGINSDIR"
  StrCpy $IndexTransactionSnapshot "$INSTDIR.ottrin-setup\index-transaction.json"
  StrCpy $IndexTransactionActive 1
  ClearErrors
  ExecWait '"$PLUGINSDIR\index-controller.exe" --installer-transaction Prepare "$IndexTransactionSnapshot"' $0
  IfErrors index_prepare_failed
  ${If} $0 != 0
    Goto index_prepare_failed
  ${EndIf}

  StrCpy $PayloadTransactionPhase Publish
  Call RunPayloadTransaction
  StrCmp $0 0 +2
  Goto payload_transaction_failed
  StrCpy $PayloadPublished 1
  StrCpy $PayloadTransactionPhase BeginIntegration
  Call RunPayloadTransaction
  StrCmp $0 0 +2
  Goto payload_transaction_failed
  ClearErrors
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

  IfErrors background_integration_launch_failed
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
  ; Index commit is the point of no return. A cleanup failure retains previous
  ; files for review; it must not restore an old payload under the new task.
  StrCpy $PayloadTransactionActive 0
  StrCpy $PayloadTransactionPhase Commit
  Call RunPayloadTransaction
  ${If} $0 != 0
    MessageBox MB_OK|MB_ICONEXCLAMATION "Ottrin is installed, but previous files could not be cleaned up. They remain at $INSTDIR.ottrin-setup for review." /SD IDOK
  ${ElseIf} $PayloadTransactionResult != ""
    MessageBox MB_OK|MB_ICONINFORMATION "$PayloadTransactionResult" /SD IDOK
  ${EndIf}
  Goto background_integration_done

index_prepare_failed:
  MessageBox MB_OK|MB_ICONSTOP "Ottrin could not safely stop background indexing. Application files have not been replaced. Close Ottrin and retry setup." /SD IDOK
  SetErrorLevel 1
  Abort

payload_transaction_failed:
  MessageBox MB_OK|MB_ICONSTOP "Ottrin could not safely replace the installation. Previous files and recovery information are retained at $INSTDIR.ottrin-setup. $PayloadTransactionResult" /SD IDOK
  SetErrorLevel 1
  Abort

index_payload_failed:
  MessageBox MB_OK|MB_ICONSTOP "Ottrin could not stage all application files. Setup will restore the previous installation where possible." /SD IDOK
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
  SetOutPath "$INSTDIR"
SectionEnd

Function RunPayloadTransaction
  Delete "$PLUGINSDIR\payload-notice.txt"
  System::Call 'kernel32::SetEnvironmentVariableW(w "OTTRIN_INSTALLER_PAYLOAD_PHASE", w "$PayloadTransactionPhase") i.r0'
  ${If} $0 == 0
    StrCpy $0 1
    StrCpy $PayloadTransactionResult "Could not prepare the installer process."
    Return
  ${EndIf}
  nsExec::ExecToStack '"$SYSDIR\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -Command "& ([scriptblock]::Create([IO.File]::ReadAllText($$env:OTTRIN_INSTALLER_PAYLOAD_SCRIPT))) -Phase $$env:OTTRIN_INSTALLER_PAYLOAD_PHASE"'
  Pop $0
  Pop $PayloadTransactionResult
  ${If} $0 == 0
    IfFileExists "$PLUGINSDIR\payload-notice.txt" 0 payload_no_notice
    FileOpen $2 "$PLUGINSDIR\payload-notice.txt" r
    FileReadUTF16LE $2 $PayloadTransactionResult
    FileClose $2
    StrCpy $PayloadTransactionResult "Changed or unowned files were retained at $PayloadTransactionResult"
  ${EndIf}
payload_no_notice:
FunctionEnd

Function RestoreInstallationTransaction
  StrCmp $PayloadTransactionActive 1 0 restore_installation_done
  SetOutPath "$PLUGINSDIR"
  ; A new daemon may already be running. Stop it before moving its executable
  ; or restoring the old task definition and original running intent.
  StrCmp $PayloadPublished 1 0 restore_payload
  ClearErrors
  ExecWait '"$PLUGINSDIR\index-controller.exe" --stop' $0
  IfErrors restore_installation_failed
  StrCmp $0 0 +2
  Goto restore_installation_failed
restore_payload:
  StrCpy $PayloadTransactionPhase Rollback
  Call RunPayloadTransaction
  StrCmp $0 0 +2
  Goto restore_installation_failed
  StrCpy $PayloadPublished 0
  StrCmp $IndexTransactionActive 1 0 finish_payload_rollback
  ClearErrors
  ExecWait '"$PLUGINSDIR\index-controller.exe" --installer-transaction Rollback "$IndexTransactionSnapshot"' $0
  IfErrors restore_installation_failed
  StrCmp $0 0 +2
  Goto restore_installation_failed
  StrCpy $IndexTransactionActive 0
finish_payload_rollback:
  StrCpy $PayloadTransactionPhase FinishRollback
  Call RunPayloadTransaction
  StrCmp $0 0 +2
  Goto restore_installation_failed
  StrCpy $PayloadTransactionActive 0
  ${If} $PayloadTransactionResult != ""
    MessageBox MB_OK|MB_ICONINFORMATION "$PayloadTransactionResult" /SD IDOK
  ${EndIf}
  Goto restore_installation_done
restore_installation_failed:
  MessageBox MB_OK|MB_ICONSTOP "Ottrin could not fully restore the previous installation. Previous files and recovery information remain at $INSTDIR.ottrin-setup and $IndexTransactionSnapshot. Do not delete these records; retry requires recovery of the retained transaction." /SD IDOK
restore_installation_done:
FunctionEnd

Function .onInstFailed
  Call RestoreInstallationTransaction
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
