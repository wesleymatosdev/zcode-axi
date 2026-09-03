# zcode-axi — protocol characterization (v0.1)

Runtime characterized: **zcode 0.16.5** (official), resolved via `~/.local/bin/zcode`
(shell launcher) → `node /tmp/zcode-official-glm/zcode.cjs` (node-bundle,
`apps/zcode-cli/packages/cli/dist/zcode.cjs`). `zcode doctor --json` reports
`execPath` = the running node, `runtime.platform`/`arch`.

Characterization date: 2026-09-03. Probe tool: `scripts/probe.mjs`.

**Verdict: the app-server protocol WAS characterized within the timebox.**
zcode-axi v0.1 uses it for `sessions` / `wait` / `cancel`; `run` / `resume` use
headless `zcode --json -p`; `inspect` reads the persisted SQLite store read-only
(the app-server only serves message bodies for *active* sessions — see §3.4).

---

## 1. `zcode app-server` — "ZCode Protocol" over stdio

### 1.1 Framing — NOT JSON-RPC 2.0

One JSON object per line over stdio. The server REJECTS the JSON-RPC `jsonrpc`
envelope key. Accepted message shapes (validated by a strict zod union,
`strict()` = no unknown keys):

```jsonc
// request (client -> server)
{ "id": 1, "method": "session/list", "params": {} }
// notification (client -> server, no id)
{ "method": "notifications/initialized", "params": {} }
// success response (server -> client)
{ "id": 1, "result": { ... } }
// error response (server -> client)
{ "id": 1, "error": { "code": -32601, "message": "...", "data": { ... } } }
```

`id` may be number or string. `params` must be an object when present.
`trace` is an optional extra key on requests.

Error codes observed: `-32700` parse error, `-32600` invalid message,
`-32601` method not found, `-32602` invalid params (with zod issue detail in
`error.data.message`), `-32004` "Session is not active".

### 1.2 Verbatim probe transcript (2026-09-03, zcode 0.16.5)

Attempt 1 — JSON-RPC 2.0 envelope (REJECTED, abbreviated; five identical
responses, one per input line):

```json
-> {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"zcode-axi-probe","version":"0.1.0"}}}
-> {"jsonrpc":"2.0","method":"notifications/initialized","params":{}}
-> {"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}
-> {"jsonrpc":"2.0","id":3,"method":"sessions/list","params":{}}
-> {"jsonrpc":"2.0","id":4,"method":"rpc.discover","params":{}}
<- {"error":{"code":-32600,"data":{"issues":[{"code":"invalid_union","errors":[[{"code":"unrecognized_keys","keys":["jsonrpc"],"path":[],"message":"Unrecognized key: \"jsonrpc\""}],[{"code":"unrecognized_keys","keys":["jsonrpc","id"],"path":[],"message":"Unrecognized keys: \"jsonrpc\", \"id\""}],[{"code":"unrecognized_keys","keys":["jsonrpc","method","params"],"path":[],"message":"Unrecognized Keys: \"jsonrpc\", \"method\", \"params\""}],[{"expected":"object","code":"invalid_type","path":["error"],"message":"Invalid input: expected object, received undefined"},{"code":"unrecognized_keys","keys":["jsonrpc","method","params"],"path":[],"message":"Unrecognized key: \"jsonrpc\""}]],"path":[],"message":"Invalid input"}]},"message":"Invalid ZCode Protocol message"},"id":"invalid-message"}
```

Attempt 2 — bare frame, wrong method name:

```json
-> {"id":1,"method":"initialize","params":{}}
<- {"error":{"code":-32601,"message":"Method not found: initialize"},"id":1}
```

Attempt 3 — `session/list` (empty params) — SUCCESS (first entry verbatim,
list truncated for length; 10 sessions returned live):

```json
-> {"id":1,"method":"session/list","params":{}}
<- {"id":1,"result":{"sessions":[{"createdAt":1788470528730,"mode":"build","traceId":"84de47e8-680c-4851-b6af-1c585992cfc0","sessionId":"sess_21c33923-fd70-4ab2-bc51-f6f3c313bfdf","sessionKind":"interactive","status":"idle","title":"# SWARM BRIEF — zcode-axi v0.1 (worker: zcode, GLM) You a...","titleSource":"first_input","updatedAt":1788470656390,"workspace":{"workspaceKey":"/Users/wesleymatos/projects/personal/zcode-axi","workspacePath":"/Users/wesleymatos/projects/personal/zcode-axi"}}, ...]}}
```

