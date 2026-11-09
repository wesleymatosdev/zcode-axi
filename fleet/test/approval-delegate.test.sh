#!/usr/bin/env bash
# Unit-ish tests for fleet-approval-delegate — throwaway tmux panes in a
# scratch session (window law: we create and kill only our own session).
#
# Each scenario prints a scripted prompt into a `fleet-<id>` window and then
# runs `cat`, which stays alive and TTY-echoes whatever keys the delegate
# sends — the echoed digit IS the proof an answer was pressed. Escalations
# are proven via the status file, the audit log, and a stub event command.
#
# Usage: bash fleet/test/approval-delegate.test.sh
# Exit: 0 when every assertion passes, 1 otherwise.
set -uo pipefail

FLEET_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DELEGATE="$FLEET_ROOT/fleet-approval-delegate"
[ -x "$DELEGATE" ] || { echo "delegate binary missing — run: go build -o fleet-approval-delegate fleet-approval-delegate.go (in fleet/)" >&2; exit 1; }

SESSION="fleet-test-$$"
SCRATCH="$(mktemp -d /tmp/fleet-delegate-test.XXXXXX)"
mkdir -p "$SCRATCH/logs"
PASS=0
FAIL=0

cleanup() {
  tmux kill-session -t "$SESSION" 2>/dev/null || true
  rm -rf "$SCRATCH"
}
trap cleanup EXIT

tmux new-session -d -s "$SESSION" -x 220 -y 50 || { echo "cannot create test tmux session" >&2; exit 1; }

note() { printf '%s\n' "$*"; }
ok()  { PASS=$((PASS + 1)); note "ok   - $1"; }
bad() { FAIL=$((FAIL + 1)); note "FAIL - $1"; }

# assert_contains <file> <needle> <label>
assert_contains() {
  if [ -f "$1" ] && grep -qF -- "$2" "$1"; then ok "$3"; else bad "$3 (missing: $2 in $1)"; fi
}
assert_not_contains() {
  if [ -f "$1" ] && grep -qF -- "$2" "$1"; then bad "$3 (unexpected: $2 in $1)"; else ok "$3"; fi
}

# pane_has_line <task-id> <exact-line> — the TTY echo of a sent digit is a
# line of its own below the prompt; option rows never equal a bare digit.
pane_has_line() {
  tmux capture-pane -p -t "$SESSION:fleet-$1" 2>/dev/null | grep -qxF -- "$2"
}

# new_prompt_pane <task-id> <promptfile> [loop]
#   prints the prompt file, then stays alive on `cat` (echoes sent keys).
#   loop=1: after each consumed keystroke, re-print the prompt (same
#   fingerprint) — for the answer-limit test.
new_prompt_pane() {
  local id="$1" promptfile="$2" loop="${3:-0}"
  local runner="$SCRATCH/$id.runner.sh"
  {
    printf '#!/bin/sh\n'
    if [ "$loop" = "1" ]; then
      # Clear + home before each redraw so the dialog stays at a fixed pane
      # position (a real TUI dialog is a static overlay; a scrolling wall of
      # prompts would be a test artifact).
      printf 'while :; do printf "\\033[2J\\033[H"; cat "%s"; head -c 1 >/dev/null; done\n' "$promptfile"
    else
      printf 'cat "%s"\nexec cat\n' "$promptfile"
    fi
  } >"$runner"
  chmod +x "$runner"
  tmux new-window -d -t "$SESSION" -n "fleet-$id" "$runner" 2>/dev/null
  sleep 0.5
}

# Stub event publisher: records invocations instead of touching Telegram.
cat >"$SCRATCH/event.sh" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >> "$SCRATCH/events.log"
EOF
chmod +x "$SCRATCH/event.sh"
: >"$SCRATCH/events.log"

deleg_args() { # <task-id>
  printf '%s' "--once --session $SESSION --status $SCRATCH/$1.status --log $SCRATCH/$1.approvals.log --event-cmd $SCRATCH/event.sh"
}

