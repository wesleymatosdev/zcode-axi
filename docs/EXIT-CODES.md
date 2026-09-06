# zcode-axi exit codes

Stable contract for machine callers (Hermes). Every error line is printed to
stderr as `zcode-axi: <message> (exit N)` so callers can branch on either.

| Code | Meaning | Produced by |
|------|---------|-------------|
| 0 | ok | any successful command |
| 1 | runtime error | zcode missing/failing, headless run non-zero exit, app-server protocol error, session store unusable, session not found |
| 2 | usage | argument parser (missing/conflicting flags, bad subcommand) |
| 3 | not authenticated | headless run failed with an auth-looking error (login/401/oauth/credential markers in stderr) |
| 4 | timeout | `wait` exceeded `--timeout`; app-server frame deadline (15s) exceeded |
| 5 | unsupported by runtime | operation the runtime cannot perform, stated explicitly instead of faked — currently: `cancel` on a session that is not active in the app-server (`session/stop` → `-32004`) |
| 6 | screen recording permission denied | `watch` cannot capture the screen (macOS TCC denial: capture fails or yields solid-black frames). Fails once with guidance; never retry-spams |

Notes:

- Exit 2 comes from the argument parser itself (clap); zcode-axi maps all
  its own failures onto 1/3/4/5/6.
- Login state is never read or modified; "not authenticated" is inferred
  from a trivial headless round-trip (see docs/protocol.md §4).
- `--max-turns` on `run`/`resume` does NOT fail when the runtime lacks the
  flag: zcode-axi prints a warning to stderr and runs without it, because
  zcode 0.16.5's parser rejects the flag its own `--help` advertises.
