# Shellomancy's voice

The rules and vocabulary for every user-visible string. The sweep that removes
Warp's ~1,000 string literals follows this page, so the result reads as one
app, not as `s/Warp/Shellomancy/`.

Status: **first pass, 2026-11-13.** The four names under *Decided* were chosen
by Dan. Everything under *Proposed* is a draft to cut or replace.

## How it sounds

Magic in Shellomancy is a trade. It is practical, a bit worn at the edges, and
done by people who know what they're doing. The universe has opinions about
your commands and occasionally shares one. The tone to aim for is the
matter-of-fact, wry magic of J.S. Morin's books (the *Mad Tinker Chronicles*,
*Black Ocean*): wizards who treat spellcraft like plumbing, and a world that
pushes back.

1. **Names carry the theme.** Features, menus, panels, and empty states use the
   lexicon below.
2. **Facts come first.** An error states what failed, with the exact value
   (exit code, path, setting name). An opinion may follow, at most one short
   clause, and only if the fact is already there. It never replaces it.
3. **Instructions are plain.** Steps, shortcuts, and anything the user
   follows to fix a problem use ordinary words. Destructive actions (close,
   delete, remove, discard) always use their plain verbs.
4. **Never oversold.** No "magical", "powerful", "seamless", "effortless",
   "supercharge", or exclamation-mark excitement. The magic is in the names,
   not in adjectives about them.
5. **Dry, not cute.** The universe is mildly unimpressed, never mean to the
   user and never winking at the camera. If a line would get an eye-roll on
   the second read, cut it.
6. **Technical terms stay exact.** Shell names, `PATH`, environment variables,
   exit codes, SSH, git, file paths, and keyboard keys are never themed.

## Lexicon

### Decided

| Warp says | Shellomancy says | Notes |
|---|---|---|
| Warp | **Shellomancy** | The app. Never abbreviated in the UI. |
| Block | **Spell** | One command and what came back. You *cast* a spell. "Bookmark spell", "Share spell", "Jump to previous spell". Its parts stay plain: *command* and *output*. |
| Warp Drive | **Mana Bar** | The sidebar of saved things. "Open the Mana Bar". |
| Pane | **Chamber** | A nautilus shell is chambered. "Split chamber right", "Close chamber". |
| Command Palette | **Conjure** | ⌘P. "Conjure an action…" as the placeholder. Matches the `conjure` CLI alias. |

### Proposed

| Warp says | Proposed | Why |
|---|---|---|
| Workflow | **Charm** | A small saved spell with blanks to fill in. "New charm", "No charms yet". |
| Notebook | **Scroll** | Longer written-down knowledge with runnable commands in it. |
| Launch configuration / Tab config | **Rite** | Sets up a whole arrangement of tabs and chambers in one go. |
| Command Search / history search | **Scry** | ⌃R. Looking into what was cast before. |
| Warpify (subshell, SSH) | **Enchant** | "Enchant subshell", "Enchant SSH sessions". |
| Agent (Claude Code, Codex… running in a tab) | **Familiar** | Only in Shellomancy's own surfaces ("Familiars" for the MadQTT panel). Agent product names stay as they are. |
| Running a command | **cast** (verb) | Used in names and status lines ("Last cast 3m ago"), never in instructions. |
| Command failed (non-zero exit) | **fizzled** | Always with the code: "Spell fizzled (exit 1)". |

### Stays plain

Tab, window, session, settings, keybindings, input, theme, history,
environment variables, command, output, and every technical term from rule 6.
Tabs and windows are what other tools and muscle memory call them.

### Hidden, not renamed

Features that need Warp's servers are switched off in this fork. Remove their
strings from view rather than inventing names for them: Teams, login and
accounts, Warp AI and Agent Mode, session sharing, Warp Drive sharing and
cloud sync, auto-update ("Update Warp"), and onboarding that sells them.

## Before and after

Real strings from the app.

| Before | After |
|---|---|
| Quit Warp | Quit Shellomancy |
| Warp Drive | Mana Bar |
| Command Palette | Conjure |
| Split pane right | Split chamber right |
| Close pane | Close chamber |
| Copy command | Copy command *(unchanged: plain part of a spell)* |
| Warpify subshell | Enchant subshell |
| Warpify your interactive SSH sessions. | Enchant SSH sessions so spells, scrying, and completions work there too. |
| Welcome to Warp! | Welcome to Shellomancy. |
| Something went wrong. Please try again. | That didn't take. Try again. |
| You may need to restart Warp for MacOS to apply the preferred icon style. | macOS may not honor the new icon until Shellomancy restarts. |
| *(non-zero exit status line)* | Spell fizzled (exit 1) |
| Update Warp | *(hidden: no auto-update)* |

## Scope

- **User-visible text only:** labels, menus, tooltips, toasts, empty states,
  settings descriptions, CLI help and output, bundled skills and plugin
  messages.
- **Not code.** Type, function, and module names (`Block`, `Pane`,
  `WarpDrive`), settings keys, telemetry event names, JSON field names, and
  log lines keep Warp's names. Renaming them would turn every
  [upstream sync](upstream-sync.md) into a conflict storm for no visible gain.
- **Expect conflicts anyway.** Every rewritten string is a line upstream may
  also edit. Sweep by area, one commit per area, so a conflicting area can be
  re-swept from this page instead of hand-merged.

## Open questions

- **CLI nouns.** `shellomancy pane list` vs `shellomancy chamber list`. Adding
  `chamber` as an alias keeps scripts working; replacing `pane` breaks them.
- **Notifications.** Keep the plain word, or "Omens"? Plain is the current
  default because notification settings are something people look for by name.
- **Where the universe speaks.** Only in errors and empty states, or also in
  success toasts? First pass says errors and empty states only.
