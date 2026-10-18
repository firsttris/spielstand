# Spielstand

**Self-hosted game save sync for Linux PCs and the Steam Deck: every save versioned, nothing ever
overwritten, one web UI for all your machines.**

You play on the living-room PC, then on the Steam Deck, then on the PC in the office. Spielstand keeps
the saves of every game the same on all of them, with a server on your home network as the single
source of truth and Google Drive (or any rclone remote) as the offsite copy.

![Overview of all games with their sync status on every PC](screenshots/overview.png)

## What it does

- **Syncs saves between PCs.** A small agent on each PC backs up after every session and brings the
  other PCs up to date. Optional Steam launch option: load right before the start, back up right after.
- **Never loses a save.** Before anything is written to a disk, the state on that disk is stored as a
  version of its own. Versions are never changed, only added.
- **Sees conflicts instead of guessing.** Played on two PCs without a sync in between? Both states
  are kept and you choose in the web UI. No clock comparisons, no silent overwrites.
- **Finds the saves for you.** The agent uses the [Ludusavi](https://github.com/mtkennerly/ludusavi)
  manifest, built from PCGamingWiki, native and through Proton/Wine. What it does not find, you add
  as a custom game, or let Claude Code find and propose.
- **Takes over your Ludusavi backups.** Old backups, also from Google Drive, become versions with
  their date and comment.
- **Offsite copy.** Every version is mirrored to Google Drive through rclone; a lost server is
  rebuilt from it with one command.

## Where to go next

| | |
|---|---|
| [Installation](installation.md) | the server with Docker or Podman, then one command per PC |
| [How syncing works](how-it-works.md) | versions, the rules that keep saves safe, conflicts, retention |
| [Coming from Ludusavi](ludusavi.md) | taking over existing backups |
| [The agent](agent.md) | commands, the Steam launch option, files on the PC |
| [Custom games and Claude Code](custom-games.md) | games Spielstand does not find on its own, added by hand or by an assistant |
| [Configuration](configuration.md) | every setting of the server and the agent |
| [Backup and recovery](recovery.md) | the Google Drive mirror, rebuilding a server, a forgotten password |
| [Development](development.md) | building, checks, architecture, releases |
