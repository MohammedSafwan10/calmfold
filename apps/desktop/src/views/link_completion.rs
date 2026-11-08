//! `[[` autocomplete for the note editor.
//!
//! The editor asks this provider after every edit. It answers only when the
//! caret is inside an unfinished `[[...` on the current line, with note titles
//! that match what has been typed. Accepting a suggestion replaces the typed
//! text with `Title]]`.

use anyhow::Result;
use gpui_kit::component::input::{CompletionProvider, Rope, RopeExt as _};
use gpui_kit::{App, Task, WeakEntity, Window};
use keepstone_core::NoteId;
use keepstone_core::links::open_link_query;
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    Range, TextEdit,
};

use crate::library::Library;

const MAX_SUGGESTIONS: usize = 8;

pub struct NoteLinkCompletions {
    pub library: WeakEntity<Library>,
    /// The note being edited, excluded from suggestions.
    pub current: Option<NoteId>,
}

impl CompletionProvider for NoteLinkCompletions {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        _trigger: CompletionContext,
        _window: &mut Window,
        cx: &mut App,
    ) -> Task<Result<CompletionResponse>> {
        let items = self.items(text, offset, cx);
        Task::ready(Ok(CompletionResponse::Array(items)))
    }

    fn is_completion_trigger(&self, _offset: usize, new_text: &str, _cx: &mut App) -> bool {
        // Cheap filter; `completions` decides whether a `[[` is actually open.
        !new_text.contains('\n')
    }
}

/// The part of `title` that the typed `query` matches as a case-insensitive prefix.
fn matched_prefix<'a>(title: &'a str, query: &str) -> &'a str {
    let mut end = 0;
    let mut title_chars = title.char_indices();
    for q in query.chars() {
        match title_chars.next() {
            Some((i, t)) if t.to_lowercase().eq(q.to_lowercase()) => end = i + t.len_utf8(),
            _ => return "",
        }
    }
    &title[..end]
}

impl NoteLinkCompletions {
    fn items(&self, text: &Rope, offset: usize, cx: &App) -> Vec<CompletionItem> {
        // Only the current line matters; avoid copying the whole document.
        let line_start = text.offset_to_position(offset);
        let line_begin = text.position_to_offset(&lsp_types::Position::new(line_start.line, 0));
        let line = text.slice(line_begin..offset).to_string();
        let Some((query_start, query)) = open_link_query(&line, line.len()) else {
            return Vec::new();
        };
        let Some(library) = self.library.upgrade() else {
            return Vec::new();
        };
        let suggestions = library
            .read(cx)
            .suggest_note_titles(query, MAX_SUGGESTIONS + 1)
            .unwrap_or_default();

        // If `]]` already follows the caret (auto-typed or edited), replace it too.
        let after = text.slice(offset..(offset + 2).min(text.len())).to_string();
        let end = if after == "]]" { offset + 2 } else { offset };
        let range = Range::new(
            text.offset_to_position(line_begin + query_start),
            text.offset_to_position(end),
        );

        suggestions
            .into_iter()
            .filter(|(id, _)| Some(*id) != self.current)
            .take(MAX_SUGGESTIONS)
            .map(|(_, title)| CompletionItem {
                // The menu highlights `filter_text.len()` bytes from the start of the
                // label. Highlight exactly the typed prefix, or nothing for a
                // "contains" match (the editor's own query would be off by `[[`).
                filter_text: Some(matched_prefix(&title, query).to_owned()),
                label: title.clone(),
                kind: Some(CompletionItemKind::REFERENCE),
                text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
                    range,
                    format!("{title}]]"),
                ))),
                ..Default::default()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::matched_prefix;

    #[test]
    fn prefix_highlight_matches_only_typed_letters() {
        assert_eq!(matched_prefix("Service log", "serv"), "Serv");
        assert_eq!(matched_prefix("Service log", ""), "");
        assert_eq!(matched_prefix("Old car", "car"), "");
        assert_eq!(matched_prefix("Ölwechsel", "öl"), "Öl");
    }
}
