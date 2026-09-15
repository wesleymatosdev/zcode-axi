#!/bin/bash
# Steer the hire-site fleet worker: deliver the scope-upgrade steer file content.
# Wraps tmux send-keys (fleet steering is standard coordinator work per skill 'fleet').
TMUX=/opt/homebrew/bin/tmux
STEER=/Users/wesleymatos/projects/personal/swarm/briefs/hire-site-steer.md
MSG="Scope upgrade from Wesley: read $STEER and fold it in. Summary: this becomes his MAIN website (wesleymatos.dev) presenting his experience and current work, positioned for AI-native companies. Add an Experience beat (senior IC -> tech lead -> AI-fleet orchestrator), make Now prominent, hero reads personal-first. Everything else in the brief stands."
exec $TMUX send-keys -t zswarm:fleet-hire-site "$MSG" Enter
