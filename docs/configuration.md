# Configuration

## Server

Every setting is an environment variable (or the matching command-line flag, e.g. `--public-url`).

| Variable | Default | |
|---|---|---|
| `SPIELSTAND_DATA` | `/data` | database, versions, trash |
| `SPIELSTAND_LISTEN` | `0.0.0.0:8467` | address and port |
| `SPIELSTAND_PUBLIC_URL` | from the request | the address PCs use; goes into the install command |
| `SPIELSTAND_MIRROR` | – | offsite copy: an rclone remote (`gdrive:Spielstand`) or a folder |
| `SPIELSTAND_RCLONE` | `rclone` | the rclone program |
| `SPIELSTAND_AGENT_DIR` | `/usr/share/spielstand/agent` | agent binaries served at `/download/agent/<arch>` |
| `SPIELSTAND_SECURE_COOKIES` | `false` | set `true` behind HTTPS |
| `RCLONE_CONFIG` | `/data/rclone/rclone.conf` | in the image |
| `RUST_LOG` | `info` | log level |

### Commands

```bash
spielstand-server serve                       # the default
spielstand-server import-ludusavi <source>    # folder or rclone remote
spielstand-server restore-from-mirror         # rebuild an empty server from SPIELSTAND_MIRROR
spielstand-server reset-password <password>   # and end all sessions
spielstand-server health                      # for the container health check
```

### Behind a reverse proxy

Spielstand needs WebSockets on `/api/agent/ws`. Set `SPIELSTAND_PUBLIC_URL` to the outside address
and `SPIELSTAND_SECURE_COOKIES=true` when it is served over HTTPS.

## Agent

`~/.config/spielstand/agent.toml`, written when pairing:

```toml
server = "http://homeserver:8467"
device_id = "…"
token = "…"
name = "Steam Deck"
# Also load games whose saves Steam Cloud syncs (default: back them up only)
restore_steam_cloud_games = false
# How long a conflict question waits before the PC keeps its own state
question_seconds = 60
```

`SPIELSTAND_AGENT_HOME` puts configuration and data below one folder instead (portable installs,
tests).
