# Contributing to Shellomancy

Shellomancy is a personal workshop fork of [Warp](https://github.com/warpdotdev/warp), maintained by Madness Engineering. Issues and pull requests are welcome on [this repository](https://github.com/MadnessEngineering/shellomancy), but there is no roadmap commitment, review SLA, or support channel.

- **Bugs in Warp itself** (things that also happen in stock Warp) belong upstream: see [Warp's contributing guide](https://github.com/warpdotdev/warp/blob/master/CONTRIBUTING.md).
- **Building and testing:** [docs/building.md](docs/building.md). The engineering guide for the codebase is [AGENTS.md](AGENTS.md) (upstream's, still accurate for the code).
- **Conventions for Shellomancy changes:**
  - Mark code changes with a `Shellomancy:` comment ([docs/upstream-sync.md](docs/upstream-sync.md)).
  - Prefer flags, channel config, or a guarded early return over deleting upstream code.
  - Update [docs/differences-from-warp.md](docs/differences-from-warp.md) and [docs/known-issues.md](docs/known-issues.md) in the same change.
  - Document only what the code does, and say when something hasn't been verified.
- **Security issues:** see [SECURITY.md](SECURITY.md).

By contributing you agree your changes are licensed under the same terms as the code you change: MIT for `crates/warpui_core` and `crates/warpui`, AGPL-3.0 for everything else.
