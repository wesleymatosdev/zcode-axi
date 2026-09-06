#!/usr/bin/env bash
# zc-status.sh — supervisor dashboard feed.
#
# Usage: zc-status.sh [task-id]
#
# One line per fleet task: id, last status line, log byte size, and whether
# the spawned zcode process is still alive. With no argument: all tasks.
set -euo pipefail

FLEET_ROOT="$(cd "$(dirname "$0")" && pwd)"
state="$FLEET_ROOT/state"

usage() { echo "usage: $0 [task-id]" >&2; exit 1; }

emit() {
  id="$1"
  sf="$state/$id.status"
  lf="$FLEET_ROOT/logs/$id.log"
  last="$(tail -n 1 "$sf" 2>/dev/null || true)"
  bytes=0
  [ -f "$lf" ] && bytes="$(wc -c <"$lf" | tr -d ' ')"
  pid="$(sed -n 's/^spawned: .* pid=\([0-9][0-9]*\)$/\1/p' "$sf" | tail -n 1)"
  alive=no
  if [ -n "$pid" ] && ps -p "$pid" >/dev/null 2>&1; then
    alive=yes
  fi
  printf 'id=%s\tlast="%s"\tlog_bytes=%s\tzcode_alive=%s\n' "$id" "$last" "$bytes" "$alive"
}

case $# in
  0)
    shopt -s nullglob
    files=("$state"/*.status)
    [ ${#files[@]} -gt 0 ] || { echo "zc-status: no fleet tasks" >&2; exit 1; }
    for sf in "${files[@]}"; do
      emit "$(basename "$sf" .status)"
    done
    ;;
  1)
    [ -f "$state/$1.status" ] || { echo "zc-status: no such task: $1" >&2; exit 1; }
    emit "$1"
    ;;
  *)
    usage
    ;;
esac
