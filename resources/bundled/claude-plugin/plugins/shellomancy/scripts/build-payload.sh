#!/bin/bash
# Builds a structured JSON notification payload for warp://cli-agent.
#
# Usage: source this file, then call build_payload with event-specific fields.
#
# Example:
#   source "$(dirname "${BASH_SOURCE[0]}")/build-payload.sh"
#   BODY=$(build_payload "$INPUT" "stop" \
#       --arg query "$QUERY" \
#       --arg response "$RESPONSE" \
#       --arg transcript_path "$TRANSCRIPT_PATH")
#
# The function extracts common fields (session_id, cwd, project) from the
# hook's stdin JSON (passed as $1), then merges any extra jq args you pass.

# The current protocol version this plugin knows how to produce.
PLUGIN_CURRENT_PROTOCOL_VERSION=1

# Negotiate the protocol version with Warp.
# Uses min(plugin_current, warp_declared), falling back to 1 if Warp doesn't advertise a version.
negotiate_protocol_version() {
    local warp_version="${WARP_CLI_AGENT_PROTOCOL_VERSION:-1}"
    if [ "$warp_version" -lt "$PLUGIN_CURRENT_PROTOCOL_VERSION" ] 2>/dev/null; then
        echo "$warp_version"
    else
        echo "$PLUGIN_CURRENT_PROTOCOL_VERSION"
    fi
}

build_payload() {
    local input="$1"
    local event="$2"
    shift 2

    local protocol_version
    protocol_version=$(negotiate_protocol_version)

    # Extract common fields from the hook input
    local session_id cwd project
    session_id=$(echo "$input" | jq -r '.session_id // empty' 2>/dev/null)
    cwd=$(echo "$input" | jq -r '.cwd // empty' 2>/dev/null)
    project=""
    if [ -n "$cwd" ]; then
        project=$(basename "$cwd")
    fi

    # Build the payload: common fields + any extra args passed by the caller.
    # Extra args should be jq flag pairs like: --arg key "value" or --argjson key '{"a":1}'
    jq -nc \
        --argjson v "$protocol_version" \
        --arg agent "claude" \
        --arg event "$event" \
        --arg session_id "$session_id" \
        --arg cwd "$cwd" \
        --arg project "$project" \
        "$@" \
        '{v:$v, agent:$agent, event:$event, session_id:$session_id, cwd:$cwd, project:$project} + $ARGS.named'
}

# A small, display-sized subset of a tool call's input (and, for Omnispindle
# MCP calls, the todo it touched), as compact JSON for --argjson tool_input.
# Omnispindle calls (mcp__*omnispindle*__*) give todo_id / description /
# status / todo_project, plus result_id: the first todo id in the tool's
# response (add_todo's new id). Other tools give command / file_path /
# pattern / description, cut short. Never fails: "{}" on bad input.
tool_input_subset() {
    local input="$1"
    local subset
    subset=$(echo "$input" | jq -c '
        def cut($n): if type == "string" and length > $n then .[0:$n] + "…" else . end;
        (.tool_name // "") as $tool
        | (if (.tool_input | type) == "object" then .tool_input else {} end) as $in
        | (if ($in.updates | type) == "object" then $in.updates else {} end) as $up
        | if ($tool | test("^mcp__.*omnispindle.*__")) then
            {
              todo_id: ($in.todo_id // $in.id // null),
              description: (($in.description // $up.description // null) | cut(120)),
              status: ($in.status // $up.status // null),
              todo_project: ($in.project // null),
              result_id: ((.tool_response // null) | tostring
                  | (capture("(?<id>[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})") // {})
                  | .id // null)
            }
          else
            {
              command: ($in.command // null | cut(120)),
              file_path: ($in.file_path // $in.notebook_path // null),
              pattern: ($in.pattern // null | cut(60)),
              description: ($in.description // null | cut(80))
            }
          end
        | with_entries(select(.value != null))' 2>/dev/null)
    if [ -z "$subset" ]; then
        subset='{}'
    fi
    echo "$subset"
}
