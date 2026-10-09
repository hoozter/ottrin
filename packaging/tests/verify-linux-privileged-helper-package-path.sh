#!/usr/bin/env bash
set -euo pipefail

# Check produced packages, not just the staging recipes. Each argument is
# FORMAT=ARTIFACT, where FORMAT is portable, appimage, deb, or rpm.
(( $# > 0 )) || { echo "usage: $0 FORMAT=ARTIFACT [...]" >&2; exit 2; }
WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT
POLICY_REL=usr/share/polkit-1/actions/org.ottrin.filesystem.policy
HELPER_REL=usr/libexec/ottrin/ottrin-priv-helper
INSTALLED_HELPER=/$HELPER_REL

check_payload() {
  local label="$1" policy="$2" helper="$3"
  test -x "$helper" || { echo "$label: helper missing or not executable: $helper" >&2; exit 1; }
  python3 - "$policy" "$INSTALLED_HELPER" "$label" <<'PY'
import sys
import xml.etree.ElementTree as ET

policy, expected, label = sys.argv[1:]
root = ET.parse(policy).getroot()
annotations = root.findall(".//action[@id='org.ottrin.filesystem.privileged']/annotate[@key='org.freedesktop.policykit.exec.path']")
if len(annotations) != 1 or annotations[0].text != expected:
    raise SystemExit(f"{label}: policy exec.path does not name installed helper {expected}")
PY
}

for argument in "$@"; do
  format="${argument%%=*}"
  artifact="${argument#*=}"
  test "$format" != "$artifact" && test -f "$artifact" || {
    echo "usage: $0 FORMAT=ARTIFACT [...]" >&2; exit 2;
  }
  artifact="$(realpath "$artifact")"
  destination="$WORK/$format"
  mkdir -p "$destination"
  case "$format" in
    portable)
      tar -xzf "$artifact" -C "$destination"
      stage="$(find "$destination" -mindepth 1 -maxdepth 1 -type d -print -quit)"
      check_payload "$format" "$stage/packaging/linux/org.ottrin.filesystem.policy" "$stage/bin/ottrin-priv-helper"
      grep -Fq "install -D -m 0755 ./bin/ottrin-priv-helper $INSTALLED_HELPER" "$stage/RUNNING.md"
      grep -Fq -- "OTTRIN_PRIV_HELPER=$INSTALLED_HELPER" "$stage/bin/ottrin"
      ;;
    appimage)
      (cd "$destination" && "$artifact" --appimage-extract >/dev/null)
      stage="$destination/squashfs-root"
      check_payload "$format" "$stage/$POLICY_REL" "$stage/$HELPER_REL"
      grep -Fq -- "OTTRIN_PRIV_HELPER=$INSTALLED_HELPER" "$stage/AppRun"
      ;;
    deb)
      dpkg-deb -x "$artifact" "$destination"
      check_payload "$format" "$destination/$POLICY_REL" "$destination/$HELPER_REL"
      ;;
    rpm)
      (cd "$destination" && 7z x -so "$artifact" | cpio -idm --quiet)
      check_payload "$format" "$destination/$POLICY_REL" "$destination/$HELPER_REL"
      ;;
    *) echo "Unknown format: $format" >&2; exit 2 ;;
  esac
  echo "$format: packaged helper and policy path PASS"
done
