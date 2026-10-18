# Custom games and Claude Code

The agent finds the saves of over 10,000 games on its own. For the rest, such as a small indie game,
a game in an unusual folder or a save the manifest does not know yet, you add a **custom game**: a
name and the paths of its saves. You can do it yourself in the web UI, or have an assistant such as
[Claude Code](https://claude.com/claude-code) look on the PC and propose it.

## A custom game

A custom game is an entry like the ones in [Ludusavi's manifest](ludusavi.md): a name and one or
more paths, written with the manifest's placeholders so that they fit every PC:

| Placeholder | Is |
|---|---|
| `<home>` | the home folder |
| `<xdgData>`, `<xdgConfig>` | `~/.local/share`, `~/.config` |
| `<winAppData>`, `<winLocalAppData>`, `<winLocalAppDataLow>` | `AppData/Roaming`, `AppData/Local`, `AppData/LocalLow` in every known Wine prefix |
| `<winDocuments>`, `<winPublic>`, `<winProgramData>`, `<winDir>` | the other Windows folders in a prefix |
| `<base>` | the game's install folder |

`*` and `**` match any name after the first folder: `<xdgData>/Indie/saves/*.sav`.

A custom game is either a **new game** or **adds places to a game of the manifest**, for example
when a game keeps its saves somewhere else on your PC. Its saves are versioned, synced and shown
like every other game's; the web UI says *Custom game* where they were found.

To keep a path from catching more than saves, it has to start with a placeholder followed by a
named folder (`<home>/*` is refused) and may not contain `..`. Nor may it reach the folders that
hold keys or passwords (`~/.ssh`, `~/.gnupg`, the keyrings, Spielstand's own folders), by name, by
a pattern that could match them (`<home>/.local/share/key*`, `<home>/.config/<game>`) or as a
folder above them (`<home>/.local`). Whatever path or version names such a file, the agent never
backs it up and never writes it.

## In the web UI

**Settings → Custom games** lists them. Adding or changing one there makes it apply on every PC:
the agents find its saves within seconds.

## From a PC: proposals

On a PC, the agent's command line tries a custom game out and proposes it:

```bash
spielstand-agent custom add --name "Indie Game" --path "<xdgData>/IndieGame/saves" --dry-run
```

`--dry-run` changes nothing: it lists the files the paths match and how each would be stored. The
same command without `--dry-run` proposes the game. It is refused when the paths match no file
(`--allow-empty` allows it), more than 10,000 files or 1 GB, or a file another game already backs
up.

A PC proposes only **games nobody knows yet**: the name may not be in the manifest or have
versions from another PC. More places for a known game are added in the web UI (*Adds to a game
of the manifest*); otherwise the proposing PC's versions of that game, which every PC loads, would
carry files the others never agreed to.

A proposal applies **only on the PC that made it, and only as a backup**, until it is taken over
for every PC in the web UI (**Settings → Custom games → Take over for every PC**). The game's page
says so for that PC. The other PCs do not know the game until then, so nothing of it reaches
them, and a PC, or whoever holds its token, can never make the other PCs back up files it chose.

`spielstand-agent custom list` shows the custom games in effect on the PC, `custom remove` withdraws
a proposal; one that applies everywhere is changed or removed in the web UI.

## With Claude Code

The agent comes with a [skill](https://code.claude.com/docs/en/skills) for Claude Code
that knows these commands and how to use them:

```bash
spielstand-agent skill install    # writes ~/.claude/skills/spielstand/SKILL.md
```

Then start Claude Code on the PC and ask, for example: *"I play Eden and a few indie games. Check
which of my saves Spielstand does not back up yet."* Claude Code lists what Spielstand found, looks
for what is missing, tries paths with `--dry-run` until only saves are left, and asks before it
proposes a game. Run `skill install` again after an update of the agent.

The commands it uses are there for any assistant or script; each has `--json`:

| Command | |
|---|---|
| `spielstand-agent games --json` | every game the PC has something to do with: where the saves are, how they were found, the mode, what is unclear |
| `spielstand-agent explain GAME --json` | one game: every save file, how it is stored, why it is synced or only backed up, whether the PC is up to date |
| `spielstand-agent custom add … --dry-run --json` | what a custom game would back up |
| `spielstand-agent choose GAME PATH` | the game's Wine prefix on this PC, when several fit (or, for a Switch game, which emulator's save it is) |
| `spielstand-agent backup-only GAME` | back a game up only, on every PC |

A PC may decide only what concerns itself or is safe: proposals, its own prefix choice, and
switching a game to *back up only*. Switching a game to *synchronize* again, taking a proposal over,
loading versions and resolving conflicts stay in the web UI.