# ---------------------------------------------------------------- scenario 1
# allow: read under ~/projects/personal → presses the session-scope option 2
note ""
note "== scenario 1: allow read-own-tree (presses session-scope option) =="
P="$SCRATCH/p1.txt"
cat >"$P" <<EOF
Read file
Read($HOME/projects/personal/zcode-axi/fleet/state/demo.json)
 Do you want to proceed?
❯ 1. Yes
   2. Yes, allow reading from $HOME/projects/personal/zcode-axi/fleet/state during this session
   3. No
Esc to cancel · Tab to amend
EOF
new_prompt_pane "t1-read" "$P"
run1_out="$(eval "\"$DELEGATE\" $(deleg_args t1-read) t1-read" 2>&1)"
assert_contains "$SCRATCH/t1-read.approvals.log" "class=read-own-tree option=2" "auto-answer audited as read-own-tree option 2"
if pane_has_line "t1-read" "2"; then ok "pane received the digit 2"; else bad "pane received the digit 2"; fi
if ! pane_has_line "t1-read" "3"; then ok "no wrong option pressed"; else bad "no wrong option pressed (digit 3 found)"; fi

# ---------------------------------------------------------------- scenario 2
# allow: dir-access grant ("always allow access to <projects dir>") → option 2
note ""
note "== scenario 2: allow dir-access =="
P="$SCRATCH/p2.txt"
cat >"$P" <<EOF
Run shell command
cat /Users/wesleymatos/projects/personal/zcode-axi/fleet/state/demo.json
 Do you want to proceed?
❯ 1. Yes
   2. Yes, and always allow access to $HOME/projects/personal/zcode-axi/fleet/state from this project
   3. No
Esc to cancel · Tab to amend
EOF
new_prompt_pane "t2-dir" "$P"
eval "\"$DELEGATE\" $(deleg_args t2-dir) t2-dir" >/dev/null 2>&1
assert_contains "$SCRATCH/t2-dir.approvals.log" "class=dir-access option=2" "dir-access auto-answer audited (option 2)"
if pane_has_line "t2-dir" "2"; then ok "pane received the digit 2"; else bad "pane received the digit 2"; fi

# ---------------------------------------------------------------- scenario 3
# deny: git push → no keys, needs-decision once, one event (watch-mode dedupe)
note ""
note "== scenario 3: deny network mutation, escalate ONCE =="
P="$SCRATCH/p3.txt"
cat >"$P" <<EOF
Run shell command
git push origin main
 Do you want to proceed?
❯ 1. Yes
   2. Yes, and always allow access to $HOME/projects/personal/zcode-axi from this project
   3. No
Esc to cancel · Tab to amend
EOF
new_prompt_pane "t3-push" "$P"
"$DELEGATE" --once --interval 1 --session "$SESSION" --status "$SCRATCH/t3-push.status" \
  --log "$SCRATCH/t3-push.approvals.log" --event-cmd "$SCRATCH/event.sh" t3-push >/dev/null 2>&1
if pane_has_line "t3-push" "1" || pane_has_line "t3-push" "3"; then
  bad "deny case sent no keys"; else ok "deny case sent no keys"; fi
assert_contains "$SCRATCH/t3-push.status" "needs-decision: pending approval dialog:" "escalation appended to status file"
assert_contains "$SCRATCH/t3-push.approvals.log" "approval:escalate fp=" "escalation audited"
assert_contains "$SCRATCH/events.log" "needs-decision" "one fleet-event published"
EV_BEFORE="$(wc -l <"$SCRATCH/events.log" | tr -d ' ')"
# watch mode for 2 polls against the SAME dialog: must not re-notify
"$DELEGATE" --interval 1 --session "$SESSION" --status "$SCRATCH/t3-push.status" \
  --log "$SCRATCH/t3-push.approvals.log" --event-cmd "$SCRATCH/event.sh" t3-push >/dev/null 2>&1 &
WATCH3=$!
sleep 3
kill "$WATCH3" 2>/dev/null
wait "$WATCH3" 2>/dev/null
EV_AFTER="$(wc -l <"$SCRATCH/events.log" | tr -d ' ')"
if [ "$EV_BEFORE" = "$EV_AFTER" ]; then ok "dedupe: no repeat event for the same dialog"; else bad "dedupe: events went $EV_BEFORE → $EV_AFTER"; fi

