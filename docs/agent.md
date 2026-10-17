# The agent

`spielstand-agent` runs on every PC as a systemd user service. It finds the saves, uploads new
versions, loads new ones from the server and asks when two PCs disagree.

## What it does on its own

- **After a game ends** (seen within 5 seconds, native and Proton) it backs up and uploads.
- **When another PC uploads**, the server tells it over a live connection; if the game is not
  running and the saves on this PC are unchanged since the last sync, it loads the new state.
- **Every 30 minutes** it catches up on anything missed (a PC that was asleep), **every 6 hours** it
  scans for newly installed games.
- **Offline** it keeps working: versions wait in the outbox and go up when the server is back.

## Commands

| Command | |
|---|---|
| `spielstand-agent status` | pairing, server, what waits for upload |
| `spielstand-agent scan` | the games with saves on this PC |
| `spielstand-agent sync [game]` | sync one game, or all, now |
| `spielstand-agent launch [--game NAME] %command%` | Steam launch option: load before, back up after |
| `spielstand-agent pair --server URL --code CODE [--name NAME]` | pair again (the install script does this) |
| `spielstand-agent run` | the background service itself |

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
| `~/.local/share/spielstand/` | the local state, the outbox, the trash, the manifest cache |
| `~/.config/systemd/user/spielstand-agent.service` | the service |

## Removing it

```bash
systemctl --user disable --now spielstand-agent
rm ~/.local/bin/spielstand-agent ~/.config/systemd/user/spielstand-agent.service
rm -r ~/.config/spielstand ~/.local/share/spielstand
```

Then remove the device in the web UI. Its versions stay on the server.
