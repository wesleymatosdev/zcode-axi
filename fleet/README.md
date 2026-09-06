# fleet — firstmate-style visible ZCode crewmates + the Hermes→ZCode connector

Hermes dispatch routed through ZCode (`zcode -p`, free Flash lane) with
firstmate's proven visibility pattern: named tmux windows (watchable),
append-only status files, wait-then-follow watchers, and completion gated on
evidence — never self-report. All scripts are bash, `set -euo pipefail`,
and live in this directory; all fleet state lives under `fleet/` inside this
repo. Nothing under `~/.zcode` is ever written.

## Scripts

| Script | Purpose |
|---|---|
| `zc-spawn.sh <task-id> <brief-file> [cwd]` | spawn a named tmux window `fleet-<task-id>` in session `zswarm` running `zcode -p "$(cat <brief-file>)"`, output tee'd to `fleet/logs/<task-id>.log`; returns immediately |
| `zc-watch.sh <task-id>` | wait-then-follow watcher: waits for the status file to exist, prints each new line as it lands, exits when the LAST line is terminal |
| `zc-status.sh [task-id]` | dashboard feed: `id`, last status line, log byte size, zcode process alive (yes/no); no argument = all tasks |
| `zc-complete.sh <task-id> <done\|failed> <one-line-summary>` | coordinator-only terminal status write, after artifact verification |

Layout (runtime dirs are gitignored; scripts create them on demand):

```
fleet/
  zc-spawn.sh zc-watch.sh zc-status.sh zc-complete.sh
  probe-brief.txt        trivial end-to-end probe
  logs/<task-id>.log     full zcode output (tee'd from the tmux pane)
  state/<task-id>.status append-only status file (the contract)
```

## Status-line grammar

`fleet/state/<task-id>.status` is append-only. Lines, in the order they may
appear:

```
spawned: <iso-ts> brief=<path> cwd=<cwd> pid=<zcode-pid>
working: <what is happening now>
blocked: <what is blocking, and on whom>
needs-decision: <the question, and the options>
done: <one-line evidence-backed summary>
failed: <one-line reason>
```

- `done:` / `failed:` are terminal: the watcher exits on them. A terminal
  line is never written by the worker itself (see the evidence gate).
- Nonterminal lines (`working:`, `blocked:`, `needs-decision:`, `spawned:`)
  are never "closed" by later nonterminal lines — the file is a log, not a
  state machine; only `done:`/`failed:` end the watch.

## Dispatch record shape

The `spawned:` line is written by `zc-spawn.sh` only:

```
spawned: 2026-09-06T17:20:05Z brief=/abs/path/brief.txt cwd=/abs/cwd pid=4242
```

`pid` is the spawned zcode process (best-effort; `unknown` if it could not
be resolved in the bounded post-spawn wait). `zc-status.sh` derives liveness
from this pid.

Task windows are created with `remain-on-exit on` (scoped to fleet windows
only): after zcode exits, the pane stays inspectable (`capture-pane`) until
the coordinator archives the task with
`tmux kill-window -t zswarm:fleet-<task-id>`. The last log line is always
the fleet marker `[fleet] zcode exited rc=<n>` with the real pipeline exit
code.

## Watcher pattern

The Hermes coordinator dispatches as a background job with exit
notification:

```
fleet/zc-spawn.sh mytask swarm/briefs/mytask.txt ~/projects/myrepo
fleet/zc-watch.sh mytask        # background; exit notification re-enters
                                # the coordinator when the task terminates
```

`zc-watch.sh` prints each new status line as it lands (so the coordinator
log carries the task's narrative) and exits 0 the moment the last line is
`done:`/`failed:`.

## The evidence gate

**A worker's self-report is NOT done.** `done:` is written only by the
coordinator, via `zc-complete.sh <task-id> done <summary>`, after verifying
artifacts directly: commits exist in the repo, tests actually ran and pass,
the report file exists and says what the worker claims. `failed:` likewise
belongs to the coordinator's verdict, not the worker's mood.

## Lane law

This fleet runs on the free Flash lane only. Before and after every
dispatch, grep the day's zcode log for model strings:

```
grep -oE '"model"[^,}]*' ~/.zcode/cli/log/zcode-$(date +%Y-%m-%d).jsonl \
  | sort | uniq -c
```

Any non-Flash model string attributable to our processes (anything that is
not a `*Flash*`/`glm-*-flash` variant) = kill that task's process tree
immediately and record an incident in the coordinator log. Never let a
metered model silently ride a fleet dispatch.
