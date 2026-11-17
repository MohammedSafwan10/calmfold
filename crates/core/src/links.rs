//! Obsidian-style wiki-link extraction.
//!
//! Supported forms: `[[Target]]`, `[[Target|Alias]]`, `[[Target#Heading]]`,
//! `[[Target#Heading|Alias]]` and embeds `![[Target]]`. Links inside fenced
//! code blocks (```` ``` ```` / `~~~`) and inline code spans are ignored.
//!
//! Targets are matched to note titles case-insensitively, the same as Obsidian.

use std::ops::Range;

/// One `[[...]]` occurrence in a note body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiLink {
    pub target: String,
    pub heading: Option<String>,
    pub alias: Option<String>,
    pub embed: bool,
}

/// Returns every wiki link in `body`, in document order.
#[must_use]
pub fn extract_wiki_links(body: &str) -> Vec<WikiLink> {
    wiki_link_spans(body)
        .into_iter()
        .map(|(_, link)| link)
        .collect()
}

/// Every wiki link with the byte range it occupies in `body` (including a
/// leading `!` for embeds and both bracket pairs).
#[must_use]
pub fn wiki_link_spans(body: &str) -> Vec<(Range<usize>, WikiLink)> {
    let mut spans = Vec::new();
    let mut fence: Option<&str> = None;
    let mut line_start = 0;

    for line in body.split_inclusive('\n') {
        let offset = line_start;
        line_start += line.len();
        let trimmed = line.trim_start();
        if let Some(marker) = fence {
            if trimmed.starts_with(marker) {
                fence = None;
            }
            continue;
        }
        if trimmed.starts_with("```") {
            fence = Some("```");
            continue;
        }
        if trimmed.starts_with("~~~") {
            fence = Some("~~~");
            continue;
        }
        scan_line(line, offset, &mut spans);
    }
    spans
}

/// Rewrites wiki links as ordinary Markdown links so a renderer can show and
/// route them: `[[Target|Alias]]` becomes `[Alias](<url_for(target)>)`.
/// Text outside links, and links inside code, is left exactly as it was.
#[must_use]
pub fn to_markdown_links(body: &str, url_for: impl Fn(&WikiLink) -> String) -> String {
    let mut out = String::with_capacity(body.len() + 32);
    let mut last = 0;
    for (range, link) in wiki_link_spans(body) {
        out.push_str(&body[last..range.start]);
        let text = link.alias.clone().unwrap_or_else(|| match &link.heading {
            Some(h) => format!("{} › {h}", link.target),
            None => link.target.clone(),
        });
        out.push('[');
        for ch in text.chars() {
            if matches!(ch, '[' | ']' | '\\' | '*' | '_' | '`') {
                out.push('\\');
            }
            out.push(ch);
        }
        out.push_str("](");
        out.push_str(&url_for(&link));
        out.push(')');
        last = range.end;
    }
    out.push_str(&body[last..]);
    out
}

/// Distinct link targets, lower-cased for case-insensitive matching, in first-seen order.
#[must_use]
pub fn unique_targets(body: &str) -> Vec<String> {
    let mut seen = Vec::<String>::new();
    for link in extract_wiki_links(body) {
        let key = normalize_target(&link.target);
        if !seen.contains(&key) {
            seen.push(key);
        }
    }
    seen
}

