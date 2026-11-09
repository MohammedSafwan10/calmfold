use calmfold_core::links::{normalize_target, unique_targets};
use calmfold_core::note::preview_text;
use calmfold_core::tags::{extract_tags, normalize_tag};
use calmfold_core::{Note, NoteId, Timestamp};
use rusqlite::{OptionalExtension, Row, params};

use crate::codec::{fts_query, opt_ts_from_sql, ts_from_sql, ts_to_sql, uuid_from_sql};
use crate::{StorageResult, Store};

/// Lightweight row for note lists. Never loads the full body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSummary {
    pub id: NoteId,
    pub title: String,
    pub preview: String,
    pub pinned: bool,
    pub updated_at: Timestamp,
}

/// Characters of body read for list previews.
const PREVIEW_SOURCE_CHARS: i64 = 400;
const PREVIEW_CHARS: usize = 120;

const SUMMARY_COLUMNS: &str = "n.id, n.title, substr(n.body, 1, ?1), n.pinned, n.updated_at";

impl Store {
    /// Inserts or updates a note and refreshes its outgoing links atomically.
    pub fn save_note(&mut self, note: &Note) -> StorageResult<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO notes (id, title, body, pinned, created_at, updated_at, deleted_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (id) DO UPDATE SET
                title = excluded.title,
                body = excluded.body,
                pinned = excluded.pinned,
                updated_at = excluded.updated_at,
                deleted_at = excluded.deleted_at",
            params![
                note.id().to_string(),
                note.title(),
                note.body(),
                note.is_pinned(),
                ts_to_sql(note.created_at()),
                ts_to_sql(note.updated_at()),
                note.deleted_at().map(ts_to_sql),
            ],
        )?;
        let id = note.id().to_string();
        tx.execute("DELETE FROM note_links WHERE source_id = ?1", [&id])?;
        {
            let mut insert = tx.prepare(
                "INSERT OR IGNORE INTO note_links (source_id, target_key) VALUES (?1, ?2)",
            )?;
            for target in unique_targets(note.body()) {
                insert.execute(params![id, target])?;
            }
        }
        tx.execute("DELETE FROM note_tags WHERE note_id = ?1", [&id])?;
        {
            let mut insert =
                tx.prepare("INSERT OR IGNORE INTO note_tags (note_id, tag) VALUES (?1, ?2)")?;
            for tag in extract_tags(note.body()) {
                insert.execute(params![id, tag])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn note(&self, id: NoteId) -> StorageResult<Option<Note>> {
        self.conn
            .query_row(
                "SELECT id, title, body, pinned, created_at, updated_at, deleted_at
                 FROM notes WHERE id = ?1",
                [id.to_string()],
                raw_note,
            )
            .optional()?
            .map(RawNote::into_note)
            .transpose()
    }

    /// Active notes (or trashed ones), pinned first, most recently edited first.
    pub fn list_notes(&self, trashed: bool) -> StorageResult<Vec<NoteSummary>> {
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS} FROM notes n
             WHERE (n.deleted_at IS NOT NULL) = ?2
             ORDER BY n.pinned DESC, n.updated_at DESC"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params![PREVIEW_SOURCE_CHARS, trashed], raw_summary)?;
        collect_summaries(rows)
    }

    /// Full-text search over title and body of active notes, best match first.
    pub fn search_notes(&self, query: &str, limit: usize) -> StorageResult<Vec<NoteSummary>> {
        let Some(fts) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS} FROM notes_fts f
             JOIN notes n ON n.rowid = f.rowid
             WHERE notes_fts MATCH ?2 AND n.deleted_at IS NULL
             ORDER BY bm25(notes_fts, 5.0, 1.0)
             LIMIT ?3"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt.query_map(params![PREVIEW_SOURCE_CHARS, fts, limit], raw_summary)?;
        collect_summaries(rows)
    }

