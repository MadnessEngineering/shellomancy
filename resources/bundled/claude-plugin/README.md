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
