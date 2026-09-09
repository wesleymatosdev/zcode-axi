#!/bin/bash
# queue-list.sh — monitor feed for the bench drain cron: deterministic listing
# of pending Flash-mirror seeds. Empty output = nothing to drain = cron tick
# is free. A new .seed file changes the output and wakes the drain.
FLEET_BENCH="${FLEET_BENCH:-$HOME/projects/personal/zcode-axi/fleet/bench}"
ls -1 "$FLEET_BENCH"/queue/*.seed 2>/dev/null
exit 0
