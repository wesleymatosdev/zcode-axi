# zcode-axi push-prep report — 2026-09-06

Verification that the repository is ready for Wesley's review, performed
immediately before push. Working scope: only this repository; no pushes, no
process/tmux/`~/.zcode` interaction, no GUI or permission changes.

## Repository state at verification time

- Branch: `main`, in sync with `origin/main` (0 ahead / 0 behind) **before**
  this report's commit.
- HEAD: `6b69b5c7b76cf3269283bacd5fb9c488f0413f01`
  (`notify: fix real transport — message via stdin, target --to telegram:W`)
- `git status --porcelain` before this report: **empty** — clean tree, no
  dirty or untracked files. The only change made by this push-prep pass is
  this report file.

## Gates run (exact commands and results)

All three gates pass. Run on macOS arm64, rust cargo, from the repo root.

```
$ cargo fmt --check
FMT_EXIT=0                                # no output — correctly formatted

$ cargo test
     Running unittests src/lib.rs         test result: ok. 0 passed; 0 failed
     Running unittests src/main.rs        test result: ok. 0 passed; 0 failed
     Running tests/units.rs               test result: ok. 38 passed; 0 failed; 0 ignored (1.31s)
   Doc-tests zcode_axi                    test result: ok. 0 passed; 0 failed
TEST_EXIT=0                               # 38 total, all passing

$ cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.07s
CLIPPY_EXIT=0                             # zero warnings
```

Prior runs recorded in `evidence/monitor-20260906/` (cargo-test.txt,
cargo-clippy.txt, cargo-fmt-check.txt, cargo-build-release.txt) are from the
monitor-repair commit series; the counts there (35 tests) predate the three
notify tests added in `c70ca1d`/`6b69b5c`, consistent with the current 38.

## README accuracy audit (claims vs. code/evidence)

The README references no evidence paths directly; every behavioral claim and
every referenced file was checked against source and the evidence tree.
**All accurate — no corrections needed.**

| README claim | Verified against |
|---|---|
| `docs/EXIT-CODES.md` exit-code contract link | file exists; exit 6 = screen-recording denial, fails once, never retry-spams (`docs/EXIT-CODES.md:14`) |
| `watch [--window-substr ZCode] [--interval-ms 1000] [--notify none\|telegram] [--duration-secs S] [--dump-frames DIR]` | `src/cli.rs:94-116` — same flags, same defaults (`ZCode`, `1000`, `none`, `0`, optional) |
| events `watch_start`, `processing`, `state`, `heartbeat`, `watch_end` | `src/watch.rs:344, 426/438, 550, 459, 478` |
| heartbeat ~10 s | `src/watch.rs:28` (`HEARTBEAT = 10s`) |
| max 1 telegram alert per state per 5 min; `notified:false` on suppression | `src/notify.rs:16` (`RATE_LIMIT = 5*60s`); rate-limiter unit tests pass |
| `--dump-frames` saves PNG + OCR pairs, max 10/run | `src/watch.rs:30` (`MAX_DUMPS = 10`) |
| task index via `sqlite3 -readonly`, refreshed ≤1×/10 s, corroboration only (cannot prove success alone) | `src/watch.rs:26` (`TASKS_REFRESH = 10s`); repaired classifier behavior asserted by passing tests (`classifier_fixture_samples_per_state`, task-index non-authority test) |
| OCR models from `$ZCODE_AXI_OCR_MODELS_DIR` or `~/.cache/zcode-axi/ocrs/`, error message carries download commands | `src/ocr.rs:13-30` |
| 64×36 block-grid frame diff | `src/framediff.rs:8-9` (`SIG_W=64`, `SIG_H=36`) |
| Screen Recording denial → exit 6 once | `evidence/watch-permission-denied.txt` (verbatim stderr, exit 6) |

Evidence tree integrity: every file referenced by `evidence/README.md` exists
(`watch-live-run.jsonl`, `menubar-frame.{png,ocr.txt}`,
`fixture-permission-frame.{png,ocr.txt}`, `watch-permission-denied.txt`,
`monitor-20260906/*`).

## Documentation discrepancies

**None blocking.** One judgment call worth recording for the reviewer:

- `evidence/README.md` §1 documents a pre-repair live run using
  `--window-substr Menubar`. The repaired window selector (see
  `MONITOR-REPAIR-REPORT-20260906.md` §"root causes") now requires the exact
  `ZCode` application owner, so that exact command would no longer select the
  menubar. This is intentionally left untouched: `evidence/README.md` is a
  dated historical record of real runs, and the monitor-repair report cites
  that same menubar evidence as the recorded defect. Rewriting it would falsify
  the audit trail.

Per the task constraints, no code or README changes were made.

## Blockers for push

None. Gates green, clean tree, README accurate against code and evidence.

Non-blocker context (operational, not repository readiness): the live
coordinator gates described in `MONITOR-REPAIR-REPORT-20260906.md` (60 s
main-window live capture via LaunchServices, telegram delivery gate) remain
pending and are explicitly out of scope for this push-prep pass.

## Commit

This report is the only change in the local push-prep commit (conventional
commit on `main`). Not pushed, per instructions.
