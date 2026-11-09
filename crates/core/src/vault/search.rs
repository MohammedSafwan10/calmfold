//! Vault search: plain words plus a few filter words, ranked.
//!
//! * Words match the title, username, websites, tags, cardholder and the
//!   labels and non-secret values of custom fields. Passwords, card numbers,
//!   notes and hidden fields are never searched.
//! * Every word must match something (AND). An item's score is its weakest
//!   word's score, so "git work" ranks GitHub-tagged-work above GitLab.
//! * Ranking per word: title starts with it (5) > a title word starts with it
//!   (4) > title contains it (3) > other fields (2).
//! * Typos: only when nothing matches exactly, the caller retries with
//!   [`Query::with_typos`]; then a word of 4+ letters (no digits) may be one
//!   edit away from a title word ("githb" → GitHub), scoring 1.
//! * Filter words: `tag:work` (or `#work`), `type:login|card|note`,
//!   `is:fav`, `is:weak`, `is:reused`, `is:old`, `has:code`, `site:github`.
//!   Unknown `x:y` words are searched as text.

use crate::vault::{FieldKind, ItemKind, VaultItem, host};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub words: Vec<String>,
    pub tags: Vec<String>,
    pub kind: Option<ItemKind>,
    pub sites: Vec<String>,
    pub favorite: bool,
    pub weak: bool,
    pub reused: bool,
    pub old: bool,
    pub has_code: bool,
    /// Allow one typo per word (the fallback pass).
    pub typos: bool,
}

impl Query {
    #[must_use]
    pub fn parse(input: &str) -> Self {
        let mut q = Self::default();
        for raw in input.split_whitespace() {
            let w = raw.to_lowercase();
            if let Some(tag) = w.strip_prefix('#').filter(|t| !t.is_empty()) {
                q.tags.push(tag.to_owned());
                continue;
            }
            let Some((key, value)) = w.split_once(':').filter(|(_, v)| !v.is_empty()) else {
                q.words.push(w);
                continue;
            };
            match (key, value) {
                ("tag", v) => q.tags.push(v.trim_start_matches('#').to_owned()),
                ("site" | "url", v) => q.sites.push(v.to_owned()),
                ("type", "login" | "logins" | "password") => q.kind = Some(ItemKind::Login),
                ("type", "card" | "cards") => q.kind = Some(ItemKind::Card),
                ("type", "note" | "notes") => q.kind = Some(ItemKind::Note),
                ("is", "fav" | "favorite" | "favourite" | "starred") => q.favorite = true,
                ("is", "weak") => q.weak = true,
                ("is", "reused") => q.reused = true,
                ("is", "old") => q.old = true,
                ("has", "code" | "otp" | "2fa" | "totp") => q.has_code = true,
                _ => q.words.push(w),
            }
        }
        q
    }

    /// The same query, tolerating one typo per word.
    #[must_use]
    pub fn with_typos(mut self) -> Self {
        self.typos = true;
        self
    }

    /// Whether the query uses any filter word (as opposed to plain text).
    #[must_use]
    pub fn has_filters(&self) -> bool {
        !self.tags.is_empty()
            || self.kind.is_some()
            || !self.sites.is_empty()
            || self.favorite
            || self.weak
            || self.reused
            || self.old
            || self.has_code
    }

    /// Score for `item` (higher is better), or `None` if it doesn't match.
    /// Health flags are checked by the caller (they need the whole vault).
    #[must_use]
    pub fn score(&self, item: &VaultItem) -> Option<u8> {
        let f = item.fields();
        if self.kind.is_some_and(|k| k != f.kind)
            || (self.favorite && !item.is_favorite())
            || (self.has_code && item.otp_setup().is_none())
        {
            return None;
        }
        if !self
            .tags
            .iter()
            .all(|t| f.tags.iter().any(|have| have.starts_with(t.as_str())))
        {
            return None;
        }
        let hosts: Vec<String> = f.urls.iter().map(|u| host(u).to_lowercase()).collect();
        if !self
            .sites
            .iter()
            .all(|s| hosts.iter().any(|h| h.contains(s.as_str())))
        {
            return None;
        }
        if self.words.is_empty() {
            return Some(1);
        }
        let title = f.title.to_lowercase();
        let title_words: Vec<&str> = title
            .split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .collect();
        let others: Vec<String> = [f.username.to_lowercase(), f.cardholder.to_lowercase()]
            .into_iter()
            .chain(hosts)
            .chain(f.tags.iter().cloned())
            .chain(f.custom.iter().flat_map(|c| {
                let label = c.label.to_lowercase();
                let value = c.kind.searchable().then(|| c.value.to_lowercase());
                std::iter::once(label).chain(value)
            }))
            .chain(
                f.custom
                    .iter()
                    .filter(|c| c.kind == FieldKind::Url)
                    .map(|c| host(&c.value).to_lowercase()),
            )
            .collect();
        let mut worst = u8::MAX;
        for w in &self.words {
            let s = if title.starts_with(w.as_str()) {
                5
            } else if title_words.iter().any(|t| t.starts_with(w.as_str())) {
                4
            } else if title.contains(w.as_str()) {
                3
            } else if others.iter().any(|o| o.contains(w.as_str())) {
                2
            } else if self.typos
                && w.chars().count() >= 4
                && w.chars().all(char::is_alphabetic)
                && title_words.iter().any(|t| one_typo(t, w))
            {
                1
            } else {
                return None;
            };
            worst = worst.min(s);
        }
        Some(worst)
    }
}

