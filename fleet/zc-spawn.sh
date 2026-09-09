#!/usr/bin/env bash
# zc-spawn.sh — spawn a named fleet worker in tmux session `zswarm`.
#
# Usage: zc-spawn.sh [--harness zcode|codex|claude] [--model <id>] <task-id> <brief-file> [cwd]
#
# fleet-v2 multi-harness (2026-09-09):
#   zcode  (default) — interactive TUI, --mode yolo, brief delivered via
#           send-keys after the Flash footer appears. Free lane.
#   codex / claude   — one-shot exec in the window (visible output, exits
#           when done): `codex exec --sandbox workspace-write` /
#           `claude -p --model <id>`. Prompt = the same short pointer to the
#           brief file, so briefs stay harness-uniform. For when the ZCode
#           window is busy/out of its free window and metered headroom
#           exists — the COORDINATOR picks the harness (routing law in the
#           fleet skill); the script never falls back silently on its own.
#
# Window law: never kill/rename windows we did not create. Open status file
# for a task id = refused (fresh id for retries). remain-on-exit + pinned
# automatic-rename keep finished panes inspectable and names stable.
set -euo pipefail

FLEET_ROOT="$(cd "$(dirname "$0")" && pwd)"

harness="zcode"
model=""
while [ $# -gt 0 ]; do
  case "$1" in
    --harness) harness="$2"; shift 2 ;;
    --model)   model="$2";   shift 2 ;;
    --harness=*|--model=*) echo "zc-spawn: use space-separated flags" >&2; exit 1 ;;
    *) break ;;
  esac
done

usage() { echo "usage: $0 [--harness zcode|codex|claude] [--model <id>] <task-id> <brief-file> [cwd]" >&2; exit 1; }

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
case "$harness" in
  zcode|codex|claude) ;;
  *) echo "zc-spawn: harness must be zcode|codex|claude: $harness" >&2; exit 1 ;;
esac
[ -f "$brief" ] || { echo "zc-spawn: brief not found: $brief" >&2; exit 1; }
[ -d "$cwd" ] || { echo "zc-spawn: cwd not found: $cwd" >&2; exit 1; }
command -v tmux >/dev/null || { echo "zc-spawn: tmux not on PATH" >&2; exit 1; }

# Resolve binaries explicitly: non-interactive shells lack ~/.local/bin, and
# the fleet must never silently fall back to a different binary on PATH.
BIN_DIR="$HOME/.local/bin"
case "$harness" in
  zcode)  ENGINE="${ZCODE_BIN:-$BIN_DIR/zcode}";  [ -x "$ENGINE" ] || { echo "zc-spawn: zcode not found at $ENGINE" >&2; exit 1; } ;;
  codex)  ENGINE="${CODEX_BIN:-$BIN_DIR/codex}";  [ -x "$ENGINE" ] || { echo "zc-spawn: codex not found at $ENGINE" >&2; exit 1; } ;;
  claude) ENGINE="${CLAUDE_BIN:-$BIN_DIR/claude}"; [ -x "$ENGINE" ] || { echo "zc-spawn: claude not found at $ENGINE" >&2; exit 1; } ;;
esac

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

# The brief pointer: identical for every harness — briefs are files, agents read them.
pointer="Read $brief_abs and do what it says. Progress: append one-line 'working:'/'blocked:'/'needs-decision:' entries to $status. Finish: append one line starting 'ready-for-review:'. Never write done: or failed: yourself — the coordinator closes tasks."

# Prose NEVER goes through the pane command line (quoting a paragraph through
# sh -c is how the first claude probe died). Pointer goes to a file; the pane
# runs a generated launcher that reads it. Only validated paths get interpolated.
pointer_file="$FLEET_ROOT/state/$task_id.pointer"
printf '%s' "$pointer" >"$pointer_file"

