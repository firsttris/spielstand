---
name: spielstand
description: Look after this PC's game saves with Spielstand — list which games it backs up and syncs, explain where a game's saves are, add games Spielstand does not find on its own (custom games), choose a game's Wine prefix or emulator. Use when the user asks about their games, saves or save games, Spielstand, or a game whose saves are missing.
---

# Spielstand on this PC

Spielstand keeps game saves in step between PCs. `spielstand-agent` runs here as a service; it
finds the saves of over 10,000 games on its own (Ludusavi's manifest, Steam, Heroic, Lutris,
Wine prefixes, Prefixr, the Switch emulators Eden, Citron, yuzu, Suyu, Sudachi and Ryujinx).
Your job is the rest: what it does not find, and what it cannot decide alone.

Its rule is that no save is ever lost: it backs up before it writes and never decides by
clocks. Nothing you do here can delete a save. Still, ask the user before every command that
changes something (`custom add` without `--dry-run`, `custom remove`, `choose`, `backup-only`).

## Look first

```sh
spielstand-agent games --json        # every game this PC has something to do with
spielstand-agent explain "<game>" --json   # one game: its files, how they are stored, why
spielstand-agent custom list --json  # the custom games in effect here
```

In `games --json` each game has:

- `source`: how its saves were found (`steam`, `prefixr`, `gameFolder`, `heroic`, `lutris`,
  `ludusavi`, `custom`, or an emulator such as `Eden`).
- `location`: the folder the saves are in; `prefix`: its Wine prefix.
- `effectiveMode`: `sync` (loads the newest state from other PCs) or `backup` (only backs up);
  `configuredMode` is what the web UI says, `foreignSync` names another sync program that forces
  `backup`, `proposed` means a custom game that waits to be taken over.
- `problem`: what is unclear (`noPrefix`, `severalPrefixes` with `candidates`,
  `severalEmulators` with `candidates`, `startInEmulator`, `failed` with `message`).
- `tracked` (this PC keeps it in step) and `onServer` (the server has a state of it).

`online: false` means the server did not answer; then `onServer` is unknown.

## Add a game Spielstand does not find

1. Ask which games are missing, or compare what the user plays with `games --json`.
2. Find where the game keeps its saves: look on the disk (`ls`, `find`), in the game's folder,
   its Wine prefix (`<prefix>/drive_c/users/steamuser/AppData/…`, `Documents/My Games/…`), and
   what you know of the game. Look at the files: saves are small and change when the user
   plays; logs, caches, shaders and screenshots are not saves.
3. Write the paths with the manifest's placeholders, so they fit every PC:

   | Placeholder | Is |
   |---|---|
   | `<home>` | the home folder |
   | `<xdgData>`, `<xdgConfig>` | `~/.local/share`, `~/.config` |
   | `<winAppData>`, `<winLocalAppData>`, `<winLocalAppDataLow>` | `AppData/Roaming`, `AppData/Local`, `AppData/LocalLow` in every known Wine prefix |
   | `<winDocuments>`, `<winPublic>`, `<winProgramData>`, `<winDir>` | the other Windows folders in a prefix |
   | `<base>` | the game's install folder (needs `--install-dir`) |
   | `<storeUserId>`, `<osUserName>` | a launcher's user id, the user name (inside a path) |

   `*` and `**` are allowed after the first folder. A path must start with a placeholder and
   name a real folder next: `<home>/*` is refused, and so is anything that could reach the
   folders for keys and passwords (`<home>/.ssh`, `<home>/.local/share/key*`, `<home>/.local`).
4. Try it — this changes nothing:

   ```sh
   spielstand-agent custom add --name "Game Name" --path "<xdgData>/GameName/saves" --dry-run --json
   ```

   Read the list of files. Narrow the path until only saves are left. A PC proposes only games
   nobody knows yet: if the manifest or the server already knows the game (it says so), its saves
   are elsewhere on this PC. Then tell the user the path; they add it in the web UI
   (**Settings → Custom games**, kind *Adds to a game of the manifest*).
5. Show the user the files and ask. Then the same command without `--dry-run`.
6. `spielstand-agent explain "Game Name"` shows it after the service picked it up (a few seconds).

A custom game added here is in effect only on this PC, and only backed up, until the user takes
it over in the web UI (**Settings → Custom games**). Tell them that. Then every PC finds it.

Use the game's usual name (as on Steam or its own site). Several paths: repeat `--path`.

## Unclear cases

- `severalPrefixes`: ask which prefix is the game's, then
  `spielstand-agent choose "<game>" <prefix>`.
- `severalEmulators`: ask which emulator the user plays the game in, then
  `spielstand-agent choose "<game>" <one of the candidates>`.
- `startInEmulator`: the user starts the game once in Ryujinx; nothing to do here.
- `foreignSync` with `excluded: false`: Syncthing also syncs these files. Explain; the user can
  exclude them in `.stignore` or keep it backed up only.
- A game the user does not want synced: `spielstand-agent backup-only "<game>"` (switching it back
  to sync is done in the web UI).

## Other commands

```sh
spielstand-agent status --json     # pairing, server, what waits for upload
spielstand-agent sync "<game>"     # sync one game now (loads only what is safe to load)
spielstand-agent custom remove "<game>"   # withdraw a custom game this PC proposed
```

Versions, conflicts and loading an older state are in the web UI; send the user there.
