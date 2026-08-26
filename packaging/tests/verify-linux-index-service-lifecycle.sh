#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
APP="${OTTRIN_APP:-$ROOT/target/debug/ottrin-app}"
INDEXD="${OTTRIN_INDEXD:-$ROOT/target/debug/ottrin-indexd}"
for binary in "$APP" "$INDEXD"; do
  if [[ ! -x "$binary" ]]; then
    printf 'required executable is unavailable: %s\n' "$binary" >&2
    exit 1
  fi
done

RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
SANDBOX="$ROOT/.hermes/service-harness/$RUN_ID"
RUNTIME="$ROOT/.hermes/service-runtime-$RUN_ID"
mkdir -p "$SANDBOX"/{bin,cache,config,home,stage} "$RUNTIME"
chmod 700 "$RUNTIME"
cp "$APP" "$SANDBOX/stage/ottrin"
cp "$INDEXD" "$SANDBOX/stage/ottrin-indexd"
printf '#!/bin/sh\nexit 0\n' >"$SANDBOX/bin/crontab"
chmod 755 "$SANDBOX/bin/crontab"
printf 'alpha\n' >"$SANDBOX/home/alpha-service-fixture.txt"

export SANDBOX RUNTIME

dbus-run-session -- bash -s <<'INNER'
set -euo pipefail
export HOME="$SANDBOX/home"
export XDG_CONFIG_HOME="$SANDBOX/config"
export XDG_CACHE_HOME="$SANDBOX/cache"
export XDG_RUNTIME_DIR="$RUNTIME"
export PATH="$SANDBOX/bin:$PATH"
APP="$SANDBOX/stage/ottrin"
UNIT="$XDG_CONFIG_HOME/systemd/user/ottrin-index.service"
DB="$XDG_CACHE_HOME/ottrin/search.db"
SOCKET="$XDG_RUNTIME_DIR/ottrin/index-v1.sock"

systemd --user --unit=basic.target >"$SANDBOX/systemd.log" 2>&1 &
SYSTEMD_PID=$!
cleanup() {
  systemctl --user stop ottrin-index.service >/dev/null 2>&1 || true
  systemctl --user exit >/dev/null 2>&1 || kill "$SYSTEMD_PID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

for attempt in $(seq 1 100); do
  state="$(systemctl --user is-system-running 2>/dev/null || true)"
  case "$state" in
    running|degraded) break ;;
  esac
  if [[ "$attempt" == 100 ]]; then
    printf 'isolated systemd user manager did not become ready\n' >&2
    exit 1
  fi
  sleep 0.05
done
systemctl --user import-environment HOME XDG_CONFIG_HOME XDG_CACHE_HOME XDG_RUNTIME_DIR PATH

"$APP" index service-install >"$SANDBOX/install.out"
test -f "$UNIT"
grep -Fq "ExecStart=\"$SANDBOX/stage/ottrin-indexd\" --index-daemon" "$UNIT"
systemctl --user is-enabled --quiet ottrin-index.service
for attempt in $(seq 1 100); do
  if systemctl --user is-active --quiet ottrin-index.service; then
    break
  fi
  if systemctl --user is-failed --quiet ottrin-index.service; then
    systemctl --user status --no-pager ottrin-index.service >"$SANDBOX/service-failure.log" 2>&1 || true
    cat "$SANDBOX/service-failure.log" >&2
    exit 1
  fi
  sleep 0.05
done
systemctl --user status --no-pager ottrin-index.service >"$SANDBOX/service-status-initial.log" 2>&1 || true
if ! systemctl --user is-active --quiet ottrin-index.service; then
  cat "$SANDBOX/service-status-initial.log" >&2
  exit 1
fi

wait_for_query() {
  local needle="$1"
  local output="$2"
  for attempt in $(seq 1 100); do
    if "$APP" search "$needle" --json >"$output" 2>"$output.err" && \
       python3 - "$output" <<'PY'
import json, sys
with open(sys.argv[1], encoding='utf-8') as stream:
    found = json.load(stream)['total'] == 1
sys.exit(0 if found else 1)
PY
    then
      return 0
    fi
    sleep 0.05
  done
  printf 'query did not observe %s\n' "$needle" >&2
  return 1
}

wait_for_query alpha-service-fixture "$SANDBOX/alpha.json"
"$APP" index status --json >"$SANDBOX/status.json"
python3 - "$SANDBOX/status.json" <<'PY'
import json, sys
with open(sys.argv[1], encoding='utf-8') as stream:
    status = json.load(stream)
assert status['configured_roots'] == 1, status
assert status['watcher_ready'] is True, status
PY

"$APP" search alpha-service-fixture --json >"$SANDBOX/client-a.json" &
CLIENT_A=$!
"$APP" index status --json >"$SANDBOX/client-b.json" &
CLIENT_B=$!
wait "$CLIENT_A"
wait "$CLIENT_B"

printf 'beta\n' >"$HOME/beta-watcher-fixture.txt"
wait_for_query beta-watcher-fixture "$SANDBOX/beta.json"
"$APP" index rebuild --json >"$SANDBOX/rebuild.json"

PID_BEFORE="$(systemctl --user show -p MainPID --value ottrin-index.service)"
"$APP" index service-restart >"$SANDBOX/restart.out"
PID_AFTER="$(systemctl --user show -p MainPID --value ottrin-index.service)"
test "$PID_BEFORE" != "$PID_AFTER"
wait_for_query beta-watcher-fixture "$SANDBOX/after-restart.json"

test -f "$DB"
cp "$SANDBOX/stage/ottrin-indexd" "$SANDBOX/stage/ottrin-indexd.upgrade"
mv "$SANDBOX/stage/ottrin-indexd.upgrade" "$SANDBOX/stage/ottrin-indexd"
PID_BEFORE_UPGRADE="$PID_AFTER"
"$APP" index service-install >"$SANDBOX/upgrade.out"
PID_AFTER_UPGRADE="$(systemctl --user show -p MainPID --value ottrin-index.service)"
test "$PID_BEFORE_UPGRADE" != "$PID_AFTER_UPGRADE"
test ! -e "$SOCKET" || systemctl --user is-active --quiet ottrin-index.service
wait_for_query beta-watcher-fixture "$SANDBOX/after-upgrade.json"

"$APP" index service-status >"$SANDBOX/service-status.out"
grep -Fq 'enabled=true' "$SANDBOX/service-status.out"
"$APP" index service-remove >"$SANDBOX/remove.out"
test ! -e "$UNIT"
! systemctl --user is-active --quiet ottrin-index.service
test -f "$DB"
"$APP" index service-status >"$SANDBOX/service-status-removed.out"
grep -Fq 'enabled=false' "$SANDBOX/service-status-removed.out"

printf 'packaged Linux index service lifecycle: PASS\n'
INNER

printf 'evidence: %s\n' "$SANDBOX"
