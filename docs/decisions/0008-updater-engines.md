# 0008: Gate native updater engines behind installed-app prototypes

Status: proposed engine selection; accepted isolation and release gates.

The custom updater has signed-feed and hash verification but has not passed a
real installed-app upgrade on Mac or Windows. Hardening its download/save
boundary is useful immediately; replacing installation mechanics is a separate
change. A successful process spawn is not proof of successful startup.

Preferred candidates are Sparkle 2 for macOS and Velopack for Windows. Keep
Calmfold's actions, settings and update status native GPUI. No HTML release-note
renderer, extra direct GPUI dependency or workspace unsafe code is permitted.

The isolated `tools/update-engine-probe` pins gpui-updater-sparkle 0.4.4 and
Velopack 1.2.161. Safe calls compile and development-process guards run on Mac.
CI is configured to run the same non-installing probes on Mac and Windows.
These dependencies are not linked into Calmfold.

The new third-party Sparkle bridge depends on a UI-free core (no direct GPUI),
but its implementation uses Sparkle's standard user driver. It does not provide
our intended GPUI-only interaction. Sparkle's SPUUserDriver supports custom UI;
the next experiment must demonstrate a safe bridge or isolated native helper,
plus correct app lifecycle and save/relaunch handling. Do not substitute a weak
custom installer when the native engine fails to initialize.

Pinned Velopack 1.2.161's `HttpSource::get_release_feed` parses downloaded JSON
directly; package checksums are not a publisher signature over that catalog.
Before adoption, implement/review an authenticated `UpdateSource` that verifies
the exact catalog bytes with a pinned key before parsing, requires SHA-256 and
bounded package sizes, and does not send its default staging/install identifier
to our static host. Do not replace the existing signed-feed boundary with the
documentation's bare HttpSource quick start. OS code signing is complementary.

Before adopting either engine, prove two disposable installed versions updating
on their native OS, including interruption, signature rejection, permissions,
multiple running copies and failed relaunch. Never test on the owner's app/data.
Database backup/compatibility is a separate prerequisite for binary recovery.

Website belongs on Coolify; immutable, versioned release artifacts belong on the
public releases repository. Build all artifacts and signed metadata into a draft
before publication. The private source repo's GITHUB_TOKEN cannot be assumed to
have write access to that separate public repo; provision narrowly scoped
release credentials before automating publication. Hosting receives no signing
private keys. Development never uses the Mac's signing identities or keychain.

See [implementation plan](../plans/13-production-updates.md) for evidence and
primary sources. Public release signing, actual upgrades and recovery remain
unchecked until proven; none is implied by the API smoke probe.
