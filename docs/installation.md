# Installation

Spielstand has two parts: the **server**, one container on a machine that is always on (home
server, NAS, Raspberry Pi), and the **agent**, one small program on every PC you play on.

## 1. The server

=== "Docker Compose"

    ```yaml
    services:
      spielstand:
        image: tristanteu/spielstand:latest
        container_name: spielstand
        restart: unless-stopped
        ports:
          - "8467:8467"
        volumes:
          - ./data:/data
        environment:
          SPIELSTAND_PUBLIC_URL: http://homeserver:8467
          SPIELSTAND_MIRROR: gdrive:Spielstand
    ```

    ```bash
    docker compose up -d
    ```

=== "Podman Quadlet"

    Save as `~/.config/containers/systemd/spielstand.container`:

    ```ini
    [Container]
    Image=docker.io/tristanteu/spielstand:latest
    ContainerName=spielstand
    PublishPort=8467:8467
    Volume=spielstand-data:/data:Z
    Environment=SPIELSTAND_PUBLIC_URL=http://homeserver:8467
    Environment=SPIELSTAND_MIRROR=gdrive:Spielstand
    AutoUpdate=registry
    HealthCmd=spielstand-server health

    [Service]
    Restart=always

    [Install]
    WantedBy=default.target
    ```

    ```bash
    systemctl --user daemon-reload && systemctl --user start spielstand
    ```

The image is also on `ghcr.io/firsttris/spielstand` and runs on `amd64` and `arm64`. Both files
are in [`deploy/`](https://github.com/firsttris/spielstand/tree/main/deploy).

Set `SPIELSTAND_PUBLIC_URL` to the address your PCs reach the server under; it goes into the install
command. Then open `http://homeserver:8467` and choose the password for the web UI.

## 2. Google Drive as the offsite copy

The image contains [rclone](https://rclone.org). Set up a remote called `gdrive` once:

```bash
docker compose run --rm --entrypoint rclone spielstand config
```

Choose *Google Drive*. Scope *drive.file* is enough for the mirror; to take over Ludusavi
backups that another program put on the Drive, choose the full *drive* scope. The configuration lands in
`/data/rclone/rclone.conf`, inside the volume. With `SPIELSTAND_MIRROR=gdrive:Spielstand` every new
version is copied there within seconds; *Settings* shows when the mirror was last current.

Any other rclone remote works the same way, and so does a plain folder (`SPIELSTAND_MIRROR=/backup/spielstand`).

## 3. Every PC

In the web UI, **Devices → Add device** shows a command with a pairing code that is valid for 15
minutes. Run it in a terminal on the PC, as your normal user:

```bash
curl -fsSL http://homeserver:8467/install.sh | sh -s -- K7Q-2MX
```

It needs no root and works on Bazzite, SteamOS (Steam Deck desktop mode), Fedora, Ubuntu, Arch and
every other distribution with systemd:

1. puts `spielstand-agent` into `~/.local/bin`,
2. pairs the PC with the server and asks for a name ("Steam Deck", "Wohnzimmer-PC"),
3. sets up and starts the systemd user service `spielstand-agent`.

The device shows up in the web UI right away. Its first scan finds every game with saves on that PC
and uploads them.

!!! tip "Steam launch option (optional)"
    Without it, the agent notices a game ending within seconds and syncs then. With it, the newest
    state is loaded right before the game starts as well. In Steam: *Properties → Launch options*:

    ```
    spielstand-agent launch %command%
    ```

## Updating

The server: pull the new image (`docker compose pull && docker compose up -d`; the Quadlet updates
itself with `podman auto-update`).

The agents: after a server update, **Devices** marks every PC whose agent is older with *Update
available*. **Update** there makes the agent download the server's version, check that it runs,
replace itself and restart; the PC stays the same device with its name and settings. It does not
while a game runs on that PC, and the PC has to be online.

An older agent that cannot do that yet shows the command instead, to run once on that PC, without
a pairing code:

```sh
curl -fsSL http://homeserver:8467/install.sh | sh
```

Running the install command with a new pairing code does no harm either: a PC that is still paired
keeps its device and only gets a new token.
