//! Creates notes of known sizes for `scripts/perf.sh`.
//!
//! `cargo run -p calmfold-storage --example perf_seed -- <db-path>`
//!
//! Each note is realistic Markdown (headings, lists, bold, links, tags, code,
//! a table) repeated to size, so the editor does the same styling work a
//! real note needs.

#![allow(clippy::expect_used)] // Dev tool: failing loudly is the right behavior.

use calmfold_core::{Note, clock};
use calmfold_storage::Store;

const SECTION: &str = "## Section {n}\n\
Some **important** text with *emphasis*, a [[Service log]] link and a #project/tag. \
Prices went from `5W-30` to ==highlighted== values, see https://example.com/page.\n\
\n\
- First bullet with a bit of text to wrap on narrow windows\n\
- Second bullet, **bold** in the middle\n\
  - Nested item\n\
- [ ] An open checklist item\n\
- [x] A finished one\n\
\n\
| Item | Qty | Price |\n\
|:--|--:|--:|\n\
| Oil | 4 | 32.00 |\n\
\n\
```rust\nfn main() { println!(\"section {n}\"); }\n```\n\
\n\
> A short quote to finish the section.\n\n";

fn body(target: usize) -> String {
    let mut out = String::with_capacity(target + SECTION.len());
    let mut n = 1;
    while out.len() < target {
        out.push_str(&SECTION.replace("{n}", &n.to_string()));
        n += 1;
    }
    out
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: perf_seed <db-path>");
    if let Some(dir) = std::path::Path::new(&path).parent() {
        std::fs::create_dir_all(dir).expect("create data folder");
    }
    let mut store = Store::open(std::path::Path::new(&path)).expect("open store");
    let sizes = [
        ("Perf 1 KB", 1_000),
        ("Perf 50 KB", 50_000),
        ("Perf 500 KB", 500_000),
        ("Perf 2 MB", calmfold_core::MAX_BODY_BYTES - 4_000),
    ];
    for (title, size) in sizes {
        let note = Note::new(title, body(size), clock::now()).expect("note");
        store.save_note(&note).expect("save");
        println!("{title}: {} bytes", note.body().len());
    }
}
