# Shellomancy plugin for Claude Code

A local Claude Code plugin marketplace that ships inside the Shellomancy app
bundle (`Contents/Resources/bundled/claude-plugin`). The app installs it from
disk with `claude plugin marketplace add <this dir>`; nothing is fetched from
the network.

The hooks emit OSC 777 sequences that Shellomancy turns into agent status,
notifications, and permission prompts.

Forked from [warpdotdev/claude-code-warp](https://github.com/warpdotdev/claude-code-warp)
at `b32182f` (plugin version 2.4.0) under the MIT license in `LICENSE`.
Versions continue from there: 3.0.0 is the first Shellomancy release, so any
2.x Warp plugin still installed reads as outdated and gets replaced.

3.1.0 adds a `PreToolUse` hook and sends a small subset of each tool call's
input (for Omnispindle MCP calls: the todo id, description and status), so
the tab can show which todo a session is working and what it's doing right
now. Claude Code loads plugin hooks when a session starts: after updating,
restart running `claude` sessions (or run `/plugin` and reload) to pick it
up. Sessions still on 3.0.0 keep working; their tabs just show less.
