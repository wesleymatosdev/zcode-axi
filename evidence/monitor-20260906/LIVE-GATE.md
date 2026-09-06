# Live monitor gate — 2026-09-06

Parent independently ran cargo test (35 passed), cargo fmt --check, cargo clippy --all-targets -- -D warnings, cargo build --release successfully after owner-only selector correction.

Installed release into existing ~/Applications/ZCodeWatcher.app, preserving previous app in local evidence/monitor-20260906/bundle-backup/ (do not commit binary backup). Re-signed ad hoc with same dev.wesleymatos.zcode-watcher bundle identity. codesign --verify succeeded. Post-sign binary cmp differs because signing changes executable signature; no claim of byte identity made.

Actual LaunchServices test: open -n -g -W -a ~/Applications/ZCodeWatcher.app with explicit --stdout/--stderr absolute evidence paths and watch --window-substr ZCode --duration-secs 60 --interval-ms 1000 --notify none --dump-frames absolute live-frames dir.

FAILED at permission gate before capture: live-60s.jsonl is empty; live-60s.stderr reports screen recording permission denied, only Window Server menubar enumerable, exit 6. open itself returned 0 (launcher status is not child success).

No image captured, no OCR pass, no 60-second loop proof, no Telegram notification tested. No ZCode prompt sent. No permission changed or retried. User must authorize the dedicated ZCodeWatcher app in System Settings > Privacy & Security > Screen & System Audio Recording (wording may vary). Never grant Terminal. Re-test only after user confirms.

Code remains uncommitted; worker sandbox blocked .git writes. Monitor is NOT operational. The historic crash remains unproven.
