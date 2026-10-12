#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
verify_artifacts=true
require_optional_tools=false

usage() {
  cat <<'EOF'
Run Ottrin's non-publishing release preflight.

Usage:
  ./scripts/release-preflight.sh [options]

Options:
  --source-only             Run source and script gates without inspecting dist/.
  --require-optional-tools  Fail unless ShellCheck, actionlint, appstreamcli,
                            and xmllint are installed.
  --help                    Show this help.

This command never commits, tags, pushes, publishes, or installs system files.
It validates the current worktree and, by default, the existing release
artifacts in dist/. Build or rebuild artifacts separately before running it.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --source-only)
      verify_artifacts=false
      ;;
    --require-optional-tools)
      require_optional_tools=true
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      printf 'Unknown option: %s\n\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

cd "$ROOT"

run_gate() {
  local label="$1"
  shift
  printf '\n==> %s\n' "$label"
  "$@"
}

optional_gate() {
  local command_name="$1"
  local label="$2"
  shift 2
  if command -v "$command_name" >/dev/null 2>&1; then
    run_gate "$label" "$@"
  elif [[ "$require_optional_tools" == true ]]; then
    printf 'Required preflight tool is unavailable: %s\n' "$command_name" >&2
    exit 1
  else
    printf '\n==> Skipping %s; %s is unavailable\n' "$label" "$command_name"
  fi
}

run_gate "Formatting" cargo fmt --all --check
run_gate "Workspace check" cargo check --workspace --all-targets
run_gate "Workspace tests" cargo test --workspace
run_gate "Strict Clippy" \
  cargo clippy --workspace --all-targets --all-features -- -D warnings

shell_scripts=(
  packaging/package-lifecycle.sh
  build-all.sh
  packaging/linux/package-appimage.sh
  packaging/linux/package-portable.sh
  packaging/linux/sign-release-artifacts.sh
  packaging/linux/verify-release-signatures.sh
  packaging/linux/package-pdfium.sh
  packaging/linux/stage-native-package.sh
  packaging/linux/package-deb.sh
  packaging/linux/package-rpm.sh
  packaging/linux/bundle-runtime-libs.sh
  packaging/macos/stage-app-bundle.sh
  packaging/macos/package-app.sh
  packaging/macos/sign-and-notarize-release.sh
  packaging/macos/verify-release-signatures.sh
  packaging/verify-release-artifacts.sh
  packaging/tests/verify-linux-privileged-helper-package-path.sh
  packaging/windows/package-installer.sh
  packaging/windows/package-msi.sh
  packaging/windows/package-portable.sh
  packaging/tests/verify-release-workflow-dependencies.sh
  packaging/tests/verify-release-signing-integration.sh
  packaging/tests/verify-windows-installer-clean-checkout.sh
  packaging/tests/verify-windows-installer-index-lifecycle.sh
  packaging/tests/verify-windows-msi-lifecycle.sh
  packaging/tests/verify-windows-installer-output-path.sh
  packaging/tests/verify-windows-installer-powershell-provenance.sh
  packaging/tests/verify-windows-installer-stage-version.sh
  packaging/tests/verify-windows-runtime-temp-cleanup.sh
  packaging/tests/verify-linux-appimage-documentation.sh
  packaging/tests/verify-linux-gio-runtime-contract.sh
  packaging/tests/verify-linux-portable-running-instructions.sh
  packaging/tests/verify-linux-native-package-contract.sh
  packaging/tests/verify-macos-app-bundle-contract.sh
  packaging/tests/verify-macos-package-contract.sh
  scripts/linux-launch-smoke.sh
  scripts/windows-gui-subsystem-smoke.sh
  scripts/windows-runtime-launch-smoke.sh
  packaging/tests/verify-linux-privileged-helper-policy.sh
  packaging/tests/verify-release-checklist-provenance.sh
  packaging/tests/verify-release-preflight-contract-coverage.sh
  packaging/tests/verify-release-preflight-shell-coverage.sh
  packaging/tests/verify-release-provenance.sh
  packaging/tests/verify-windows-opengl-loader-contract.sh
  packaging/tests/verify-windows-graphics-runtime.sh
  scripts/release-preflight.sh
)

