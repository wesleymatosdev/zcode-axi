#!/usr/bin/env bash
# zc-steer.sh — coordinator-only steering/injection into a live worker TUI.
#
# Usage: zc-steer.sh <task-id> <one-line message>
#
# fleet-v2 P1. Appends a `steered:` line to the task status (the durable
# record) and delivers the message to the worker's pane with tmux send-keys
# (-l, then Enter as a separate call — the proven lane). Messages are
# SHORT POINTERS by contract ("re-read section X of your brief",
# "decision: take option B"); long instructional bodies belong in the brief
# file, not the composer.
set -euo pipefail

FLEET_ROOT="$(cd "$(dirname "$0")" && pwd)"

[ $# -eq 2 ] || { echo "usage: $0 <task-id> <one-line message>" >&2; exit 1; }
task_id="$1"
msg="$2"
status="$FLEET_ROOT/state/$task_id.status"
window="fleet-$task_id"

[ -f "$status" ] || { echo "zc-steer: no such task: $task_id" >&2; exit 1; }
if [ "${#msg}" -gt 300 ]; then
  echo "zc-steer: message too long (${#msg} > 300 chars); put detail in the brief and steer with a pointer" >&2
  exit 1
fi
if ! tmux list-windows -t zswarm -F '#{window_name}' 2>/dev/null | grep -qx "$window"; then
  echo "zc-steer: window zswarm:$window does not exist" >&2
  exit 1
fi

msg="${msg//$'\n'/ }"
printf 'steered: %s\n' "$msg" >>"$status"
tmux send-keys -t "zswarm:$window" -l "$msg"
tmux send-keys -t "zswarm:$window" Enter
echo "steered $task_id"
