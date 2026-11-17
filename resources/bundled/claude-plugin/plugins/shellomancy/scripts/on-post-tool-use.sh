#!/bin/bash
# Hook script for Claude Code PostToolUse event
# Sends a structured notification after a tool call completes, transitioning
# the session status from Blocked back to Running. Carries a small subset of
# the tool input so Shellomancy's tab can say what happened, and which
# Omnispindle todo the session is working.

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

BODY=$(build_payload "$INPUT" "tool_complete" \
    --arg tool_name "$TOOL_NAME" \
    --argjson tool_input "$(tool_input_subset "$INPUT")")

"$SCRIPT_DIR/warp-notify.sh" "warp://cli-agent" "$BODY"
