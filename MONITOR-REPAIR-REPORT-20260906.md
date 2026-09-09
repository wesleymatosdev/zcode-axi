# Monitor repair report — 2026-09-06

## Status

Repository repair and non-GUI verification are complete. The release binary was built and tested. The monitor is **not ready for operational sign-off** until the coordinator-owned live GUI capture, 60-second monitoring, and notification-delivery gates pass.

No ZCode call, prompt submission, GUI interaction, permission change, deployment, push, install, or external-directory access was performed.

## Demonstrated root causes

1. Window selection accepted a title or application substring match without requiring a verified ZCode owner. The checked-in live evidence records selection of `Window Server — Menubar` at `2560x30`, followed by marker-free OCR. That is not the ZCode main window (`evidence/watch-live-run.jsonl`, `evidence/menubar-frame.ocr.txt`). The repaired selector requires the exact case-insensitive application owner `ZCode`, rejects title-only matches and `ZCodeWatcher`, applies the existing minimum geometry, and chooses the largest qualifying owner window.
2. Marker-free OCR was converted to `done` solely because the newest task-index row was historically `completed`. The same live evidence records `confidence:"tasks"` immediately after OCR of the menubar. The repaired classifier permits the task index only to corroborate an OCR `task completed` marker; empty/unknown OCR remains unknown. An idle, unavailable, stale, or completed index row cannot independently prove success.

## Hypotheses not presented as root cause

The prior claim of a native abort between capture and signature/uniform/OCR was not reproducible from repository evidence. A generated 2560x1440 RGBA frame now exercises signature generation and downscaling in a regression test. Invalid dimensions, multiplication overflow, and buffer-length mismatch now return descriptive errors instead of panicking. This hardens one deterministic panic boundary but does not prove that it caused the historical process exit.

OCR model readiness and macOS capture permission remain environment-dependent. Model loading already reports missing/load/init errors, and capture permission already has an explicit exit-6 path. No external model or permission location was read during this repair.

## Changes

- Added a pure, tested main-window selector: exact case-insensitive `ZCode` application owner, minimum `320x200`, and largest usable area. Larger Chrome/editor windows titled `ZCode`, system surfaces, and `ZCodeWatcher` are rejected.
- `watch` captures only the selected window. Both retained PoC entry points were changed from monitor/full-desktop capture to the same window-only selector.
- Removed task-index-only completion classification. `done` requires OCR and may be raised from `ocr` to `ocr+tasks` confidence only when the latest task agrees.
- Made frame signature validation checked and fallible, including zero size, integer overflow, and RGBA buffer mismatch.
- Made PoC OCR previews truncate by Unicode characters instead of slicing at an arbitrary UTF-8 byte boundary, which could panic on non-ASCII OCR text.
- Added `processing` JSON events with stage, outcome (`running`, `awaiting_approval`, `done`, `unchanged`, `unknown`, or `error`), elapsed OCR milliseconds, capture/signature timing, OCR character count, and iteration. Existing permission/unavailable failures remain explicit nonzero errors rather than completion.
- Added regression tests for menubar rejection/main-window preference, task-index non-authority, checked-in OCR evidence, realistic 2560x1440 processing, and invalid frame input.

The synthetic 2560x1440 buffer and existing OCR fixture are fixtures, not live capture proof.

## Verification

All required commands passed:

- `cargo fmt --check` — exit 0; `evidence/monitor-20260906/cargo-fmt-check.txt` (empty stdout/stderr is expected)
- `cargo clippy --all-targets -- -D warnings` — exit 0; `evidence/monitor-20260906/cargo-clippy.txt`
- `cargo test` — exit 0; 35 passed, 0 failed; `evidence/monitor-20260906/cargo-test.txt`
- `cargo build --release` — exit 0; `evidence/monitor-20260906/cargo-build-release.txt`

Release artifact: `target/release/zcode-axi`, Mach-O arm64, SHA-256 `42cc1187d598e975ba682ecf01da65c7c0215db802784c09ee37a23c93e49ace` (`evidence/monitor-20260906/release-binary.txt`).

## Pending coordinator gates and exact later commands

These commands are instructions for the coordinator and were not run here. Do not launch the bundle executable from a shell: that process inherits the terminal's TCC identity and does not exercise the bundle's Screen Recording grant. Launch through LaunchServices with `open`; replace `APP`, `ABSLOG`, `ABSERR`, and `ABSDIR` with the application name/path and absolute repository evidence paths:

```sh
open -n -g -W -a APP --stdout ABSLOG --stderr ABSERR --args watch \
  --duration-secs 60 --notify none --dump-frames ABSDIR
```

The coordinator must confirm `watch_start` reports main-window-sized geometry, processing heartbeats continue for 60 seconds, no desktop image is captured, and unknown/error/unavailable outcomes are not `done`. Notification delivery is a separate explicit gate:

```sh
open -n -g -W -a APP --stdout ABSLOG --stderr ABSERR --args watch \
  --duration-secs 60 --notify telegram
```

Do not approve Screen Recording automatically. If exit 6 reports permission denied, the coordinator must handle that gate interactively and rerun.

## Commit

The scoped local commit could not be created because the sandbox exposes `.git` read-only. The attempted repository-local command failed before staging or committing with:

```text
fatal: Unable to create '~/project/projects/personal/zcode-axi/.git/index.lock': Operation not permitted
```

No bypass or permission escalation was attempted, and no push was performed. The coordinator can create the scoped commit after reviewing the preserved working tree with:

```sh
git add Cargo.toml Cargo.lock README.md src/classify.rs src/cli.rs \
  src/framediff.rs src/lib.rs src/main.rs src/watch.rs src/poc.rs \
  examples/screenpipe-poc.rs tests/units.rs evidence/monitor-20260906 \
  MONITOR-REPAIR-REPORT-20260906.md
git diff --cached --check
git commit -m "repair window monitor processing"
```
