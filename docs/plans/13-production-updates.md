# Production updates and distribution

Status: first hardening milestone complete; engine migration in progress.
This checklist is evidence-based: a compiling adapter or
unit test does not count as a successful installed-app upgrade.

## Architecture and release boundaries

- Keep the app and update controls native GPUI. No HTML release-note renderer.
- Preferred installation engines: Sparkle 2 on macOS, Velopack on Windows,
  subject to disposable installed-app prototypes. Reuse the existing action and
  status interface. Never ship two competing automatic installers.
- Coolify hosts the website at `calmfold.nexdark.com`; GitHub hosts versioned
  release artifacts. A branded feed URL may redirect to the authoritative feed;
  do not independently generate two feeds or re-sign on the web server.
- Only public keys reach the app or website. Protect release signing credentials
  separately from hosting credentials. Document recovery/key rotation before GA.
- Keep builds unsigned during development. Do not use an identity on this Mac or
  touch its keychain. Public release signing/notarization requires a separately
  provisioned owner-approved CI signing environment.
- Website completion and encrypted sync are separate projects. Do not create a
  sync database just to serve updates.

## 1. Harden the existing boundary before engine migration

- [x] Stream downloads into private temporary files, with incremental hash and
      exact size verification. Enforce signed size before writing extra bytes;
      remove incomplete files on every error. Avoid archive-sized RAM allocation.
- [x] Validate release versions, HTTPS URLs, hashes and size in pure core rules.
      Never derive cache paths from unchecked local ready metadata.
- [x] Persist ready state atomically and durably; persistence failure cannot
      report Ready. Hold one lock through check/download/stage transitions.
- [x] Preserve preference changes made while a network request is running.
- [x] Refuse update restart when Notes, Developer or Passwords cannot save.
- [x] Cover truncated/oversized/corrupt streams, write/read failures, persisted
      state, unsafe cache versions and draft preservation with regressions.

## 2. Prove installation engines in isolation

- [x] Review actual Sparkle bridge and Velopack APIs/dependency trees. Do not add
      direct GPUI or introduce workspace unsafe code. A new Rust bridge is not
      automatically production-proven because Sparkle itself is established.
- [x] Compile reproducible isolated probes without changing the production app.
      Mac development guards pass for pinned bridge 0.4.4 / Velopack 1.2.161.
      Native Windows/Mac CI is configured; this does not prove installation.
- [ ] macOS: package two disposable app versions, signed with temporary update
      keys only; prove check, download, install and relaunch. No real app/data.
- [ ] Windows host: prove per-user install, update with app running, locked files,
      two instances and relaunch. A macOS cross-check is not a Windows runtime test.
- [ ] Confirm no webview is used in our update UI and verify engine signature
      checks, cancellation, permissions and minimum OS/architecture handling.
- [ ] Velopack: authenticated catalog source (verify signed bytes before parse,
      mandatory SHA-256/size), without the default per-install staging ID query.
      Its 1.2.161 HttpSource quick start is not a signed-catalog implementation.
- [ ] Record adapter decision and replace the legacy file-swap installer only
      after the corresponding platform passes. Do not leave a hidden fallback
      that installs with weaker verification.

## 3. Publish releases atomically

- [ ] CI runs quality checks and verifies version/tag/package agreement.
- [ ] Build on native Mac/Windows runners, package, sign update artifacts, verify
      signatures with pinned public keys, then upload all assets to a draft.
- [ ] Enable GitHub immutable releases and publish the complete draft last;
      never use `--clobber` on published release assets.
- [ ] Configure stable/beta channels and minimum compatible versions. Add signed
      metadata expiry/replay protection with operational refresh/key rotation.
- [ ] Publish a real test feed before changing the stable feed URL. Hosting loss
      must leave local notes, tasks and vault usable offline.

## 4. Data safety and recovery

- [ ] Save every relevant draft before relaunch, including open task sheets.
- [ ] Back up data before incompatible database migrations, with a documented
      restore policy. Binary rollback alone cannot undo a schema migration.
- [ ] Prove behavior for interrupted install and new-version early crash. Keep
      recoverable state until successful startup is confirmed; do not describe
      process spawning as health confirmation or promise automatic DB rollback.
- [ ] Check disk space and install permissions before downloading/applying.

## 5. Calm UX and performance

- [ ] Check after startup without delaying the first window; daily checks with
      backoff/jitter, one notification, manual retry and clear offline behavior.
- [ ] Native progress/cancel/retry, Restart now/Later, plain-text release notes.
- [ ] Add verified delta updates with full-package fallback after full upgrades
      pass; measure actual artifact size, peak memory and UI responsiveness.
- [ ] Bound and clean staging/cache files; avoid unbounded retained versions.

## Completion gate

- [ ] Full local quality gate, engine tests and light/dark native UI review.
- [ ] Real old-to-new installed-app upgrade on both supported platforms.
- [ ] Failures: offline, truncation, corrupt signature/hash, disk full, denied
      permissions, interrupted install, double instance, failed save/relaunch.
- [ ] Test current and older supported releases, then private → beta → stable.
- [ ] Update HANDOFF and security/release runbook with verified results and limits.

## Primary references checked 2026-11-16

- https://sparkle-project.org/documentation/
- https://sparkle-project.org/documentation/custom-user-interfaces/
- https://docs.velopack.io/getting-started/rust
- https://docs.velopack.io/packaging/deltas
- https://docs.github.com/en/code-security/concepts/supply-chain-security/immutable-releases
- https://theupdateframework.io/docs/security/
- https://docs.rs/gpui-updater-sparkle/0.4.4 (new bridge; audit before adoption)

## First milestone evidence (2026-11-16)

- `scripts/check.sh`: 345 tests, including 75 UI tests; fmt and strict Clippy pass.
- Fixed 32 KiB transfer buffer; each chunk is checked against signed size before
  writing, then the incremental SHA-256 is checked before extraction. No archive
  sized memory buffer. Partial download files use private RAII temp files.
- Atomic ready/preferences state now propagates errors; the request's lock stays
  alive through staging. Preference-completion regression preserves a mid-request
  switch to disabled automatic checks.
- UI regressions cover immediate note flush, failed note save, open task sheet,
  invalid environment keys and unapplied imports at update-restart time.
- `tools/update-engine-probe`: pinned safe Rust APIs compile/run on this Mac;
  neither engine enters the production dependency graph. Tree has no GPUI,
  WebKit or webview dependency. The Sparkle bridge uses Sparkle's standard driver
  when a framework is embedded, so a GPUI-only adapter still needs work.
- Existing GitHub secret named MINISIGN_KEY is present; its value was not read.
  Public calmfold-releases currently has no published releases. Do not mark
  publishing or signature/key correspondence verified from secret presence.
- Native Settings shell viewed in light and dark. Real window clicks remain
  unavailable while off-screen; changed restart behavior is proven headlessly.