    /// Active notes that contain a `[[link]]` to `title` (case-insensitive).
    pub fn backlinks(&self, title: &str) -> StorageResult<Vec<NoteSummary>> {
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS} FROM note_links l
             JOIN notes n ON n.id = l.source_id
             WHERE l.target_key = ?2 AND n.deleted_at IS NULL
             ORDER BY n.updated_at DESC"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(
            params![PREVIEW_SOURCE_CHARS, normalize_target(title)],
            raw_summary,
        )?;
        collect_summaries(rows)
    }

    /// Tags used by active notes, with how many notes use each, A→Z.
    pub fn note_tags(&self) -> StorageResult<Vec<(String, usize)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT t.tag, count(*) FROM note_tags t JOIN notes n ON n.id = t.note_id
             WHERE n.deleted_at IS NULL GROUP BY t.tag ORDER BY t.tag",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
        rows.map(|row| {
            let (tag, n) = row?;
            Ok((tag, usize::try_from(n).unwrap_or(0)))
        })
        .collect()
    }

    /// Active notes tagged `tag` (case-insensitive, `#` optional), pinned first, newest first.
    pub fn notes_with_tag(&self, tag: &str) -> StorageResult<Vec<NoteSummary>> {
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS} FROM note_tags t JOIN notes n ON n.id = t.note_id
             WHERE t.tag = ?2 AND n.deleted_at IS NULL
             ORDER BY n.pinned DESC, n.updated_at DESC"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(
            params![PREVIEW_SOURCE_CHARS, normalize_tag(tag)],
            raw_summary,
        )?;
        collect_summaries(rows)
    }

    /// IDs of active notes tagged `tag`.
    pub fn note_ids_with_tag(&self, tag: &str) -> StorageResult<Vec<NoteId>> {
        Ok(self
            .notes_with_tag(tag)?
            .into_iter()
            .map(|n| n.id)
            .collect())
    }

    /// Active notes that mention `title` as plain text but don't link to it
    /// (Obsidian's "unlinked mentions"). Excludes the note itself.
    pub fn unlinked_mentions(
        &self,
        note: NoteId,
        title: &str,
        limit: usize,
    ) -> StorageResult<Vec<NoteSummary>> {
        let phrase = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if phrase.chars().count() < 3 {
            return Ok(Vec::new()); // too short to be meaningful
        }
        let fts = format!("body : \"{}\"", phrase.replace('"', "\"\""));
        let sql = format!(
            "SELECT {SUMMARY_COLUMNS} FROM notes_fts f JOIN notes n ON n.rowid = f.rowid
             WHERE notes_fts MATCH ?2 AND n.deleted_at IS NULL AND n.id != ?3
               AND NOT EXISTS (SELECT 1 FROM note_links l WHERE l.source_id = n.id AND l.target_key = ?4)
             ORDER BY n.updated_at DESC LIMIT ?5"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = stmt.query_map(
            params![
                PREVIEW_SOURCE_CHARS,
                fts,
                note.to_string(),
                normalize_target(title),
                limit
            ],
            raw_summary,
        )?;
        collect_summaries(rows)
    }

    /// Full bodies of all active notes (for export).
    pub fn all_notes(&self) -> StorageResult<Vec<Note>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, title, body, pinned, created_at, updated_at, deleted_at
             FROM notes WHERE deleted_at IS NULL ORDER BY title COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], raw_note)?.collect::<Vec<_>>();
        rows.into_iter().map(|r| r?.into_note()).collect()
    }

    /// Finds an active note whose title matches a link target. Matching is
    /// whitespace- and case-insensitive with full Unicode case folding (done
    /// in Rust, because SQLite's `lower()` only folds ASCII). Most recently
    /// edited wins when titles collide.
    pub fn find_note_by_title(&self, title: &str) -> StorageResult<Option<NoteId>> {
        let key = normalize_target(title);
        Ok(self
            .active_titles()?
            .into_iter()
            .find(|(_, t)| normalize_target(t) == key)
            .map(|(id, _)| id))
    }

    /// Note titles for `[[` autocomplete: titles starting with `query` first,
    /// then titles containing it, each group most recently edited first.
    pub fn suggest_note_titles(
        &self,
        query: &str,
        limit: usize,
    ) -> StorageResult<Vec<(NoteId, String)>> {
        let key = normalize_target(query);
        let mut prefix = Vec::new();
        let mut contains = Vec::new();
        for (id, title) in self.active_titles()? {
            let norm = normalize_target(&title);
            if norm.starts_with(&key) {
                prefix.push((id, title));
            } else if norm.contains(&key) {
                contains.push((id, title));
            }
        }
        prefix.extend(contains);
        prefix.truncate(limit);
        Ok(prefix)
    }

    /// (id, title) of active notes, most recently edited first.
    fn active_titles(&self) -> StorageResult<Vec<(NoteId, String)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, title FROM notes WHERE deleted_at IS NULL ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.map(|row| {
            let (id, title) = row?;
            Ok((NoteId::from_uuid(uuid_from_sql(&id, "notes.id")?), title))
        })
        .collect()
    }

    /// Removes a note forever. Links from it are removed by cascade.
    pub fn delete_note_permanently(&mut self, id: NoteId) -> StorageResult<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM notes WHERE id = ?1", [id.to_string()])?
            > 0)
    }

    /// Permanently deletes every trashed note. Returns how many were removed.
    pub fn empty_note_trash(&mut self) -> StorageResult<usize> {
        Ok(self
            .conn
            .execute("DELETE FROM notes WHERE deleted_at IS NOT NULL", [])?)
    }

    pub fn count_notes(&self) -> StorageResult<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM notes WHERE deleted_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(n).unwrap_or(0))
    }
}

