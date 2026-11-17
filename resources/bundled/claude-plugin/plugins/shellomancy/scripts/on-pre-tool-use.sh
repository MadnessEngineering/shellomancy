#!/bin/bash
# Hook script for Claude Code PreToolUse event
# Tells Shellomancy which tool is about to run (and a short preview of its
# input) so the tab can show live activity: "running cargo test",
# "editing mana_bar_view.rs". Prints nothing to stdout, so it never affects
# Claude Code's permission decision.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/should-use-structured.sh"

# No legacy equivalent for this hook
if ! should_use_structured; then
    exit 0
fi

source "$SCRIPT_DIR/build-payload.sh"

# Read hook input from stdin
INPUT=$(cat)

TOOL_NAME=$(echo "$INPUT" | jq -r '.tool_name // empty' 2>/dev/null)

BODY=$(build_payload "$INPUT" "tool_start" \
    --arg tool_name "$TOOL_NAME" \
    --argjson tool_input "$(tool_input_subset "$INPUT")")

"$SCRIPT_DIR/warp-notify.sh" "warp://cli-agent" "$BODY"
