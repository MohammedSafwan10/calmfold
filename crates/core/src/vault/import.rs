//! CSV import from other password managers.
//!
//! The format is recognized from the header row:
//! * Bitwarden: `folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp`
//! * 1Password: `Title,Url,Username,Password,OTPAuth,Favorite,Archived,Tags,Notes`
//! * Chrome / Edge / Brave / Safari: `name,url,username,password,note` (Safari: `Title,URL,Username,Password,Notes,OTPAuth`)
//!
//! Rows that can't become an item (no title and no site) are skipped and
//! counted, never fatal. A bad one-time-code field drops just that field.

use std::collections::HashMap;

use crate::vault::{ItemFields, ItemKind, host};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Bitwarden,
    OnePassword,
    Browser,
}

impl Source {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Bitwarden => "Bitwarden",
            Self::OnePassword => "1Password",
            Self::Browser => "browser",
        }
    }
}

/// What a CSV turned into. `fields` hold secrets; drop them promptly.
#[derive(Debug, Default)]
pub struct Parsed {
    pub source: Option<Source>,
    pub items: Vec<(ItemFields, bool)>,
    pub skipped: usize,
}

/// Largest file we read (keeps a wrong file from freezing the app).
pub const MAX_IMPORT_BYTES: usize = 32 * 1024 * 1024;

/// Parses an export. `Err` only when the header isn't a known format.
pub fn parse_csv(text: &str) -> Result<Parsed, &'static str> {
    if text.len() > MAX_IMPORT_BYTES {
        return Err("That file is too large to import.");
    }
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(text.trim_start_matches('\u{feff}').as_bytes());
    let headers: Vec<String> = reader
        .headers()
        .map_err(|_| "That file isn't a CSV export.")?
        .iter()
        .map(|h| h.trim().to_lowercase())
        .collect();
    let col: HashMap<&str, usize> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| (h.as_str(), i))
        .collect();
    let has = |k: &str| col.contains_key(k);
    let source = if has("login_password") && has("name") {
        Source::Bitwarden
    } else if has("title") && has("password") && (has("otpauth") || has("tags") || has("url")) {
        Source::OnePassword
    } else if has("password") && (has("url") || has("name")) {
        Source::Browser
    } else {
        return Err(
            "Ottero didn't recognize this CSV. Export from Bitwarden, 1Password or your browser.",
        );
    };

    let mut out = Parsed {
        source: Some(source),
        ..Parsed::default()
    };
    for record in reader.records() {
        let Ok(record) = record else {
            out.skipped += 1;
            continue;
        };
        let get = |k: &str| {
            col.get(k)
                .and_then(|i| record.get(*i))
                .map(|v| v.trim().to_owned())
                .unwrap_or_default()
        };
        let (mut f, favorite) = match source {
            Source::Bitwarden => {
                let kind = match get("type").as_str() {
                    "note" => ItemKind::Note,
                    "card" => ItemKind::Card,
                    _ => ItemKind::Login,
                };
                let mut f = ItemFields::new(kind);
                f.title = get("name");
                f.username = get("login_username");
                f.password = get("login_password");
                f.urls = split_urls(&get("login_uri"));
                f.totp = get("login_totp");
                f.notes = get("notes");
                let folder = get("folder");
                if !folder.is_empty() {
                    f.tags = vec![folder];
                }
                (f, get("favorite") == "1")
            }
            Source::OnePassword => {
                let mut f = ItemFields::new(ItemKind::Login);
                f.title = get("title");
                f.username = get("username");
                f.password = get("password");
                f.urls = split_urls(&get("url"));
                f.totp = get("otpauth");
                f.notes = get("notes");
                f.tags = get("tags")
                    .split([',', ';'])
                    .map(|t| t.trim().to_owned())
                    .filter(|t| !t.is_empty())
                    .collect();
                let fav = matches!(
                    get("favorite").to_lowercase().as_str(),
                    "true" | "1" | "yes"
                );
                (f, fav)
            }
            Source::Browser => {
                let mut f = ItemFields::new(ItemKind::Login);
                f.title = if has("name") {
                    get("name")
                } else {
                    get("title")
                };
                f.username = get("username");
                f.password = get("password");
                f.urls = split_urls(&get("url"));
                f.notes = if has("note") {
                    get("note")
                } else {
                    get("notes")
                };
                f.totp = get("otpauth");
                (f, false)
            }
        };
        if f.title.trim().is_empty() {
            f.title = f.urls.first().map(|u| host(u)).unwrap_or_default();
        }
        if f.title.trim().is_empty() {
            out.skipped += 1;
            continue;
        }
        if !f.totp.is_empty() && crate::vault::totp::parse(&f.totp).is_err() {
            f.totp.clear();
        }
        out.items.push((f, favorite));
    }
    Ok(out)
}

fn split_urls(s: &str) -> Vec<String> {
    s.split([',', '\n'])
        .map(|u| u.trim().to_owned())
        .filter(|u| !u.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_bitwarden() {
        let csv = "folder,favorite,type,name,notes,fields,reprompt,login_uri,login_username,login_password,login_totp\n\
                   Work,1,login,GitHub,,,0,https://github.com,sam,pw1,\n\
                   ,,note,Wi-Fi,\"code 1234\",,0,,,,\n\
                   ,,login,,,,0,,,,\n";
        let p = parse_csv(csv).expect("parse");
        assert_eq!(p.source, Some(Source::Bitwarden));
        assert_eq!(p.items.len(), 2);
        assert_eq!(p.skipped, 1, "no title, no site");
        let (gh, fav) = &p.items[0];
        assert_eq!(
            (
                gh.title.as_str(),
                gh.username.as_str(),
                gh.password.as_str()
            ),
            ("GitHub", "sam", "pw1")
        );
        assert!(*fav);
        assert_eq!(gh.tags, ["Work"]);
        assert_eq!(p.items[1].0.kind, ItemKind::Note);
    }

    #[test]
    fn reads_1password_and_browsers() {
        let op = "Title,Url,Username,Password,OTPAuth,Favorite,Archived,Tags,Notes\n\
                  Bank,https://bank.com,me,pw,not-a-code,true,false,\"money, home\",hi\n";
        let p = parse_csv(op).expect("parse");
        assert_eq!(p.source, Some(Source::OnePassword));
        let (bank, fav) = &p.items[0];
        assert!(bank.totp.is_empty(), "bad code dropped, item kept");
        assert_eq!(bank.tags, ["money", "home"]);
        assert!(*fav);

        let chrome =
            "\u{feff}name,url,username,password,note\n,https://www.example.com/login,me,pw,\n";
        let p = parse_csv(chrome).expect("parse");
        assert_eq!(p.source, Some(Source::Browser));
        assert_eq!(p.items[0].0.title, "example.com", "titled from the site");
        assert!(parse_csv("a,b\n1,2\n").is_err());
    }
}
