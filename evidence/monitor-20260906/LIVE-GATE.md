# Live monitor gate — 2026-09-06

Parent independently ran cargo test (35 passed), cargo fmt --check, cargo clippy --all-targets -- -D warnings, cargo build --release successfully after owner-only selector correction.

Installed release into existing ~/Applications/ZCodeWatcher.app, preserving previous app in local evidence/monitor-20260906/bundle-backup/ (do not commit binary backup). Re-signed ad hoc with same dev.wesleymatos.zcode-watcher bundle identity. codesign --verify succeeded. Post-sign binary cmp differs because signing changes executable signature; no claim of byte identity made.

Actual LaunchServices test: open -n -g -W -a ~/Applications/ZCodeWatcher.app with explicit --stdout/--stderr absolute evidence paths and watch --window-substr ZCode --duration-secs 60 --interval-ms 1000 --notify none --dump-frames absolute live-frames dir.

FAILED at permission gate before capture: live-60s.jsonl is empty; live-60s.stderr reports screen recording permission denied, only Window Server menubar enumerable, exit 6. open itself returned 0 (launcher status is not child success).

## Resolution (same day, 11:28 local)

Root cause chain, all evidence-backed:
1. TCC keys ad-hoc-signed apps by binary cdhash; the pre-repair grant belonged to the pre-repair binary. Toggling Settings did not re-anchor it. (No codesigning identity on this machine: `security find-identity` = 0 valid; every rebuild re-breaks the grant.)
2. After `tccutil reset ScreenCapture dev.wesleymatos.zcode-watcher`, the watcher STILL exited 6 without prompting: macOS only shows the consent dialog in response to a real capture-access request, and the watcher gated on window enumeration alone — it never asked. Fixed by adding `ensure_capture_permission()` (CGPreflightScreenCaptureAccess / CGRequestScreenCaptureAccess — the screenpipe pattern) before `find_window` in both `watch` and `poc`.
3. Consent dialog confirmed visible on screen (computer-use capture, "ZCodeWatch deseja gravar a tela e o áudio"); Wesley allowed it in System Settings.

PASSED gate (live-gate-pass.jsonl/.stderr/.ocr.txt, committed): 60 iterations @1000ms, ZCode window 1200x800, 2088 OCR chars of real GUI content, empty stderr, exit 0. Monitor operational for capture/OCR/classify. Telegram notification delivery still untested; the historic full-frame crash remains unproven/unreproduced.

No image captured, no OCR pass, no 60-second loop proof, no Telegram notification tested. No ZCode prompt sent. No permission changed or retried. User must authorize the dedicated ZCodeWatcher app in System Settings > Privacy & Security > Screen & System Audio Recording (wording may vary). Never grant Terminal. Re-test only after user confirms.

Code remains uncommitted; worker sandbox blocked .git writes. Monitor is NOT operational. The historic crash remains unproven.
