#!/usr/bin/env bash
# Static preflight guard only; native lifecycle acceptance remains separate.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
python3 - "$ROOT/packaging/windows/installer.nsi" <<'PY'
from pathlib import Path
import sys
source=Path(sys.argv[1]).read_text()
def require(text, scope=source):
    if text not in scope: raise SystemExit('Missing Windows installer lifecycle guard: '+text)
setup=source.split('Section "Ottrin" SEC_MAIN',1)[1].split('SectionEnd',1)[0]
# Recovery must precede a new transaction; extraction finishes before stopping
# the daemon, publication precedes registration, and durable commit precedes
# disposal of the scheduler snapshot. Search in order because recovery itself
# can also finish an earlier Commit.
position=0
for token in [
    'StrCpy $PayloadTransactionPhase RecoveryPlan',
    'StrCpy $PayloadTransactionPhase Initialize',
    'File /r "${STAGE}\\*.*"',
    '--installer-transaction Prepare',
    'StrCpy $PayloadTransactionPhase BeginIntegration',
    'StrCpy $PayloadTransactionPhase Publish',
    '"$INSTDIR\\bin\\ottrin.exe" index service-install',
    '--installer-transaction Activate',
    'StrCpy $PayloadTransactionPhase SealCommit',
    '--installer-transaction Commit',
    'StrCpy $PayloadTransactionPhase Commit',
]:
    found=setup.find(token,position)
    if found<0: raise SystemExit('Windows installer lifecycle order is missing: '+token)
    position=found+len(token)
require('File /oname=index-controller.exe "${STAGE}\\bin\\ottrin-indexd.exe"',setup)
require('!define MUI_CUSTOMFUNCTION_ABORT RestoreInstallationTransaction')
require('Function .onInstFailed\n  Call RestoreInstallationTransaction')
require('kernel32::CreateMutexW')
require('Global\\OttrinInstaller')
require('StrCpy $IndexTransactionSnapshot "$INSTDIR.ottrin-setup\\index-transaction.json"',setup)
restore=source.split('Function RestoreInstallationTransaction',1)[1].split('FunctionEnd',1)[0]
position=0
for token in ['--stop','StrCpy $PayloadTransactionPhase Rollback','--installer-transaction Rollback','StrCpy $PayloadTransactionPhase FinishRollback']:
    found=restore.find(token,position)
    if found<0: raise SystemExit('Windows rollback lifecycle order is missing: '+token)
    position=found+len(token)
cleanup=source.split('Section "Uninstall"',1)[1]
require('"$INSTDIR\\bin\\ottrin.exe" --uninstall',cleanup)
require('The application has been left installed so cleanup can be retried.',cleanup)
require('Installed Apps registration has been retained so uninstall can be retried.',cleanup)
if cleanup.index('"$INSTDIR\\bin\\ottrin.exe" --uninstall') >= cleanup.index('RMDir /r "$INSTDIR"'):
    raise SystemExit('Uninstall must complete application cleanup before deleting payload')
require('SetErrorLevel 1')
print('Verified Windows installer recovery, staging, index and commit source contract.')
PY
