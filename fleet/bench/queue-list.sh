#!/bin/bash
# queue-list.sh — monitor feed for the bench drain cron: deterministic listing
# of pending Flash-mirror seeds. Empty output = nothing to drain = cron tick
# is free. A new .seed file changes the output and wakes the drain.
ls -1 /Users/wesleymatos/projects/personal/zcode-axi/fleet/bench/queue/*.seed 2>/dev/null
exit 0