struct RawNote {
    id: String,
    title: String,
    body: String,
    pinned: bool,
    created_at: i64,
    updated_at: i64,
    deleted_at: Option<i64>,
}

impl RawNote {
    fn into_note(self) -> StorageResult<Note> {
        Ok(Note::from_parts(
            NoteId::from_uuid(uuid_from_sql(&self.id, "notes.id")?),
            self.title,
            self.body,
            self.pinned,
            ts_from_sql(self.created_at, "notes.created_at")?,
            ts_from_sql(self.updated_at, "notes.updated_at")?,
            opt_ts_from_sql(self.deleted_at, "notes.deleted_at")?,
        ))
    }
}

fn raw_note(row: &Row<'_>) -> rusqlite::Result<RawNote> {
    Ok(RawNote {
        id: row.get(0)?,
        title: row.get(1)?,
        body: row.get(2)?,
        pinned: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        deleted_at: row.get(6)?,
    })
}

type RawSummary = (String, String, String, bool, i64);

fn raw_summary(row: &Row<'_>) -> rusqlite::Result<RawSummary> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
    ))
}

fn collect_summaries(
    rows: impl Iterator<Item = rusqlite::Result<RawSummary>>,
) -> StorageResult<Vec<NoteSummary>> {
    rows.map(|row| {
        let (id, title, body_head, pinned, updated_at) = row?;
        Ok(NoteSummary {
            id: NoteId::from_uuid(uuid_from_sql(&id, "notes.id")?),
            title,
            preview: preview(&body_head),
            pinned,
            updated_at: ts_from_sql(updated_at, "notes.updated_at")?,
        })
    })
    .collect()
}

