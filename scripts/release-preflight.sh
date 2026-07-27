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
  build-all.sh
  packaging/linux/package-appimage.sh
  packaging/linux/package-portable.sh
  packaging/verify-release-artifacts.sh
  packaging/windows/package-installer.sh
  packaging/windows/package-portable.sh
  packaging/tests/verify-windows-installer-powershell-provenance.sh
  scripts/release-preflight.sh
)

run_gate "Shell syntax" bash -n "${shell_scripts[@]}"
run_gate "Windows PowerShell installer provenance guard" \
  ./packaging/tests/verify-windows-installer-powershell-provenance.sh
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