/// The key used to match a link target against a note title.
#[must_use]
pub fn normalize_target(target: &str) -> String {
    target
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// URL scheme used for in-app note links in rendered Markdown.
pub const NOTE_LINK_PREFIX: &str = "ottero://note/";

/// `ottero://note/<percent-encoded target>`.
#[must_use]
pub fn note_link_url(target: &str) -> String {
    let mut url = String::from(NOTE_LINK_PREFIX);
    for byte in target.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            url.push(char::from(*byte));
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}

/// Inverse of [`note_link_url`]. `None` for other URLs or invalid encoding.
#[must_use]
pub fn parse_note_link_url(url: &str) -> Option<String> {
    let encoded = url.strip_prefix(NOTE_LINK_PREFIX)?;
    let bytes = encoded.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = encoded.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    let target = String::from_utf8(out).ok()?;
    (!target.trim().is_empty()).then_some(target)
}

/// If the cursor sits inside an unfinished `[[...` on its line, returns the
/// byte offset just after `[[` and the text typed so far. Used for autocomplete.
#[must_use]
pub fn open_link_query(text: &str, cursor: usize) -> Option<(usize, &str)> {
    let before = text.get(..cursor)?;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    let line = &before[line_start..];
    let open = line.rfind("[[")?;
    let query = &line[open + 2..];
    if query.contains("]]") || query.contains(['[', ']', '|', '#']) {
        return None;
    }
    Some((line_start + open + 2, query))
}

fn scan_line(line: &str, offset: usize, out: &mut Vec<(Range<usize>, WikiLink)>) {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut in_code = false;

    while i < bytes.len() {
        match bytes[i] {
            b'`' => {
                in_code = !in_code;
                i += 1;
            }
            b'[' if !in_code && bytes.get(i + 1) == Some(&b'[') => {
                let embed = i > 0 && bytes[i - 1] == b'!';
                let start = i + 2;
                // `[[` and `]]` are ASCII, so these byte offsets are char boundaries.
                match line[start..].find("]]") {
                    Some(len) => {
                        let end = start + len + 2;
                        if let Some(link) = parse_inner(&line[start..start + len], embed) {
                            let from = if embed { i - 1 } else { i };
                            out.push((offset + from..offset + end, link));
                        }
                        i = end;
                    }
                    None => return,
                }
            }
            _ => i += 1,
        }
    }
}

fn parse_inner(inner: &str, embed: bool) -> Option<WikiLink> {
    if inner.contains('[') {
        return None;
    }
    let (reference, alias) = match inner.split_once('|') {
        Some((r, a)) => (r, non_empty(a)),
        None => (inner, None),
    };
    let (target, heading) = match reference.split_once('#') {
        Some((t, h)) => (t, non_empty(h)),
        None => (reference, None),
    };
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    Some(WikiLink {
        target: target.to_owned(),
        heading,
        alias,
        embed,
    })
}

fn non_empty(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(body: &str) -> Vec<String> {
        extract_wiki_links(body)
            .into_iter()
            .map(|l| l.target)
            .collect()
    }

    #[test]
    fn parses_all_link_forms() {
        let links = extract_wiki_links("See [[Car]], [[Car#Insurance|policy]] and ![[diagram]].");
        assert_eq!(
            links,
            vec![
                WikiLink {
                    target: "Car".into(),
                    heading: None,
                    alias: None,
                    embed: false
                },
                WikiLink {
                    target: "Car".into(),
                    heading: Some("Insurance".into()),
                    alias: Some("policy".into()),
                    embed: false,
                },
                WikiLink {
                    target: "diagram".into(),
                    heading: None,
                    alias: None,
                    embed: true
                },
            ]
        );
    }

    #[test]
    fn ignores_code_blocks_and_inline_code() {
        let body = "real [[A]]\n```\n[[B]]\n```\n`[[C]]` and [[D]]\n~~~\n[[E]]\n~~~";
        assert_eq!(targets(body), vec!["A", "D"]);
    }

    #[test]
    fn ignores_malformed_links() {
        assert!(targets("[[]] [[ | x]] [[unclosed").is_empty());
        assert!(targets("[[a [[b]]").is_empty());
    }

    #[test]
    fn handles_unicode_around_links() {
        assert_eq!(
            targets("日本語 [[ノート]] émoji 🚗 [[Café]]"),
            vec!["ノート", "Café"]
        );
    }

    #[test]
    fn spans_cover_the_whole_link_including_embed_bang() {
        let body = "a [[X]] b\n![[Y|y]]";
        let spans = wiki_link_spans(body);
        assert_eq!(&body[spans[0].0.clone()], "[[X]]");
        assert_eq!(&body[spans[1].0.clone()], "![[Y|y]]");
    }

    #[test]
    fn rewrites_links_for_rendering_and_leaves_code_alone() {
        let body = "See [[Car]], [[Car#Service|the service]] and ![[A_b]].\n```\n[[Code]]\n```\n`[[Inline]]`";
        let out = to_markdown_links(body, |l| note_link_url(&l.target));
        assert_eq!(
            out,
            "See [Car](ottero://note/Car), [the service](ottero://note/Car) and [A\\_b](ottero://note/A_b).\n```\n[[Code]]\n```\n`[[Inline]]`"
        );
    }

    #[test]
    fn note_urls_round_trip_unicode_and_spaces() {
        for target in ["Service log", "Café ☕", "a/b?c#d", "日本語"] {
            let url = note_link_url(target);
            assert!(!url.contains(' '));
            assert_eq!(parse_note_link_url(&url).as_deref(), Some(target));
        }
        assert_eq!(parse_note_link_url("https://example.com"), None);
        assert_eq!(parse_note_link_url("ottero://note/%ZZ"), None);
        assert_eq!(parse_note_link_url("ottero://note/"), None);
    }

    #[test]
    fn open_link_query_detects_unfinished_links() {
        let t = "hi [[Ser";
        assert_eq!(open_link_query(t, t.len()), Some((5, "Ser")));
        assert_eq!(open_link_query("[[", 2), Some((2, "")));
        assert_eq!(open_link_query("[[Done]] x", 10), None);
        assert_eq!(open_link_query("[[a|b", 5), None);
        assert_eq!(open_link_query("[[a\nb", 5), None, "stops at line breaks");
        assert_eq!(
            open_link_query("日本 [[ノ", "日本 [[ノ".len()),
            Some(("日本 [[".len(), "ノ"))
        );
        assert_eq!(open_link_query("abc", 99), None);
    }

    #[test]
    fn unique_targets_are_case_insensitive() {
        assert_eq!(
            unique_targets("[[Car]] [[car]] [[ Car  Notes ]]"),
            vec!["car", "car notes"]
        );
    }
}
