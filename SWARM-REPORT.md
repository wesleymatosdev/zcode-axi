# SWARM-REPORT — zcode-axi v0.1

Card: t_868f3cf1 · Worker: zcode (GLM) · Date: 2026-09-03 · Repo: `~/project/projects/personal/zcode-axi`

## 1. Protocol characterization result

**SUCCEEDED within the timebox** — no fallback to pure `zcode -p` scraping was
needed (headless mode is still used for `run`/`resume` by design).

- `zcode app-server` speaks a custom **"ZCode Protocol"**: newline-delimited
  JSON over stdio that REJECTS the JSON-RPC 2.0 `jsonrpc` envelope key.
  Frames are bare `{id, method, params}` / `{id, result}` / `{id, error}`.
- Full method table extracted from the official bundle and live-verified:
  `session/list` (used by `sessions`/`wait`), `session/stop` (used by
  `cancel`; active sessions only — `-32004` otherwise), plus ~50 more
  documented methods (`session/create|send|fork|compact|goal|...`,
  `workspace/*`, `automation/*`, `usage/stats`).
- No handshake needed; `session/list` works as the first frame.
- Headless contract captured verbatim: `zcode --json -p` emits
  `{sessionId, response, usage, projection}` with exit 0.
- Two runtime quirks found and documented:
  1. **`--max-turns` is advertised in `--help` but rejected by the 0.16.5
     argument parser** (help/parsing drift). zcode-axi capability-probes it
     with a model-free `--max-turns 1 --version` invocation and forwards it
     only when supported; otherwise warns and proceeds.
  2. `session/read`/`session/messages` serve **active sessions only**, so
     `inspect` reads the persisted store instead.
- Persisted store located at `~/.zcode/cli/db/db.sqlite` (tables `session`,
  `message`, `part`); opened strictly read-only (`mode=ro` URI).
- Everything verbatim in `docs/protocol.md`; probe script at `scripts/probe.mjs`.

## 2. Command surface shipped

| Command | Behavior |
|---|---|
| `status` | zcode path + version, doctor exit, auth ok/not-ok (doctor + trivial headless round-trip; credentials never read), campaign-window advisory (Sep 3–20 2026, 12:00–22:00 America/Sao_Paulo, advisory-only disclaimer, `next_window_unix`) |
| `run --cwd --goal [--max-turns]` | headless dispatch; prints session id + exit code + response |
| `sessions` | live via app-server `session/list`; read-only sqlite fallback if the app-server fails |
| `inspect <id>` | metadata + last 10 messages from the persisted store, live status merged in best-effort |
| `wait <id> [--timeout S]` | polls `session/list` until `status=idle`; exit 4 on timeout |
| `resume <id> --goal [--max-turns]` | `--resume <id>` + headless prompt; validates id and existence first |
| `cancel <id>` | app-server `session/stop`; exit 5 with explicit message for non-active sessions |

Global: `--json` / `--pretty` / `--full` (truncation off), `--zcode-bin`
override. No prompts, no TUI. Exit codes 0/1/2/3/4/5 documented in
`docs/EXIT-CODES.md`. Truncation always prints a hint.

## 3. Gate results

| # | Gate | Result | Evidence |
|---|------|--------|----------|
| 1 | `cargo fmt --check` | **PASS** (clean) | rerun in-repo |
| 2 | `cargo clippy --all-targets -- -D warnings` | **PASS** (clean) | rerun in-repo |
| 3 | `cargo test` | **PASS** — 20 tests, 0 failed | incl. arg parsing, JSON rendering, exit-code mapping, protocol-frame encoding vs a fake in-test stdio server (hidden `app-server` subcommand), fake-zcode shell scripts, sqlite fixture store, window math. No test touches the live runtime |
| 4 | `cargo build --release` | **PASS** — `target/release/zcode-axi` (2.9 MB) | — |
| 5 | Live round-trip | **PASS** — `status`: version 0.16.5 + auth ok; `run --cwd /tmp/axi-run-test --goal "Reply with exactly: OK" --max-turns 2`: session id captured, `response=OK`, exit 0 (with the documented max-turns warning) | `docs/live-roundtrip-evidence.md` (all 7 commands + `--json` variants, verbatim) |
| 6 | Not installed into PATH/profile | **PASS** — binary only at `target/release/zcode-axi` | — |

## 4. Commit list

```
09f95e4 docs: exit-code contract and live round-trip evidence
c2e6ca1 feat: command surface (status/run/sessions/inspect/wait/resume/cancel)
b976632 feat: core modules — protocol client, runtime, read-only store
7e5f074 docs: characterize zcode 0.16.5 app-server and headless protocol
```

(SWARM-REPORT committed as a 5th commit; local only, no remote operations.)

## 5. Hard-rule compliance

- Official runtime only: every invocation goes through the installed
  `zcode` binary; nothing wrapped, copied, or reimplemented.
- Login state untouched: no `login`/`logout`, no credential reads — auth is
  inferred from behavior.
- No `git push`, no `gh`, no remote ops; local commits only.
- No processes killed other than zcode-axi's own child probes (app-server
  children are killed on drop); `madeline-site` untouched.
- Session store opened `mode=ro`; zcode-axi never writes it.

## 6. What v0.2 should add

1. **Streaming runs**: `session/create` + `session/send` + `session/subscribe`
   over the app-server instead of blocking headless `-p`, so Hermes can watch
   events live and run concurrent sessions in one server process (protocol
   groundwork already characterized; `--output-format stream-json` on the
   headless path is a cheaper intermediate step).
2. **Real cancellation**: map `cancel` to a live run's process (via
   `session/cancelBackgroundTask` or child pid tracking) instead of exiting 5
   for non-active sessions.
3. **`sessions` filters**: `--cwd`, `--status`, `--since`, `--workspace`.
4. **`inspect` tool-call detail**: expose non-text parts (tool invocations,
   diffs) currently skipped by the text-part query.
5. **Config file** (`~/.config/zcode-axi.toml`): default zcode bin, list
   limits, timeouts, so flags don't repeat per call.
6. **Auth-marker hardening**: broaden the not-authenticated classification
   with observed real-world logged-out stderr (v0.1 could only test against
   synthetic markers, since logging out was forbidden).
7. **version pinning guard**: warn loudly when runtime != characterized
   version family (0.16.x) since the protocol surface drifts (as `--max-turns`
   already proved).
