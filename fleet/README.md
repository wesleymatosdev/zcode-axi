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
| `fleet-spawn [--harness zcode\|zcode-auto\|codex\|codex-tui\|claude\|claude-code\|ollama] [--model <id>] <task-id> <brief-file> [cwd]` | spawn named window `fleet-<task-id>` in tmux session `zswarm` on the chosen harness (TUI via send-keys, one-shot via argv — see the script's own header comment for the full per-harness table), verify readiness/delivery; tee via `tmux pipe-pane` to `logs/<task-id>.log` |
| `fleet-watch [--timeout SEC] <task-id>` | wait-then-follow watcher; exits on ANY attention state: `done:` `failed:` `blocked:` `needs-decision:` `ready-for-review:` (exit 2 on timeout = stall). Also bridges a sandboxed worker's `<cwd>/.fleet-status` fallback lines into the canonical status file |
| `fleet-status [--archive] [--archive-age-min N] [task-id]` | dashboard feed: id, last status line, log size, worker pid alive. `--archive` additionally runs `fleet-archive`'s scale-to-zero sweep |
| `fleet-archive [--age-min N] [--dry-run]` | kills windows whose task reached an attention state ≥N minutes ago (default 10); also scans every live `fleet-*` window for an unanswered dialog and prints `DIALOG-PENDING <window>` (flag only — never auto-killed) |
| `fleet-complete <task-id> <done\|failed> <summary>` | coordinator-only terminal write, after artifact verification; prints a reminder if the task's window is still open (unarchived) |
| `fleet-steer <task-id> <one-line>` | coordinator steering: durable `steered:` line + send-keys pointer (≤300 chars; detail belongs in the brief) |
| `fleet-watchdog` | watchdog: appends `failed: worker died mid-task` to open tasks whose pid is gone |
| `fleet-digest` | deterministic digest for the cron monitor: OPEN (escalations + liveness only), CLOSED (fresh 48h). Progress chatter is invisible by design |
| `fleet-event <task-id> <event> [detail]` | publishes an attention-state event to the fleet's Telegram topic (`hermes send`); called by `fleet-watch` on exit |

## The loop (how the coordinator dispatches)

```
bash fleet/fleet-spawn <id> brief.md <cwd>         # foreground: returns ready/delivery evidence
bash fleet/fleet-watch --timeout 2400 <id>         # BACKGROUND job with exit notification —
                                                # any attention state re-enters the
                                                # coordinator on its own; no polling
# on wake: verify artifacts (report exists, commits, tests), then:
bash fleet/fleet-complete <id> done "<one-line evidence-backed summary>"
```

AFK lane: the `fleet-v2 digest supervisor` cron (every 10m) hashes
`fleet-watchdog && fleet-digest` output; a change wakes it to verify and push
ONE line per task to Telegram ("✅ id: result · evidence"). Identical digest =
free tick. The Telegram context contract: one dispatch line in, one
completion line out; everything else stays on disk.

## Status-line grammar

`fleet/state/<task-id>.status` is append-only:

```
spawned: <iso-ts> brief=<path> cwd=<cwd> pid=<zcode-pid>     # fleet-spawn only
dispatched: <iso-ts> ready=yes|no mode=send-keys              # fleet-spawn only
working: <what is happening now>                              # worker
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
  state/<task-id>.status append-only status file (the contract)
  <task-cwd>/.fleet-status  sandbox fallback (only when a one-shot harness
                             can't reach state/<id>.status directly);
                             fleet-watch bridges it back into state/<id>.status
```

Nothing under `~/.zcode` is ever written by these scripts.