Attempt 4 — `usage/stats` (empty params) — param validation error teaches the schema:

```json
-> {"id":2,"method":"usage/stats","params":{}}
<- {"error":{"code":-32602,"data":{"name":"ZodError","message":"[\n  {\n    \"code\": \"invalid_value\",\n    \"values\": [\"all\",\"7d\",\"30d\"],\n    \"path\": [\"range\"],\n    \"message\": \"Invalid option: expected one of \\\"all\\\"|\\\"7d\\\"|\\\"30d\\\"\"\n  }\n]"},"message":"Invalid params — range: Invalid option: expected one of \"all\"|\"7d\"|\"30d\""},"id":2}
```

Param-schema discovery round (empty params, reading the zod errors):

```json
-> {"id":1,"method":"session/create","params":{}}
<- {"error":{"code":-32602,...,"message":"Invalid params — workspace.workspacePath: Invalid input: expected string, received undefined; workspace.workspaceKey: Invalid input: expected string, received undefined"},"id":1}

-> {"id":2,"method":"session/send","params":{}}
<- {"error":{"code":-32602,..."message":"Invalid params — sessionId: Invalid input: expected string, received undefined; content: Invalid input: expected string, received undefined"},"id":2}

-> {"id":3,"method":"session/stop","params":{}}
<- {"error":{"code":-32602,..."message":"Invalid params — sessionId: Invalid input: expected string, received undefined"},"id":3}

-> {"id":4,"method":"session/read","params":{}}
<- {"error":{"code":-32602,..."message":"Invalid params — sessionId: Invalid input: expected string, received undefined"},"id":4}
```

Active-session requirement (session id from `session/list`, but not loaded in
the app-server process):

```json
-> {"id":2,"method":"session/read","params":{"sessionId":"sess_b7ed13dc-fa65-4a22-a3e9-611a0998f9dd"}}
<- {"error":{"code":-32004,"message":"Session is not active: sess_b7ed13dc-fa65-4a22-a3e9-611a0998f9dd"},"id":2}

-> {"id":3,"method":"session/messages","params":{"sessionId":"sess_b7ed13dc-fa65-4a22-a3e9-611a0998f9dd"}}
<- {"error":{"code":-32004,"message":"Session is not active: sess_b7ed13dc-fa65-4a22-a3e9-611a0998f9dd"},"id":3}
```

### 1.3 Method table (extracted from the official bundle, live-verified where noted)

From the method constants table in `zcode.cjs` (`rr = { sessionCreate:
"session/create", ... }`). Live-verified: `session/list`. Zod-verified via
empty-params probe: `session/create`, `session/send`, `session/stop`,
`session/read`, `session/resume`, `usage/stats`.

- `session/create` — params `{workspace: {workspacePath, workspaceKey}, ...}`
- `session/resume` — params `{sessionId, ...}`
- `session/list` — `{}` → `{sessions: [{sessionId, status, title, mode, sessionKind, titleSource, traceId, createdAt, updatedAt, workspace{workspaceKey, workspacePath}}]}`
- `session/subagents`, `session/requestRuntimePreferences`
- `session/read`, `session/messages`, `session/events` — **active sessions only** (error `-32004` otherwise)
- `session/subscribe` / `session/unsubscribe` — streaming events
- `session/send` — `{sessionId, content}` (active sessions)
- `session/stop` — `{sessionId}` (active sessions)
- `session/cancelBackgroundTask`, `session/fork`, `session/compact`, `session/goal`, `session/close`, `session/setModel`, `session/setThoughtLevel`, `session/setMode`, `session/updateRuntimeModelConfig`
- `workspace/readState`, `workspace/generateText` (+ many workspace config setters)
- `mcp/list`, `plugins/list` (+ plugin/marketplace management)
- `automation/*` (create/update/list/delete/…), `usage/stats`, `session/usage`

Not JSON-RPC extensions: `tools/list`, `resources/*`, `prompts/*`,
`sampling/createMessage`, `elicitation/create` strings in the bundle are the
MCP *client* side of zcode (it connects out to MCP servers); they are not
app-server methods.

No handshake is required: `session/list` works immediately as the first frame.

### 1.4 Consequences for zcode-axi v0.1

- `sessions` → spawn `zcode app-server`, send `session/list`, print, exit.
- `wait <id>` → poll `session/list` until `status` leaves `"busy"`/reaches
  `"idle"` (interval 1s) or `--timeout` (exit 4).
- `cancel <id>` → `session/stop`; `-32004` (not active) ⇒ exit 5 with an
  explicit "unsupported by runtime for non-active sessions" message, because
  v0.1 never creates app-server sessions itself.
