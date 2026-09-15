# RULES.md — fleet operating rules (durable summary; fleet skill is the full source)

This file summarizes the hard rules of the Madeline/fleet workflow so any agent (or human) can follow them. Full detail + provenance lives in `~/.hermes/skills/software-development/fleet/SKILL.md`.

## Dispatch
1. Fleet-shaped work → write a SEED brief first (`swarm/briefs/<task>-seed.md`), then spawn workers. No inline absorption.
2. Fresh worker per phase: scout → plan → brief → execute → review. Verify artifacts between phases; close with `done:` + evidence.
3. Gate calibrated: inline only if expected ≤5 tool calls AND ≤2min; abort at 8 calls/2min → dispatch. Bias toward dispatch.
4. Written briefs must be dispatched — a brief sitting undelivered is a miss (9/9 receipt).
5. One dispatch per agent turn; report to Wesley in ONE line (verdict + evidence path).

## Review
1. Worker claims are internal triggers. Independent review worker (different model family) before anything surfaces.
2. Wesley reviews visual/client work on Lavish (not unfit); annotations flow to the coordinator.
3. Evidence before done — verify claims against artifacts, never trust worker summaries alone.

## Security
1. Pre-create/push scan: PII, secrets, proprietary content. Default private; public = explicit Wesley review.
2. Client copy/content belongs to the client — never republish, never send client-facing docs without Wesley's go.
3. Secrets via 1Password (`op` CLI), never echoed to panes/logs/status files.

## Publishing
1. NEVER post directly to LinkedIn/social — Buffer DRAFTS only; Wesley controls the queue (9/9 rule after an unauthorized post was deleted).
2. Source packets for Wesley's writing; no ghostwriting, no draft copy in his voice.

## Tooling
1. PoCs/experiments default to Go (protobuf-native, quick); Rust variation follows once proven — both best-in-class, Rust = the no-GC variation. Avoid Python for new tooling (pipeline reuse is a justified exception).
2. Local-first: sensitive data stays local; cloud upload requires explicit opt-in.
3. Push over poll: fleet-bus (busd + protobuf + NATS/JetStream) is the notification layer; status files are the durable record; polling is loud fallback only.
4. Ports/adapters: consumers (hermes webhook, other IDEs) implement the Notifier port; nothing hermes-specific in the core.

## Clients (Madeline et al.)
1. Client copy is client copy — never rewrite, never republish client-facing docs without Wesley's go.
2. No deploys/publishes without Wesley's explicit chat approval (staging + lavish always first).
3. Decisions with real options → decision sheet / needs-decision, never invented.
