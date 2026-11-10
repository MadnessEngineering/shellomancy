#!/bin/bash
# Hook script for Claude Code SessionStart event
# Shows welcome message and Warp detection status

# Check if running in Warp terminal
if [ "$TERM_PROGRAM" = "WarpTerminal" ]; then
    # Running in Warp - notifications will work
    cat << 'EOF'
{
  "systemMessage": "🔔 Shellomancy plugin active. Shellomancy will tell you when tasks complete or input is needed."
}
EOF
else
    exit 0
fi
