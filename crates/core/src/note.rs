//! Notes: Markdown documents that can link to each other with `[[wiki links]]`.

use serde::{Deserialize, Serialize};

use crate::text::check_body;
use crate::{DomainError, DomainResult, NoteId, Timestamp, normalize_title};

/// Title used when the user leaves a note's title blank.
pub const UNTITLED: &str = "Untitled";

/// A Markdown note.
///
/// Lifecycle: `active` ⇄ `trashed` (soft delete). A trashed note is read-only
/// until restored. Permanent deletion is a storage concern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    id: NoteId,
    title: String,
    body: String,
    pinned: bool,
    created_at: Timestamp,
    updated_at: Timestamp,
    deleted_at: Option<Timestamp>,
}

impl Note {
    /// Creates a note. A blank title becomes [`UNTITLED`].
    pub fn new(title: &str, body: impl Into<String>, now: Timestamp) -> DomainResult<Self> {
        let body = body.into();
        check_body(&body)?;
        Ok(Self {
            id: NoteId::new(),
            title: note_title(title)?,
            body,
            pinned: false,
            created_at: now,
            updated_at: now,
            deleted_at: None,
        })
    }

    /// Rebuilds a note from storage without re-validating. Only storage should call this.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn from_parts(
        id: NoteId,
        title: String,
        body: String,
        pinned: bool,
        created_at: Timestamp,
        updated_at: Timestamp,
        deleted_at: Option<Timestamp>,
    ) -> Self {
        Self {
            id,
            title,
            body,
            pinned,
            created_at,
            updated_at,
            deleted_at,
        }
    }

    /// Replaces title and body. Returns `Ok(false)` when nothing changed, so
    /// callers can skip a write and keep `updated_at` stable.
    pub fn edit(&mut self, title: &str, body: &str, now: Timestamp) -> DomainResult<bool> {
        self.ensure_active()?;
        check_body(body)?;
        let title = note_title(title)?;
        if title == self.title && body == self.body {
            return Ok(false);
        }
        self.title = title;
        self.body = body.to_owned();
        self.touch(now);
        Ok(true)
    }

    pub fn set_pinned(&mut self, pinned: bool, now: Timestamp) -> DomainResult<()> {
        self.ensure_active()?;
        if self.pinned != pinned {
            self.pinned = pinned;
            self.touch(now);
        }
        Ok(())
    }

    /// Moves the note to the trash. Idempotent.
    pub fn trash(&mut self, now: Timestamp) {
        if self.deleted_at.is_none() {
            self.deleted_at = Some(now);
            self.touch(now);
        }
    }

    /// Restores a trashed note. Idempotent.
    pub fn restore(&mut self, now: Timestamp) {
        if self.deleted_at.take().is_some() {
            self.touch(now);
        }
    }

    #[must_use]
    pub fn id(&self) -> NoteId {
        self.id
    }
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
    #[must_use]
    pub fn is_pinned(&self) -> bool {
        self.pinned
    }
    #[must_use]
    pub fn created_at(&self) -> Timestamp {
        self.created_at
    }
    #[must_use]
    pub fn updated_at(&self) -> Timestamp {
        self.updated_at
    }
    #[must_use]
    pub fn deleted_at(&self) -> Option<Timestamp> {
        self.deleted_at
    }
    #[must_use]
    pub fn is_trashed(&self) -> bool {
        self.deleted_at.is_some()
    }

    /// First non-empty body line without Markdown heading markers, for list previews.
    #[must_use]
    pub fn preview(&self, max_chars: usize) -> String {
        preview_text(&self.body, max_chars)
    }

    fn ensure_active(&self) -> DomainResult<()> {
        if self.is_trashed() {
            Err(DomainError::Deleted)
        } else {
            Ok(())
        }
    }

    fn touch(&mut self, now: Timestamp) {
        // Never move backwards, even if the system clock does.
        self.updated_at = now.max(self.updated_at);
    }
}

/// Preview text for lists: the first non-empty, non-heading line (headings
/// usually repeat the title), falling back to the first heading. Markdown
/// heading markers are removed and the result is truncated with `…`.
#[must_use]
pub fn preview_text(body: &str, max_chars: usize) -> String {
    let mut lines = body.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.clone().next().unwrap_or("");
    let line = lines.find(|l| !l.starts_with('#')).unwrap_or(first);
    let line = plain_text(line.trim_start_matches('#').trim());
    let line = line.as_str();
    let mut out: String = line.chars().take(max_chars).collect();
    if line.chars().count() > max_chars {
        out.push('…');
    }
    out
}

