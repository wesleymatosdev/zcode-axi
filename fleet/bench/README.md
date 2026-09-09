# bench — same-task frontier vs Flash mirror

Every task dispatched to a FRONTIER model (Astra/Opus) gets mirrored on Flash
during the free window (12:00–22:00), so capability comparisons accumulate
from real work, not synthetic benchmarks.

## Flow

1. Coordinator dispatches a frontier task → drops a copy of the seed brief
   here: `queue/<task-id>.seed`, with a `frontier-result:` line appended when
   the frontier run closes (path to its final artifact).
2. At 12:00 daily, the `fleet bench drain` cron (monitor: `queue-list.sh`,
   fires only when the queue is non-empty) executes each queued seed on the
   free Flash lane — collapsed pipeline (prep+execute in one worker).
3. Flash output is saved to `results/<task-id>.flash.md`; the comparison
   line goes into `results/LEDGER.md` (task, frontier model, flash verdict,
   artifact paths).

## Rules

- Queue items are SEED briefs — the exact prompt the frontier run got, never
  a rewritten one (same-task comparability).
- The mirror runs inside the free window only; the drain cron never spawns
  Flash outside 12:00–22:00.
- Raw outputs are never edited — comparisons cite paths and quote lines.
- Ledger format: one row per task, append-only, newest last.
