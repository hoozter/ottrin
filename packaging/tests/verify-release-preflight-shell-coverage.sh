#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PREFLIGHT="$ROOT/scripts/release-preflight.sh"

require_shell_gate() {
  local script="$1"
  if ! sed -n '/^shell_scripts=(/,/^)$/p' "$PREFLIGHT" | grep -Fqx "  $script"; then
    printf 'release preflight omits shell syntax coverage for: %s\n' "$script" >&2
    exit 1
  fi
}

# The installer is a release artifact producer, not merely a helper around the
# portable stage. It carries the graphics-runtime and UAC-facing payload, so a
# preflight syntax pass must cover it alongside every other shell packager.
require_shell_gate 'packaging/windows/package-installer.sh'
require_shell_gate 'packaging/linux/bundle-runtime-libs.sh'
require_shell_gate 'packaging/macos/stage-app-bundle.sh'
require_shell_gate 'packaging/macos/package-app.sh'
require_shell_gate 'packaging/tests/verify-macos-app-bundle-contract.sh'
require_shell_gate 'packaging/tests/verify-macos-package-contract.sh'
require_shell_gate 'scripts/linux-launch-smoke.sh'
require_shell_gate 'scripts/windows-gui-subsystem-smoke.sh'
require_shell_gate 'scripts/windows-runtime-launch-smoke.sh'
require_shell_gate 'packaging/tests/verify-windows-opengl-loader-contract.sh'

printf 'Verified release preflight shell coverage.\n'
