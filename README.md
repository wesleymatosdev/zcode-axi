# zcode-axi

Machine-friendly control plane around the official zcode runtime. Never
prompts, never opens the TUI, never touches login state. Deterministic,
parseable output only (compact one-line records by default; `--json` for
whole documents; `--pretty` for humans). Exit-code contract for machine
callers: [docs/EXIT-CODES.md](docs/EXIT-CODES.md).

```
cargo build --release && target/release/zcode-axi --help
```

## Commands

| Command | Behavior |
|---|---|
| `status` | zcode path + version, auth inference, campaign-window advisory (advisory only) |
| `run --cwd DIR --goal TEXT [--max-turns N]` | dispatch a headless run; prints session id, response, exit code |
| `sessions` | list sessions known to the runtime (live via app-server) |
| `inspect <sess_id>` | one persisted session: metadata + recent messages (read-only) |
| `wait <sess_id> [--timeout S]` | block until the session reports status "idle" |
| `resume <sess_id> --goal TEXT [--max-turns N]` | continue a persisted session headlessly |
| `cancel <sess_id>` | stop an active session via the app-server |
| `watch` | screen-watch the zcode GUI window; report task-state transitions as JSON lines (see below) |
| `tasks [--limit N]` | list GUI tasks from `~/.zcode/v2/tasks-index.sqlite` via `sqlite3 -readonly` (never in-process, never writes) |

Global flags: `--json`, `--pretty` (mutually exclusive), `--full` (disable
truncation), `--zcode-bin` / `ZCODE_AXI_ZCODE_BIN`.

## zcode-axi watch

Makes GUI task state observable without polling screenshots from an
orchestrator:

1. capture the zcode window with `xcap`
2. diff downsampled grayscale signatures (64×36 block grid); only changed
   frames are OCR'd — cursor blink stays below the change threshold
3. OCR with pure-Rust `ocrs` (RTen runtime; models auto-resolved from
   `$ZCODE_AXI_OCR_MODELS_DIR` or `~/.cache/zcode-axi/ocrs/`, see the error
   message for exact download commands)
4. classify the text: `awaiting_approval` (permission dialog) > `running`
   (status line) > `done` (transcript), cross-checked against
   `~/.zcode/v2/tasks-index.sqlite` (read-only subprocess, refreshed ≤1×/10s)
   only to corroborate OCR completion; the task index cannot prove success
   on its own
5. emit one JSON line per event on stdout; on state **transitions** (not
   every poll) optionally push a telegram alert via hermes

```
target/release/zcode-axi watch [--window-substr ZCode] [--interval-ms 1000] \
    [--notify none|telegram] [--duration-secs S] [--dump-frames DIR]
```

- Events: `watch_start`, `processing` (stage/timing plus explicit classified,
  unchanged, unknown, or error outcome), `state` (with `from`/`state`/
  `confidence`/`task_title`/`frame_hash`/`notified`), `heartbeat` (~10 s),
  `watch_end`.
- Rate limit: max 1 telegram alert per state per 5 minutes; suppression is
  reported in the event (`notified:false`, `notify_detail`).
- macOS Screen Recording permission: if denied (capture fails or frames are
  solid black) the command fails ONCE with exit 6 and guidance — it never
  retry-spams TCC.
- `--dump-frames DIR` saves changed-frame PNG + OCR text pairs (max 10/run)
  as evidence.

Read-only guarantees: `watch`/`tasks` only read `~/.zcode` (via
`sqlite3 -readonly` subprocess and the task index; no in-process sqlite for
the task store) and write only stdout JSON — plus hermes telegram pushes
when, and only when, `--notify telegram` is set.

## Packaging ZCodeWatcher.app

`watch` runs inside the packaged bundle (`dev.wesleymatos.zcode-watcher`,
installed at `~/Applications/ZCodeWatcher.app`) so macOS grants Screen
Recording to the bundle. See
[packaging-watcher/README.md](packaging-watcher/README.md): why ad-hoc
signing re-breaks the TCC grant on every rebuild (cdhash rebinding) and how
the stable-identity build/sign/re-grant flow fixes it
(`packaging-watcher/sign-with-identity.sh`).
