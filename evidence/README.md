# Evidence — `zcode-axi watch` (2026-09-05/06, macOS arm64, Wesley's machine)

All files produced by real runs of `target/release/zcode-axi` / repo examples
on this machine. No screen capture was faked; see the honest limitation in
§3.

## 1. Live watch loop over a real captured window (menubar)

Command:

```
./target/release/zcode-axi watch --duration-secs 20 --interval-ms 1000 \
    --window-substr Menubar --notify none --dump-frames /tmp/zcode-axi-menubar
```

- `menubar-frame.png` — real xcap capture of the one enumerable window
  (2560x30 system menubar), dumped by the `--dump-frames` evidence path.
- `menubar-frame.ocr.txt` — what the pure-Rust `ocrs` engine read from those
  exact pixels (system is pt-BR: "Chrome … Arquivo, Editar, Visualizar,
  Histórico, Favoritos, Perfis, …"). Rough quality on a 30 px strip, but
  genuinely engine output — and correctly containing NO zcode task markers.
- `watch-live-run.jsonl` — verbatim stdout: `watch_start` → `state` event
  (OCR found no marker; the `sqlite3 -readonly` task-index cross-check
  settled the state to `done` with `confidence:"tasks"` against the real
  `~/.zcode/v2/tasks-index.sqlite`) → `heartbeat` → `watch_end`, exit 0.

## 2. OCR → classifier proof (synthetic zcode-like frame)

Command: `cargo run --release --example ocr-fixture` (repo example; renders
text with a built-in 5x7 bitmap font, then runs the REAL production OCR path
`Ocr::load()` + `Ocr::text()` and the real classifier).

- `fixture-permission-frame.png` — rendered frame: status line "WORKING FOR
  12S" + dialog "PERMISSION REQUIRED / ALLOW ONCE  ALLOW ALWAYS".
- `fixture-permission-frame.ocr.txt` — OCR output: `WOFEIN FOR 125 /
  PERMISSION FE WIRED / ALLOW ONCE ALLOW ALWAY!` (imperfect on a pixel
  font — the real GUI renders anti-aliased text at much larger sizes).
- Classification result: `awaiting_approval` — the marker set is
  deliberately redundant ("allow once" matched even though the headline
  garbled), which is exactly the robustness the classifier needs against
  noisy OCR. Asserted in the example (it fails if classification misses).

## 3. Screen Recording permission denial (honest limitation)

Command: `./target/release/zcode-axi watch --duration-secs 20`

- `watch-permission-denied.txt` — verbatim stderr + exit code 6.

The ZCode GUI **was running** (ZCode.app 3.11.2, pid 32590) during these
runs, but this process tree (zcode-cli session host) lacks the macOS Screen
Recording TCC grant: xcap enumerates ONLY system surfaces ("Window Server —
Menubar"); other applications' windows are hidden. `watch` detects that
signature, fails ONCE with exit 6 and grant guidance, and never retries.
Consequence: a live `state=running` transition on the actual ZCode window
could NOT be demonstrated from this session; the loop itself (capture →
diff → OCR → classify → task-index cross-check → JSON events) is proven
live on the menubar window in §1, and the classifier is proven against
zcode-marker text in §2 + unit tests. Granting Screen Recording to the host
app requires an interactive System Settings action by the user.
