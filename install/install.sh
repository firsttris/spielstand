#!/bin/sh
# Installs the Spielstand agent for the current user and pairs it with the server.
#
#   curl -fsSL __SPIELSTAND_SERVER__/install.sh | sh -s -- <pairing-code>
#
# No root needed: the binary goes to ~/.local/bin, the service is a systemd user unit.
# Environment: SPIELSTAND_NAME (device name), SPIELSTAND_NO_SYSTEMD=1 (skip the service).
set -eu

SERVER="__SPIELSTAND_SERVER__"
VERSION="__SPIELSTAND_VERSION__"
CODE="${1:-}"

say() { printf '%s\n' "$*"; }
fail() { printf 'spielstand: %s\n' "$*" >&2; exit 1; }

[ -n "$CODE" ] || fail "Kopplungscode fehlt: curl -fsSL $SERVER/install.sh | sh -s -- <code>"
[ "$(id -u)" != 0 ] || fail "bitte als normaler Benutzer ausführen, nicht als root"
command -v curl >/dev/null 2>&1 || fail "curl wird gebraucht"

case "$(uname -m)" in
  x86_64 | amd64) ARCH=x86_64 ;;
  aarch64 | arm64) ARCH=aarch64 ;;
  *) fail "Architektur $(uname -m) wird nicht unterstützt" ;;
esac

BIN_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
BIN="$BIN_DIR/spielstand-agent"
mkdir -p "$BIN_DIR"

say "Lade Spielstand-Agent $VERSION ($ARCH) …"
TMP="$(mktemp "${TMPDIR:-/tmp}/spielstand-agent.XXXXXX")"
trap 'rm -f "$TMP"' EXIT
curl -fsSL "$SERVER/download/agent/$ARCH" -o "$TMP"
chmod 0755 "$TMP"
mv "$TMP" "$BIN"
trap - EXIT

NAME="${SPIELSTAND_NAME:-}"
if [ -z "$NAME" ]; then
  NAME="$(hostname 2>/dev/null || cat /etc/hostname 2>/dev/null || echo pc)"
  if [ -r /dev/tty ] && [ -t 1 ]; then
    printf 'Name für dieses Gerät [%s]: ' "$NAME"
    read -r ANSWER </dev/tty || ANSWER=""
    [ -n "$ANSWER" ] && NAME="$ANSWER"
  fi
fi

"$BIN" pair --server "$SERVER" --code "$CODE" --name "$NAME"

if [ "${SPIELSTAND_NO_SYSTEMD:-0}" != 1 ] && command -v systemctl >/dev/null 2>&1; then
  UNIT_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"
  mkdir -p "$UNIT_DIR"
  cat >"$UNIT_DIR/spielstand-agent.service" <<UNIT
[Unit]
Description=Spielstand agent: keeps game saves in sync with $SERVER
After=network-online.target

[Service]
ExecStart=$BIN run
Restart=on-failure
RestartSec=10
# A background service: the PC in use comes first.
Nice=10
IOSchedulingClass=idle

[Install]
WantedBy=default.target
UNIT
  systemctl --user daemon-reload
  systemctl --user enable --now spielstand-agent.service
  say "Dienst läuft: systemctl --user status spielstand-agent"
else
  say "Ohne systemd: starte den Agent mit '$BIN run'"
fi

say ""
say "Fertig. \"$NAME\" ist mit $SERVER gekoppelt."
say "Optional für Steam-Spiele (lädt vor dem Start, sichert danach):"
say "  Startoption: $BIN launch %command%"
