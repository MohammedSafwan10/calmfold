# Ottero — approved Quiet Loop direction

The owner selected concept 01 on 2026-11-17.

- `source/icon-master.png`: clean square raster master, otter symbol only on an
  opaque ivory background. No labels or wordmark. Apply platform masks separately.
- `concept-01-reference.png`: original concept board, including wordmark direction.
  This board is reference material, not an application icon.
- `source/generation-prompt.txt`: prompt used with the built-in image generator.

Preserve the curled charcoal otter, small ivory eye, inward forepaw and open O
silhouette. Derive all sizes from the same approved artwork. For symbol-only,
monochrome and transparent assets, isolate or faithfully trace the silhouette;
do not put the entire ivory square into a tray icon or horizontal logo.

## Handoff for the rename

Rename the public product to **Ottero by Nexdark Labs** and replace the old
artwork throughout the app, website and project documentation. Audit the app,
downloads and separate website repositories and their hosting/build configuration
before changing GitHub repository names or release URLs. No final domain has been
selected. The app rename is done (ADR 0009, 2026-11-17); repositories, release
URLs and the website are not renamed yet.

Treat this as a migration, not a global text replacement: preserve existing user
data/vault access, backups, signing keys and updates from already installed
Calmfold releases. Keep compatibility identifiers or provide tested migrations
where needed. Preserve historical immutable release assets and accurate license/
third-party attributions. Generate ICNS, ICO, PNG, favicon and website assets,
then verify both platform packages, old-to-new updates and light/dark appearance.
