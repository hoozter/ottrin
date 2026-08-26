#!/usr/bin/env bash
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
# upgrade must refresh that registration after the payload reaches its final
# path, and a failure must leave an explicit, usable installed-app state rather
# than pretending background refresh succeeded.
require_literal 'ExecWait '\''"$INSTDIR\bin\ottrin.exe" index service-install'\'' $0'
require_literal 'Ottrin was installed, but its background search refresh could not be enabled.'
require_literal 'You can retry from Search settings.'
require_literal 'background_integration_done:'
require_literal 'ExecWait '\''"$INSTDIR\bin\ottrin.exe" --uninstall'\'' $0'
require_literal 'The application has been left installed so cleanup can be retried.'
require_literal 'Installed Apps registration has been retained so uninstall can be retried.'

file_line="$(grep -Fn 'File /r "${STAGE}\*.*"' "$INSTALLER" | cut -d: -f1)"
install_line="$(grep -Fn 'ExecWait '\''"$INSTDIR\bin\ottrin.exe" index service-install'\'' $0' "$INSTALLER" | cut -d: -f1)"
section_end_line="$(awk 'NR > 36 && /^SectionEnd$/ { print NR; exit }' "$INSTALLER")"
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
