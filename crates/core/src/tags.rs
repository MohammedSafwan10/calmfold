//! `#tags` in note bodies (Obsidian style).
//!
//! A tag starts with `#` at the start of a word, then a letter, then letters,
//! digits, `_`, `-` or `/` (nested tags like `#work/clients`). `# Heading`
//! (with a space) is a heading, not a tag. Tags inside code are ignored.
//! Tags are compared case-insensitively and stored lower-case.

use std::ops::Range;

/// Every tag occurrence with its byte range (including `#`), in document order.
#[must_use]
pub fn tag_spans(body: &str) -> Vec<(Range<usize>, String)> {
    let mut spans = Vec::new();
    let mut fence: Option<&str> = None;
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let trimmed = line.trim_start();
        if let Some(marker) = fence {
            if trimmed.starts_with(marker) {
                fence = None;
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(&trimmed[..3]);
            continue;
        }
        scan(line, start, &mut spans);
    }
    spans
}

/// Distinct tags (lower-case, without `#`) in first-seen order.
#[must_use]
pub fn extract_tags(body: &str) -> Vec<String> {
    let mut tags: Vec<String> = Vec::new();
    for (_, tag) in tag_spans(body) {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    tags
}

/// Normalizes user input like `#Car`, ` car ` to the stored form `car`.
#[must_use]
pub fn normalize_tag(input: &str) -> String {
    input.trim().trim_start_matches('#').to_lowercase()
}

fn scan(line: &str, offset: usize, out: &mut Vec<(Range<usize>, String)>) {
    let mut in_code = false;
    let mut prev: Option<char> = None;
    let mut chars = line.char_indices().peekable();
    while let Some((i, ch)) = chars.next() {
        if ch == '`' {
            in_code = !in_code;
        } else if ch == '#'
            && !in_code
            && prev.is_none_or(|p| p.is_whitespace() || p == '(')
            && chars.peek().is_some_and(|(_, c)| c.is_alphabetic())
        {
            let mut end = i + 1;
            while let Some(&(j, c)) = chars.peek() {
                if c.is_alphanumeric() || matches!(c, '_' | '-' | '/') {
                    end = j + c.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let tag = line[i + 1..end].trim_end_matches(['/', '-']);
            let end = i + 1 + tag.len();
            out.push((offset + i..offset + end, tag.to_lowercase()));
            prev = line[..end].chars().last();
            continue;
        }
        prev = Some(ch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_tags_and_skips_headings_numbers_and_code() {
        let body = "# Heading\n#car notes #Work/Clients, (#idea) a#b #1 `#code`\n```\n#fenced\n```\n#last-";
        assert_eq!(extract_tags(body), ["car", "work/clients", "idea", "last"]);
    }

    #[test]
    fn spans_point_at_the_tag_text() {
        let body = "see #Car and #café";
        let spans = tag_spans(body);
        assert_eq!(&body[spans[0].0.clone()], "#Car");
        assert_eq!(&body[spans[1].0.clone()], "#café");
        assert_eq!(spans[1].1, "café");
    }

    #[test]
    fn duplicates_are_case_insensitive() {
        assert_eq!(extract_tags("#Car #car #CAR"), ["car"]);
        assert_eq!(normalize_tag(" #Car "), "car");
    }
}
