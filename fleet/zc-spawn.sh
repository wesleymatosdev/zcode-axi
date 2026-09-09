#!/usr/bin/env bash
# zc-spawn.sh — spawn a named ZCode TUI crewmate in tmux session `zswarm`.
#
# Usage: zc-spawn.sh <task-id> <brief-file> [cwd]
#
# fleet-v2 P1 (2026-09-08): TUI mode. zcode 0.16.5 has NO positional prompt
# for the TUI (a positional arg parses as a subcommand) and --prompt/-p are
# headless (retired). So: open the bare TUI, wait for readiness (model
# footer), then deliver the brief pointer via `tmux send-keys` + Enter —
# the proven firstmate lane. Pane output is captured with `tmux pipe-pane`
# into fleet/logs/<task-id>.log (a pipe through tee would steal the PTY).
#
# Window law: NEVER kills or renames windows it did not create. A task id
# whose window exists, or whose status file is still open (nonterminal), is
# refused — re-dispatch uses a fresh id.
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
# Resolve zcode explicitly: non-interactive shells lack ~/.local/bin, and the
# fleet must never silently fall back to a different binary on PATH.
ZCODE="${ZCODE_BIN:-$HOME/.local/bin/zcode}"
[ -x "$ZCODE" ] || { echo "zc-spawn: zcode not found at $ZCODE" >&2; exit 1; }

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
if [ -f "$status" ]; then
  last="$(tail -n 1 "$status")"
  case "$last" in
    done:* | failed:*) ;;  # archived task id reused: truncate to fresh file
    *) echo "zc-spawn: task $task_id still open (last: $last); use a fresh id" >&2; exit 1 ;;
  esac
  : >"$status"
fi
: >"$log"

# Session zswarm: create only if missing. Other sessions/windows untouched.
tmux has-session -t zswarm 2>/dev/null || tmux new-session -d -s zswarm -x 220 -y 50

# Single-quote a string for safe embedding in the pane's sh command line.
esc() { printf "'%s'" "${1//\'/\'\\\'\'}"; }

# Bare TUI + yolo (no args: a positional arg parses as a subcommand on
# 0.16.5; --prompt/-p are headless-retired). yolo because build-mode turns
# every fleet status write into an approval dialog — workers must not
# depend on a human clicking Allow. Correctness comes from the evidence
# gate (coordinator verifies artifacts), not from per-command prompts.
# NO exec — the shell must survive to keep the pane alive if zcode exits.
tmux new-window -d -t zswarm -n "$window" \
  "cd $(esc "$cwd_abs") && $(esc "$ZCODE") --mode yolo"
tmux set-option -t "zswarm:$window" remain-on-exit on >/dev/null
# Pin the name: zcode's TUI (and tmux automatic-rename) otherwise rename the
# window — a renamed window once made zc-recover declare a live worker dead.
tmux set-option -t "zswarm:$window" automatic-rename off >/dev/null
# Fleet log: tmux-side capture keeps the TUI on a real PTY.
tmux pipe-pane -t "zswarm:$window" "cat >> $(esc "$log")"

# Resolve the zcode pid: pane shell's child running zcode.
pane_pid="$(tmux list-panes -t "zswarm:$window" -F '#{pane_pid}' 2>/dev/null | head -n 1)"
zpid="unknown"
if [ -n "$pane_pid" ]; then
  for _ in $(seq 1 40); do
    if ps -o command= -p "$pane_pid" 2>/dev/null | grep -qi zcode; then
      zpid="$pane_pid"
      break
    fi
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

# Readiness: wait for the TUI model footer (GLM-5.3-Flash) in the pane.
ready=no
for _ in $(seq 1 60); do
  if tmux capture-pane -p -t "zswarm:$window" 2>/dev/null | grep -q "GLM-5.3-Flash"; then
    ready=yes
    break
  fi
  sleep 0.5
done

# Deliver the brief pointer: send-keys -l, then Enter as a separate call.
pointer="Read $brief_abs and do what it says. Progress: append one-line 'working:'/'blocked:'/'needs-decision:' entries to $status. Never write done: or failed: yourself — the coordinator closes tasks."
tmux send-keys -t "zswarm:$window" -l "$pointer"
tmux send-keys -t "zswarm:$window" Enter

# Verify delivery: the brief's basename must appear in pane scrollback.
delivery="unverified"
for _ in $(seq 1 20); do
  if tmux capture-pane -p -t "zswarm:$window" -S -200 2>/dev/null | grep -qF "$(basename "$brief_abs")"; then
    delivery="send-keys"
    break
  fi
  sleep 0.5
done
printf 'dispatched: %s ready=%s mode=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$ready" "$delivery" >>"$status"

echo "spawned zswarm:$window pid=$zpid ready=$ready delivery=$delivery"
echo "status=$status"
echo "log=$log"
if [ "$delivery" = "unverified" ]; then
  echo "zc-spawn: WARNING — pointer not confirmed in pane; inspect zswarm:$window" >&2
  exit 2
fi
