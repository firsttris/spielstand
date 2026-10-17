# Notes for Claude

## Documentation

The documentation is in `docs/` (<https://firsttris.github.io/spielstand/>). Read the pages that
touch the change before starting: `how-it-works.md` for the sync rules, `development.md` for how the
pieces fit. Links in the README and the docs point to the GitHub Pages site, not to `docs/*.md`.

## Reference implementations

Base details on the sources, not on memory: [Ludusavi](https://github.com/mtkennerly/ludusavi) at the
pinned version (`=0.31.0`; `src/scan.rs`, `src/scan/layout.rs`, `src/resource/manifest.rs`,
`src/prelude.rs`) for scanning and the backup layout. Clone it shallowly
(`git clone --depth 1 --branch v0.31.0`) into the scratchpad and name the source file in comments and
commit messages where it settles a detail. Ludusavi is used only through `agent/src/engine.rs` and
`server/src/importer.rs`; a version bump starts there and in their tests.

## With every change

Update what the change affects, in the same pull request, and only that:

- the tests for what changed (Rust unit and API tests, the agent's sync scenarios, Vitest,
  Playwright in `web/e2e`);
- the simulation's oracle in `crates/agent/tests/simulation.rs` when the sync rules change;
- the TypeScript bindings when a type in `crates/protocol` changes
  (`cargo test -p spielstand-protocol --features ts`);
- both message files `web/messages/de.json` and `en.json`;
- the docs in `docs/` that describe it;
- the README when a feature a user sees is added, changed or removed;
- the screenshots after a change to the look: **Actions → Update screenshots → Run workflow**, then
  look at the new pictures.

## Checks

Green before every push, then check CI:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cd web && pnpm lint && pnpm typecheck && pnpm test && pnpm build && pnpm e2e
mkdocs build --strict    # when docs/ or mkdocs.yml changed
```

`install/test.sh target/debug` tests the one-line install; it must run as a normal user.

## Pull requests

- One feature per pull request; it carries only its own commits.
- Before every push check whether the branch's pull request is still open. If it is merged, start
  again from the current `origin/main` and open a **new** pull request.
- Merge method: squash. Force-push: allowed with `--force-with-lease` on the own branch after a merge.

## Working rules

- Commands as root (`sudo`, `pkexec`) are run by the user; don't work around that.
- The user's running processes are not stopped; use other ports for tests (the e2e demo uses 8468).
- Language: code, comments, docs, commits and pull requests in English; the web UI in German and
  English (Paraglide, German is the base locale); the agent's and install script's messages in German.

## Code style

Technology choices follow `tech-stack.md` in firsttris/workflows. Deviations: no Bun, pnpm only for
the web UI's build and tests; ts-rs for the shared types.

Clean Code, written functionally: clear names, small functions that do one thing, dependencies
passed in, no duplication or dead code, comments that say why; the ideas behind SOLID and the
design patterns are welcome as functions, data and closures, but not as classes, inheritance or
class hierarchies.

Functional, not object-oriented: pure functions that take data and return data, side effects at
the edges; function components and hooks, no classes; errors as values. A file does one thing:
split it past ~400 lines, a function past ~80. A refactor changes no behaviour.

## Project specifics

- **The one rule: no save is ever lost.** Before the agent writes to a disk it snapshots it; the
  server moves a head only by compare-and-swap; imported versions never become head on their own;
  decisions use ancestry (`parent`, the device's base), never clocks. A change that weakens one of
  these needs a test in the simulation that shows why it is still safe.
- The demo server (`cargo run -p spielstand-server --example demo`) has three devices, a conflict,
  a running game and a device that is behind; password `spielstand-demo`.
- UI components are written in the shadcn/ui style in `web/src/components/ui` (Radix + cva); add new
  ones the same way.