run_gate "Shell syntax" bash -n "${shell_scripts[@]}"
run_gate "Windows PowerShell installer provenance guard" \
  bash packaging/tests/verify-windows-installer-powershell-provenance.sh
run_gate "Windows installer staged package-version guard" \
  bash packaging/tests/verify-windows-installer-stage-version.sh
run_gate "Windows installer clean-checkout guard" \
  bash packaging/tests/verify-windows-installer-clean-checkout.sh
run_gate "Windows installer background-index lifecycle" \
  bash packaging/tests/verify-windows-installer-index-lifecycle.sh
run_gate "Windows MSI lifecycle and provenance" \
  bash packaging/tests/verify-windows-msi-lifecycle.sh
run_gate "Release workflow dependency guards" \
  bash packaging/tests/verify-release-workflow-dependencies.sh
run_gate "Release signing integration contract" \
  bash packaging/tests/verify-release-signing-integration.sh
run_gate "Linux release signing runtime" \
  python3 packaging/tests/verify-linux-release-signing-runtime.py
run_gate "Linux AppImage documentation contract" \
  bash packaging/tests/verify-linux-appimage-documentation.sh
run_gate "Linux GIO runtime packaging contract" \
  bash packaging/tests/verify-linux-gio-runtime-contract.sh
run_gate "Linux portable runtime-instructions contract" \
  bash packaging/tests/verify-linux-portable-running-instructions.sh
run_gate "Linux native package contract" \
  bash packaging/tests/verify-linux-native-package-contract.sh
run_gate "macOS app-bundle staging contract" \
  bash packaging/tests/verify-macos-app-bundle-contract.sh
run_gate "macOS app archive contract" \
  bash packaging/tests/verify-macos-package-contract.sh
run_gate "Linux privileged-helper policy contract" \
  bash packaging/tests/verify-linux-privileged-helper-policy.sh
run_gate "Release checklist provenance contract" \
  bash packaging/tests/verify-release-checklist-provenance.sh
run_gate "Release preflight packaging-contract coverage" \
  bash packaging/tests/verify-release-preflight-contract-coverage.sh
run_gate "Release preflight shell coverage" \
  bash packaging/tests/verify-release-preflight-shell-coverage.sh
run_gate "Release artifact provenance contract" \
  bash packaging/tests/verify-release-provenance.sh
run_gate "Windows graphics runtime contract" \
  bash packaging/tests/verify-windows-graphics-runtime.sh
if [[ "$verify_artifacts" == true ]]; then
  run_gate "Windows app-local OpenGL loader contract" \
    bash packaging/tests/verify-windows-opengl-loader-contract.sh
fi
run_gate "Windows installer output-path guard" \
  bash packaging/tests/verify-windows-installer-output-path.sh
run_gate "Windows runtime temporary-directory cleanup" \
  bash packaging/tests/verify-windows-runtime-temp-cleanup.sh
if [[ "$verify_artifacts" == true ]]; then
  if command -v wine >/dev/null 2>&1; then
    run_gate "Windows portable OpenGL compatibility launch" \
      bash scripts/windows-runtime-launch-smoke.sh \
        dist/Ottrin-0.1.0-windows-x64/bin/ottrin.exe --renderer opengl
  else
    printf '\n==> Windows runtime launch unavailable; Wine is not installed\n' >&2
  fi
fi
optional_gate shellcheck "ShellCheck" shellcheck -x "${shell_scripts[@]}"
optional_gate actionlint "GitHub Actions workflows" actionlint \
  .github/workflows/ci.yml .github/workflows/release.yml
optional_gate appstreamcli "AppStream metadata" appstreamcli validate --no-net \
  packaging/linux/ottrin.appdata.xml
optional_gate xmllint "XML metadata" xmllint --noout \
  packaging/linux/ottrin.appdata.xml \
  packaging/linux/org.ottrin.filesystem.policy

if [[ "$verify_artifacts" == true ]]; then
  run_gate "Release artifacts" ./packaging/verify-release-artifacts.sh dist
else
  printf '\n==> Skipping release artifacts (--source-only)\n'
fi

printf '\nOttrin release preflight passed.\n'
