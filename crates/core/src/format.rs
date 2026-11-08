//! Markdown formatting commands (bold, headings, lists, …) as pure text edits.
//!
//! Each function takes the document and the selection (byte offsets) and
//! returns an [`Edit`]: replace `range` with `text`, then select `selection`.
//! Formatting stays plain Markdown, so notes remain portable (Obsidian, GitHub).
//! There are deliberately no font colors or sizes: headings carry size.

use std::ops::Range;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub text: String,
    /// Selection after the edit, in offsets of the new document.
    pub selection: Range<usize>,
}

/// Inline styles that wrap text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inline {
    Bold,
    Italic,
    Strikethrough,
    Code,
    Highlight,
}

impl Inline {
    #[must_use]
    pub fn marker(self) -> &'static str {
        match self {
            Self::Bold => "**",
            Self::Italic => "*",
            Self::Strikethrough => "~~",
            Self::Code => "`",
            Self::Highlight => "==",
        }
    }
}

/// Line-level formats. Applying the active one again removes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    Paragraph,
    Heading(u8),
    Bullet,
    Numbered,
    Checklist,
    Quote,
}

impl Block {
    fn prefix(self, n: usize) -> String {
        match self {
            Self::Paragraph => String::new(),
            Self::Heading(level) => format!("{} ", "#".repeat(usize::from(level.clamp(1, 6)))),
            Self::Bullet => "- ".into(),
            Self::Numbered => format!("{n}. "),
            Self::Checklist => "- [ ] ".into(),
            Self::Quote => "> ".into(),
        }
    }

    /// The block format of a line and the length of its prefix.
    fn detect(line: &str) -> (Self, usize) {
        let hashes = line.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
            return (Self::Heading(u8::try_from(hashes).unwrap_or(1)), hashes + 1);
        }
        for (prefix, block) in [
            ("- [ ] ", Self::Checklist),
            ("- [x] ", Self::Checklist),
            ("- ", Self::Bullet),
            ("* ", Self::Bullet),
            ("> ", Self::Quote),
        ] {
            if line.starts_with(prefix) {
                return (block, prefix.len());
            }
        }
        let digits = line.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && line[digits..].starts_with(". ") {
            return (Self::Numbered, digits + 2);
        }
        (Self::Paragraph, 0)
    }
}

/// Wraps the selection in `style` markers, or removes them if already wrapped.
/// With no selection, inserts an empty pair and puts the caret between.
#[must_use]
pub fn toggle_inline(doc: &str, sel: Range<usize>, style: Inline) -> Edit {
    let m = style.marker();
    let sel = clamp(doc, sel);
    let inner = &doc[sel.clone()];
    // Already wrapped inside the selection: "**bold**" selected.
    if inner.len() >= 2 * m.len() && inner.starts_with(m) && inner.ends_with(m) {
        let text = inner[m.len()..inner.len() - m.len()].to_owned();
        let end = sel.start + text.len();
        return Edit {
            range: sel.clone(),
            text,
            selection: sel.start..end,
        };
    }
    // Wrapped just outside the selection: "**|bold|**".
    if sel.start >= m.len() && doc[..sel.start].ends_with(m) && doc[sel.end..].starts_with(m) {
        let range = sel.start - m.len()..sel.end + m.len();
        let start = range.start;
        return Edit {
            range,
            text: inner.to_owned(),
            selection: start..start + inner.len(),
        };
    }
    let text = format!("{m}{inner}{m}");
    let start = sel.start + m.len();
    Edit {
        range: sel,
        text,
        selection: start..start + inner.len(),
    }
}

/// Sets the block format of every line the selection touches.
#[must_use]
pub fn set_block(doc: &str, sel: Range<usize>, block: Block) -> Edit {
    let sel = clamp(doc, sel);
    let start = doc[..sel.start].rfind('\n').map_or(0, |i| i + 1);
    let end = doc[sel.end..].find('\n').map_or(doc.len(), |i| sel.end + i);
    let lines: Vec<&str> = doc[start..end].split('\n').collect();
    // Toggle off when every line already has this format.
    let all_same = lines.iter().all(|l| Block::detect(l).0 == block);
    let target = if all_same && block != Block::Paragraph {
        Block::Paragraph
    } else {
        block
    };

    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let (_, prefix_len) = Block::detect(line);
        out.push(format!("{}{}", target.prefix(i + 1), &line[prefix_len..]));
    }
    let text = out.join("\n");
    let caret = start + text.len();
    let selection = if sel.is_empty() {
        caret..caret
    } else {
        start..caret
    };
    Edit {
        range: start..end,
        text,
        selection,
    }
}