# ---------------------------------------------------------------- scenario 4
# deny (unmatched): unknown tool prompt escalates as unmatched
note ""
note "== scenario 4: unmatched dialog escalates (default-deny) =="
P="$SCRATCH/p4.txt"
cat >"$P" <<EOF
WebFetch
WebFetch(https://example.com/api)
 Do you want to proceed?
❯ 1. Yes
   2. No
Esc to cancel · Tab to amend
EOF
new_prompt_pane "t4-unmatched" "$P"
eval "\"$DELEGATE\" $(deleg_args t4-unmatched) t4-unmatched" >/dev/null 2>&1
assert_contains "$SCRATCH/t4-unmatched.approvals.log" "class=unmatched" "unmatched dialog escalated as unmatched"
assert_contains "$SCRATCH/t4-unmatched.status" "needs-decision: pending approval dialog:" "unmatched dialog surfaced in status file"

# ---------------------------------------------------------------- scenario 5
# never touch non-prompt output
note ""
note "== scenario 5: non-prompt output is never touched =="
P="$SCRATCH/p5.txt"
cat >"$P" <<EOF
working: analyzing the fleet logs
I will proceed with the implementation now.
working: writing the report
EOF
new_prompt_pane "t5-cruise" "$P"
EV5_BASE="$(wc -l <"$SCRATCH/events.log" | tr -d ' ')"
eval "\"$DELEGATE\" $(deleg_args t5-cruise) t5-cruise" >/dev/null 2>&1
[ -s "$SCRATCH/t5-cruise.approvals.log" ] && bad "cruising pane: no audit lines" || ok "cruising pane: no audit lines"
[ -e "$SCRATCH/t5-cruise.status" ] && bad "cruising pane: no status writes" || ok "cruising pane: no status writes"
EV5="$(wc -l <"$SCRATCH/events.log" | tr -d ' ')"
if [ "$EV5" = "$EV5_BASE" ]; then ok "cruising pane: no events"; else bad "cruising pane: event count grew"; fi
if pane_has_line "t5-cruise" "1" || pane_has_line "t5-cruise" "2"; then bad "cruising pane: no keys sent"; else ok "cruising pane: no keys sent"; fi

# ---------------------------------------------------------------- scenario 6
# watch mode end-to-end lifecycle: answers a dialog, exits when window dies
note ""
note "== scenario 6: watch lifecycle (answers, then exits when archived) =="
P="$SCRATCH/p6.txt"
cat >"$P" <<EOF
Run shell command
git status --short
 Do you want to proceed?
❯ 1. Yes
   2. Yes, and don't ask again for: git status *
   3. No
Esc to cancel · Tab to amend
EOF
new_prompt_pane "t6-git" "$P"
"$DELEGATE" --interval 1 --session "$SESSION" --status "$SCRATCH/t6-git.status" \
  --log "$SCRATCH/t6-git.approvals.log" --event-cmd "$SCRATCH/event.sh" t6-git >/dev/null 2>&1 &
WATCH6=$!
for _ in $(seq 1 20); do
  if grep -q "approval:auto class=git-readonly" "$SCRATCH/t6-git.approvals.log" 2>/dev/null; then break; fi
  sleep 0.5
done
assert_contains "$SCRATCH/t6-git.approvals.log" "class=git-readonly option=2" "watch mode answered git-readonly with don't-ask-again option"
tmux kill-window -t "$SESSION:fleet-t6-git" 2>/dev/null
DEAD=""
for _ in $(seq 1 20); do
  if ! kill -0 "$WATCH6" 2>/dev/null; then DEAD=yes; break; fi
  sleep 0.5
done
kill "$WATCH6" 2>/dev/null
wait "$WATCH6" 2>/dev/null
if [ -n "$DEAD" ]; then ok "delegate exited after window archived"; else bad "delegate exited after window archived"; fi

# ---------------------------------------------------------------- scenario 7
# answer limit: same dialog returning again and again → escalate, stop pressing
note ""
note "== scenario 7: answer limit reached → escalate =="
P="$SCRATCH/p7.txt"
cat >"$P" <<EOF
Read file
Read($HOME/projects/personal/zcode-axi/fleet/state/loop.json)
 Do you want to proceed?
❯ 1. Yes
   2. Yes, allow reading from $HOME/projects/personal/zcode-axi/fleet/state during this session
   3. No
Esc to cancel · Tab to amend
EOF
new_prompt_pane "t7-loop" "$P" 1
FLEET_DELEGATE_MAX_ANSWERS=2 FLEET_DELEGATE_ANSWER_COOLDOWN_SEC=0 \
  "$DELEGATE" --interval 1 --session "$SESSION" --status "$SCRATCH/t7-loop.status" \
  --log "$SCRATCH/t7-loop.approvals.log" --event-cmd "$SCRATCH/event.sh" t7-loop >/dev/null 2>&1 &
WATCH7=$!
for _ in $(seq 1 30); do
  if grep -q "answer limit reached" "$SCRATCH/t7-loop.approvals.log" 2>/dev/null; then break; fi
  sleep 0.5
done
kill "$WATCH7" 2>/dev/null; wait "$WATCH7" 2>/dev/null
ANSWERS="$(grep -c 'approval:auto' "$SCRATCH/t7-loop.approvals.log" 2>/dev/null || true)"
if [ "$ANSWERS" = "2" ]; then ok "answered exactly max (2) times"; else bad "answered exactly max (2) times (got $ANSWERS)"; fi
assert_contains "$SCRATCH/t7-loop.approvals.log" "(answer limit reached)" "answer-limit escalation audited"
assert_contains "$SCRATCH/t7-loop.status" "needs-decision: pending approval dialog:" "answer-limit surfaced in status file"

# ---------------------------------------------------------------- scenario 8
# classify: approval-dialog / alive-recent / alive-silent / dead pane
note ""
note "== scenario 8: classify (fleet-wait revival input) =="
P="$SCRATCH/p8.txt"
cat >"$P" <<EOF
Run shell command
git log --oneline -3
 Do you want to proceed?
❯ 1. Yes
   2. Yes, and don't ask again for: git log *
   3. No
Esc to cancel · Tab to amend
EOF
new_prompt_pane "t8-dlg" "$P"
cls() { # <task-id> — classify with scratch FLEET_ROOT so log-age checks hit scratch logs
  FLEET_ROOT="$SCRATCH" "$DELEGATE" classify --session "$SESSION" --status "$SCRATCH/$1.status" "$1"
}
if cls t8-dlg | grep -q "^approval-dialog git log"; then
  ok "classify: dialog pane → approval-dialog with prompt head"
else
  bad "classify: dialog pane → approval-dialog with prompt head"
fi
printf 'x' >>"$SCRATCH/logs/t8-alive.log"
tmux new-window -d -t "$SESSION" -n "fleet-t8-alive" "exec cat" 2>/dev/null; sleep 0.3
if cls t8-alive | grep -q "^alive-recent "; then ok "classify: fresh log activity → alive-recent"; else bad "classify: fresh log activity → alive-recent"; fi
tmux new-window -d -t "$SESSION" -n "fleet-t8-silent" "exec cat" 2>/dev/null; sleep 0.3
touch -t 202601010000 "$SCRATCH/logs/t8-silent.log"
if cls t8-silent | grep -q "^alive-silent "; then ok "classify: stale log → alive-silent"; else bad "classify: stale log → alive-silent"; fi
tmux new-window -d -t "$SESSION" -n "fleet-t8-dead" "exit 1" 2>/dev/null
tmux set-option -t "$SESSION:fleet-t8-dead" remain-on-exit on >/dev/null 2>&1
sleep 0.5
if cls t8-dead | grep -q "^dead-pane "; then ok "classify: dead pane → dead-pane"; else bad "classify: dead pane → dead-pane"; fi
if cls t8-gone | grep -q "^dead-pane window-missing"; then ok "classify: missing window → dead-pane"; else bad "classify: missing window → dead-pane"; fi

# ---------------------------------------------------------------- scenario 9
# allow: file-edit dialog (basename only, the claude-code manual-mode shape)
note ""
note "== scenario 9: allow file-edit (status basename, accept-edits option) =="
P="$SCRATCH/p9.txt"
cat >"$P" <<EOF
Do you want to make this edit to t9.status?
❯ 1. Yes
   2. Yes, and switch to accept edits (auto-approve file edits and common file commands) for this session (shift+tab)
   3. No
Esc to cancel · Tab to amend
EOF
new_prompt_pane "t9-edit" "$P"
"$DELEGATE" --once --session "$SESSION" --status "$SCRATCH/t9.status" \
  --log "$SCRATCH/t9-edit.approvals.log" --event-cmd "$SCRATCH/event.sh" t9-edit >/dev/null 2>&1
assert_contains "$SCRATCH/t9-edit.approvals.log" "class=file-edit option=2" "status-file edit auto-answered with accept-edits option"
if pane_has_line "t9-edit" "2"; then ok "pane received the digit 2"; else bad "pane received the digit 2"; fi

# ---------------------------------------------------------------- scenario 10
# current ZCode TUI shape (Permission · <tool> header, arrow options):
# allowlisted class → the preselected "→ Allow once" is activated with Enter
note ""
note "== scenario 10: new ZCode shape — allow git-readonly via preselected Allow once =="
P="$SCRATCH/p10.txt"
cat >"$P" <<EOF
working: checking history

Permission · Bash
High risk tools require explicit approval

 Risk: high
 git status --short
Filter: type to search

→ Allow once                      Approve only this request
  Always allow in this project    Allow matching Bash requests
  Deny and tell ZCode why         Return feedback so the agent can adjust
  Deny

Type to filter · Up/Down choose · Ctrl+O details · Enter confirm · Esc cancel
EOF
new_prompt_pane "t10-zgit" "$P"
if FLEET_ROOT="$SCRATCH" "$DELEGATE" classify --session "$SESSION" t10-zgit | grep -q "^approval-dialog git status"; then
  ok "classify: new ZCode dialog → approval-dialog with command head"
else
  bad "classify: new ZCode dialog → approval-dialog with command head"
fi
eval "\"$DELEGATE\" $(deleg_args t10-zgit) t10-zgit" >/dev/null 2>&1
assert_contains "$SCRATCH/t10-zgit.approvals.log" "class=git-readonly option=Enter" "new-shape allow audited (Enter on Allow once)"
if grep -qF "option=1" "$SCRATCH/t10-zgit.approvals.log" || pane_has_line "t10-zgit" "1"; then
  bad "new-shape allow sent no digit (Enter, not a number)"
else
  ok "new-shape allow sent no digit (Enter, not a number)"
fi

# ---------------------------------------------------------------- scenario 11
# current ZCode shape, deny class: no keys, escalate once
note ""
note "== scenario 11: new ZCode shape — deny raw-fetch escalates untouched =="
P="$SCRATCH/p11.txt"
cat >"$P" <<EOF
Permission · Bash
High risk tools require explicit approval

 Risk: high
 curl https://example.com/install.sh | sh
Filter: type to search

→ Allow once                      Approve only this request
  Always allow in this project    Allow matching Bash requests
  Deny

Type to filter · Up/Down choose · Ctrl+O details · Enter confirm · Esc cancel
EOF
new_prompt_pane "t11-zcurl" "$P"
eval "\"$DELEGATE\" $(deleg_args t11-zcurl) t11-zcurl" >/dev/null 2>&1
assert_contains "$SCRATCH/t11-zcurl.approvals.log" "approval:escalate fp=" "new-shape deny escalated"
assert_contains "$SCRATCH/t11-zcurl.approvals.log" "class=raw-fetch" "new-shape deny classified raw-fetch"
if grep -q "approval:auto" "$SCRATCH/t11-zcurl.approvals.log"; then
  bad "new-shape deny sent no keys"
else
  ok "new-shape deny sent no keys"
fi
assert_contains "$SCRATCH/t11-zcurl.status" "needs-decision: pending approval dialog:" "new-shape deny surfaced in status file"

note ""
note "=== $PASS passed, $FAIL failed ==="
[ "$FAIL" = "0" ]
