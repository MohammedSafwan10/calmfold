# Contributing to Ottero

Ottero is a small, actively developed personal app. Useful bug reports,
focused fixes and thoughtful design improvements are welcome. Discuss a new
feature or architectural change in an issue before starting a large pull request.
Contributions are submitted under the project’s [GPLv3 license](LICENSE), unless
a different arrangement is explicitly agreed before submission. You keep your
copyright; no copyright assignment or separate CLA is required.

## Get running

Install the toolchain selected by `rust-toolchain.toml`. macOS builds need Xcode
and its Metal toolchain. Windows builds need the MSVC Rust toolchain, Visual
Studio C++ build tools and CMake.

```sh
git clone https://github.com/MohammedSafwan10/ottero.git
cd ottero
cargo run
```

Debug builds use a separate **Ottero Dev** data folder. For experiments, set
`OTTERO_DATA_DIR` to a new disposable directory. Never run seeds or migration
experiments against your personal database or vault.

## Make a focused change

Read [AGENTS.md](AGENTS.md), [HANDOFF.md](HANDOFF.md) and the nearest crate's
`AGENTS.md`. The [architecture](docs/architecture.md) and
[design system](docs/design-system.md) explain the boundaries.

- Keep domain rules in core, persistence in storage, and screen IO behind Library.
- Use GPUI Kit's real APIs and theme tokens. Check UI changes in light and dark,
  at a smaller window size, and with the keyboard.
- Add regression coverage at the layer where the behavior lives. Include useful
  empty/error states; keep unrelated refactors out of a fix.
- Keep migrations append-only. Discuss vault formats, crypto, sync and updater
  trust changes before implementing them.
- Use fictional data in screenshots and tests. Never commit credentials, vault
  backups, personal databases or signing keys.

Before opening a pull request, run:

```sh
scripts/check.sh
```

On Windows, run that command from Git Bash. CI repeats formatting, strict Clippy
and the full tests on macOS and Windows. Updater changes also run actual installed
upgrade/relaunch and tamper-rejection tests on native runners.

## Open a pull request

Explain the problem, the resulting behavior and how you verified it. Include
window-only before/after screenshots for visual changes, using sample data.
Call out platform behavior you could not check. Update the relevant docs and
feature-plan status when the behavior changes.

AI-assisted contributions meet the same review and testing bar. Review generated
code yourself and be able to explain the implementation and its dependencies.

## Report a problem

Include the Ottero version, OS/architecture, steps to reproduce, expected
behavior and what happened. Remove personal content from logs and screenshots.
Keep feature requests focused on the problem they would solve.

Report exploitable vulnerabilities privately using [the security policy](SECURITY.md),
rather than a public issue. Please keep discussion respectful and specific;
critique the change, not the person. See the [Code of Conduct](CODE_OF_CONDUCT.md).
