#!/usr/bin/env bash
# zc-event.sh — worker/completion event publisher (fleet pub-sub, 2026-09-09).
#
# Usage: zc-event.sh <task-id> <event> [one-line detail]
#   event: ready-for-review | blocked | needs-decision | failed | done
#
# Subscribes the event to the fleet topic: sends a one-line message to
# Wesley's Telegram via `hermes send` (reuses gateway creds, no LLM cost).
# The live Telegram session receives it as conversation context; the digest
# cron remains the durable subscriber that verifies/closes.
#
# Classifier-safe: single hermes send call, fixed shape, no script chaining.
set -euo pipefail

[ $# -ge 2 ] || { echo "usage: $0 <task-id> <event> [detail]" >&2; exit 1; }
task_id="$1"
event="$2"
detail="${3:-}"

case "$event" in
  ready-for-review) icon="🔔" ;;
  blocked)          icon="⛔" ;;
  needs-decision)   icon="❓" ;;
  failed)           icon="❌" ;;
  done)             icon="✅" ;;
  *) echo "zc-event: unknown event: $event" >&2; exit 1 ;;
esac

msg="${icon} fleet/${task_id} ${event}${detail:+ — ${detail}}"
HERMES_BIN="$HOME/.hermes/hermes-agent/venv/bin/hermes"
exec "$HERMES_BIN" send -t telegram "$msg"
