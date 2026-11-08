# 02 Notes

Goal: Obsidian-level notes: fast Markdown, `[[links]]`, backlinks, search.

## Done

- [x] Note model (title, body, pinned, soft delete), validation, 2 MiB limit
- [x] Wiki-link parser (`[[A]]`, `[[A|alias]]`, `[[A#h]]`, `![[embed]]`, skips code)
- [x] SQLite + FTS5 (diacritic-insensitive, prefix search, bm25 title boost)
- [x] Links table updated atomically on save; backlinks query
- [x] UI: list with search, pin, trash view, empty trash (confirmed)
- [x] Editor with debounced autosave (500 ms), flush on switch/blur
- [x] Edit / Preview (Markdown rendering), backlinks panel ("Linked from")
- [x] Move to trash with Undo; delete forever with confirmation
- [x] Plain-text list previews (Markdown markers stripped)

## Next (in order)

1. [x] Click `[[link]]` in preview to open (create if missing)
2. [x] Link autocomplete when typing `[[` (editor completion menu)
3. [ ] Unlinked mentions under backlinks
4. [ ] Tags (`#tag`) with a sidebar child list
5. [ ] Daily notes (one per date, opened from Today)
6. [ ] Virtualized note list (`v_virtual_list`) once > 1,000 notes
7. [ ] Import a Markdown/Obsidian folder; export to `.md` files
8. [ ] Graph view
9. [ ] Attachments (images) stored in the data folder

## Tests to add with each step

Parser tests for new syntax; storage tests for new queries; one UI flow per feature.
