#!/usr/bin/env bash
# Provision only disposable GitHub-hosted runners; never change a developer host.
set -euo pipefail
if [[ ${GITHUB_ACTIONS:-} != true || ${RUNNER_OS:-} != Linux || ${RUNNER_ENVIRONMENT:-} != github-hosted || ! -f ${GITHUB_ENV:-} ]]; then
  echo 'Linux isolation provisioning requires a disposable GitHub-hosted Linux runner.' >&2
  exit 1
fi

for program in /usr/bin/bwrap /usr/bin/systemd-run /usr/bin/systemctl; do
  test -x "$program"
done
restriction=/proc/sys/kernel/apparmor_restrict_unprivileged_userns
before=absent
if [[ -f "$restriction" ]]; then
  before="$(cat "$restriction")"
fi
if [[ "$before" == 1 ]]; then
  # Use Ubuntu's restrictive profile, not an unconfined replacement or a sysctl
  # bypass. This policy belongs to the disposable runner, never the application.
  profile=/etc/apparmor.d/bwrap-userns-restrict
  if [[ ! -f "$profile" ]]; then
    sudo install -m 0644 /usr/share/apparmor/extra-profiles/bwrap-userns-restrict "$profile"
  fi
  sudo apparmor_parser -r "$profile"
fi

user_id="$(id -u)"
sudo loginctl enable-linger "$(id -un)"
sudo systemctl start "user@${user_id}.service"
export XDG_RUNTIME_DIR="/run/user/$user_id"
export DBUS_SESSION_BUS_ADDRESS="unix:path=$XDG_RUNTIME_DIR/bus"
# A real user manager is necessary for TasksMax/MemoryMax and descendant teardown.
# A standalone dbus-run-session would leave those assertions untested.
ready=false
for ((attempt=0; attempt<50; attempt++)); do
  if systemctl --user show-environment >/dev/null 2>&1; then
    ready=true
    break
  fi
  sleep 0.2
done
[[ "$ready" == true ]]
printf 'XDG_RUNTIME_DIR=%s\nDBUS_SESSION_BUS_ADDRESS=%s\n' "$XDG_RUNTIME_DIR" "$DBUS_SESSION_BUS_ADDRESS" >> "$GITHUB_ENV"

unit="ottrin-ci-isolation-$$"
trap 'systemctl --user stop "$unit.service" >/dev/null 2>&1 || true' EXIT
sandbox=(/usr/bin/bwrap --unshare-all --die-with-parent --clearenv)
for root in /usr /etc /bin /lib /lib64; do
  if [[ -e "$root" ]]; then sandbox+=(--ro-bind "$root" "$root"); fi
done
sandbox+=(--proc /proc --dev /dev --tmpfs /tmp /usr/bin/true)
timeout 20s systemd-run --user --wait --collect --pipe --quiet \
  --unit="$unit" --property=Type=exec --property=TasksMax=32 \
  --property=MemoryMax=256M --property=RuntimeMaxSec=15s --property=LimitCORE=0 "${sandbox[@]}"
if [[ -f "$restriction" ]]; then
  [[ "$(cat "$restriction")" == "$before" ]]
fi
printf 'Real user-systemd/Bubblewrap isolation ready; namespace restriction=%s\n' "$before"
