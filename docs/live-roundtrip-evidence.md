# zcode-axi v0.1 — live round-trip evidence

Date: 2026-09-03. Runtime: official zcode 0.16.5 (`~/.local/bin/zcode` → node bundle).
Binary: `target/release/zcode-axi` (release build). All transcripts verbatim.

## Gate 5a — `zcode-axi status`

```
$ zcode-axi status
zcode=/Users/wesleymatos/.local/bin/zcode	version=0.16.5	doctor_exit=0	auth=ok	in_free_window=true
window_advisory	now is inside the daily free window (local 2026-09-03 18:38:59)

exit_code=0
```

## Gate 5b — `zcode-axi run --cwd <tmpdir> --goal "Reply with exactly: OK" --max-turns 2`

Note: the first stderr line is the documented `--max-turns` capability
warning (zcode 0.16.5's parser rejects the flag despite `--help` listing
it; see docs/protocol.md §3.1). The run proceeds without it and captures the OK.

```
$ zcode-axi run --cwd /tmp/axi-run-test --goal "Reply with exactly: OK" --max-turns 2
session_id=sess_eac8b65f-548b-4820-9e1b-2796eb65309f	exit_code=0
response=OK

exit_code=0
```

## `sessions` (live via app-server `session/list`)

```
$ zcode-axi sessions
sess_73b8d5f8-a34c-4d34-a994-6029d661c917	idle	2026-09-03 21:39:18Z	# SWARM BRIEF — push sweep round 2 (worker: zcode, GLM) Y...
sess_21c33923-fd70-4ab2-bc51-f6f3c313bfdf	idle	2026-09-03 21:39:18Z	# SWARM BRIEF — zcode-axi v0.1 (worker: zcode, GLM) You a...
sess_0c6214d2-c75d-4922-bec7-6adb8e2d5fdd	idle	2026-09-03 21:39:18Z	# SWARM BRIEF — skills.wesleymatos.dev go-live (worker: z...
sess_eac8b65f-548b-4820-9e1b-2796eb65309f	idle	2026-09-03 21:39:11Z	Reply with exactly: OK
sess_b0534bf7-8ed5-4946-b36a-2a870b3519a5	idle	2026-09-03 21:38:59Z	Reply with exactly: OK
sess_5a5669ad-13a6-41cb-9321-ff58310133fe	idle	2026-09-03 21:35:59Z	# SWARM BRIEF — repo hygiene pass 2 (worker: zcode, GLM) ...
sess_12245936-1d79-42d3-bc37-011b4e9f73ad	idle	2026-09-03 21:35:03Z	# SWARM BRIEF — push sweep (worker: zcode, GLM) You are a...
sess_48f1c27f-3780-4c1d-ba5f-6f2798772aa7	idle	2026-09-03 21:32:18Z	# SWARM BRIEF — oss-site (worker: zcode, GLM) You are a b...
sess_4dc475ca-8f4d-49dd-9788-e968365fc8bc	idle	2026-09-03 21:30:56Z	# SWARM BRIEF — repo hygiene: commit all unpushed local w...
sess_10cc345b-58dc-4a4f-b699-b40bc8eae9c6	idle	2026-09-03 21:28:19Z	# SWARM BRIEF — website nav + skills Pages wiring (worker...
sess_69b50c9d-6684-41c6-85a0-7baad0e2421f	idle	2026-09-03 21:27:43Z	Reply with exactly: OK
sess_95bcb582-3660-402b-9cb2-75f5c5c9fcd0	idle	2026-09-03 21:22:41Z	# SWARM BRIEF — repo hygiene: commit all unpushed local w...
sess_b7ed13dc-fa65-4a22-a3e9-611a0998f9dd	idle	2026-09-03 21:21:16Z	Reply with exactly: OK
sess_511be6d5-6f46-4673-8ca0-baba3a30032e	idle	2026-09-03 21:21:11Z	Reply with exactly: OK
sess_d27aac16-0e99-48d0-986c-734a3d382b69	idle	2026-09-03 21:21:05Z	Reply with exactly: OK
sess_8745a59c-0071-4cde-910d-570eaa57e72b	idle	2026-09-03 20:28:49Z	What is 2+2? Answer with just the number.
sess_b0e1d15e-187b-4417-a4f1-b5512c5b45b9	idle	2026-09-03 20:28:16Z	Reply with exactly: OK
exit_code=0
```