/// Strips the Markdown a reader would not see: emphasis and code markers,
/// list bullets, and `[[link|alias]]` brackets (keeping the visible text).
fn plain_text(line: &str) -> String {
    let line = line
        .strip_prefix("- [ ] ")
        .or_else(|| line.strip_prefix("- [x] "))
        .or_else(|| line.strip_prefix("- "))
        .or_else(|| line.strip_prefix("* "))
        .or_else(|| line.strip_prefix("> "))
        .unwrap_or(line);
    let linked = crate::links::to_markdown_links(line, |_| String::new());
    // `[text]()` → `text`
    let unlinked = linked.replace("]()", "").replace("\\", "");
    let mut out = String::with_capacity(unlinked.len());
    let mut chars = unlinked.chars().peekable();
    let mut prev_bracket_open = true;
    while let Some(ch) = chars.next() {
        match ch {
            '*' | '`' | '~' => {}
            '_' if chars.peek() == Some(&'_') => {
                chars.next();
            }
            '[' if prev_bracket_open => {}
            _ => out.push(ch),
        }
        prev_bracket_open = ch.is_whitespace() || ch == '(';
    }
    out
}

fn note_title(raw: &str) -> DomainResult<String> {
    match normalize_title(raw) {
        Err(DomainError::EmptyTitle) => Ok(UNTITLED.to_owned()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs: i64) -> Timestamp {
        Timestamp::from_second(secs).expect("valid timestamp")
    }

    #[test]
    fn blank_title_becomes_untitled() {
        let note = Note::new("   ", "", t(1)).expect("note");
        assert_eq!(note.title(), UNTITLED);
    }

    #[test]
    fn edit_reports_changes_and_updates_timestamp() {
        let mut note = Note::new("A", "body", t(1)).expect("note");
        assert_eq!(note.edit("A", "body", t(5)), Ok(false));
        assert_eq!(note.updated_at(), t(1), "no-op edit keeps updated_at");

        assert_eq!(note.edit("B", "body", t(5)), Ok(true));
        assert_eq!(note.title(), "B");
        assert_eq!(note.updated_at(), t(5));
    }

    #[test]
    fn updated_at_never_goes_backwards() {
        let mut note = Note::new("A", "", t(10)).expect("note");
        note.edit("B", "", t(3)).expect("edit");
        assert_eq!(note.updated_at(), t(10));
    }

    #[test]
    fn trashed_note_is_read_only_until_restored() {
        let mut note = Note::new("A", "", t(1)).expect("note");
        note.trash(t(2));
        assert!(note.is_trashed());
        assert_eq!(note.edit("B", "", t(3)), Err(DomainError::Deleted));
        assert_eq!(note.set_pinned(true, t(3)), Err(DomainError::Deleted));

        note.restore(t(4));
        assert!(!note.is_trashed());
        assert_eq!(note.edit("B", "", t(5)), Ok(true));
    }

    #[test]
    fn trash_and_restore_are_idempotent() {
        let mut note = Note::new("A", "", t(1)).expect("note");
        note.trash(t(2));
        note.trash(t(9));
        assert_eq!(note.deleted_at(), Some(t(2)));
        note.restore(t(3));
        note.restore(t(9));
        assert_eq!(note.updated_at(), t(3));
    }

    #[test]
    fn preview_skips_headings_and_truncates() {
        let note = Note::new("A", "\n# Heading here\nbody text", t(1)).expect("note");
        assert_eq!(note.preview(100), "body text");
        assert_eq!(note.preview(4), "body…");
        let md = Note::new(
            "A",
            "- Renews in **May**. See [[Service log]] and [[Car|my car]] `now`",
            t(1),
        )
        .expect("note");
        assert_eq!(
            md.preview(100),
            "Renews in May. See Service log and my car now"
        );
        let only_heading = Note::new("A", "## Just a heading", t(1)).expect("note");
        assert_eq!(only_heading.preview(100), "Just a heading");
    }
}
