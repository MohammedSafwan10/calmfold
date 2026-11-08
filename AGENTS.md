# AGENTS.md

Guidance for coding agents (and humans) working on **Keepstone**, a local-first,
all-in-one personal app (notes, tasks, later a password vault, calendar,
habits, finance) written in Rust with a GPUI desktop UI.

Read this file, then [`HANDOFF.md`](HANDOFF.md) for the current state, then the
`AGENTS.md` of the crate you are changing. The closest `AGENTS.md` wins.

## Commands

```bash
source "$HOME/.cargo/env"          # if cargo is not on PATH
scripts/check.sh                   # fmt --check + clippy -D warnings + all tests (run before every commit)
cargo run                          # run the desktop app (debug data: "Keepstone Dev" folder)
scripts/dev.sh [--sample]          # run + auto-rebuild/relaunch on save (needs watchexec)
KEEPSTONE_DATA_DIR=/tmp/ks cargo run   # run against a throwaway data folder
cargo test -p keepstone-desktop --test ui   # headless UI integration tests only
scripts/screenshot.sh /tmp/k.png   # capture ONLY the Keepstone window (never the full screen)
```

Toolchain is pinned in `rust-toolchain.toml` (Rust 1.99.1). The first build
takes ~3 minutes; GPUI dependencies are optimised even in debug (see
`[profile.dev.package."*"]`) so the app stays smooth during development.

## Workspace map

| Path | Crate | Responsibility | May depend on |
| --- | --- | --- | --- |
| `crates/core` | `keepstone-core` | Domain models and rules (Note, Task, IDs, wiki links, state machines). No IO, no clock except `clock.rs`. | nothing internal |
| `crates/storage` | `keepstone-storage` | SQLite schema, migrations, FTS5 search, repositories. | core |
| `crates/crypto` | `keepstone-crypto` | Argon2id KDF, XChaCha20-Poly1305 envelope, zeroizing keys. For the vault. | nothing internal |
| `crates/ui` | `keepstone-ui` | Design system: themes, fonts, appearance settings, icon assets, reusable components, feedback (confirm/undo/error). | gpui-kit |
| `apps/desktop` | `keepstone-desktop` | The GPUI app: shell, sidebar, screens, actions, command palette. | all of the above |

Dependency direction is one-way: `desktop → ui/storage/crypto → core`. Never
import the UI from core/storage/crypto. Mobile and web shells will reuse
core/storage/crypto later, so keep them UI-free.

## Hard rules

1. **Never invent a GPUI / gpui-kit API.** Look up the real signature in
   `~/.cargo/registry/src/*/gpui-component-0.9.0`, `gpui-base-0.9.0`,
   `gpui-pre-0.3.11`, or the vendored skills in `.kiro/skills/gpui-kit`.
   Read `.kiro/skills/gpui-kit-design-guides` before any UI change.
2. **One UI dependency.** Depend on `gpui-kit` only (it pins a `gpui-pre`
   snapshot). Never add `gpui` directly. Upgrade GPUI only by upgrading
   gpui-kit, then rerun every test and look at the app.
3. **No hard-coded colors** in feature code. Use `cx.theme()` tokens. Raw hex
   lives only in `crates/ui/themes/keepstone.json`.
4. **Views never touch SQLite.** They call `Library` methods
   (`apps/desktop/src/library.rs`), which apply domain rules, persist, and emit
   `LibraryEvent`.
5. **Domain rules live in core** and take `now` as a parameter. Use
   `keepstone_core::clock::now()` at the call site.
6. **Destructive actions** follow `keepstone_ui::feedback`: reversible → run
   and offer Undo; permanent → `confirm_destructive`; errors → `error_toast`.
7. **Every command is an action** (`apps/desktop/src/actions.rs`), handled at
   app level in `shell::register_commands`. Add it to the palette list and
   the shortcuts table when it has a shortcut.
8. **New Lucide icon?** Add it to `AppIcons` in `crates/ui/src/assets.rs`.
   A test fails if you forget (unlisted icons render blank).
9. **Migrations are append-only.** Never edit a shipped `.sql` file; add
   `000N_*.sql` and append it in `crates/storage/src/schema.rs`.
10. **Security.** No secrets, note bodies, or passwords in logs or error
    messages. No `unsafe` (forbidden workspace-wide). Crypto changes need
    tests and must follow `crates/crypto/AGENTS.md`.
11. **Do not code-sign** with any identity found on the machine. Development
    builds must not touch the system keychain (see `docs/security.md`).
12. **Screenshots** only via `scripts/screenshot.sh` (Keepstone window only).

## Definition of done

- `scripts/check.sh` passes (fmt, clippy `-D warnings`, all tests).
- Behaviour has a test at the right layer: pure logic → unit test in the
  crate; persistence → storage test; UI flow → `apps/desktop/tests/ui.rs`.
- You ran the app and looked at the changed screen in light and dark mode.
- Empty, error, and loading states exist for anything new.
- `HANDOFF.md` is updated (current state, what changed, next steps).
- If a plan in `docs/plans/` changed, update its status checklist.

## Docs

- `docs/architecture.md`: layers, data flow, startup, state machines.
- `docs/design-system.md`: theme, typography, spacing, components, feedback rules.
- `docs/security.md`: threat model, vault crypto design, keychain policy.
- `docs/testing.md`: what to test where and how.
- `docs/roadmap.md` and `docs/plans/*.md`: one plan per feature, in order.
- `docs/decisions/*.md`: short ADRs for choices that are expensive to reverse.

## Style

- Rust 2024 edition, `cargo fmt` defaults, Clippy pedantic-lite via workspace lints.
- `unwrap`/`expect` are warned in production code; use `?`, `context`, or a
  logged fallback. Allowed in tests.
- Names: spell `Context` out; `cx` is reserved for GPUI contexts. Avoid `Kind`
  suffixes. Stable, domain-derived `ElementId`s for list rows (`note-{id}`).
- Comments explain *why*. Module docs describe the state machine when there is one.
- Commit messages: imperative, short subject (`Add note backlinks panel`).
