#!/usr/bin/env bash
# zc-recover.sh — fleet watchdog: fail only provably dead workers.
#
# Usage: zc-recover.sh
#
# fleet-v2 P1 (revised after the 2026-09-08 false-positive incident): a task
# is declared dead ONLY when its worker pid is gone. A missing/moved window
# name is NOT death (tmux automatic-rename flicker bit us once); the window
# check is advisory only. For every open (nonterminal) task whose pid is
# dead, appends the coordinator verdict
#   failed: worker died mid-task (pid <pid> gone); artifacts NOT verified
# Silent (exit 0) when nothing to do; prints one line per action otherwise.
set -euo pipefail

FLEET_ROOT="$(cd "$(dirname "$0")" && pwd)"
state="$FLEET_ROOT/state"

shopt -s nullglob
for sf in "$state"/*.status; do
  id="$(basename "$sf" .status)"
  last="$(tail -n 1 "$sf")"
  case "$last" in
    done:* | failed:* | blocked:* | needs-decision:*) continue ;;
  esac
  pid="$(sed -n 's/^spawned: .* pid=\([0-9][0-9]*\)$/\1/p' "$sf" | tail -n 1)"
  if [ -n "$pid" ] && ! ps -p "$pid" >/dev/null 2>&1; then
    printf 'failed: worker died mid-task (pid %s gone); artifacts NOT verified — coordinator must reconcile or re-dispatch\n' "$pid" >>"$sf"
    echo "recovered: $id -> failed (pid $pid gone)"
  fi
done
exit 0