- `inspect <id>` → NOT the app-server (bodies unavailable for inactive
  sessions); read-only SQLite instead (§2).

## 2. Persisted session storage (disk, read-only fallback/inspect)

- `~/.zcode/cli/db/db.sqlite` (+ `-wal`, `-shm`) — SQLite 3.
  - `session` table: `id` (= `sess_...`), `project_id`, `directory`, `title`,
    `version`, `time_created`, `time_updated`, `task_type`, ...
  - `message` table: `id`, `session_id`, `time_created`, `data` (JSON:
    `role`, `time`, `modelID`, `providerID`, `tokens`, `finish`, `semantics`, ...).
  - `part` table: content parts per message: `data` JSON, e.g.
    `{"type":"text","text":"Reply with exactly: OK","time":{...}}`; other
    observed types: `"step-start"`.
- `~/.zcode/cli/rollout/model-io-sess_<id>.jsonl` — raw model I/O logs per session.
- zcode-axi opens the DB with `mode=ro` (URI) only. It never writes.

Sample (verbatim, `sqlite3 ... "SELECT data FROM part LIMIT 1"`):

```json
{"type":"text","text":"Reply with exactly: OK","time":{"start":1788467291983,"end":1788467291983}}
```

## 3. Headless mode (`zcode -p`) — used by `run` / `resume`

### 3.1 Actual accepted flags (0.16.5)

The CLI parser is `node:util.parseArgs` (strict). Accepted global options:
`-h/--help`, `-v/--version`, `--json`, `--output-format text|json|stream-json`,
`--no-color`, `--no-browser`, `--browser-use`, `--browser-executable`,
`-p/--prompt <text>`, `--attach <path>` (multi), `--cwd <path>`, `--locale`,
`--resume <sessionId>`, `--target <text>`, `--target-replace`, `-c/--continue`,
`-f/--force`, `--force-mcs`, `--mode`, `--verbose`, `--stdio`, `--surface`.

**`--max-turns` is NOT accepted by the 0.16.5 parser even though `--help`
lists it (help/parsing drift).** Verbatim:

```
$ zcode --max-turns 2 -p "hi"
Unknown option '--max-turns'. To specify a positional argument starting with a '-', place it at the end of the command after '--', as in '-- "--max-turns"'
(exit 1)
```

(Also not accepted, despite help text: `--allowed-tools`, `--disallowed-tools`
as parser options — only `--disallowedTools`/`--disallowed-tools` appear in an
internal set used elsewhere; not relied on by zcode-axi.)

Cheap capability probe (no model call) — unsupported flag fails before
`--version` short-circuits:

```
$ zcode --max-turns 1 --version
Unknown option '--max-turns'. ...   (exit 1)
```

zcode-axi uses this probe in `run`/`resume`: forward `--max-turns` to the
runtime only if supported; otherwise warn on stderr and omit it.

### 3.2 Headless JSON output contract (verbatim, 2026-09-03)

```
$ zcode --json --cwd /tmp/axi-run-test -p "Reply with exactly: OK"
```

```json
{
  "sessionId": "sess_69b50c9d-6684-41c6-85a0-7baad0e2421f",
  "traceId": "e8ccf63b-c15b-4ca4-8a7b-a129a6b5ca05",
  "turnId": "turn_7c94b56e-3d1a-47eb-a898-6679efafbf10",
  "response": "OK",
  "usage": {
    "source": "provider",
    "modelRequestCount": 1,
    "inputTokens": 12308,
    "outputTokens": 14,
    "totalTokens": 12322,
    "cacheReadTokens": 9728,
    "cacheWriteTokens": 0,
    "reasoningTokens": 0,
    "webFetchRequests": 0,
    "webSearchRequests": 0
  },
  "eventCount": 23,
  "projection": {
    "status": "idle",
    "turnCount": 1,
    "totalTokenCount": 12322,
    "contextUsed": 12322,
    "contextWindow": 200000
  }
}
```

Process exit code 0. This is the machine contract `zcode-axi run` parses and
re-emits. `--resume <id>` composes with `-p` for `zcode-axi resume`.

## 4. Auth model (no credential reads)

- Login state is inferred, never read: `zcode doctor --json` exit code plus a
  trivial headless round-trip (`-p "Reply with exactly: OK"`, checked for a
  0 exit and non-empty `response`). zcode-axi never touches
  `~/.zcode/cli/credentials.json`, never calls `zcode login`/`logout`.
