# Development

Requires Rust (stable), Node.js 24 and pnpm 10.

```bash
git clone https://github.com/firsttris/spielstand && cd spielstand
cd web && pnpm install && pnpm build && cd ..
cargo run -p spielstand-server --example demo   # server with demo data on :8467, password spielstand-demo
cd web && pnpm dev                              # UI with hot reload on :5173, proxied to :8467
```

## Architecture

```
crates/
  protocol/   types shared by server, agent and web UI (TypeScript through ts-rs)
  core/       pure logic: the sync decision, packing, portable paths, retention
  server/     axum + SQLite: API, web UI (embedded), mirror, Ludusavi import, retention
  agent/      the PC side: Ludusavi scanner, disk writes, sync, daemon, launch wrapper
web/          React 19, TanStack Router and Query, Tailwind 4, shadcn/ui-style components, Paraglide
install/      install.sh, served by the server with its own address filled in
docker/       the image: server, agent binaries, rclone
```

- **The decision** (`core/src/decide.rs`) is a pure function of the PC's base, the disk and the
  server's current state; everything around it only collects these and carries out the result.
- **The plan** (`core/src/plan.rs`) turns the decision and the game's mode into what to do: adopt a
  known state, report an overwrite, upload, load. In *back up only* it never writes; a property
  test holds it to that.
- **Wine prefixes** (`core/src/portable.rs`, `agent/src/prefixes.rs`): saves in a prefix are stored
  as `<prefix>/drive_c/users/<user>/…` and resolved into this PC's prefix when written.
- **The agent** (`agent/src/agent.rs`) holds what a PC knows: settings from the server, where saves
  are, which games are installed here (`games_to_follow`). `sync.rs` has the flows, `watch.rs` the
  save-folder watcher, `report.rs` what goes to the web UI, `foreign.rs` Syncthing and Stellwerk.
  The watcher, the server's pushes and the catch-up run as separate tasks; each flow holds its
  game's lock (`lock_game`), so none decides on what another one has only half done.
- **Switch emulators** (`agent/src/emulators/`): `yuzu.rs` and `ryujinx.rs` read each family's
  layout (profiles, save folders, names), `mod.rs` finds the installs and picks a title's save
  folder, `scanner.rs` adds them to Ludusavi's scanner. Saves are stored as `<switch>/…`.
- **Uploads** (`server/src/versions.rs`) move the current state with
  `UPDATE games SET head = ? WHERE id = ? AND head IS ?`; anything else becomes a branch.
- **Writing saves** (`agent/src/disk.rs`) snapshots first, stages, verifies, renames and rolls back.
- **Custom games** (`core/src/custom.rs` checks the paths, `server/src/custom_games.rs` stores them):
  a device's proposal goes only into that device's `AgentSettings` and is backed up only there until
  the web UI takes it over; a device proposes only names nobody knows yet (`custom_games::is_new`),
  so its versions reach no other device. `disk.rs` never reads or writes a file in the folders for
  keys and passwords (`custom::is_protected_file`), whatever path or version names it. The agent adds them to Ludusavi's manifest (`Scanner::set_custom_games`).
- **The command line** for people and assistants: `agent/src/inspect.rs` (what `games` and
  `explain` show), `custom.rs` (try out, propose), `choices.rs` (prefix, back up only), `text.rs` (the
  German answers); `main.rs` and `commands.rs` are the binary. `agent/skill/SKILL.md` is the skill
  for Claude Code that `skill install` writes.
- **Ludusavi** is used as a library, pinned to one version (`ludusavi = "=0.31.0"`), behind the
  `Scanner` trait in `agent/src/engine.rs` with tests against the real crate.

The types in `web/src/bindings` are generated:

```bash
cargo test -p spielstand-protocol --features ts
```

## Tests

| | |
|---|---|
| `cargo test -p spielstand-core` | decision table, packing, paths, retention (with property tests) |
| `cargo test -p spielstand-server` | API against a real server: CAS, parallel uploads, auth, mirror, import |
| `cargo test -p spielstand-agent` | sync scenarios with two PCs, the real Ludusavi scanner, the command line as a binary |
| `cargo test -p spielstand-agent --test simulation` | random histories of PCs, outages and restores; `SPIELSTAND_SIM_CASES` sets how many |
| `cd web && pnpm test` | unit tests (Vitest) |
| `cd web && pnpm e2e` | Playwright against the demo server |
| `cd web && pnpm e2e:stack` | Playwright against the whole chain: a server and two real agent processes with their own homes and Wine prefixes (`cargo build -p spielstand-agent` first) |
| `install/test.sh target/debug` | the one-line install against a fresh server (as a normal user) |

The simulation checks the central promise: whatever happens, every state a game's saves were ever
in on any PC is still a version. CI runs 64 histories, a nightly job 3000.

## Checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && pnpm lint && pnpm typecheck && pnpm test && pnpm build && pnpm e2e && pnpm e2e:stack
mkdocs build --strict
```

## The whole chain by hand

```bash
cargo build -p spielstand-agent
cargo run -p spielstand-server --example stack
```

starts a server on `:8469` (password `spielstand-stack`) and two PCs, *Desktop* and *Steam Deck*,
each a real agent process with its own home folder, Prefixr library and Hades prefix. `GET
http://127.0.0.1:8470/pcs` shows each PC's save file; `POST /pcs/{name}/stop` and `/start` switch a
PC off and on. Edit a save file and watch it arrive on the other PC.

## Screenshots

`pnpm screenshots` (in `web/`) takes the pictures in `docs/screenshots` from the demo server. On
GitHub: **Actions → Update screenshots → Run workflow**.

## Releases

**Actions → Bump version** raises the version in `Cargo.toml` and `web/package.json`, tags it and
starts the release: the agent for `x86_64` and `aarch64`, the image for Docker Hub
(`tristanteu/spielstand`) and GHCR, and the GitHub release with the agent binaries.
