#!/bin/bash
# usage-check.sh — live quota snapshot across the routing lanes (fleet refresh-on-read).
export GLM_API_KEY="$(grep -E '^GLM_API_KEY=' "$HOME/.hermes/.env" | head -1 | cut -d= -f2-)"
AI_USAGE="$HOME/projects/personal/ai-usage-optimizer/ai-usage-rs/target/release/ai-usage"
"$AI_USAGE" collect 2>&1 | tail -1
"$AI_USAGE" status 2>&1 | grep -E "zai|claude-pro|chatgpt|ollama-local"
date
