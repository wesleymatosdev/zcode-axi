# Packaging ZCodeWatcher.app

This directory packages `target/release/zcode-axi` as the menu-bar-less app
bundle the monitor runs through LaunchServices:

- [`Info.plist`](Info.plist) — bundle `dev.wesleymatos.zcode-watcher`,
  `CFBundleExecutable zcode-axi`, `LSUIElement`, and the
  `NSScreenCaptureUsageDescription` string macOS shows next to the consent
  dialog.
- [`sign-with-identity.sh`](sign-with-identity.sh) — build + assemble +
  code-sign the bundle. Ad-hoc by default; a stable identity keeps the TCC
  grant alive across rebuilds.

Installed location: `~/Applications/ZCodeWatcher.app` (what
`swarm/scripts/launch-zcode-watcher.sh` and `open -a` resolve to).

## Why every ad-hoc rebuild breaks Screen Recording

macOS TCC does not key a Screen Recording grant to the bundle id alone. For
an **ad-hoc-signed** app (`codesign -s -`) the bundle has no certificate, so
the code's designated requirement degrades to *this exact binary* — TCC keys
the grant to the binary's **cdhash**. A rebuild produces a different binary,
a different cdhash, and the previous grant no longer matches: the watcher
fails the permission gate with exit 6 and someone has to re-consent by hand.

This bit the project twice on 2026-09-06; the full root-cause chain is in
[`../evidence/monitor-20260906/LIVE-GATE.md`](../evidence/monitor-20260906/LIVE-GATE.md).
This machine has **zero** codesigning identities
(`security find-identity -v` → 0 valid), so every build so far was ad-hoc
and every rebuild re-broke the grant.

An **identity-signed** bundle gets a certificate-anchored designated
requirement (bundle id + that certificate). TCC keys the grant to the
requirement, not the cdhash, so rebuilds signed with the *same* identity
keep the grant. That is the whole fix: create one identity once, always sign
with it.

## One-time identity creation (Wesley, interactively — never an agent)

Creating or importing a keychain identity pops user-facing dialogs, so this
is deliberately a human, manual, one-time step. The exact commands live in
the commented **ONE-TIME IDENTITY SETUP** block at the bottom of
[`sign-with-identity.sh`](sign-with-identity.sh) (openssl self-signed cert
with `extendedKeyUsage=codeSigning`, `security import … -T /usr/bin/codesign`,
`security find-identity -v` to confirm). Suggested identity name:
`zcode-watcher-codesign`.

The script never executes that block, and `tests/units.rs` enforces that no
keychain-mutating `security` verb ever appears outside its comments.

## Building and signing

```sh
# plan only — prints build/codesign commands + any TCC procedure, runs nothing
packaging-watcher/sign-with-identity.sh --dry-run

# historical default: ad-hoc sign (grant will break again — transitional only)
packaging-watcher/sign-with-identity.sh

# the real fix: sign with the stable identity, stage the bundle
packaging-watcher/sign-with-identity.sh zcode-watcher-codesign

# sign AND replace ~/Applications/ZCodeWatcher.app
packaging-watcher/sign-with-identity.sh zcode-watcher-codesign --install
```

Every run verifies with `codesign --verify --deep --strict`, prints
`codesign -dv` output, and compares the requested identity against the
signature of the installed bundle. If (and only if) the identity changed, it
prints the re-consent procedure below.

## Re-grant flow (only when the signing identity changes)

Run with Wesley present, in this order:

1. Clear the stale grant, scoped to this bundle only:
   `tccutil reset ScreenCapture dev.wesleymatos.zcode-watcher`
2. Install the identity-signed bundle
   (`sign-with-identity.sh <identity> --install`).
3. Launch through LaunchServices — `open -a ~/Applications/ZCodeWatcher.app
   --args watch --duration-secs 5 --interval-ms 1000 --notify none`. The
   bundle process (never a shell) must be the one to ask: the watcher calls
   `CGRequestScreenCaptureAccess` (`ensure_capture_permission`, the
   screenpipe pattern) *before* window enumeration, which is what makes
   macOS actually show the consent dialog. Launching the binary directly
   from a terminal would inherit the terminal's TCC identity instead.
4. Allow **ZCodeWatcher** (never Terminal) in System Settings → Privacy &
   Security → Screen (& System Audio) Recording, then rerun the 60 s live
   gate from `evidence/monitor-20260906/LIVE-GATE.md`.

After that single consent, rebuilds signed with the same identity need no
re-grant. Deleting the keychain identity, or switching identities, re-triggers
the whole flow.

## Hard rules

- No agent/harness may create or import keychain identities or run the
  signed install — both happen with Wesley present.
- The script only stages into `packaging-watcher/build/` (git-ignored) and
  only touches `~/Applications` under an explicit `--install`.
- `tccutil reset` is *printed*, never executed, by the script.
