#!/usr/bin/env bash
# Build + code-sign ZCodeWatcher.app.
#
# Default stays ad-hoc (`codesign --force --sign -`), the historical default.
# Pass a keychain identity name to sign with a STABLE identity instead: TCC
# keys ad-hoc-signed bundles by binary cdhash, so every rebuild orphans the
# Screen Recording grant for dev.wesleymatos.zcode-watcher. An
# identity-signed bundle gets a certificate-anchored designated requirement,
# so the grant survives rebuilds as long as the identity does not change.
#
# This script NEVER mutates a keychain. Creating or importing keychain
# identities pops user-facing dialogs and is a one-time manual step by
# Wesley — see the commented ONE-TIME IDENTITY SETUP section at the bottom
# and packaging-watcher/README.md. Agents must never execute that section.
#
# usage: packaging-watcher/sign-with-identity.sh [--dry-run] [--install]
#                                                [--app PATH] [IDENTITY]
set -euo pipefail

APP_NAME="ZCodeWatcher.app"
BUNDLE_ID="dev.wesleymatos.zcode-watcher"
EXEC_NAME="zcode-axi" # must match CFBundleExecutable in Info.plist

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLIST_SRC="$REPO_ROOT/packaging-watcher/Info.plist"
REL_BIN="$REPO_ROOT/target/release/$EXEC_NAME"
STAGE_ROOT="$REPO_ROOT/packaging-watcher/build"
STAGE_APP="$STAGE_ROOT/$APP_NAME"

usage() {
  cat <<'USAGE'
usage: packaging-watcher/sign-with-identity.sh [--dry-run] [--install] [--app PATH] [IDENTITY]

Build ZCodeWatcher.app from target/release/zcode-axi and code-sign it.

  IDENTITY    keychain identity (certificate common name) to sign with.
              Omitted -> ad-hoc signature (codesign -s -), the historical
              default. Ad-hoc signatures are re-keyed by cdhash on every
              build, which orphans the Screen Recording TCC grant; pass a
              stable identity to keep the grant across rebuilds.
  --dry-run   print the exact plan (build, bundle assembly, codesign,
              verification, any TCC re-consent procedure) without building
              or signing anything.
  --install   after signing, replace ~/Applications/ZCodeWatcher.app with
              the freshly signed bundle. Run this with Wesley present: an
              identity change invalidates the current TCC grant.
  --app PATH  installed bundle to compare the signing identity against
              (default: ~/Applications/ZCodeWatcher.app).

Exit codes: 0 ok, 2 usage error, nonzero if any step fails.
This script never mutates a keychain; identity creation is a documented
manual one-time step (see the bottom of this file and the README).
USAGE
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 2
}

# ---------------------------------------------------------------- arguments

identity=""
dry_run=0
install=0
app_path="${HOME}/Applications/${APP_NAME}"

while [ $# -gt 0 ]; do
  case "$1" in
    -h | --help)
      usage
      exit 0
      ;;
    --dry-run)
      dry_run=1
      ;;
    --install)
      install=1
      ;;
    --app)
      [ $# -ge 2 ] || die "--app needs a path argument"
      app_path="$2"
      shift
      ;;
    -*)
      usage >&2
      die "unknown option: $1"
      ;;
    *)
      [ -z "$identity" ] || die "only one IDENTITY argument is allowed"
      identity="$1"
      ;;
  esac
  shift
done

if [ "$identity" = "-" ]; then
  die 'pass an identity NAME, or no argument for ad-hoc ("-" is reserved)'
fi

# ------------------------------------------------------------------- helpers

# Print each command instead of running it under --dry-run.
run() {
  if [ "$dry_run" = 1 ]; then
    printf '[dry-run] %s\n' "$*"
  else
    printf '+ %s\n' "$*"
    "$@"
  fi
}

installed_designator() {
  local dv auth
  if ! dv="$(codesign -dv "$app_path" 2>&1)"; then
    echo "unreadable"
    return
  fi
  if printf '%s\n' "$dv" | grep -q '^Signature=adhoc'; then
    echo "adhoc"
    return
  fi
  auth="$(printf '%s\n' "$dv" | sed -n 's/^Authority=//p' | head -n 1)"
  if [ -n "$auth" ]; then
    echo "$auth"
  else
    echo "unsigned-or-unknown"
  fi
}

print_regrant_procedure() {
  cat <<EOF

=========================================================================
 TCC RE-CONSENT REQUIRED (signing identity changed vs installed bundle)
   installed : ${installed_id}
   requested : ${requested_id}

 The Screen Recording grant for ${BUNDLE_ID} is keyed to the OLD
 signature. After installing the new signature, re-consent exactly once:

   1) tccutil reset ScreenCapture ${BUNDLE_ID}

   2) install the signed bundle (re-run this script with --install),
      with Wesley present — do not install silently over a live grant.

   3) launch through LaunchServices so the BUNDLE (not the terminal)
      asks for consent — the app itself calls CGRequestScreenCaptureAccess
      (ensure_capture_permission) and macOS shows the dialog:
        open -a ~/Applications/${APP_NAME} --args watch \\
          --duration-secs 5 --interval-ms 1000 --notify none

   4) allow ZCodeWatcher (never Terminal) in
      System Settings > Privacy & Security > Screen (and System Audio) Recording,
      then rerun the 60s live gate (evidence/monitor-20260906/LIVE-GATE.md).

 After this ONE re-consent, rebuilds signed with the SAME identity keep
 the grant; only an identity change re-triggers this procedure.
=========================================================================
EOF
}

# ------------------------------------------------- identity-change detection

