# Keepstone patches to zorite-editor 0.11.3

Upstream: https://github.com/packetThrower/zorite (MIT, see `LICENSE`).
Vendored because profiling (`scripts/perf.sh`, `docs/perf.md`) found
per-frame work that grows with document length. Each patch is marked in the
source with `Keepstone patch Pn`. Keep this list current; offer the patches
upstream.

| # | Where | Problem | Fix |
| --- | --- | --- | --- |
| P1 | `lib.rs` `line_starts`, `element.rs` prepaint | `line_end`/`row_col` rebuilt the whole line table on every call; prepaint called them per row → O(lines²) per frame. | Line starts cached per `content_gen` (also dropped at every text mutation); prepaint loops derive line ends from the table they hold. |
| P2 | `element.rs` `shape_document` | Every line scanned every table/math/mermaid/property/fold list → O(lines × regions) per frame. | Per-line lookup tables built once per frame. Same first-match semantics. |
| P3 | `lib.rs` `ShapedDoc::push_offscreen` | Each off-screen line shaped an empty string every frame. | Off-screen lines push an unshaped `WrappedLine::default()`. |
| P4 | `element.rs` cache keys | SipHash for per-line cache keys on every line every frame. | `foldhash` (fixed seed, deterministic). |
| P5 | `element.rs` `shape_document` | `math_regions(content)` re-parsed the document every frame. | Uses the per-generation scan cache. |
| P6 | `element.rs`, `lib.rs` | Ten per-line vectors grown from empty each frame. | `ShapedDoc::with_capacity(lines)`. |
| P7 | `element.rs` `shape_document` | Off-screen table and code rows were always shaped. | Skipped when far outside the shaping band; code rows reuse cached (wrap rows, width). |
| — | `tables.rs`, `lib.rs` | Deprecated `ScrollWheelEvent` / `on_scroll_wheel`. | Renamed to `ScrollEvent` / `on_scroll`. |

Removed from the vendored copy: `examples/` and dev-dependencies (unused here).
