#!/usr/bin/env bash
# E2E: dispatch a trivial throwaway task whose brief FORCES a permission
# prompt (read of a file outside the worker's cwd, inside the allowlisted
# projects tree), and prove the approval delegate clears it so the task
# completes without coordinator involvement.
#
# Shape mirrors the observed stalls (ao-zcode-agent2, orca-upstream-zcode2):
# claude-code on claude-haiku-4-5 — auto permission mode is UNAVAILABLE on
# haiku, so the TUI falls back to manual mode and prompts for every read
# outside the cwd. zcode --mode yolo never prompts, so it can't exercise
# this path. This is a REAL TUI dialog, not a fake pane.
#
# Usage: bash fleet/test/e2e-approval-clear.test.sh
set -uo pipefail

FLEET_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TS="$(date +%H%M%S)"
TASK_ID="e2e-appr-$TS"
CWD="$HOME/projects/personal/zcode-axi"
WINDOW="fleet-$TASK_ID"
PROBE="$HOME/projects/personal/approval-e2e-probe-$TS.json"
REPORT="$FLEET_ROOT/logs/e2e-appr-$TS.report.txt"

cleanup() {
  tmux kill-window -t "zswarm:$WINDOW" 2>/dev/null || true
  rm -f "$PROBE"
}
trap cleanup EXIT

fail() { echo "E2E-FAIL: $*" >&2; exit 1; }

# Probe file: outside the worker cwd (forces the prompt), inside
# ~/projects/personal (allowlisted by fleet/approval-policy.txt).
printf '{"probe": "approval-e2e", "ts": "%s"}\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"$PROBE"

cat >"$FLEET_ROOT/logs/e2e-appr-$TS.brief.md" <<EOF
# Task: approval-e2e probe (throwaway)

1. Append one line 'working: starting approval e2e' to $FLEET_ROOT/state/$TASK_ID.status
2. Read the file $PROBE (read access is expected and pre-approved).
3. Append its literal 'probe' field value to $REPORT.
4. Append one line starting 'ready-for-review:' to $FLEET_ROOT/state/$TASK_ID.status including the probe value.
Never write done: or failed:.
EOF
BRIEF="$FLEET_ROOT/logs/e2e-appr-$TS.brief.md"

echo "e2e: dispatching $TASK_ID (claude-code on haiku, cwd=$CWD)"
if ! "$FLEET_ROOT/fleet-spawn" --harness claude-code --model claude-haiku-4-5 "$TASK_ID" "$BRIEF" "$CWD" >"$FLEET_ROOT/logs/e2e-appr-$TS.spawn.log" 2>&1; then
  cat "$FLEET_ROOT/logs/e2e-appr-$TS.spawn.log" >&2
  fail "fleet-spawn failed"
fi
grep -E "delegate:|spawned zswarm" "$FLEET_ROOT/logs/e2e-appr-$TS.spawn.log"

# The wait: proves the task reaches ready-for-review unattended. If the
# delegate does NOT clear the dialog, this times out and the test fails.
START="$(date +%s)"
if ! "$FLEET_ROOT/fleet-wait" --timeout 420 "$TASK_ID" >"$FLEET_ROOT/logs/e2e-appr-$TS.wait.log" 2>&1; then
  echo "--- wait output:"; tail -5 "$FLEET_ROOT/logs/e2e-appr-$TS.wait.log"
  echo "--- status:"; cat "$FLEET_ROOT/state/$TASK_ID.status" 2>/dev/null
  echo "--- approvals:"; cat "$FLEET_ROOT/logs/$TASK_ID.approvals.log" 2>/dev/null
  echo "--- pane tail:"; tmux capture-pane -p -t "zswarm:$WINDOW" -S -25 2>/dev/null | tail -15
  fail "fleet-wait did not reach an attention state within 420s"
fi
DUR=$(( $(date +%s) - START ))

grep -q "ready-for-review" "$FLEET_ROOT/logs/e2e-appr-$TS.wait.log" || fail "attention state was not ready-for-review: $(tail -1 "$FLEET_ROOT/logs/e2e-appr-$TS.wait.log")"
grep -qE "class=read-own-tree" "$FLEET_ROOT/logs/$TASK_ID.approvals.log" 2>/dev/null \
  || fail "delegate audit has no read-own-tree auto-answer (did a dialog even appear?)"
grep -q "approval-e2e" "$REPORT" 2>/dev/null || fail "report missing probe value — task did not truly complete"
grep -qi "needs-decision" "$FLEET_ROOT/state/$TASK_ID.status" && fail "status contains an escalation — delegate did not clear the dialog"

echo ""
echo "E2E-PASS: dialog auto-cleared, task completed unattended in ${DUR}s"
echo "  audit:    $(grep 'approval:auto' "$FLEET_ROOT/logs/$TASK_ID.approvals.log" | head -1)"
echo "  status:   $(tail -1 "$FLEET_ROOT/state/$TASK_ID.status")"
