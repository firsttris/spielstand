# The agent

`spielstand-agent` runs on every PC as a systemd user service. It finds the saves, uploads new
versions, loads new ones from the server and asks when two PCs disagree.

## What it does on its own

- **When a save changes** it backs up: the agent watches every save folder, and once the files have
  been quiet for 10 seconds the new state becomes a version. That holds in both
  [modes](how-it-works.md#modes-sync-or-back-up-only) and also while the game runs.
- **After a game ends** (seen within 5 seconds, native and Proton) it syncs the game.
- **When another PC uploads**, the server tells it over a live connection; if the game is not
  running and the saves on this PC are unchanged since the last sync, it loads the new state.
- **Every 30 minutes** it catches up on anything missed (a PC that was asleep), **every 6 hours** it
  scans for newly installed games.
- **Offline** it keeps working: versions wait in the outbox and go up when the server is back.

It stays in the background: the service runs at low CPU and disk priority, and the scan for games
uses two threads, so it takes a few minutes after a start rather than all cores.

## Where it looks for saves

The agent uses [Ludusavi](ludusavi.md)'s scanner and its manifest of over 10,000 games. Besides
Steam, Heroic, Lutris and the default Wine prefix, it looks in:

- **Game folders** set per PC on the *Devices* page (e.g. `~/Games`): Wine prefixes up to three
  folders deep, and the games installed there.
- **Prefixr**: every prefix and game in Prefixr's library (`~/.config/com.tristan.prefixr/config.json`).
  A game found there counts as installed on this PC, so a new PC loads its saves before the first
  start. See [Games in Wine prefixes](how-it-works.md#games-in-wine-prefixes).
- **A prefix chosen in the web UI** for a game on this PC.
- **Switch emulators**: Eden, Citron, yuzu, Suyu, Sudachi and Ryujinx, see
  [Switch emulators](emulators.md).
- **Custom games**: games the manifest lacks, or more places for one it knows, added in the web UI
  or proposed from a PC, see [Custom games and Claude Code](custom-games.md).

The PC reports to the server where each game's save is, which tool syncs it besides Spielstand and
what is unclear; the game's page shows it.

## Commands

| Command | |
|---|---|
| `spielstand-agent status` | pairing, server, what waits for upload |
| `spielstand-agent scan` | the games with saves on this PC |
| `spielstand-agent games` | every game this PC has something to do with: where its saves are, its mode, what is unclear |
| `spielstand-agent explain GAME` | one game: its save files, how they are stored, why it is synced or only backed up |
| `spielstand-agent custom list` / `add` / `remove` | [custom games](custom-games.md): list, try out (`--dry-run`) and propose, withdraw |
| `spielstand-agent choose GAME PATH` | the game's Wine prefix (or emulator save folder) on this PC, when several fit |
| `spielstand-agent backup-only GAME` | back a game up only, on every PC |
| `spielstand-agent skill install` | the skill for [Claude Code](custom-games.md#with-claude-code) |
| `spielstand-agent sync [game]` | sync one game, or all, now |
| `spielstand-agent launch [--game NAME] %command%` | Steam launch option: load before, back up after |
| `spielstand-agent pair --server URL --code CODE [--name NAME]` | pair (the install script does this); pairing again keeps this PC the same device |
| `spielstand-agent run` | the background service itself |

`status`, `scan`, `games`, `explain` and `custom list` and `custom add` print JSON with `--json`, for
scripts and assistants.

The service: `systemctl --user status spielstand-agent`, logs with
`journalctl --user -u spielstand-agent`.

## The Steam launch option

```
spielstand-agent launch %command%
```

Before the game starts, the agent loads the newest state (after saving the one on the disk). If the
game is running on another PC right now, a desktop notification says so. After the game ends, it backs up. Steam
games are recognised by their `SteamAppId`; for other launchers pass the name as in the Ludusavi
manifest: `spielstand-agent launch --game "Hades II" %command%`.

## Files on the PC

| | |
|---|---|
| `~/.local/bin/spielstand-agent` | the program |
| `~/.config/spielstand/agent.toml` | server, device token, name, settings |
| `~/.local/share/spielstand/` | the local state, the outbox, the trash, the manifest cache, the settings from the server (`settings.json`) |
| `~/.local/share/stellwerk-agent/ignore.d/spielstand.list` | the save folders Stellwerk leaves out, if Stellwerk is installed |
| `~/.config/systemd/user/spielstand-agent.service` | the service |

## Removing it

```bash
systemctl --user disable --now spielstand-agent
rm ~/.local/bin/spielstand-agent ~/.config/systemd/user/spielstand-agent.service
rm -r ~/.config/spielstand ~/.local/share/spielstand
```

Then remove the device in the web UI. Its versions stay on the server.
