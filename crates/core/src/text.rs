use crate::{DomainError, DomainResult};

/// Longest title we accept, counted in Unicode scalar values.
pub const MAX_TITLE_CHARS: usize = 300;

/// Largest note or task body we accept (2 MiB). Keeps a single paste from
/// freezing the editor or bloating the database.
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Trims a title, collapses internal whitespace runs (including newlines) to a
/// single space, and enforces the length limit.
pub fn normalize_title(raw: &str) -> DomainResult<String> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return Err(DomainError::EmptyTitle);
    }
    if collapsed.chars().count() > MAX_TITLE_CHARS {
        return Err(DomainError::TitleTooLong {
            max: MAX_TITLE_CHARS,
        });
    }
    Ok(collapsed)
}

pub(crate) fn check_body(body: &str) -> DomainResult<()> {
    if body.len() > MAX_BODY_BYTES {
        return Err(DomainError::BodyTooLarge {
            max_bytes: MAX_BODY_BYTES,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(
            normalize_title("  Buy \n milk\t now ").as_deref(),
            Ok("Buy milk now")
        );
    }

    #[test]
    fn rejects_blank_titles() {
        assert_eq!(normalize_title(" \n\t "), Err(DomainError::EmptyTitle));
    }

    #[test]
    fn counts_characters_not_bytes() {
        let ok = "é".repeat(MAX_TITLE_CHARS);
        assert!(normalize_title(&ok).is_ok());
        let too_long = "é".repeat(MAX_TITLE_CHARS + 1);
        assert!(matches!(
            normalize_title(&too_long),
            Err(DomainError::TitleTooLong { .. })
        ));
    }

    #[test]
    fn rejects_oversized_bodies() {
        assert!(check_body(&"a".repeat(MAX_BODY_BYTES)).is_ok());
        assert!(check_body(&"a".repeat(MAX_BODY_BYTES + 1)).is_err());
    }
}
