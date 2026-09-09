#!/usr/bin/env bash
# zc-digest.sh — deterministic fleet digest (monitor feed for the Telegram cron).
#
# Usage: zc-digest.sh
#
# Output is DETERMINISTIC for a given fleet state: stable section order,
# tasks sorted by id, no timestamps of its own. The cron monitor hashes this
# output; identical output between ticks costs nothing, changed output wakes
# the coordinator agent to verify + push one line to Wesley.
#
# Sections:
#   OPEN   — tasks whose last status line is nonterminal (with worker pid
#            liveness so a dead worker is visible before zc-recover runs)
#   CLOSED — tasks whose last line is done:/failed: AND whose status file
#            changed within the last 48h (mtime window keeps this bounded)
set -euo pipefail

FLEET_ROOT="$(cd "$(dirname "$0")" && pwd)"
state="$FLEET_ROOT/state"

shopt -s nullglob
[ -d "$state" ] || exit 0

open_out=""
closed_out=""
for sf in $(ls "$state"/*.status 2>/dev/null | sort); do
  id="$(basename "$sf" .status)"
  last="$(tail -n 1 "$sf")"
  case "$last" in
    done:* | failed:*)
      # Only freshly-closed tasks (48h mtime window).
      if [ -n "$(find "$sf" -mtime -2 2>/dev/null)" ]; then
        closed_out+="CLOSED $id :: $last"$'\n'
      fi
      ;;
    *)
      # OPEN task. Progress chatter (working:/spawned:/steered: lines) must
      # NOT wake the monitor — render the last line ONLY when it is an
      # escalation. Liveness is tracked so a dead worker changes the digest.
      pid="$(sed -n 's/^spawned: .* pid=\([0-9][0-9]*\)$/\1/p' "$sf" | tail -n 1)"
      alive=unknown
      if [ -n "$pid" ]; then
        if ps -p "$pid" >/dev/null 2>&1; then alive=yes; else alive=NO; fi
      fi
      line=""
      case "$last" in
        blocked:* | needs-decision:* | ready-for-review:*) line="$last" ;;
      esac
      open_out+="OPEN $id worker=$alive :: $line"$'\n'
      ;;
  esac
done

[ -n "$open_out" ] && printf '%s' "$open_out"
[ -n "$closed_out" ] && printf '%s' "$closed_out"
exit 0