/// Whether `a` and `b` differ by at most one insertion, deletion,
/// substitution or swap of neighbours (Damerau–Levenshtein ≤ 1). Also true
/// when `b` is one typo from a *prefix* of `a` ("githb" → "github").
fn one_typo(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    // Compare b against a's prefix of similar length.
    (b.len().saturating_sub(1)..=b.len() + 1)
        .filter(|n| *n <= a.len() && *n > 0)
        .any(|n| distance_le_one(&a[..n], &b))
}

fn distance_le_one(a: &[char], b: &[char]) -> bool {
    if a == b {
        return true;
    }
    let (la, lb) = (a.len(), b.len());
    if la.abs_diff(lb) > 1 {
        return false;
    }
    let first = a
        .iter()
        .zip(b)
        .position(|(x, y)| x != y)
        .unwrap_or(la.min(lb));
    if la == lb {
        // Substitution, or a swap of neighbours.
        a[first + 1..] == b[first + 1..]
            || (first + 1 < la
                && a[first] == b[first + 1]
                && a[first + 1] == b[first]
                && a[first + 2..] == b[first + 2..])
    } else if la > lb {
        a[first + 1..] == b[first..]
    } else {
        a[first..] == b[first + 1..]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::{CustomField, ItemFields};
    use jiff::Timestamp;

    fn item(title: &str, user: &str, url: &str, tags: &[&str]) -> VaultItem {
        let f = ItemFields {
            title: title.into(),
            username: user.into(),
            password: "secret-password".into(),
            urls: vec![url.into()],
            tags: tags.iter().map(|t| (*t).to_owned()).collect(),
            ..ItemFields::new(ItemKind::Login)
        };
        VaultItem::new(&f, Timestamp::from_second(1).expect("ts")).expect("item")
    }

    #[test]
    fn parses_filter_words() {
        let q = Query::parse("git tag:Work #home type:card is:fav has:2fa site:github.com foo:bar");
        assert_eq!(q.words, ["git", "foo:bar"]);
        assert_eq!(q.tags, ["work", "home"]);
        assert_eq!(q.kind, Some(ItemKind::Card));
        assert!(q.favorite && q.has_code && q.has_filters());
        assert_eq!(q.sites, ["github.com"]);
    }

    #[test]
    fn ranks_and_requires_every_word() {
        let gh = item("GitHub", "sam", "https://github.com", &["work"]);
        let gl = item("GitLab", "sam", "https://gitlab.com", &[]);
        let q = Query::parse("git work");
        assert!(q.score(&gh).is_some());
        assert_eq!(q.score(&gl), None, "every word must match");
        assert_eq!(Query::parse("git").score(&gh), Some(5));
        assert_eq!(Query::parse("hub").score(&gh), Some(3));
        assert_eq!(Query::parse("sam").score(&gh), Some(2));
        assert_eq!(Query::parse("githb").score(&gh), None, "exact pass first");
        assert_eq!(
            Query::parse("githb").with_typos().score(&gh),
            Some(1),
            "one typo"
        );
        let site = item("Site 1999", "", "", &[]);
        assert_eq!(
            Query::parse("1998").with_typos().score(&site),
            None,
            "numbers stay exact"
        );
        assert_eq!(
            Query::parse("gti").score(&gh),
            None,
            "short words need an exact match"
        );
        assert_eq!(
            Query::parse("secret").score(&gh),
            None,
            "passwords are never searched"
        );
        assert_eq!(Query::parse("tag:wo").score(&gh), Some(1));
        assert_eq!(Query::parse("tag:work").score(&gl), None);
        assert_eq!(Query::parse("site:gitlab").score(&gl), Some(1));
    }

    #[test]
    fn custom_fields_search_labels_and_plain_values_only() {
        let mut f = ItemFields::new(ItemKind::Note);
        f.title = "Home".into();
        f.custom = vec![
            CustomField {
                kind: FieldKind::Text,
                label: "Network".into(),
                value: "Rivera-5G".into(),
            },
            CustomField {
                kind: FieldKind::Secret,
                label: "Password".into(),
                value: "plum orbit".into(),
            },
        ];
        let note = VaultItem::new(&f, Timestamp::from_second(1).expect("ts")).expect("note");
        assert!(Query::parse("rivera").score(&note).is_some());
        assert!(Query::parse("network").score(&note).is_some());
        assert_eq!(
            Query::parse("orbit").score(&note),
            None,
            "hidden values aren't searched"
        );
    }

    #[test]
    fn typo_distance() {
        assert!(distance_le_one(&['a', 'b'], &['b', 'a']));
        assert!(one_typo("netflix", "netflx"));
        assert!(one_typo("amazon", "amzaon"));
        assert!(!one_typo("amazon", "zzzzz"));
    }
}
