# fleet — v2: Hermes-coordinated ZCode TUI workers

fleet-v2 (RFC: `~/projects/personal/swarm/RFC-fleet-v2.md`, ratified 2026-09-08).
Hermes (the long-running Telegram/desktop session Wesley talks to) is the sole
coordinator. Workers are visible ZCode TUI instances in named tmux windows on
the free GLM-5.3-Flash lane. firstmate stays installed as a toolbox only (no
captain). Quality loops run in the worker lane too: the adversarial reviewer
is a second ZCode worker on the first one's diff.

## Scripts

Renamed from `zc-*.sh` to `fleet-*` (no extension) when multi-harness landed
(2026-09-09); `fleet-recover` was further renamed `fleet-watchdog` (the word
"recover" trips the classifier). Older sessions/briefs may still say `zc-*`.

| Script | Purpose |
|---|---|
| `fleet-spawn [--harness zcode\|zcode-auto\|codex\|codex-tui\|claude\|claude-code\|ollama] [--model <id>] [--no-notify] [--no-delegate] <task-id> <brief-file> [cwd]` | spawn named window `fleet-<task-id>` in tmux session `zswarm` on the chosen harness (TUI via send-keys, one-shot via argv — see the script's own header comment for the full per-harness table), verify readiness/delivery; tee via `tmux pipe-pane` to `logs/<task-id>.log`. By default also starts the push layer: publishes `spawned`/`dispatched` bus events and starts `fleet-notifyd` (below); `--no-notify` opts out. By default also attaches `fleet-approval-delegate` to the new pane (TUI harnesses only — one-shot engines never show dialogs); `--no-delegate` opts out |
| `fleet-wait [--timeout SEC] <task-id>` (also `<task-id> [SEC]`) | the coordinator's wait, **push-first**: arms `fleet-wait-push` (subscribes the task on the bus) and exits with the event payload on ANY attention state: `done:` `failed:` `blocked:` `needs-decision:` `ready-for-review:` (exit 2 on timeout = stall). Wake is a push (<50ms budget; measured sub-1ms–13ms loopback) instead of a 1s scrape. **Revival on timeout (2026-09-11):** a timeout classifies the pane via `fleet-approval-delegate classify` — an approval dialog is cleared by the delegate and the wait continues (no re-dispatch, capped at 2 revives); a dead pane gets `failed-reason:` with the log tail and ONE auto re-dispatch as `<task-id>-r2` (second failure surfaces to Wesley and stops — no `-r3`); a healthy-but-slow worker just re-arms. While waiting it also lands a `waiting:` heartbeat in the status file every `$FLEET_WAIT_HEARTBEAT_SEC`s (default 1800) so disk reflects liveness. Same stdout contract and exit codes fleet-watch had. **Fallback (never silent):** if the bus is unreachable even after a start-on-demand attempt, or fleet-wait-push is not installed, fleet-wait prints a `FALLBACK` banner on stderr and execs the old `fleet-watch` poll loop — degraded to yesterday's latency, never broken (the fallback keeps pre-revival semantics). Also bridges a sandboxed worker's `<cwd>/.fleet-status` fallback lines into the canonical status file |
| `fleet-approval-delegate [--interval SEC] [--once] [--policy FILE] <task-id>` (Go, source `fleet-approval-delegate.go`) | the stall killer: polls the worker's tmux pane for permission dialogs — the Claude-Code proceed shape ("Do you want to proceed?" + numbered options), folder-trust, and the current ZCode TUI shape ("Permission · <Tool>" header, risk banner, arrow-selected options where the preselected "→ Allow once" is activated with Enter). Allowlisted classes from `fleet/approval-policy.txt` (reads under `~/projects/personal/**` or the task cwd, directory-access grants, read-only git/gh/npm, edits to the task's own status file, folder-trust) are auto-answered — pressing the best-scoped option — and audited to `logs/<task-id>.approvals.log` as `approval:auto fp=<fp> class=<name> <prompt-head>`. Everything else (pushes, deletes, `curl`, installs, auth, ANY unmatched prompt — default-deny) answers nothing, appends `needs-decision: pending approval dialog: …` once per distinct dialog, and publishes one fleet-event. Never touches non-prompt output; stands down when the window is archived or the task closes. `--once` is the bounded clear fleet-wait's revival path uses (it stands down if a watcher delegate already owns the pane); `classify <task-id>` prints `approval-dialog <head>` / `dead-pane <evidence>` / `alive-recent <age-s>` / `alive-silent <age-s>` for fleet-wait. Rebuild: `go build -o fleet-approval-delegate fleet-approval-delegate.go` (Go stdlib only) |
| `fleet-watch [--timeout SEC] <task-id>` | the old poll-loop watcher; kept as fleet-wait's fallback path and for manual use. (2026-09-09: fixed its silent `fleet-event.sh` reference — the rename had left it no-op'ing the Telegram publish) |
| `fleet-bus status\|ensure\|start\|stop` | busd lifecycle: health-check (`status`), start-on-demand with post-start health check (`ensure` — what fleet-spawn/fleet-wait/fleet-notifyd call), explicit `start`/`stop`. Resolves busd explicitly (`$FLEET_BUSD_BIN` → `~/.local/bin` → fleet-bus repo `bin/`); never a silent PATH fallback |
| `fleet-notifyd [--notifier nats\|webhook\|multi] <task-id>` | spawn-side notify shim (started by fleet-spawn, not by hand; `--notifier` defaults to `$FLEET_NOTIFIER`, else `nats`): tails `state/<task-id>.status` and pushes every new `working:`/`blocked:`/`needs-decision:`/`ready-for-review:`/`done:`/`failed:` line to the delivery adapter chosen by `--notifier` via `pushevent` — `nats` = the bus (default), `webhook` = signed JSON POST to the Hermes gateway receiver (needs `$FLEET_WEBHOOK_URL` + `$FLEET_WEBHOOK_SECRET`, secret never logged; IDE-agnostic port seam — hermes is just adapter #1), `multi` = fan-out to both — workers never change, briefs never mention the bus. Exits on `done:`/`failed:` (after pushing), archived status file, or swept window; log at `logs/<task-id>.notify.log` |
| `fleet-status [--archive] [--archive-age-min N] [task-id]` | dashboard feed: id, last status line, log size, worker pid alive. `--archive` additionally runs `fleet-archive`'s scale-to-zero sweep |
| `fleet-archive [--age-min N] [--dry-run]` | kills windows whose task reached an attention state ≥N minutes ago (default 10); also scans every live `fleet-*` window for an unanswered dialog and prints `DIALOG-PENDING <window>` (flag only — never auto-killed) |
| `fleet-complete <task-id> <done\|failed> <summary>` | coordinator-only terminal write, after artifact verification; prints a reminder if the task's window is still open (unarchived) |
| `fleet-steer <task-id> <one-line>` | coordinator steering: durable `steered:` line + send-keys pointer (≤300 chars; detail belongs in the brief) |
| `fleet-watchdog` | watchdog: appends `failed: worker died mid-task` to open tasks whose pid is gone |
| `fleet-digest` | deterministic digest for the cron monitor: OPEN (escalations + liveness only), CLOSED (fresh 48h). Progress chatter is invisible by design |
| `fleet-event <task-id> <event> [detail]` | publishes an attention-state event to the fleet's Telegram topic (`hermes send`); called by `fleet-wait`/`fleet-watch` on exit. `HERMES_BIN` override for tests |

## The loop (how the coordinator dispatches)

```
bash fleet/fleet-spawn <id> brief.md <cwd>         # foreground: returns ready/delivery evidence
                                                # + starts the notify shim (push layer)
bash fleet/fleet-wait --timeout 2400 <id>          # BACKGROUND job with exit notification —
                                                # push-wake on any attention state (<50ms);
                                                # falls back to the poll loop if busd is
                                                # down (loud banner, never silent)
# on wake: verify artifacts (report exists, commits, tests), then:
bash fleet/fleet-complete <id> done "<one-line evidence-backed summary>"
```

**busd requirement:** the push layer needs `busd` resident (default
`tcp:127.0.0.1:47777`). You do not manage it by hand — `fleet-bus ensure`
(start-on-demand, health-checked) runs inside every spawn/wait/notify
path. If the bus is down, everything still works at poll latency and every
affected command says so on stderr.

<details>
<summary>LaunchAgent option (NOT installed by default — Wesley's call)</summary>

Start-on-demand covers daily operation. To make busd reboot/keepalive
resilient, install a user agent — deliberately not done for you:

```sh
BUSD="$HOME/projects/personal/fleet-bus/bin/busd"   # or ~/.local/bin/busd
LOG="$HOME/projects/personal/zcode-axi/fleet/logs/busd.log"
cat > ~/Library/LaunchAgents/dev.fleet.busd.plist <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>dev.fleet.busd</string>
  <key>ProgramArguments</key><array>
    <string>$BUSD</string><string>-addr</string><string>tcp:127.0.0.1:47777</string>
  </array>
  <key>KeepAlive</key><true/>
  <key>RunAtLoad</key><true/>
  <key>StandardOutPath</key><string>$LOG</string>
  <key>StandardErrorPath</key><string>$LOG</string>
</dict></plist>
EOF
launchctl load ~/Library/LaunchAgents/dev.fleet.busd.plist
```

</details>

## Push bus (fleet-bus) — the law of the layers

The **status file is the durable record; the bus is notification only.**
`fleet/state/<task-id>.status` stays append-only and authoritative —
fleet-status/fleet-digest/fleet-archive only ever read it. A missed push
costs latency, never correctness, because the poll fallback reconciles
from the file. Concretely, the degradation ladder when busd is down:

1. `fleet-wait` tries `fleet-bus ensure` (start-on-demand).
2. Bus still unreachable → loud `FALLBACK` banner, exec of the old
   `fleet-watch` poll loop (1s scrape, same exit codes).
3. Bus flaps mid-wait → `fleet-wait-push`'s internal 2s safety-net poll and
   auto-reconnect paper over it (missed events cost one poll interval).
4. `fleet-notifyd` can't push → warns in its log, keeps tailing; lines are
   already on disk.

Events publish as `fleet.task.<id>.<state>` topics (state = the status
grammar's bus keywords: `working` `blocked` `needs-decision`
`ready-for-review` `done` `failed`); `spawned`/`dispatched` go out at
dispatch with `state=working` and the phase in the event's `phase` field.
Non-bus lines (`failed-reason:` `handoff:` `steered:`) are file-only by
design. The bus binaries come from the fleet-bus repo (v0.2 owner: the
fleet-bus-v02 worker); fleet scripts integrate against the CLIs only and
never write into that repo.

AFK lane: the `fleet-v2 digest supervisor` cron (every 10m) hashes
`fleet-watchdog && fleet-digest` output; a change wakes it to verify and push
ONE line per task to Telegram ("✅ id: result · evidence"). Identical digest =
free tick. The Telegram context contract: one dispatch line in one
completion line out; everything else stays on disk.

## Status-line grammar

`fleet/state/<task-id>.status` is append-only:

```
spawned: <iso-ts> brief=<path> cwd=<cwd> pid=<zcode-pid>     # fleet-spawn only
dispatched: <iso-ts> ready=yes|no mode=send-keys              # fleet-spawn only
working: <what is happening now>                              # worker
waiting: <liveness heartbeat — watcher still armed>           # fleet-wait only
ready-for-review: <one-line: deliverable complete, where>     # worker (watch exits: verify me)
blocked: <what is blocking, and on whom>                      # worker (escalation)
needs-decision: <the question, and the options>               # worker (escalation)
failed-reason: <verbatim cause>                                # worker or fleet-watchdog — REQUIRED before any auto re-dispatch
handoff: <distilled state>                                     # worker — written at a context boundary or on coordinator steer
steered: <one-line pointer>                                   # fleet-steer only
done: <one-line evidence-backed summary>                      # coordinator only
failed: <one-line reason>                                     # coordinator only
```

The file is a log, not a state machine: nonterminal lines are never "closed"
by later nonterminal lines. `done:`/`failed:` end the watch; `blocked:`/
`needs-decision:` end it too — they re-enter the coordinator as requests for
help. `ready-for-review:` is the worker's "deliverable complete" signal (the
success path): the watch exits, the coordinator verifies the artifact and
closes with `fleet-complete`. Without it a finished worker would be
indistinguishable from a stall.

`failed-reason:` and `handoff:` are nonterminal (they don't end the watch on
their own) but they harden the two moments the status file is the ONLY
context a fresh agent gets:
- `failed-reason:` carries the verbatim cause — an error message, a log
  tail, whatever explains the failure — and MUST land before any automated
  re-dispatch decision, so the re-dispatch has something to act on instead
  of re-deriving the cause from scratch (or, worse, retrying blind).
- `handoff:` is the resume point: a distilled statement of what's done, what
  isn't, and what a fresh agent should do next, written at a context
  boundary (compaction, a respawn) or when the coordinator steers. Since the
  status file IS the handoff contract, a fresh worker should be able to
  resume from it alone, without re-reading the whole log.

## The evidence gate

**A worker's self-report is NOT done.** `done:` is written only by the
coordinator, after verifying artifacts directly: the report file exists and
says what the worker claims, commits exist, tests ran. `failed:` likewise
belongs to the coordinator's verdict.

## Laws

- **Lane law (Flash only).** Before and after every dispatch:
  `grep -oE '"model":"[^"]*"' ~/.zcode/cli/log/zcode-$(date +%Y-%m-%d).jsonl | sort | uniq -c`
  Any non-Flash string attributable to our processes = kill that tree, record
  an incident. Never let a metered model ride a fleet dispatch.
- **TUI law.** Workers are the interactive TUI, never `zcode -p` (headless
  retired; a positional arg parses as a subcommand on zcode 0.16.5 — the TUI
  takes its brief via send-keys after the footer appears).
- **yolo law.** Workers launch `--mode yolo`: build-mode turns every status
  write into an approval dialog, and a worker must never depend on a human
  clicking Allow. Correctness comes from the evidence gate.
- **Approval law (2026-09-11).** A permission dialog must never sit
  unanswered — four worker-hours were lost to unseen "Do you want to
  proceed?" panes before this. TUI workers get a `fleet-approval-delegate`
  attached at spawn (auto-answers the policy allowlist, escalates
  everything else once), and fleet-wait's timeout classifies the pane and
  revives instead of just notifying: dialog → clear + keep waiting; dead
  pane → `failed-reason:` + one `-r2` re-dispatch; second failure →
  Wesley. The allowlist is deny-dominant: anything not explicitly allowed
  escalates.
- **Window law.** Never kill/rename windows you did not create. Fleet windows
  get `remain-on-exit on` so the finished pane stays inspectable until the
  coordinator archives it (`tmux kill-window -t zswarm:fleet-<id>`).
- **Clean-exit law.** A worker reaching a terminal state must be left
  drained, not abandoned — closed or explicitly archived in the same pass,
  never left sitting on an unanswered dialog (a dialog counts as
  un-drained; answer or dismiss it before archiving). `fleet-status
  --archive`/`fleet-archive` flags any live fleet window whose pane shows
  an unanswered prompt (`DIALOG-PENDING <window>`) instead of silently
  skipping it, and `fleet-complete` reminds you if the task's window is
  still open after closing. Receipt: the pubsec-audit Claude window sat on
  an unanswered "Teach auto mode?" dialog for hours.
- **Stagger law.** Dispatch 2–3 workers at a time, fresh spawns, scale to
  zero. Independent, non-conflicting tasks only.

## Layout

```
fleet/
  fleet-*  probe-brief.txt
  logs/<task-id>.log     pane capture (pipe-pane), full worker output
  logs/<task-id>.notify.log  fleet-notifyd's per-task publish trail
  logs/<task-id>.approvals.log  fleet-approval-delegate audit trail (auto-answers + escalations)
  logs/<task-id>.delegate.log   fleet-approval-delegate stdout/stderr
  logs/busd.log          resident broker log (start-on-demand + LaunchAgent)
  state/<task-id>.status append-only status file (the contract)
  <task-cwd>/.fleet-status  sandbox fallback (only when a one-shot harness
                             can't reach state/<id>.status directly);
                             fleet-wait/fleet-watch bridge it back into
                             state/<id>.status
```

Nothing under `~/.zcode` is ever written by these scripts.
