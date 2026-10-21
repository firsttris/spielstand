# Switch emulators

Spielstand keeps the saves of Nintendo Switch emulators in step too: the yuzu family (**Eden**,
**Citron**, **yuzu**, **Suyu**, **Sudachi**) and **Ryujinx** (also its continuation Ryubing). Nothing
has to be set up; the agent finds the emulators on its own.

## One game per title

Every Switch title is a game of its own, named as the emulator knows it (*The Legend of Zelda:
Tears of the Kingdom*). The yuzu family knows the name only of games installed to the NAND, not of
games in a games folder; for those the server looks the title id up in
[titledb](https://github.com/blawar/titledb) (see `SPIELSTAND_SWITCH_TITLES` in the
[configuration](configuration.md)). Internally it is keyed by its title id (`Switch 0100F2C0115B6000`), not by
the emulator: the save files are the same in all of them, so a state from Eden on the desktop
loads into Ryujinx on the Steam Deck and back. Versions, conflicts, modes and alerts work as for
any other game.

The emulator's own files in a save folder (`.yuzu_save_size`, `.citron_save_size`, `ExtraData0`/`1`)
are not part of the save and are left alone.

## Where the saves are

| Emulator | Folder | Flatpak |
|---|---|---|
| Eden | `~/.local/share/eden` | `dev.eden_emu.eden` |
| yuzu | `~/.local/share/yuzu` | `org.yuzu_emu.yuzu` |
| Citron, Suyu, Sudachi | `~/.local/share/citron`, `…/suyu`, `…/sudachi` | – |
| Ryujinx, Ryubing | `~/.config/Ryujinx` | `org.ryujinx.Ryujinx` |

`$XDG_DATA_HOME` and `$XDG_CONFIG_HOME` are honoured; a NAND folder moved in the emulator's settings
(`nand_directory`, Eden's `save_directory`) is followed.

## Profiles

Every installation of an emulator has its own profile id, chosen at random on its first start. A
save is stored without it and loaded into the **current profile** of the emulator on each PC: in
the yuzu family the one selected in its settings, in Ryujinx the last one used.

## Ryujinx: start the game once

The yuzu family uses a save folder that is simply there, so a PC that has never started a game
gets its save before the first start. Ryujinx only uses save folders it created itself. Until a
game has been started there once, the game's page says so for that PC: *Start the game once in
Ryujinx*. After that, the save is loaded like any other.

## Several emulators on one PC

If more than one emulator on a PC has a save of a game, Spielstand does not guess which one is
yours: the game's page asks for that PC, and until then nothing of it is backed up or loaded
there. A save folder Eden links to Ryujinx ("Link to Ryujinx") counts once.

## While an emulator runs

Which title an emulator is playing is not visible from outside. While one runs, every Switch game
on that PC counts as open: changes are backed up, nothing is written. When the emulator is closed,
newer states from other PCs are loaded right away.

## Not supported yet

- Portable installs (a `user` folder next to the emulator, Ryujinx's `portable` folder).
- Citron's custom and mirrored save paths.
- Saves that belong to the device rather than a profile (few games use them).