if [ -n "$identity" ]; then
  requested_id="$identity"
  sign_arg="$identity"
else
  requested_id="adhoc"
  sign_arg="-"
fi
installed_id="$(installed_designator)"

identity_changed=1
if [ "$requested_id" = "adhoc" ]; then
  [ "$installed_id" = "adhoc" ] && identity_changed=0
else
  case "$installed_id" in
    *"$requested_id"*) identity_changed=0 ;;
  esac
fi

printf 'bundle        : %s (%s)\n' "$APP_NAME" "$BUNDLE_ID"
printf 'installed sig : %s\n' "$installed_id"
printf 'requested sig : %s\n' "$requested_id"
if [ "$identity_changed" = 0 ]; then
  printf 'identity      : unchanged — Screen Recording grant should survive this build\n'
else
  printf 'identity      : CHANGED — one-time TCC re-consent will be required (printed below)\n'
fi

# -------------------------------------------------------------------- build

if [ "$dry_run" = 1 ]; then
  printf '[dry-run] (cd %s && cargo build --release)\n' "$REPO_ROOT"
else
  (cd "$REPO_ROOT" && cargo build --release)
  [ -f "$REL_BIN" ] || {
    printf 'error: release binary missing: %s\n' "$REL_BIN" >&2
    exit 1
  }
fi

# ------------------------------------------------------- assemble the bundle

run mkdir -p "$STAGE_APP/Contents/MacOS"
run cp "$PLIST_SRC" "$STAGE_APP/Contents/Info.plist"
run cp "$REL_BIN" "$STAGE_APP/Contents/MacOS/$EXEC_NAME"
run chmod 755 "$STAGE_APP/Contents/MacOS/$EXEC_NAME"

# -------------------------------------------------------------------- sign

run codesign --force --sign "$sign_arg" "$STAGE_APP"

# ------------------------------------------------------------------ verify

run codesign --verify --deep --strict "$STAGE_APP"
if [ "$dry_run" = 1 ]; then
  printf '[dry-run] codesign -dv --verbose=2 "%s" (output printed here in a real run)\n' "$STAGE_APP"
else
  printf '---- codesign -dv ----\n'
  codesign -dv --verbose=2 "$STAGE_APP" 2>&1
fi

# ------------------------------------------------------------------ install

if [ "$install" = 1 ]; then
  run mkdir -p "${HOME}/Applications"
  run rm -rf "$app_path"
  run ditto "$STAGE_APP" "$app_path"
  printf 'installed: %s\n' "$app_path"
else
  printf 'staged (not installed): %s\n' "$STAGE_APP"
fi

# ------------------------------------------------- TCC re-consent procedure

if [ "$identity_changed" = 1 ]; then
  print_regrant_procedure
fi

# =============================================================================
# ONE-TIME IDENTITY SETUP — Wesley runs this himself, interactively, ONCE.
#
# NEVER executed by this script and never by any agent: creating or
# importing keychain identities pops user-facing dialogs and touches the
# login keychain. The test suite (tests/units.rs) enforces that every line
# below stays inside this comment block.
#
# Goal: one stable self-signed code-signing identity, e.g.
#         zcode-watcher-codesign
# so `codesign -dv` stops reporting Signature=adhoc and TCC keys the
# Screen Recording grant to the certificate instead of the binary cdhash.
#
# CLI recipe (works with LibreSSL and OpenSSL; no GUI needed):
#
#   # 1) key + self-signed cert with the code-signing extended key usage
#   cat > /tmp/zcode-watcher-codesign.cnf <<'CNF'
#   [req]
#   distinguished_name = dn
#   prompt = no
#   [dn]
#   CN = zcode-watcher-codesign
#   [v3_req]
#   keyUsage = digitalSignature
#   extendedKeyUsage = codeSigning
#   CNF
#   openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
#     -config /tmp/zcode-watcher-codesign.cnf -extensions v3_req \
#     -keyout /tmp/zcode-watcher-codesign.key \
#     -out /tmp/zcode-watcher-codesign.cer
#
#   # 2) wrap cert + key in a p12 (pick your own export password)
#   openssl pkcs12 -export \
#     -inkey /tmp/zcode-watcher-codesign.key \
#     -in /tmp/zcode-watcher-codesign.cer \
#     -out /tmp/zcode-watcher-codesign.p12 \
#     -passout pass:EXPORT_PASSWORD
#
#   # 3) import into the login keychain, pre-authorizing /usr/bin/codesign
#   #    (-T). If macOS later asks "codesign wants to use a key", click
#   #    Always Allow — that prompt is expected, one-time, and why this
#   #    section must be run by a human.
#   security import /tmp/zcode-watcher-codesign.p12 \
#     -k ~/Library/Keychains/login.keychain-db \
#     -P EXPORT_PASSWORD \
#     -T /usr/bin/codesign
#
#   # 4) confirm the identity is listed and valid
#   security find-identity -v
#     -> ... 1 valid identities found
#        ... "zcode-watcher-codesign"
#
#   # 5) first identity-signed build + install + ONE re-consent
#   packaging-watcher/sign-with-identity.sh zcode-watcher-codesign --install
#   (follow the printed TCC procedure; see packaging-watcher/README.md)
#
# GUI alternative: Keychain Access > Certificate Assistant > Create
# Certificate; name zcode-watcher-codesign, identity type Self-Signed Root,
# certificate type Code Signing. Same follow-up steps 4-5.
#
# Shred the intermediates afterwards:
#   rm -f /tmp/zcode-watcher-codesign.{cnf,key,cer,p12}
# =============================================================================
