#!/usr/bin/env bash
# zc-spawn.sh — spawn a named ZCode crewmate in tmux session `zswarm`.
#
# Usage: zc-spawn.sh <task-id> <brief-file> [cwd]
#
# Creates tmux window `fleet-<task-id>` (session `zswarm` is created if it
# does not exist). The pane runs `zcode -p "<brief content>"` with all
# output tee'd to fleet/logs/<task-id>.log, and a `spawned:` line is
# appended to fleet/state/<task-id>.status. Returns immediately after
# spawn — it never waits for the run to finish.
#
# Window law: NEVER kills or renames windows it did not create. A task id
# whose window already exists is refused, never clobbered.
set -euo pipefail

FLEET_ROOT="$(cd "$(dirname "$0")" && pwd)"

usage() { echo "usage: $0 <task-id> <brief-file> [cwd]" >&2; exit 1; }

[ $# -ge 2 ] && [ $# -le 3 ] || usage
task_id="$1"
brief="$2"
cwd="${3:-$PWD}"

case "$task_id" in
  *[!A-Za-z0-9._-]*)
    echo "zc-spawn: task-id may contain only [A-Za-z0-9._-]: $task_id" >&2
    exit 1
    ;;
esac
[ -f "$brief" ] || { echo "zc-spawn: brief not found: $brief" >&2; exit 1; }
[ -d "$cwd" ] || { echo "zc-spawn: cwd not found: $cwd" >&2; exit 1; }
command -v tmux >/dev/null || { echo "zc-spawn: tmux not on PATH" >&2; exit 1; }
command -v zcode >/dev/null || { echo "zc-spawn: zcode not on PATH" >&2; exit 1; }

brief_abs="$(cd "$(dirname "$brief")" && pwd)/$(basename "$brief")"
cwd_abs="$(cd "$cwd" && pwd)"
mkdir -p "$FLEET_ROOT/logs" "$FLEET_ROOT/state"
log="$FLEET_ROOT/logs/$task_id.log"
status="$FLEET_ROOT/state/$task_id.status"
window="fleet-$task_id"

if tmux list-windows -t zswarm -F '#{window_name}' 2>/dev/null | grep -qx "$window"; then
  echo "zc-spawn: window zswarm:$window already exists; refusing to clobber" >&2
  exit 1
fi

# Session zswarm: create only if missing. Other sessions/windows untouched.
tmux has-session -t zswarm 2>/dev/null || tmux new-session -d -s zswarm

# Single-quote a string for safe embedding in the pane's sh command line.
esc() { printf "'%s'" "${1//\'/\'\\\'\'}"; }

# The pane runs the brief through zcode, records the real exit code in the
# log, and the window stays up after exit (remain-on-exit, set below) so
# the coordinator can capture-pane the evidence before archiving the task.
tmux new-window -d -t zswarm -n "$window" \
  "cd $(esc "$cwd_abs") && set -o pipefail && zcode -p \"\$(cat $(esc "$brief_abs"))\" 2>&1 | tee -a $(esc "$log"); rc=\$?; echo \"[fleet] zcode exited rc=\$rc\" | tee -a $(esc "$log")"

# Window law: options are scoped to OUR window only. remain-on-exit keeps
# the finished crewmate inspectable until the coordinator archives it.
tmux set-option -t "zswarm:$window" remain-on-exit on >/dev/null

# Best-effort zcode pid (a child of the pane shell). Bounded wait; the
# spawn never blocks on the run itself.
pane_pid="$(tmux list-panes -t "zswarm:$window" -F '#{pane_pid}' 2>/dev/null | head -n 1)"
zpid="unknown"
if [ -n "$pane_pid" ]; then
  for _ in $(seq 1 40); do
    for child in $(pgrep -P "$pane_pid" 2>/dev/null || true); do
      if ps -o command= -p "$child" 2>/dev/null | grep -qi zcode; then
        zpid="$child"
        break
      fi
    done
    [ "$zpid" != "unknown" ] && break
    sleep 0.25
  done
fi

ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
printf 'spawned: %s brief=%s cwd=%s pid=%s\n' "$ts" "$brief_abs" "$cwd_abs" "$zpid" >>"$status"
echo "spawned zswarm:$window pid=$zpid"
echo "status=$status"
echo "log=$log"
