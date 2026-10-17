# Backup and recovery

## What the mirror holds

With `SPIELSTAND_MIRROR` set, the server copies every version to the mirror as soon as it arrives:

```
Spielstand/
  elden-ring/
    20261017T092958939Z-0a2f18349905.tar.zst   the save files
    20261017T092958939Z-0a2f18349905.json      PC, time, parent, files, hashes
  _state/heads.json                            current states, labels, kept versions
```

The packages are plain `tar.zst` files; any version can be unpacked by hand
(`tar --zstd -xf …`) without Spielstand.

## Rebuilding a lost server

Start a new, empty server with the same `SPIELSTAND_MIRROR` and the rclone configuration, then:

```bash
docker compose run --rm spielstand restore-from-mirror
docker compose up -d
```

All versions, current states and labels come back. The PCs have to be paired again (the device
tokens are not in the mirror on purpose); their saves are not touched by that.

## Backing up the server itself

Everything is in the data volume (`/data`): `spielstand.db` and the `blobs/` folder. Stopping the
container and copying the folder is a complete backup.

## Forgotten password

```bash
docker compose exec spielstand spielstand-server reset-password 'a new password'
```

## A PC broke

Its versions are on the server. Pair the new PC; its first sync loads the current state of every
game it has installed.
