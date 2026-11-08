//! Obsidian-style wiki-link extraction.
//!
//! Supported forms: `[[Target]]`, `[[Target|Alias]]`, `[[Target#Heading]]`,
//! `[[Target#Heading|Alias]]` and embeds `![[Target]]`. Links inside fenced
//! code blocks (```` ``` ```` / `~~~`) and inline code spans are ignored.
//!
//! Targets are matched to note titles case-insensitively, the same as Obsidian.

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
    let mut links = Vec::new();
    let mut fence: Option<&str> = None;

    for line in body.lines() {
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
        scan_line(line, &mut links);
    }
    links
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

fn scan_line(line: &str, out: &mut Vec<WikiLink>) {
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
                        if let Some(link) = parse_inner(&line[start..start + len], embed) {
                            out.push(link);
                        }
                        i = start + len + 2;
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
    fn unique_targets_are_case_insensitive() {
        assert_eq!(
            unique_targets("[[Car]] [[car]] [[ Car  Notes ]]"),
            vec!["car", "car notes"]
        );
    }
}