fn preview(body_head: &str) -> String {
    preview_text(body_head, PREVIEW_CHARS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use calmfold_core::clock;

    fn store() -> Store {
        Store::open_in_memory().expect("store")
    }

    fn at(secs: i64) -> Timestamp {
        Timestamp::from_second(secs).expect("ts")
    }

    #[test]
    fn save_and_load_round_trip() {
        let mut store = store();
        let note = Note::new("Car", "# Insurance\nRenew in May", clock::now()).expect("note");
        store.save_note(&note).expect("save");
        assert_eq!(store.note(note.id()).expect("load"), Some(note));
    }

    #[test]
    fn missing_note_is_none() {
        assert_eq!(store().note(NoteId::new()).expect("load"), None);
    }

    #[test]
    fn list_orders_pinned_then_recent_and_separates_trash() {
        let mut store = store();
        let old = Note::new("Old", "", at(1)).expect("note");
        let mut pinned = Note::new("Pinned", "", at(2)).expect("note");
        pinned.set_pinned(true, at(2)).expect("pin");
        let new = Note::new("New", "", at(3)).expect("note");
        let mut trashed = Note::new("Gone", "", at(4)).expect("note");
        trashed.trash(at(5));
        for n in [&old, &pinned, &new, &trashed] {
            store.save_note(n).expect("save");
        }

        let titles: Vec<_> = store
            .list_notes(false)
            .expect("list")
            .into_iter()
            .map(|n| n.title)
            .collect();
        assert_eq!(titles, ["Pinned", "New", "Old"]);

        let trash = store.list_notes(true).expect("trash");
        assert_eq!(trash.len(), 1);
        assert_eq!(trash[0].title, "Gone");
    }

    #[test]
    fn search_finds_title_and_body_and_ignores_trash() {
        let mut store = store();
        let a = Note::new("Groceries", "milk and bread", at(1)).expect("note");
        let b = Note::new("Café ideas", "espresso", at(2)).expect("note");
        let mut c = Note::new("Old milk", "", at(3)).expect("note");
        c.trash(at(4));
        for n in [&a, &b, &c] {
            store.save_note(n).expect("save");
        }

        let hits = store.search_notes("mil", 10).expect("search");
        assert_eq!(hits.iter().map(|h| h.id).collect::<Vec<_>>(), [a.id()]);

        let accent = store.search_notes("cafe", 10).expect("search");
        assert_eq!(accent.len(), 1, "diacritics are folded");

        // FTS syntax in user input must not error.
        for nasty in ["\"", "AND", "NEAR(", "*", "title:x", "(-)"] {
            store
                .search_notes(nasty, 10)
                .expect("search survives syntax");
        }
        assert!(store.search_notes("   ", 10).expect("search").is_empty());
    }

    #[test]
    fn search_index_follows_edits() {
        let mut store = store();
        let mut note = Note::new("Draft", "alpha", at(1)).expect("note");
        store.save_note(&note).expect("save");
        note.edit("Draft", "beta", at(2)).expect("edit");
        store.save_note(&note).expect("save");
        assert!(store.search_notes("alpha", 10).expect("search").is_empty());
        assert_eq!(store.search_notes("beta", 10).expect("search").len(), 1);

        store.delete_note_permanently(note.id()).expect("delete");
        assert!(store.search_notes("beta", 10).expect("search").is_empty());
    }

    #[test]
    fn backlinks_track_links_and_update_on_edit() {
        let mut store = store();
        let car = Note::new("Car", "", at(1)).expect("note");
        let mut log =
            Note::new("Log", "Fixed [[car]] brakes, see [[Car#Service]]", at(2)).expect("note");
        store.save_note(&car).expect("save");
        store.save_note(&log).expect("save");

        let back = store.backlinks("Car").expect("backlinks");
        assert_eq!(back.iter().map(|b| b.id).collect::<Vec<_>>(), [log.id()]);

        log.edit("Log", "nothing linked", at(3)).expect("edit");
        store.save_note(&log).expect("save");
        assert!(store.backlinks("Car").expect("backlinks").is_empty());
    }

    #[test]
    fn trashed_notes_do_not_appear_as_backlinks() {
        let mut store = store();
        let mut log = Note::new("Log", "[[Car]]", at(1)).expect("note");
        store.save_note(&log).expect("save");
        log.trash(at(2));
        store.save_note(&log).expect("save");
        assert!(store.backlinks("car").expect("backlinks").is_empty());
    }

    #[test]
    fn tags_are_indexed_counted_and_follow_edits() {
        let mut store = store();
        let mut car = Note::new("Car", "Service #car #money", at(1)).expect("note");
        let bills = Note::new("Bills", "Due #Money", at(2)).expect("note");
        let mut gone = Note::new("Old", "#car", at(3)).expect("note");
        gone.trash(at(4));
        for n in [&car, &bills, &gone] {
            store.save_note(n).expect("save");
        }
        assert_eq!(
            store.note_tags().expect("tags"),
            [("car".to_owned(), 1), ("money".to_owned(), 2)]
        );
        let money: Vec<_> = store
            .notes_with_tag("#Money")
            .expect("tagged")
            .into_iter()
            .map(|n| n.title)
            .collect();
        assert_eq!(money, ["Bills", "Car"]);

        car.edit("Car", "no tags now", at(5)).expect("edit");
        store.save_note(&car).expect("save");
        assert!(store.notes_with_tag("car").expect("tagged").is_empty());
    }

    #[test]
    fn unlinked_mentions_find_plain_text_only() {
        let mut store = store();
        let target = Note::new("Service log", "", at(1)).expect("note");
        let plain = Note::new("Trip", "check the service log before leaving", at(2)).expect("note");
        let linked = Note::new("Car", "see [[Service log]]", at(3)).expect("note");
        let other = Note::new("Food", "nothing here", at(4)).expect("note");
        for n in [&target, &plain, &linked, &other] {
            store.save_note(n).expect("save");
        }
        let hits: Vec<_> = store
            .unlinked_mentions(target.id(), target.title(), 10)
            .expect("mentions")
            .into_iter()
            .map(|n| n.title)
            .collect();
        assert_eq!(hits, ["Trip"]);
        assert!(
            store
                .unlinked_mentions(target.id(), "ab", 10)
                .expect("short")
                .is_empty()
        );
        store
            .unlinked_mentions(target.id(), "quote\" OR x", 10)
            .expect("odd titles are safe");
    }

    #[test]
    fn find_by_title_is_case_insensitive() {
        let mut store = store();
        let car = Note::new("My Car", "", at(1)).expect("note");
        store.save_note(&car).expect("save");
        assert_eq!(
            store.find_note_by_title("my  car").expect("find"),
            Some(car.id())
        );
        assert_eq!(store.find_note_by_title("bike").expect("find"), None);
        let oil = Note::new("Ölwechsel Über", "", at(2)).expect("note");
        store.save_note(&oil).expect("save");
        assert_eq!(
            store.find_note_by_title("ölwechsel über").expect("find"),
            Some(oil.id()),
            "non-ASCII case folding"
        );
    }

    #[test]
    fn suggestions_rank_prefix_matches_first() {
        let mut store = store();
        for (title, secs) in [
            ("Car service", 1),
            ("Old car", 2),
            ("Café", 3),
            ("Groceries", 4),
        ] {
            store
                .save_note(&Note::new(title, "", at(secs)).expect("note"))
                .expect("save");
        }
        let titles = |q: &str| {
            store
                .suggest_note_titles(q, 10)
                .expect("suggest")
                .into_iter()
                .map(|(_, t)| t)
                .collect::<Vec<_>>()
        };
        assert_eq!(titles("car"), ["Car service", "Old car"]);
        assert_eq!(titles("CA"), ["Café", "Car service", "Old car"]);
        assert_eq!(titles("").len(), 4);
        assert_eq!(store.suggest_note_titles("", 2).expect("suggest").len(), 2);
    }

    #[test]
    fn empty_trash_only_removes_trashed() {
        let mut store = store();
        let keep = Note::new("Keep", "", at(1)).expect("note");
        let mut gone = Note::new("Gone", "[[Keep]]", at(1)).expect("note");
        gone.trash(at(2));
        store.save_note(&keep).expect("save");
        store.save_note(&gone).expect("save");
        assert_eq!(store.empty_note_trash().expect("empty"), 1);
        assert_eq!(store.count_notes().expect("count"), 1);
        assert!(store.note(gone.id()).expect("load").is_none());
    }

    #[test]
    fn previews_skip_headings_and_truncate() {
        assert_eq!(preview("\n## Title\nbody"), "body");
        assert_eq!(preview("## Title"), "Title");
        let long = "x".repeat(PREVIEW_CHARS + 5);
        assert_eq!(preview(&long).chars().count(), PREVIEW_CHARS + 1);
    }
}