# Launch per harness. NO exec (the shell must survive), remain-on-exit,
# pinned automatic-rename, pipe-pane capture — same window law for all.
case "$harness" in
  zcode)
    # yolo: build-mode turns every status write into an approval dialog;
    # correctness comes from the evidence gate, not per-command prompts.
    tmux new-window -d -t zswarm -n "$window" \
      "cd $(esc "$cwd_abs") && $(esc "$ENGINE") --mode yolo"
    ;;
  codex)
    tmux new-window -d -t zswarm -n "$window" \
      "cd $(esc "$cwd_abs") && $(esc "$ENGINE") exec --skip-git-repo-check --sandbox workspace-write \"\$(cat $(esc "$pointer_file"))\" 2>&1 | tee -a $(esc "$log"); echo \"[fleet] codex exited rc=\$?\" | tee -a $(esc "$log")"
    ;;
  claude)
    mflag=""
    [ -n "$model" ] && mflag="--model $(esc "$model")"
    tmux new-window -d -t zswarm -n "$window" \
      "cd $(esc "$cwd_abs") && $(esc "$ENGINE") -p $mflag \"\$(cat $(esc "$pointer_file"))\" 2>&1 | tee -a $(esc "$log"); echo \"[fleet] claude exited rc=\$?\" | tee -a $(esc "$log")"
    ;;
esac
tmux set-option -t "zswarm:$window" remain-on-exit on >/dev/null
# Pin the name: agent TUIs (and tmux automatic-rename) otherwise rename the
# window — a renamed window once made zc-recover declare a live worker dead.
tmux set-option -t "zswarm:$window" automatic-rename off >/dev/null
# Fleet log: tmux-side capture keeps TUIs on a real PTY (zcode); one-shot
# harnesses tee their own output too — capture adds pane-side evidence.
tmux pipe-pane -t "zswarm:$window" "cat >> $(esc "$log")"

# Resolve the engine pid: pane shell's child matching the harness binary.
pane_pid="$(tmux list-panes -t "zswarm:$window" -F '#{pane_pid}' 2>/dev/null | head -n 1)"
zpid="unknown"
if [ -n "$pane_pid" ]; then
  for _ in $(seq 1 40); do
    if ps -o command= -p "$pane_pid" 2>/dev/null | grep -qi "$harness"; then
      zpid="$pane_pid"
      break
    fi
    for child in $(pgrep -P "$pane_pid" 2>/dev/null || true); do
      if ps -o command= -p "$child" 2>/dev/null | grep -qi "$harness"; then
        zpid="$child"
        break
      fi
    done
    [ "$zpid" != "unknown" ] && break
    sleep 0.25
  done
fi

ts="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
printf 'spawned: %s harness=%s model=%s brief=%s cwd=%s pid=%s\n' \
  "$ts" "$harness" "${model:-default}" "$brief_abs" "$cwd_abs" "$zpid" >>"$status"

ready=no
case "$harness" in
  zcode)
    # Readiness: wait for the TUI model footer (GLM-5.3-Flash) in the pane.
    for _ in $(seq 1 60); do
      if tmux capture-pane -p -t "zswarm:$window" 2>/dev/null | grep -q "GLM-5.3-Flash"; then
        ready=yes
        break
      fi
      sleep 0.5
    done
    # Deliver the brief pointer: send-keys -l, then Enter as a separate call.
    tmux send-keys -t "zswarm:$window" -l "$pointer"
    tmux send-keys -t "zswarm:$window" Enter
    delivery="unverified"
    for _ in $(seq 1 20); do
      if tmux capture-pane -p -t "zswarm:$window" -S -200 2>/dev/null | grep -qF "$(basename "$brief_abs")"; then
        delivery="send-keys"
        break
      fi
      sleep 0.5
    done
    ;;
  codex|claude)
    # One-shot: prompt went in via argv. Ready = engine process alive and
    # first pane output landed (bounded). No send-keys involved.
    delivery="argv"
    for _ in $(seq 1 40); do
      if [ "$zpid" != "unknown" ] && [ -n "$(tmux capture-pane -p -t "zswarm:$window" 2>/dev/null | tr -d '[:space:]')" ]; then
        ready=yes
        break
      fi
      sleep 0.5
    done
    ;;
esac
printf 'dispatched: %s ready=%s mode=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$ready" "$delivery" >>"$status"

echo "spawned zswarm:$window harness=$harness model=${model:-default} pid=$zpid ready=$ready delivery=$delivery"
echo "status=$status"
echo "log=$log"
if [ "$delivery" = "unverified" ] || [ "$ready" = "no" ]; then
  echo "zc-spawn: WARNING — delivery/readiness not confirmed; inspect zswarm:$window (zcode lane unhealthy => coordinator may re-dispatch on another harness per routing law)" >&2
  exit 2
fi
