# Isolated native updater probe

From the repository root:

```sh
CARGO_TARGET_DIR="$PWD/target/update-engine-probe" cargo run --locked --manifest-path tools/update-engine-probe/Cargo.toml
```

This checks the pinned engines' real safe Rust APIs and development-run guards.
It makes no network update requests, never installs anything, uses no signing
identity and is not linked into Calmfold. Its own Cargo.lock is checked in.
The build downloads dependencies normally. A passing probe is **not** evidence
that an installed-app update works.

Sparkle bridge 0.4.4 is a new third-party bridge, separate from Sparkle itself.
Its current implementation uses Sparkle's standard user driver; it does not
provide our planned GPUI-only UI. Do not adopt it unchanged or load HTML notes.
Audit/customize the bridge or choose an isolated native helper after the real
bundle prototype. Do not add gpui-updater's direct GPUI adapter to Calmfold.

Velopack is pinned to stable 1.2.161 for this probe. Packaging and native Windows
execution are still required; the CLI's build-time .NET requirement does not
justify adding a .NET runtime to the Rust app.

The probe never requests a feed. For production, the default HttpSource needs
an authenticated UpdateSource wrapper: it parses unsigned catalog JSON and sends
a staging ID query. Preserve publisher authentication with a signed catalog,
require SHA-256 and bounded sizes, and omit unnecessary installation identifiers.
See ADR 0008; checksums alone do not authenticate the catalog's publisher.
