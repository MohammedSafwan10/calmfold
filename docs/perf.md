# Editor performance

Measured, not eyeballed. Re-run after any editor or layout change.

## Method

`scripts/perf.sh` (needs Accessibility permission and an awake display):

1. Builds with `--features perf --profile perf` (release optimizations + symbols).
2. Seeds `/tmp/ks-perf` with four notes of realistic Markdown (headings, bold,
   links, tags, lists, checkboxes, a table and a code block per section):
   1.5 KB, 50 KB, 500 KB and 2 MB (the note size limit).
3. For each note: opens it with the caret at the end, then sends **real**
   keystrokes through the macOS HID event tap (200 characters at 40 ms, i.e.
   fast typing) or 120 scroll-wheel events at 16 ms.
4. GPUI's own profiler records the foreground journal. `apps/desktop/src/perf.rs`
   reports, from the first input on:
   - **input latency**: from the start of each `key_down`/`scroll` dispatch to
     the end of the presentation that first showed it;
   - **draw time** per frame, **late** frames (work over one refresh) and
     **missed refreshes**;
   - slow views, tasks and actions by name.
5. Runs with fewer than 20 measured inputs are reported as INVALID, never as a result.

`PERF_SAMPLE=1` adds `sample` call stacks per run (`target/perf-reports/*.sample.txt`).

Machine: MacBook Air (M4, 16 GB), macOS 27.0.1, built-in display at 60 Hz
(16.7 ms per refresh). Autosave runs during these runs (500 ms debounce).

## Results (2026-11-09)

| Note | Action | Before p50 / p95 | After p50 / p95 / max | Late frames after |
| --- | --- | --- | --- | --- |
| 1.5 KB | typing | 7 / 14 ms (few samples) | 13.0 / 16.4 / 21.5 ms | 0 / 207 |
| 1.5 KB | scroll | 16 / 22 ms | 14.1 / 21.2 / 21.8 ms | 0 / 120 |
| 50 KB | typing | **81 / 341 ms**, 15 of 22 frames late | 16.4 / 23.5 / 29.0 ms | 0 / 204 |
| 50 KB | scroll | 65 / 86 ms | 15.6 / 22.7 / 23.7 ms | 0 / 120 |
| 500 KB | typing | 1,478 / 2,752 ms (keystrokes queued) | 40.0 / 49.1 / 137 ms | 135 / 199 |
| 500 KB | scroll | 131 / 150 ms | 33.2 / 34.0 / 51.7 ms | 69 / 69 |
| 2 MB | scroll | not measurable | 139 / 150 ms | 18 / 18 |
| 2 MB | typing | not measurable | not measured (run invalid) | — |

How to read the latency: it includes waiting for the next display refresh, so
on a 60 Hz panel a perfect frame shows between ~1 and ~17 ms after the key.
**No frame missed a refresh while typing or scrolling notes up to 50 KB**; the
p95 above 16.7 ms there is refresh-alignment wait plus the ~6–13 ms draw, not
dropped frames.

## What was wrong and what changed

Profiling showed all the time inside the note editor's layout pass, which
walks the whole document every frame. Seven fixes in the vendored editor
(`vendor/zorite-editor/PATCHES.md`): a quadratic line lookup, per-line scans of
every table/math region, shaping empty strings for off-screen lines, SipHash
keys, a whole-document math re-parse per frame, vector regrowth, and shaping
off-screen table/code rows.

## Honest status

- Up to 50 KB (far beyond most notes): no dropped frames typing or scrolling.
- 500 KB: usable but not 60 fps (~34 ms frames ≈ 30 fps). 2 MB: ~7 fps.
- The remaining cost is still O(lines) per frame: the editor re-walks every
  line on each keystroke. The real fix is incremental layout (reuse unchanged
  lines' results across edits). Tracked in `docs/TASKS.md`.
- Memory was not measured.
