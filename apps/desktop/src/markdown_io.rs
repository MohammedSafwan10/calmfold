//! Markdown folder import/export (Obsidian-compatible: one `Title.md` per note).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use calmfold_core::{MAX_BODY_BYTES, Note};

/// Writes `notes` into `dir`. Never overwrites: name clashes get " (2)" etc.
pub fn export(notes: &[Note], dir: &Path) -> Result<usize> {
    fs::create_dir_all(dir).with_context(|| format!("Couldn't create {}", dir.display()))?;
    for note in notes {
        let path = free_path(dir, &file_stem(note.title()));
        fs::write(&path, note.body())
            .with_context(|| format!("Couldn't write {}", path.display()))?;
    }
    Ok(notes.len())
}

/// Reads `*.md` files under `dir` (recursively, skipping hidden folders such
/// as `.obsidian` and `.trash`) as (title, body). Oversized or non-UTF-8 files
/// are skipped.
pub fn read_dir(dir: &Path) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    walk(dir, &mut out, 0).with_context(|| format!("Couldn't read {}", dir.display()))?;
    out.sort_by_cached_key(|(title, _)| title.to_lowercase());
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<(String, String)>, depth: usize) -> std::io::Result<()> {
    if depth > 16 {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk(&path, out, depth + 1)?;
        } else if kind.is_file()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("md"))
        {
            let too_big = entry
                .metadata()
                .map(|m| m.len() > MAX_BODY_BYTES as u64)
                .unwrap_or(true);
            if too_big {
                continue;
            }
            if let (Ok(body), Some(stem)) = (fs::read_to_string(&path), path.file_stem()) {
                out.push((stem.to_string_lossy().into_owned(), body));
            }
        }
    }
    Ok(())
}

/// A safe file name from a title: no path separators or reserved characters,
/// not a Windows device name, at most 120 characters.
fn file_stem(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .take(120)
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').trim().to_owned();
    let reserved = [
        "con", "prn", "aux", "nul", "com1", "com2", "com3", "lpt1", "lpt2", "lpt3",
    ];
    if cleaned.is_empty() || reserved.contains(&cleaned.to_lowercase().as_str()) {
        format!("{cleaned}_note").trim_start_matches('_').to_owned()
    } else {
        cleaned
    }
}

fn free_path(dir: &Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.md"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}).md")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use calmfold_core::Timestamp;

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_stem("a/b:c?"), "a-b-c-");
        assert_eq!(file_stem("  ..Hidden.. "), "Hidden");
        assert_eq!(file_stem("CON"), "CON_note");
        assert_eq!(file_stem("..."), "note");
        assert_eq!(file_stem(&"x".repeat(500)).len(), 120);
    }

    #[test]
    fn export_then_import_round_trips_without_overwriting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = Timestamp::UNIX_EPOCH;
        let notes = [
            Note::new("Car", "#car body", now).expect("note"),
            Note::new("Car", "second with same title", now).expect("note"),
            Note::new("Trip/Plan", "x", now).expect("note"),
        ];
        assert_eq!(export(&notes, dir.path()).expect("export"), 3);
        fs::create_dir(dir.path().join(".obsidian")).expect("mkdir");
        fs::write(dir.path().join(".obsidian/app.md"), "config").expect("write");
        fs::create_dir(dir.path().join("sub")).expect("mkdir");
        fs::write(dir.path().join("sub/Deep.md"), "nested").expect("write");
        fs::write(dir.path().join("image.png"), [0u8, 1, 2]).expect("write");

        let read = read_dir(dir.path()).expect("read");
        let titles: Vec<_> = read.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(titles, ["Car", "Car (2)", "Deep", "Trip-Plan"]);
        assert_eq!(read[0].1, "#car body");
    }
}
