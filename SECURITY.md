# Security Policy

Shellomancy is a personal fork of Warp maintained by Madness Engineering.

## Reporting a vulnerability

Please don't open a public issue. Report it privately through a [GitHub security advisory on this repository](https://github.com/MadnessEngineering/shellomancy/security/advisories/new).

If the problem is in Warp's own code and also affects Warp, report it to Warp as well, following [Warp's security policy](https://github.com/warpdotdev/warp/security/policy).

## Scope notes

- The local-control API (the `shellomancy` CLI) is reachable by any process running as the same OS user, and Shellomancy enables command execution through it by default. See [docs/shellomancy-cli.md](docs/shellomancy-cli.md#scripting-modes) and `specs/warp-control-cli/SECURITY.md` for the threat model. Same-user processes opening tabs is expected behavior; anything that lets a *different* user, a web page, or the network reach it is a vulnerability.
- Shellomancy is meant to make no connections to Warp's servers ([docs/privacy-and-offline.md](docs/privacy-and-offline.md)). A connection to Warp that you can reproduce is worth reporting.
