# fleet — v2: Hermes-coordinated ZCode TUI workers

fleet-v2 (RFC: `~/projects/personal/swarm/RFC-fleet-v2.md`, ratified 2026-09-08).
Hermes (the long-running Telegram/desktop session Wesley talks to) is the sole
coordinator. Workers are visible ZCode TUI instances in named tmux windows on
the free GLM-5.3-Flash lane. firstmate stays installed as a toolbox only (no
captain). Quality loops run in the worker lane too: the adversarial reviewer
is a second ZCode worker on the first one's diff.

## Scripts

| Script | Purpose |
|---|---|
| `fleet-spawn.sh <task-id> <brief-file> [cwd]` | spawn named window `fleet-<task-id>` in tmux session `zswarm`: bare `zcode --mode yolo`, wait for the Flash footer (readiness), send-keys the brief pointer, verify delivery; tee via `tmux pipe-pane` to `logs/<task-id>.log` |
| `fleet-watch.sh [--timeout SEC] <task-id>` | wait-then-follow watcher; exits on ANY attention state: `done:` `failed:` `blocked:` `needs-decision:` (exit 2 on timeout = stall) |
| `fleet-status.sh [task-id]` | dashboard feed: id, last status line, log size, worker pid alive |
| `fleet-complete.sh <task-id> <done\|failed> <summary>` | coordinator-only terminal write, after artifact verification |
| `fleet-steer.sh <task-id> <one-line>` | coordinator steering: durable `steered:` line + send-keys pointer (≤300 chars; detail belongs in the brief) |
| `fleet-recover.sh` | watchdog: appends `failed: worker died mid-task` to open tasks whose pid/window is gone |
| `fleet-digest.sh` | deterministic digest for the cron monitor: OPEN (escalations + liveness only), CLOSED (fresh 48h). Progress chatter is invisible by design |

## The loop (how the coordinator dispatches)

```
bash fleet/fleet-spawn.sh  <id> brief.md <cwd>     # foreground: returns ready/delivery evidence
bash fleet/fleet-watch.sh --timeout 2400 <id>      # BACKGROUND job with exit notification —
                                                # any attention state re-enters the
                                                # coordinator on its own; no polling
# on wake: verify artifacts (report exists, commits, tests), then:
bash fleet/fleet-complete.sh <id> done "<one-line evidence-backed summary>"
```

AFK lane: the `fleet-v2 digest supervisor` cron (every 10m) hashes
`fleet-recover.sh && fleet-digest.sh` output; a change wakes it to verify and push
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
steered: <one-line pointer>                                   # fleet-steer only
done: <one-line evidence-backed summary>                      # coordinator only
failed: <one-line reason>                                     # coordinator only
```

The file is a log, not a state machine: nonterminal lines are never "closed"
by later nonterminal lines. `done:`/`failed:` end the watch; `blocked:`/
`needs-decision:` end it too — they re-enter the coordinator as requests for
help. `ready-for-review:` is the worker's "deliverable complete" signal (the
success path): the watch exits, the coordinator verifies the artifact and
closes with `fleet-complete.sh`. Without it a finished worker would be
indistinguishable from a stall.

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
- **Stagger law.** Dispatch 2–3 workers at a time, fresh spawns, scale to
  zero. Independent, non-conflicting tasks only.

## Layout

```
fleet/
  fleet-*.sh  probe-brief.txt
  logs/<task-id>.log     pane capture (pipe-pane), full worker output
  state/<task-id>.status append-only status file (the contract)
```

Nothing under `~/.zcode` is ever written by these scripts.
