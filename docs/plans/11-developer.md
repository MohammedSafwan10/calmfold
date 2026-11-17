# Developer workspace — first release

## Scope and interaction

One Developer destination, a searchable project list and Overview / Secrets
tabs. Reuse task ProjectId values; setup notes link to real Notes. Project
metadata (name, non-secret links, folder reference) is ordinary local app data,
not encrypted. Secrets, including variable names and environment labels, stay
inside the existing password vault. No network, shell commands, clipboard
monitoring, automatic folder writes or new crypto service.

Project overview: create/edit/archive/restore, repository and useful links,
local folder reference, setup note and project task navigation. Archiving does
not delete tasks, notes or vault items. The Developer entry is optional in
Settings. Existing task projects can be promoted to developer workspaces.
Hiding Developer changes sidebar visibility only; commands and shortcuts stay
available, and hiding never deletes or disables existing data.

Secrets: an environment set per project/environment. Native masked rows,
search by key, add/edit/remove variables, import file/paste with a masked
preview, copy one value or all as .env, deliberate plaintext export. The same
vault record is readable/editable through Passwords; no duplicate secret DB.
Existing API/SSH credentials remain supported by Passwords; this release adds
structured environment sets, not a replacement SSH agent or deployment tool.

## Storage and compatibility

- Append migration 0011 for developer project metadata referencing projects
  and a setup NoteId, with created/updated/deleted timestamps.
- Add an optional environment payload to encrypted ItemFields, with explicit
  variable/size limits and redacted Debug/zeroizing secret data. Reject rather
  than truncate. Normal v1 items remain v1; environment items use item format
  v2 so older clients cannot silently discard new fields on edit. This is an
  item-data version only; the encryption envelope and KDF stay unchanged.
- Read v1/v2 items and verify backups/recovery and metadata editing preserve
  every variable. Older app versions must be upgraded before using v2 items.

## .env dialect

ASCII identifier keys, optional `export`, LF/CRLF, blanks/comments, quoted and
unquoted values, equals/hash characters inside quotes, empty values, double
quote escapes and multiline quoted values. Reject duplicate keys, invalid
syntax, NUL and oversized input with line numbers, never source text. Do not
evaluate variable expansion or command substitution: store characters
literally and explain this in import preview. Export canonical quoted values;
the built-in parser must recover identical values. Comments/layout are not
preserved in canonical export. Runtime dotenv dialects can differ.

## Security

All operations require the existing vault to be unlocked. Locking clears
editor rows, previews, reveals, searches and pending dialog results. Async
file operations carry a draft generation so late results cannot repopulate
another record or a locked vault. Exports require an explicit plaintext
confirmation and a user-chosen path, use restrictive permissions where
supported and avoid silent overwrites/partial files. Clipboard copies use the
existing secret clipboard lifecycle. No secrets in errors, logs, FTS or
plaintext project metadata. Notes and normal project metadata follow the
existing unencrypted local SQLite policy.

## Checklist

- [x] Domain project validation and .env parser/exporter, including limits
- [x] Append-only storage migration, persistence and upgrade tests
- [x] Encrypted environment payload and old-item/backup compatibility tests
- [x] Native environment editor shared with Passwords, import/copy/export
- [x] Developer project list, overview, task/note navigation, secrets and lock UX
- [x] Navigation/actions/settings/icon integration and keyboard paths
- [x] UI regression tests: project flow, import/export, lock and stale results
- [x] Full checks, light/dark review, security docs and HANDOFF updates

Verified on macOS: `scripts/check.sh` passes (329 tests, including 69 UI
tests). Native Overview and shared environment screens reviewed in light and
dark. Input automation was unavailable while windows were off-screen; flow
tests drive the native headless UI, including the real import/export dialogs.

The masked value input uses an explicit reversible escaped representation
(`\n`, `\r`, `\t`, `\\`), because GPUI Kit's masked input is single-line.
Pasted literal values are escaped automatically. Import/copy/export preserve
multiline values; UI regression tests verify this rather than relying only on
parser tests. No unmasked multiline editor is used.

Manual review fixture: `cargo run -p ottero-desktop --example seed_developer
-- /tmp/ottero-review/ottero.sqlite3` creates three demo projects and a demo
encrypted environment set. It refuses existing databases. Launch with
`OTTERO_DATA_DIR=/tmp/ottero-review OTTERO_DEV_ROUTE=developer cargo run`.
See the example's module docs for the disposable preview vault credentials.

Deferred: general developer utilities, environment comparison, CLI injection,
SSH agent, server/deployment integrations and cross-device sync.

## Audit follow-up (2026-11-16)

- [x] Reveal literal values, including multiline content, with bounded height.
- [x] Leave create forms on project selection, archived view and new environment.
- [x] Contextual lock guidance disappears after unlocking; keep genuine form errors.
- [x] Snapshot after confirmation/path selection; write/sync exports in the
  background, prevent concurrent exports and report specific safe errors.
  A write already started finishes its explicitly approved snapshot; locking
  before the path dialog returns prevents the export from starting.
- [x] Preserve invalid/unapplied-import drafts when switching Passwords items,
  filters or creating another item. Correcting validation permits the switch.
- [x] Individual/all environment copies update recently-used metadata.
- [x] Let the vault pane own Secrets scrolling, size the variable list to its
  content up to six rows, and expand revealed multiline rows within bounds.
- [x] Trim variable key whitespace in the editor, preserving literal values.
- [x] Count actual canonical escaped UTF-8 bytes; accept ordinary files above
  512 KiB when they fit the documented input/output limits.
- [x] Checkable archive command, searchable settings keywords, explicit
  DeveloperChanged event on project removal and Escape to cancel forms.
- Sidebar hiding intentionally keeps command/shortcut access; this is a
  visibility preference, not a security boundary or feature permission.
