#!/usr/bin/env bash
# E2E: fleet-wait revival path. Kill a worker pane mid-task and prove:
#   1. the waiting: heartbeat lands while the watcher waits (liveness on disk),
#   2. on timeout the pane classifies dead-pane → failed-reason: + ONE auto
#      re-dispatch as <task-id>-r2 (same brief, full wiring),
#   3. killing the re-dispatched worker too → second failure surfaces to
#      Wesley (stubbed hermes) and STOPS — no -r3.
#
# Usage: bash fleet/test/e2e-revival.test.sh
set -uo pipefail

FLEET_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TS="$(date +%H%M%S)"
TASK_ID="e2e-revive-$TS"
CWD="$HOME/projects/personal/zcode-axi"
STATUS="$FLEET_ROOT/state/$TASK_ID.status"
STATUS2="$FLEET_ROOT/state/$TASK_ID-r2.status"

# Telegram stays quiet: fleet-event's HERMES_BIN override routes "sends" to
# echo. Fast heartbeat so waiting: lines land inside the test window.
export HERMES_BIN=/bin/echo
export FLEET_WAIT_HEARTBEAT_SEC=3

fail() { echo "E2E-FAIL: $*" >&2; exit 1; }

# Bus required: the revival path is push-side; fallback execs fleet-watch
# (pre-revival semantics) and this test would prove nothing.
"$FLEET_ROOT/fleet-bus" status >/dev/null 2>&1 || "$FLEET_ROOT/fleet-bus" ensure >/dev/null 2>&1 || fail "bus unreachable — revival test needs the push layer"

cat >"$FLEET_ROOT/logs/e2e-revive-$TS.brief.md" <<EOF
# Task: revival e2e probe (throwaway)

1. Append one line 'working: reviving slowly' to $FLEET_ROOT/state/$TASK_ID.status
2. Sleep 300 seconds (simulating a long task; do nothing else).
3. Append one line starting 'ready-for-review:' to $FLEET_ROOT/state/$TASK_ID.status
Never write done: or failed:.
EOF
BRIEF="$FLEET_ROOT/logs/e2e-revive-$TS.brief.md"

echo "e2e: dispatching $TASK_ID (zcode, cwd=$CWD)"
"$FLEET_ROOT/fleet-spawn" --harness zcode "$TASK_ID" "$BRIEF" "$CWD" >"$FLEET_ROOT/logs/e2e-revive-$TS.spawn.log" 2>&1 ||
  { cat "$FLEET_ROOT/logs/e2e-revive-$TS.spawn.log" >&2; fail "fleet-spawn failed"; }

"$FLEET_ROOT/fleet-wait" --timeout 45 "$TASK_ID" >"$FLEET_ROOT/logs/e2e-revive-$TS.wait.log" 2>&1 &
WAIT_PID=$!

# Give the worker time to boot and start, then kill it mid-task.
sleep 15
tmux list-windows -t zswarm -F '#{window_name}' | grep -qx "fleet-$TASK_ID" || fail "worker window missing before kill"
tmux kill-window -t "zswarm:fleet-$TASK_ID"
echo "e2e: killed $TASK_ID at t+15s; waiting for timeout + revival"

# 1. waiting: heartbeat while the watcher waits.
HB_OK=""
for _ in $(seq 1 60); do
  [ "$(grep -c '^waiting:' "$STATUS" 2>/dev/null || true)" -ge 3 ] && { HB_OK=yes; break; }
  kill -0 "$WAIT_PID" 2>/dev/null || break
  sleep 1
done
[ -n "$HB_OK" ] || fail "no waiting: heartbeat lines landed in $STATUS"

# 2. revival: the -r2 window appears with the same brief.
R2=""
for _ in $(seq 1 90); do
  if tmux list-windows -t zswarm -F '#{window_name}' 2>/dev/null | grep -qx "fleet-$TASK_ID-r2"; then R2=yes; break; fi
  sleep 1
done
[ -n "$R2" ] || { echo "--- wait.log:"; cat "$FLEET_ROOT/logs/e2e-revive-$TS.wait.log"; fail "no re-dispatch: fleet-$TASK_ID-r2 never appeared"; }
grep -q '^failed-reason: worker died mid-task' "$STATUS" || fail "failed-reason: missing before re-dispatch"
grep -q "re-dispatching once as $TASK_ID-r2" "$FLEET_ROOT/logs/e2e-revive-$TS.wait.log" ||
  fail "wait.log lacks the re-dispatch line"
echo "e2e: revival confirmed — $TASK_ID-r2 dispatched (failed-reason: on disk)"

# 3. second failure: kill the r2 worker too → surface + STOP.
sleep 6   # let r2 boot so its death is a mid-task death, not a spawn race
tmux kill-window -t "zswarm:fleet-$TASK_ID-r2"
echo "e2e: killed $TASK_ID-r2; expecting second-failure surfacing"
wait_rc=""
for _ in $(seq 1 120); do
  if ! kill -0 "$WAIT_PID" 2>/dev/null; then break; fi
  sleep 1
done
kill "$WAIT_PID" 2>/dev/null
wait "$WAIT_PID" 2>/dev/null
RC=$?

grep -q "second failure" "$FLEET_ROOT/logs/e2e-revive-$TS.wait.log" || { echo "--- wait.log:"; cat "$FLEET_ROOT/logs/e2e-revive-$TS.wait.log"; fail "wait.log lacks the second-failure line"; }
grep -q '^failed-reason: worker died mid-task again' "$STATUS2" || fail "r2 status lacks failed-reason:"
if tmux list-windows -t zswarm -F '#{window_name}' 2>/dev/null | grep -qx "fleet-$TASK_ID-r3"; then
  fail "a -r3 window exists — revival did NOT stop after the second failure"
fi
[ "$RC" = "2" ] || fail "fleet-wait exit code = $RC, want 2 (stall/second-failure)"

# Neatness: our windows are already dead; the r2 watcher chain is done.
echo ""
echo "E2E-PASS: revival path proven (heartbeat → failed-reason + one re-dispatch → second failure surfaces, exit 2, no -r3)"
echo "  original status tail: $(tail -1 "$STATUS")"
echo "  r2 status tail:       $(tail -1 "$STATUS2")"
