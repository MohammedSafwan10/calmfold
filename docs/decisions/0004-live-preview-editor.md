# 0004 Live-preview note editor: zorite-editor

Date: 2026-11-08 · Status: accepted

**Context.** Notes were edited as raw Markdown (`**May**`) with a separate
Preview mode. People expect a document: formatting visible while typing, like
Obsidian's Live Preview, Bear or Notion. Notes must stay plain Markdown
(portable to Obsidian, lossless import/export).

Options compared (all compiled against our exact stack first):

| Option | Model | Verdict |
| --- | --- | --- |
| gpui-kit `Editor` + custom highlighter | raw text, styled ranges | Can't hide markers or size headings. |
| `gpui-kit-markdown-editor` | block tree, re-serialized on save | Rejected: changes notes on save (drops empty paragraphs, list spacing). |
| `zorite-editor` 0.11 | the buffer *is* the Markdown; markers hidden, revealed at the caret | **Chosen.** Lossless by construction. |
| Build our own | — | Months of work for caret/IME/selection/undo that zorite already has. |

**Decision.** Use `zorite-editor` (MIT) for the note body, with `ratex-gpui`
(render-only) for `$$…$$` math. Wrap it in `views/note_editor.rs`; nothing
else in the app touches zorite types.

**Exception to "one UI dependency".** Both crates depend on `gpui-pre`
directly. Allowed only while they resolve to the **same** `gpui-pre` as
gpui-kit. Both are pinned with `=`; upgrade them together with gpui-kit and
check `cargo tree -d -i gpui-pre` lists a single version.

**Audit (0.11.3).** No `unsafe`, no panics on production paths, debug logging
only behind `ZORITE_*` env vars. `set_text` normalizes `$$` fences on load
(only when `$$` is present); our round-trip test pins that every other note
loads and saves byte-identical.

**Host obligations we implement.** Save on `Changed`; `[[`/`#` clicks →
`OpenWikiLink`; URLs → `OpenLink` (http/https/mailto only); `EditMath` and
`EditProperties` must move the caret into the block (otherwise arrow keys
stop at it); theme colors mapped from `cx.theme()` into `SyntaxStyle`.

**Consequences.** Single-maintainer crate: if it stalls or a bug can't be fixed
upstream, vendor it into `crates/` (MIT allows it) rather than fork silently.
