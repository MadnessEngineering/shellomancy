# Shellomancy FAQ

**What is Shellomancy?**
A fork of the open-source Warp terminal for the madness_interactive workshop: Warp's terminal, offline, without the Oz agent platform, with a scriptable `shellomancy` CLI. See the [README](README.md).

**Is it affiliated with Warp?**
No. It is built from Warp's AGPL-3.0 / MIT source; Warp's copyright notices are kept. Warp's own FAQ is in [the upstream repository](https://github.com/warpdotdev/warp/blob/master/FAQ.md).

**Can I log in, use Warp AI, Warp Drive or session sharing?**
No. Shellomancy is offline by design and never contacts Warp's servers. To change that, see [docs/privacy-and-offline.md](docs/privacy-and-offline.md).

**Do Claude Code and other CLI agents still work in it?**
Yes, that's the intent: Warp's support for CLI agents running in the terminal is kept. (Not yet confirmed in a running build; see [docs/known-issues.md](docs/known-issues.md).)

**Will it touch my Warp settings?**
It's set up not to: it uses its own bundle id, `~/.mad/shellomancy` instead of `~/.warp`, its own Keychain items and URL scheme. To reuse themes or tab configs, copy them from `~/.warp` rather than sharing the directory.

**Is there a download?**
Not yet. Build from source ([docs/building.md](docs/building.md)). Whether a built binary may be redistributed is an open licensing question ([docs/known-issues.md](docs/known-issues.md#distribution)).

**How do I script it?**
[docs/shellomancy-cli.md](docs/shellomancy-cli.md).
