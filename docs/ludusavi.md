# Coming from Ludusavi

Spielstand finds saves with the same manifest as [Ludusavi](https://github.com/mtkennerly/ludusavi)
(it uses Ludusavi's library), so every game Ludusavi knows, Spielstand knows. What it adds is the
sync between PCs, with versions, conflict handling and one web UI, instead of a backup folder that
each PC reads and writes on its own.

## Taking over existing backups

In the web UI: **Settings → Take over from Ludusavi**. The source is either

- a folder on the server (mount it into the container), or
- an rclone remote, e.g. `gdrive:ludusavi-backup` when the backups are on Google Drive.

Every backup becomes a version with its original date, comment and lock (locked backups are marked
*keep*). Both simple and differential backups work, as folders and as ZIP files. The source is only
read. Running the import twice adds nothing twice.

On the command line:

```bash
docker compose exec spielstand spielstand-server import-ludusavi gdrive:ludusavi-backup
```

!!! note "Imported versions are never loaded on their own"
    They show up in the timeline marked *taken over from Ludusavi*. The current state of a game
    comes from your PCs. To continue from an old backup, select it and *Load this version*.

## Paths

Ludusavi stores the original paths, like `/home/tristan/.local/share/…`. Spielstand stores paths
relative to the home folder or the Steam library (`<home>/…`, `<root:steam>/…`), so a version made on
one PC loads on another with a different user name or library location. Imported backups are
converted the same way.

## Ludusavi alongside

Nothing stops you from keeping Ludusavi for other things; Spielstand neither reads nor writes its
configuration. It is best not to restore the same games with both.
