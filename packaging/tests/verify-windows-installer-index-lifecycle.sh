#!/usr/bin/env bash
# Source-contract fixtures intentionally retain literal NSIS variables.
# shellcheck disable=SC2016
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
INSTALLER="$ROOT/packaging/windows/installer.nsi"

require_literal() {
  local literal="$1"
  if ! grep -Fq -- "$literal" "$INSTALLER"; then
    printf 'Windows installer index lifecycle contract is missing: %s\n' "$literal" >&2
    exit 1
  fi
}

# The installed executable owns Task Scheduler registration. Installation and
# upgrade must quiesce the daemon before replacing files and refresh that
# registration after the payload reaches its final path. A failure must leave an explicit, usable installed-app state rather
# than pretending background refresh succeeded.
require_literal 'ExecWait '\''"$INSTDIR\bin\ottrin.exe" index service-install'\'' $0'
require_literal 'Ottrin was installed, but its background index service could not be enabled.'
require_literal 'You can retry from Services settings.'
require_literal 'background_integration_done:'
require_literal 'File /oname=index-controller.exe "${STAGE}\bin\ottrin-indexd.exe"'
require_literal '--installer-transaction Prepare'
require_literal '--installer-transaction Commit'
require_literal '--installer-transaction Rollback'
require_literal '!define MUI_CUSTOMFUNCTION_ABORT RestoreIndexTransaction'
require_literal 'Function .onInstFailed'
require_literal 'IfErrors index_payload_failed'
require_literal 'SetErrorLevel 1'
require_literal 'kernel32::CreateMutexW'
require_literal 'Global\OttrinInstaller'

require_literal 'ExecWait '\''"$INSTDIR\bin\ottrin.exe" --uninstall'\'' $0'
require_literal 'The application has been left installed so cleanup can be retried.'
require_literal 'Installed Apps registration has been retained so uninstall can be retried.'

file_line="$(grep -Fn 'File /r "${STAGE}\*.*"' "$INSTALLER" | cut -d: -f1)"
install_line="$(grep -Fn 'ExecWait '\''"$INSTDIR\bin\ottrin.exe" index service-install'\'' $0' "$INSTALLER" | cut -d: -f1)"
section_end_line="$(awk '/^SectionEnd$/ { print NR; exit }' "$INSTALLER")"
prepare_line="$(grep -Fn -- '--installer-transaction Prepare' "$INSTALLER" | cut -d: -f1)"
commit_line="$(grep -Fn -- '--installer-transaction Commit' "$INSTALLER" | cut -d: -f1)"
if [[ "$prepare_line" -ge "$file_line" || "$commit_line" -le "$install_line" || "$commit_line" -ge "$section_end_line" ]]; then
  printf 'Windows setup must prepare before payload replacement and commit after task installation.\n' >&2
  exit 1
fi
cleanup_line="$(grep -Fn 'ExecWait '\''"$INSTDIR\bin\ottrin.exe" --uninstall'\'' $0' "$INSTALLER" | cut -d: -f1)"
delete_line="$(grep -Fn 'RMDir /r "$INSTDIR"' "$INSTALLER" | cut -d: -f1)"
if [[ -z "$file_line" || -z "$install_line" || -z "$section_end_line" \
   || "$install_line" -le "$file_line" || "$install_line" -ge "$section_end_line" ]]; then
  printf 'Windows installer must register background search after staging the final payload and before completing the install section.\n' >&2
  exit 1
fi
if [[ -z "$cleanup_line" || -z "$delete_line" || "$cleanup_line" -ge "$delete_line" ]]; then
  printf 'Windows uninstaller must complete application-owned cleanup before deleting installed files.\n' >&2
  exit 1
fi

printf 'Verified Windows installer background-index lifecycle contract.\n'
