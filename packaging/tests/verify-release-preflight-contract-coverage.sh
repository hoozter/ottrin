#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PREFLIGHT="$ROOT/scripts/release-preflight.sh"

require_contract_gate() {
  local script="$1"
  if ! grep -Fqx "  bash $script" "$PREFLIGHT"; then
    printf 'release preflight omits packaging contract gate: %s\n' "$script" >&2
    exit 1
  fi
}

# --source-only is the only proportional release gate available before fresh
# artifacts exist. It must execute every checked-in packaging contract test,
# so a regression in an artifact producer or final verifier is not deferred
# until a manually assembled artifact directory happens to be available.
for script in \
  packaging/tests/verify-linux-appimage-documentation.sh \
  packaging/tests/verify-linux-gio-runtime-contract.sh \
  packaging/tests/verify-linux-portable-running-instructions.sh \
  packaging/tests/verify-linux-privileged-helper-policy.sh \
  packaging/tests/verify-release-checklist-provenance.sh \
  packaging/tests/verify-release-preflight-contract-coverage.sh \
  packaging/tests/verify-release-preflight-shell-coverage.sh \
  packaging/tests/verify-release-provenance.sh \
  packaging/tests/verify-release-workflow-dependencies.sh \
  packaging/tests/verify-windows-graphics-runtime.sh \
  packaging/tests/verify-windows-installer-clean-checkout.sh \
  packaging/tests/verify-windows-installer-output-path.sh \
  packaging/tests/verify-windows-installer-powershell-provenance.sh \
  packaging/tests/verify-windows-installer-stage-version.sh \
  packaging/tests/verify-windows-runtime-temp-cleanup.sh; do
  require_contract_gate "$script"
done

printf 'Verified release preflight packaging-contract coverage.\n'
