#!/usr/bin/env bash
# zc-watch.sh — firstmate-style wait-then-follow watcher.
#
# Usage: zc-watch.sh <task-id>
#
# Waits for fleet/state/<task-id>.status to exist (dispatch may not have
# landed yet), then follows it, printing each new line as it lands, and
# exits when the LAST line is terminal (`done:` or `failed:`). The Hermes
# coordinator runs this as a background job with exit notification — a
# completed watch re-enters the coordinator automatically.
set -euo pipefail

FLEET_ROOT="$(cd "$(dirname "$0")" && pwd)"

[ $# -eq 1 ] || { echo "usage: $0 <task-id>" >&2; exit 1; }
task_id="$1"
status="$FLEET_ROOT/state/$task_id.status"

# Wait phase: the status file may not exist yet.
while [ ! -f "$status" ]; do
  sleep 0.5
done

# Follow phase: print only new lines; exit when the last line is terminal.
last=0
while :; do
  cur="$(wc -l <"$status" | tr -d ' ')"
  if [ "$cur" -gt "$last" ]; then
    sed -n "$((last + 1)),${cur}p" "$status"
    last="$cur"
    lastline="$(tail -n 1 "$status")"
    case "$lastline" in
      done:* | failed:*) exit 0 ;;
    esac
  fi
  sleep 1
done