## `inspect <id>` (persisted store + live status merge)

```
$ zcode-axi inspect sess_eac8b65f-548b-4820-9e1b-2796eb65309f
id:            sess_eac8b65f-548b-4820-9e1b-2796eb65309f
title:         Reply with exactly: OK
directory:     /tmp/axi-run-test
task_type:     interactive
created:       2026-09-03 21:39:07Z
updated:       2026-09-03 21:39:11Z
live_status:   idle
messages:      2 shown of 2
  [user] Reply with exactly: OK
  [assistant] OK
exit_code=0
```

## `cancel <id>` on a non-active session → documented exit 5

```
$ zcode-axi cancel sess_eac8b65f-548b-4820-9e1b-2796eb65309f
zcode-axi: unsupported by runtime: session sess_eac8b65f-548b-4820-9e1b-2796eb65309f is not active in the app-server; only active sessions can be stopped (exit 5)
zcode-axi: this operation is unsupported by the runtime (exit 5)
exit_code=5
```

## `wait <id>` (already-idle session)

```
$ zcode-axi wait sess_eac8b65f-548b-4820-9e1b-2796eb65309f --timeout 10
session sess_eac8b65f-548b-4820-9e1b-2796eb65309f status=idle
exit_code=0
```

## `resume <id> --goal` (same session id, second turn)

```
$ zcode-axi resume sess_eac8b65f-548b-4820-9e1b-2796eb65309f --goal "Reply with exactly: RESUMED"
session_id=sess_eac8b65f-548b-4820-9e1b-2796eb65309f	exit_code=0
response=RESUMED
exit_code=0
```

## Machine contract (`--json`)

```json
$ zcode-axi --json status | head -c 1200
{"zcode_path":"/Users/wesleymatos/.local/bin/zcode","zcode_version":"0.16.5","doctor_exit":0,"doctor":{"cli":{"name":"zcode","processName":"zcode-cli","version":"0.16.5"},"packaging":{"default":"node-bundle","sea":"optional"},"runtime":{"arch":"arm64","cwd":"/Users/wesleymatos/projects/personal/zcode-axi","execPath":"/Users/wesleymatos/.hermes/node/bin/node","node":"v26.7.0","platform":"darwin","processTitle":"zcode-cli","sea":false}},"auth":"ok","auth_checked_by":"zcode doctor --json exit + trivial headless round-trip (headless round-trip succeeded); credentials never read","campaign_window":{"in_window":true,"window":"Sep 3–20 2026, 12:00–22:00 America/Sao_Paulo (UTC-3)","detail":"now is inside the daily free window (local 2026-09-03 18:39:50)","next_window_unix":null},"campaign_window_disclaimer":"advisory only; zcode-axi does not know or claim actual quota state"}

```

```json
$ zcode-axi --json inspect sess_eac8b65f-548b-4820-9e1b-2796eb65309f
{"session":{"id":"sess_eac8b65f-548b-4820-9e1b-2796eb65309f","directory":"/tmp/axi-run-test","title":"Reply with exactly: OK","version":"0.16.5","time_created":1788471547506,"time_updated":1788471578138,"taskType":"interactive"},"messages":[{"role":"user","text":"Reply with exactly: OK","text_truncated":false,"time":"2026-09-03 21:39:07Z"},{"role":"assistant","text":"OK","text_truncated":false,"time":"2026-09-03 21:39:08Z"},{"role":"user","text":"Reply with exactly: RESUMED","text_truncated":false,"time":"2026-09-03 21:39:33Z"},{"role":"assistant","text":"RESUMED","text_truncated":false,"time":"2026-09-03 21:39:34Z"}],"messages_total":4,"messages_shown":4,"live_status":"idle"}

```
