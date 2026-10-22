#!/bin/sh
# End-to-end test of the one-line install: a fresh server hands out install.sh and the agent,
# the script installs and pairs it, and the server lists the new device. Running it again, without
# a code and with a new one, updates the agent and keeps that one device.
#
#   install/test.sh target/debug
#
# Runs as a normal user without systemd (SPIELSTAND_NO_SYSTEMD=1), in a throw-away HOME.
set -eu

BIN="$(cd "${1:?usage: install/test.sh <dir with spielstand-server and spielstand-agent>}" && pwd)"
PORT="${SPIELSTAND_TEST_PORT:-8479}"
URL="http://127.0.0.1:$PORT"
WORK="$(mktemp -d)"
trap 'kill "$SERVER_PID" 2>/dev/null || true; rm -rf "$WORK"' EXIT

mkdir -p "$WORK/agent" "$WORK/home"
cp "$BIN/spielstand-agent" "$WORK/agent/spielstand-agent-$(uname -m)-linux"

SPIELSTAND_DATA="$WORK/data" SPIELSTAND_LISTEN="127.0.0.1:$PORT" SPIELSTAND_AGENT_DIR="$WORK/agent" \
  RUST_LOG=warn "$BIN/spielstand-server" serve &
SERVER_PID=$!
i=0
until curl -fs "$URL/healthz" >/dev/null; do
  i=$((i + 1))
  [ "$i" -lt 100 ] || { echo "server did not start" >&2; exit 1; }
  sleep 0.1
done

api() {
  curl -fsS -b "$WORK/cookies" -c "$WORK/cookies" -H 'x-spielstand-csrf: 1' -H 'content-type: application/json' "$@"
}
api -d '{"password":"install-test"}' "$URL/api/setup" >/dev/null
CODE="$(api -X POST "$URL/api/pair-codes" | sed -n 's/.*"code":"\([^"]*\)".*/\1/p')"
[ -n "$CODE" ] || { echo "no pairing code" >&2; exit 1; }

install() {
  HOME="$WORK/home" XDG_CONFIG_HOME="$WORK/home/.config" XDG_DATA_HOME="$WORK/home/.local/share" \
    SPIELSTAND_NO_SYSTEMD=1 "$@" sh -c "curl -fsSL '$URL/install.sh' | sh -s -- $CODE"
}
devices() { api "$URL/api/devices" | grep -o '"name":"install-test"' | wc -l; }

install env SPIELSTAND_NAME="install-test"
[ -x "$WORK/home/.local/bin/spielstand-agent" ] || { echo "agent not installed" >&2; exit 1; }
[ "$(devices)" = 1 ] || { echo "device not paired" >&2; exit 1; }

CODE=""
install env
[ "$(devices)" = 1 ] || { echo "update without a code changed the devices" >&2; exit 1; }

# The same server under another address (as behind a proxy) updates without a code, too.
URL_PAIRED="$URL"
URL="http://localhost:$PORT"
install env
URL="$URL_PAIRED"
[ "$(devices)" = 1 ] || { echo "update under another address changed the devices" >&2; exit 1; }

CODE="$(api -X POST "$URL/api/pair-codes" | sed -n 's/.*"code":"\([^"]*\)".*/\1/p')"
install env
[ "$(devices)" = 1 ] || { echo "pairing again added a second device" >&2; exit 1; }
echo "install test passed"