/// Wraps the selection as a Markdown link with the caret in the URL part.
#[must_use]
pub fn insert_link(doc: &str, sel: Range<usize>) -> Edit {
    let sel = clamp(doc, sel);
    let label = &doc[sel.clone()];
    let label = if label.is_empty() { "link" } else { label };
    let text = format!("[{label}](https://)");
    let url_start = sel.start + label.len() + 3;
    Edit {
        range: sel,
        text,
        selection: url_start + "https://".len()..url_start + "https://".len(),
    }
}

/// Toggles the checkbox of the checklist item on the caret's line.
#[must_use]
pub fn toggle_checkbox(doc: &str, caret: usize) -> Option<Edit> {
    let caret = caret.min(doc.len());
    let start = doc[..caret].rfind('\n').map_or(0, |i| i + 1);
    let line = &doc[start..];
    let (from, to) = if line.starts_with("- [ ] ") {
        ("- [ ] ", "- [x] ")
    } else if line.starts_with("- [x] ") {
        ("- [x] ", "- [ ] ")
    } else {
        return None;
    };
    debug_assert_eq!(from.len(), to.len());
    Some(Edit {
        range: start..start + from.len(),
        text: to.into(),
        selection: caret..caret,
    })
}

fn clamp(doc: &str, sel: Range<usize>) -> Range<usize> {
    let fix = |mut i: usize| {
        i = i.min(doc.len());
        while !doc.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let (a, b) = (fix(sel.start), fix(sel.end));
    a.min(b)..a.max(b)
}

/// The `==` delimiters of each `==highlight==` (Obsidian syntax), as
/// (opening range, closing range) pairs. CommonMark has no highlight, so
/// renderers turn these into `<mark>`. Like Obsidian: the text inside must not
/// start or end with a space, highlights stay on one line, code is skipped.
#[must_use]
pub fn highlight_spans(doc: &str) -> Vec<(Range<usize>, Range<usize>)> {
    let mut out = Vec::new();
    let mut fenced = false;
    let mut offset = 0;
    for line in doc.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let bytes = line.as_bytes();
        let mut in_code = false;
        let mut open: Option<usize> = None;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'`' => {
                    in_code = !in_code;
                    i += 1;
                }
                b'=' if !in_code && bytes.get(i + 1) == Some(&b'=') => {
                    let after = line[i + 2..].chars().next();
                    match open {
                        Some(o) if !line[..i].ends_with(char::is_whitespace) && i > o + 2 => {
                            out.push((start + o..start + o + 2, start + i..start + i + 2));
                            open = None;
                        }
                        _ if after.is_some_and(|c| !c.is_whitespace() && c != '=') => {
                            open = Some(i);
                        }
                        _ => {}
                    }
                    i += 2;
                }
                _ => i += 1,
            }
        }
    }
    out
}

/// Byte ranges that aren't prose, so spell-check skips them: fenced code
/// blocks, inline `code`, `[[links]]`, `#tags`, URLs and `$math$`. Sorted,
/// may overlap.
#[must_use]
pub fn non_prose_ranges(doc: &str) -> Vec<Range<usize>> {
    let mut out: Vec<Range<usize>> = Vec::new();
    let mut fence: Option<usize> = None;
    let mut offset = 0;
    for line in doc.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let trimmed = line.trim_start();
        let is_fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        match (fence, is_fence) {
            (Some(open), true) => {
                out.push(open..offset);
                fence = None;
                continue;
            }
            (None, true) => {
                fence = Some(start);
                continue;
            }
            (Some(_), false) => continue,
            (None, false) => {}
        }
        // Inline code and $math$, by delimiter pairs on the line.
        for delim in ['`', '$'] {
            let mut open: Option<usize> = None;
            for (i, c) in line.char_indices() {
                if c == delim {
                    match open.take() {
                        Some(o) => out.push(start + o..start + i + 1),
                        None => open = Some(i),
                    }
                }
            }
        }
        // URLs: from the scheme to the next whitespace.
        for scheme in ["https://", "http://", "mailto:", "file://"] {
            let mut from = 0;
            while let Some(i) = line[from..].find(scheme) {
                let at = from + i;
                let end = line[at..]
                    .find(char::is_whitespace)
                    .map_or(line.len(), |e| at + e);
                out.push(start + at..start + end);
                from = end;
            }
        }
    }
    if let Some(open) = fence {
        out.push(open..doc.len());
    }
    out.extend(
        crate::links::wiki_link_spans(doc)
            .into_iter()
            .map(|(r, _)| r),
    );
    out.extend(crate::tags::tag_spans(doc).into_iter().map(|(r, _)| r));
    out.sort_by_key(|r| r.start);
    out
}

