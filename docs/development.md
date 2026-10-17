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
- **Uploads** (`server/src/versions.rs`) move the current state with
  `UPDATE games SET head = ? WHERE id = ? AND head IS ?`; anything else becomes a branch.
- **Writing saves** (`agent/src/disk.rs`) snapshots first, stages, verifies, renames and rolls back.
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
| `cargo test -p spielstand-agent` | sync scenarios with two PCs, the real Ludusavi scanner |
| `cargo test -p spielstand-agent --test simulation` | random histories of PCs, outages and restores; `SPIELSTAND_SIM_CASES` sets how many |
| `cd web && pnpm test` | unit tests (Vitest) |
| `cd web && pnpm e2e` | Playwright against the demo server |
| `install/test.sh target/debug` | the one-line install against a fresh server (as a normal user) |

The simulation checks the central promise: whatever happens, every state a game's saves were ever
in on any PC is still a version. CI runs 64 histories, a nightly job 3000.

## Checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && pnpm lint && pnpm typecheck && pnpm test && pnpm build && pnpm e2e
mkdocs build --strict
```

## Screenshots

`pnpm screenshots` (in `web/`) takes the pictures in `docs/screenshots` from the demo server. On
GitHub: **Actions → Update screenshots → Run workflow**.

## Releases

**Actions → Bump version** raises the version in `Cargo.toml` and `web/package.json`, tags it and
starts the release: the agent for `x86_64` and `aarch64`, the image for Docker Hub
(`tristanteu/spielstand`) and GHCR, and the GitHub release with the agent binaries.