/// Whether `range` overlaps any of `skip` (sorted by start).
#[must_use]
pub fn overlaps_any(range: &Range<usize>, skip: &[Range<usize>]) -> bool {
    let i = skip.partition_point(|r| r.start < range.end);
    skip[..i].iter().any(|r| r.end > range.start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(doc: &str, e: &Edit) -> String {
        format!("{}{}{}", &doc[..e.range.start], e.text, &doc[e.range.end..])
    }

    fn highlighted(doc: &str) -> Vec<&str> {
        highlight_spans(doc)
            .into_iter()
            .map(|(open, close)| &doc[open.end..close.start])
            .collect()
    }

    #[test]
    fn non_prose_covers_code_links_tags_urls_and_math() {
        let doc = "Teh `codez` and [[Linkk]] #tagg see https://exmaple.com $x^2$\n```\nfnn main\n```\nwrod";
        let skip = non_prose_ranges(doc);
        let covered = |word: &str| {
            let at = doc.find(word).expect("word");
            overlaps_any(&(at..at + word.len()), &skip)
        };
        for w in ["codez", "Linkk", "tagg", "exmaple", "x^2", "fnn"] {
            assert!(covered(w), "{w} is skipped");
        }
        for w in ["Teh", "wrod", "see"] {
            assert!(!covered(w), "{w} is prose");
        }
        // An unclosed fence runs to the end.
        assert!(non_prose_ranges("a\n```\nb").iter().any(|r| r.end == 7));
    }

    #[test]
    fn finds_highlights_like_obsidian() {
        assert_eq!(
            highlighted("a ==one== and ==two words== z"),
            ["one", "two words"]
        );
        assert_eq!(highlighted("==s==\nsfs"), ["s"]);
        // Spaces inside the markers, empty, unclosed, across lines: not highlights.
        assert!(highlighted("== no == ==== ==open\nclose==").is_empty());
        // Code is left alone.
        assert!(highlighted("`==code==`\n```\n==fenced==\n```").is_empty());
        assert_eq!(highlighted("x == y and ==real=="), ["real"]);
        assert_eq!(highlighted("é ==ünï== ü"), ["ünï"]);
    }

    #[test]
    fn bold_wraps_and_unwraps() {
        let doc = "make this bold";
        let e = toggle_inline(doc, 10..14, Inline::Bold);
        let doc2 = apply(doc, &e);
        assert_eq!(doc2, "make this **bold**");
        assert_eq!(
            &doc2[e.selection.clone()],
            "bold",
            "selection stays on the word"
        );
        let e = toggle_inline(&doc2, e.selection.clone(), Inline::Bold);
        assert_eq!(apply(&doc2, &e), doc, "toggling again removes it");
        let e = toggle_inline("x **b** y", 2..7, Inline::Bold);
        assert_eq!(
            apply("x **b** y", &e),
            "x b y",
            "selecting the markers too also unwraps"
        );
    }

    #[test]
    fn empty_selection_inserts_a_pair() {
        let e = toggle_inline("ab", 1..1, Inline::Italic);
        assert_eq!(apply("ab", &e), "a**b");
        assert_eq!(e.selection, 2..2, "caret between the markers");
    }

    #[test]
    fn headings_and_lists_on_lines() {
        let doc = "one\ntwo\nthree";
        let e = set_block(doc, 0..7, Block::Bullet);
        assert_eq!(apply(doc, &e), "- one\n- two\nthree");
        let e = set_block("- one\n- two", 0..11, Block::Bullet);
        assert_eq!(apply("- one\n- two", &e), "one\ntwo", "toggle off");
        let e = set_block("## Title", 3..3, Block::Heading(1));
        assert_eq!(apply("## Title", &e), "# Title", "switches heading level");
        let e = set_block("- item", 2..2, Block::Checklist);
        assert_eq!(apply("- item", &e), "- [ ] item", "list kinds convert");
        let e = set_block("a\nb", 0..3, Block::Numbered);
        assert_eq!(apply("a\nb", &e), "1. a\n2. b");
    }

    #[test]
    fn links_and_checkboxes() {
        let e = insert_link("see docs", 4..8);
        let out = apply("see docs", &e);
        assert_eq!(out, "see [docs](https://)");
        assert_eq!(e.selection.start, out.len() - 1, "caret inside the URL");
        let e = toggle_checkbox("- [ ] milk", 7).expect("checkbox");
        assert_eq!(apply("- [ ] milk", &e), "- [x] milk");
        assert!(toggle_checkbox("plain", 2).is_none());
    }

    #[test]
    fn never_splits_characters() {
        let doc = "café ☕";
        let e = toggle_inline(doc, 4..7, Inline::Bold); // 4 is inside "é"
        let out = apply(doc, &e);
        assert!(out.contains("**"));
        assert!(std::str::from_utf8(out.as_bytes()).is_ok());
    }
}
