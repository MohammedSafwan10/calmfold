//! The note body: a live-preview Markdown editor (`zorite-editor`, ADR 0004).
//!
//! The text buffer *is* the Markdown. Formatting shows while typing; markers
//! (`**`, `#`, `[[`) are hidden and reappear only where the caret is, like
//! Obsidian's Live Preview. Saving is therefore lossless. "Show Markdown source"
//! turns styling off and shows the raw text.
//!
//! This module is the only place that touches zorite types. It adds what the
//! editor leaves to its host:
//!
//! - theme colors and fonts (`SyntaxStyle` from `cx.theme()`), rebuilt on theme change;
//! - `[[` note-link suggestions (a small menu at the caret, keyboard first);
//! - a selection toolbar (bold, italic, strikethrough, highlight, code, link);
//! - Markdown block formats (headings, lists, quote) via `calmfold_core::format`;
//! - `$$…$$` math rendered by `ratex-gpui`;
//! - spell-check through the OS checker (prose only: code, links, tags, URLs
//!   and math are skipped), right-click a flagged word for suggestions;
//! - find in note (⌘F): matches highlighted in place, ⌘G / ⇧⌘G or Enter / ⇧Enter step;
//! - routing of link, tag and math events.
//!
//! Overlay states (at most one is open):
//!
//! ```text
//!  Idle ──type `[[q`──▶ LinkMenu ──Enter/Tab/click──▶ Idle (inserts `Title]]`)
//!   │                     └──Esc / caret leaves `[[`──▶ Idle
//!   └──select text (mouse up / shift-keys)──▶ SelectionBar ──type / collapse / scroll / Esc──▶ Idle
//! ```

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use calmfold_core::NoteId;
use calmfold_core::format::{self, Block, Inline};
use calmfold_core::links::open_link_query;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, Theme, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnchoredPositionMode, App, AppContext as _, Bounds, Context, ElementId, Entity,
    EntityInputHandler as _, EventEmitter, FocusHandle, Focusable, InteractiveElement as _,
    IntoElement, MouseButton, ParentElement as _, Pixels, Render, RenderImage, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, TestSupportExt as _, WeakEntity,
    Window, anchored, deferred, div, font, point, px, rems,
};
use zorite_editor::{EditorEvent, EditorState, SyntaxStyle};

use crate::actions::{
    FormatBold, FormatBullets, FormatChecklist, FormatCode, FormatHeading1, FormatHeading2,
    FormatHeading3, FormatHighlight, FormatItalic, FormatLink, FormatNumbers, FormatParagraph,
    FormatQuote, FormatStrikethrough, NOTE_EDITOR_CONTEXT, ToggleCheckbox,
};
use crate::library::Library;

const MAX_SUGGESTIONS: usize = 8;
/// Display math is set a little larger than body text.
const MATH_EM_SCALE: f32 = 1.2;
const PLACEHOLDER: &str = "Start writing. Type [[ to link another note.";

/// What the notes screen needs to react to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteEditorEvent {
    /// The text changed (typing, paste, formatting). Schedule a save.
    Changed,
    /// The caret or selection moved; keep it in view.
    CaretMoved,
    /// Focus left the editor. Save now.
    Blurred,
    /// A `[[link]]` was clicked: open (or create) that note.
    OpenNote(String),
    /// A `#tag` was clicked: filter the list by it.
    OpenTag(String),
    /// The active find match moved; scroll it into view.
    FindMoved,
}

/// `[[` suggestions at the caret.
struct LinkMenu {
    /// Byte offset just after `[[`.
    query_start: usize,
    titles: Vec<String>,
    selected: usize,
}

type MathRender = Option<(Arc<RenderImage>, f32, f32)>;

/// The find bar's state. Matches are case-insensitive, in source byte offsets.
pub struct Find {
    pub input: Entity<InputState>,
    matches: Vec<Range<usize>>,
    active: Option<usize>,
    _subscription: Subscription,
}

pub struct NoteEditor {
    editor: Entity<EditorState>,
    library: WeakEntity<Library>,
    /// The open note, excluded from link suggestions.
    current: Option<NoteId>,
    source_mode: bool,
    link_menu: Option<LinkMenu>,
    /// Find in note, while the find bar is open.
    find: Option<Find>,
    /// Window-space bounds of the selection's first character, while the bar shows.
    selection_bar: Option<Bounds<Pixels>>,
    /// A mouse drag is selecting: show the bar only once it ends.
    pointer_down: bool,
    /// Rendered formulas by LaTeX source; cleared when color or scale changes.
    math_cache: Rc<RefCell<HashMap<String, MathRender>>>,
    code: Rc<crate::views::code_highlight::CodeHighlighter>,
    diagrams: Rc<crate::views::diagram::Diagrams>,
    spell: Rc<os_spellcheck::SpellChecker>,
    /// Debounced re-check after edits.
    spell_task: Option<gpui_kit::Task<()>>,
    spell_on: bool,
    scale: Rc<Cell<f32>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<NoteEditorEvent> for NoteEditor {}

impl NoteEditor {
    pub fn new(library: WeakEntity<Library>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| EditorState::new(window, cx).with_placeholder(PLACEHOLDER));
        let focus = editor.read(cx).focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe_in(&editor, window, Self::on_editor_event),
            cx.on_blur(&focus, window, |this, _, cx| {
                this.close_overlays(cx);
                cx.emit(NoteEditorEvent::Blurred);
            }),
            cx.observe_global_in::<Theme>(window, |this, window, cx| {
                this.apply_style(window, cx);
            }),
        ];
        let mut this = Self {
            editor,
            library,
            current: None,
            source_mode: false,
            link_menu: None,
            find: None,
            selection_bar: None,
            pointer_down: false,
            math_cache: Rc::new(RefCell::new(HashMap::new())),
            code: crate::views::code_highlight::CodeHighlighter::new(
                cx.theme().highlight_theme.clone(),
            ),
            spell: Rc::new(os_spellcheck::SpellChecker::new()),
            spell_task: None,
            spell_on: calmfold_ui::appearance::settings(cx).spell_check,
            diagrams: crate::views::diagram::Diagrams::new(
                cx.svg_renderer(),
                crate::views::diagram::DiagramColors::from_theme(cx.theme()),
                window.scale_factor(),
            ),
            scale: Rc::new(Cell::new(window.scale_factor())),
            _subscriptions: subscriptions,
        };
        this.install_providers(cx);
        this.apply_style(window, cx);
        this
    }

    // ---- Content ----

    /// Loads a note's text (not undoable, no `Changed` event).
    pub fn set_note(&mut self, id: Option<NoteId>, text: &str, cx: &mut Context<Self>) {
        self.current = id;
        self.close_overlays(cx);
        self.editor.update(cx, |e, cx| e.set_text(text, cx));
        // Same query, new note: matches follow (from the top).
        self.refresh_find(true, cx);
        self.spell_task = None;
        self.check_spelling(cx);
    }

    pub fn text(&self, cx: &App) -> String {
        self.editor.read(cx).text().to_owned()
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        let focus = self.editor.read(cx).focus_handle(cx);
        focus.focus(window, cx);
    }

    /// Focuses the editor with the caret after the last character.
    pub fn move_to_end(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, |e, cx| {
            let end = e.text().len();
            e.set_cursor(end, cx);
            e.focus(window, cx);
        });
    }

    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.editor.read(cx).focus_handle(cx).is_focused(window)
    }

    /// The caret's window-space box from the last paint.
    pub fn caret_bounds(&self, cx: &App) -> Option<Bounds<Pixels>> {
        self.editor.read(cx).caret_screen_bounds()
    }

    /// The underlying editor, for tests that drive it like a user would.
    #[doc(hidden)]
    pub fn engine(&self) -> &Entity<EditorState> {
        &self.editor
    }

    pub fn is_source_mode(&self) -> bool {
        self.source_mode
    }

    /// Raw Markdown (`true`) or live preview (`false`).
    pub fn set_source_mode(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.source_mode == on {
            return;
        }
        self.source_mode = on;
        self.close_overlays(cx);
        self.apply_style(window, cx);
        self.check_spelling(cx);
        cx.notify();
    }

    /// Note titles offered for the open `[[`, if the menu is showing (for tests).
    pub fn link_suggestions(&self) -> Option<&[String]> {
        self.link_menu.as_ref().map(|m| m.titles.as_slice())
    }

    pub fn is_selection_bar_open(&self) -> bool {
        self.selection_bar.is_some()
    }

    /// Hides overlays that would otherwise float away from their text (page scroll).
    pub fn close_overlays(&mut self, cx: &mut Context<Self>) {
        if self.link_menu.take().is_some() | self.selection_bar.take().is_some() {
            cx.notify();
        }
    }

    // ---- Find in note ----

    pub fn find(&self) -> Option<&Find> {
        self.find.as_ref()
    }

    /// (active match number, 1-based, and total), for the bar's counter.
    pub fn find_status(&self) -> Option<(usize, usize)> {
        let f = self.find.as_ref()?;
        Some((f.active.map_or(0, |a| a + 1), f.matches.len()))
    }

    /// Opens the find bar (or focuses it), seeded with the selection if it is one line.
    pub fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let seed = {
            let sel = self.selection(window, cx);
            let text = self.text(cx);
            text.get(sel)
                .filter(|s| !s.is_empty() && !s.contains('\n') && s.len() <= 200)
                .map(str::to_owned)
        };
        if self.find.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in note"));
            let sub = cx.subscribe_in(&input, window, |this, _, event, _, cx| match event {
                InputEvent::Change => this.refresh_find(true, cx),
                InputEvent::PressEnter { shift, .. } => {
                    this.step_find(if *shift { -1 } else { 1 }, cx);
                }
                _ => {}
            });
            self.find = Some(Find {
                input,
                matches: Vec::new(),
                active: None,
                _subscription: sub,
            });
        }
        let Some(find) = self.find.as_ref() else {
            return;
        };
        let input = find.input.clone();
        input.update(cx, |i, cx| {
            if let Some(seed) = seed {
                i.set_value(seed, window, cx);
            }
            i.focus(window, cx);
            i.select_all(window, cx);
        });
        self.close_overlays(cx);
        self.refresh_find(true, cx);
        cx.notify();
    }

    /// Closes the find bar and returns to the text at the active match.
    pub fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(find) = self.find.take() else {
            return;
        };
        let at = find
            .active
            .and_then(|a| find.matches.get(a))
            .map(|r| r.start);
        self.editor.update(cx, |e, cx| {
            e.set_search(Vec::new(), None, cx);
            if let Some(at) = at {
                e.set_cursor(at, cx);
            }
            e.focus(window, cx);
        });
        cx.notify();
    }

    /// Recomputes matches. With `from_caret`, the active match is the first at
    /// or after the caret (typing a query); otherwise it keeps its place (edits).
    fn refresh_find(&mut self, from_caret: bool, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let query = find.input.read(cx).value().to_string();
        let (matches, caret) = {
            let e = self.editor.read(cx);
            (zorite_editor::find_in_source(e.text(), &query), e.cursor())
        };
        let previous = find
            .active
            .and_then(|a| find.matches.get(a))
            .map(|r| r.start);
        let anchor = if from_caret { Some(caret) } else { previous };
        find.active = (!matches.is_empty()).then(|| {
            anchor
                .and_then(|at| matches.iter().position(|r| r.start >= at))
                .unwrap_or(0)
        });
        find.matches = matches.clone();
        let active = find.active;
        self.editor
            .update(cx, |e, cx| e.set_search(matches, active, cx));
        cx.emit(NoteEditorEvent::FindMoved);
        cx.notify();
    }

    /// Moves to the next (`1`) or previous (`-1`) match, wrapping around.
    pub fn step_find(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let n = find.matches.len() as isize;
        if n == 0 {
            return;
        }
        let at = find
            .active
            .map_or(if delta > 0 { -1 } else { 0 }, |a| a as isize);
        find.active = Some((at + delta).rem_euclid(n) as usize);
        let (matches, active) = (find.matches.clone(), find.active);
        self.editor
            .update(cx, |e, cx| e.set_search(matches, active, cx));
        cx.emit(NoteEditorEvent::FindMoved);
        cx.notify();
    }

    /// Window-space top of the active match's row, after layout.
    pub fn active_match_top(&self, cx: &App) -> Option<Pixels> {
        let f = self.find.as_ref()?;
        let r = f.matches.get(f.active?)?;
        self.editor.read(cx).offset_screen_top(r.start)
    }

    // ---- Style and providers ----

    // ---- Spell-check ----

    /// The words currently underlined as misspelled.
    pub fn flagged_words(&self, cx: &App) -> Vec<String> {
        let e = self.editor.read(cx);
        e.diagnostics()
            .iter()
            .filter_map(|d| e.text().get(d.range.clone()).map(str::to_owned))
            .collect()
    }

    /// Underlines misspelled prose words. Runs on the main thread (AppKit), so
    /// it is debounced and skipped for very large notes.
    fn check_spelling(&mut self, cx: &mut Context<Self>) {
        const MAX_BYTES: usize = 256 * 1024;
        let diagnostics = if self.spell_on && !self.source_mode {
            let text = self.editor.read(cx).text();
            if text.len() > MAX_BYTES {
                Vec::new()
            } else {
                let skip = calmfold_core::format::non_prose_ranges(text);
                self.spell
                    .check(text)
                    .into_iter()
                    .filter(|r| !calmfold_core::format::overlaps_any(r, &skip))
                    .map(|range| zorite_editor::Diagnostic { range })
                    .collect()
            }
        } else {
            Vec::new()
        };
        self.editor
            .update(cx, |e, cx| e.set_diagnostics(diagnostics, cx));
    }

    fn schedule_spell_check(&mut self, cx: &mut Context<Self>) {
        if !self.spell_on {
            return;
        }
        self.spell_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(400))
                .await;
            _ = this.update(cx, |this, cx| this.check_spelling(cx));
        }));
    }

    fn apply_style(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        zorite_editor::set_line_height_ratio(
            calmfold_ui::appearance::settings(cx).line_spacing.ratio(),
        );
        let spell_on = calmfold_ui::appearance::settings(cx).spell_check;
        if spell_on != self.spell_on {
            self.spell_on = spell_on;
            self.check_spelling(cx);
        }
        self.math_cache.borrow_mut().clear();
        self.code.set_theme(cx.theme().highlight_theme.clone());
        self.diagrams.update(
            crate::views::diagram::DiagramColors::from_theme(cx.theme()),
            window.scale_factor(),
        );
        self.scale.set(window.scale_factor());
        let style = (!self.source_mode).then(|| syntax_style(cx.theme()));
        let em = cx.theme().font_size.as_f32() * MATH_EM_SCALE;
        self.editor.update(cx, |e, cx| match style {
            Some(style) => {
                e.set_markdown_style(style, cx);
                e.set_block_math_em(em);
            }
            None => e.clear_markdown_style(cx),
        });
    }

    fn install_providers(&mut self, cx: &mut Context<Self>) {
        let cache = self.math_cache.clone();
        let scale = self.scale.clone();
        let theme = cx.theme();
        let color = Rc::new(Cell::new(theme.foreground));
        let em = Rc::new(Cell::new(theme.font_size.as_f32() * MATH_EM_SCALE));
        // The provider runs during paint; keep its inputs current without rebuilding it.
        let (color_in, em_in) = (color.clone(), em.clone());
        self._subscriptions
            .push(cx.observe_global::<Theme>(move |_, cx| {
                color_in.set(cx.theme().foreground);
                em_in.set(cx.theme().font_size.as_f32() * MATH_EM_SCALE);
            }));
        let code = self.code.clone();
        let diagrams = self.diagrams.clone();
        let spell = self.spell.clone();
        self.editor.update(cx, |e, _| {
            // Suggestions are slow (an XPC call): only when a word is right-clicked.
            e.on_suggest(move |word| spell.suggestions(word));
            e.set_block_mermaid_provider(move |src| diagrams.render(src));
            e.set_code_highlighter(move |lang, src| code.highlight(lang, src));
            e.set_code_languages(
                crate::views::code_highlight::COMMON_LANGUAGES
                    .iter()
                    .map(|l| SharedString::from(*l))
                    .collect(),
            );
            e.set_block_math_provider(move |latex| {
                cache
                    .borrow_mut()
                    .entry(latex.to_owned())
                    .or_insert_with(|| {
                        ratex_gpui::render::render_latex(latex, em.get(), scale.get(), color.get())
                            .map(|r| (r.image, r.width, r.height))
                    })
                    .clone()
            });
        });
    }

    // ---- Editor events ----

    fn on_editor_event(
        &mut self,
        editor: &Entity<EditorState>,
        event: &EditorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            EditorEvent::Changed => {
                self.schedule_spell_check(cx);
                self.selection_bar = None;
                self.refresh_link_menu(cx);
                self.refresh_find(false, cx);
                cx.emit(NoteEditorEvent::Changed);
                cx.emit(NoteEditorEvent::CaretMoved);
            }
            EditorEvent::SelectionChanged => {
                self.refresh_link_menu(cx);
                if !self.pointer_down {
                    self.refresh_selection_bar(window, cx);
                }
                cx.emit(NoteEditorEvent::CaretMoved);
            }
            EditorEvent::OpenWikiLink(target) => {
                let text = editor.read(cx).text();
                if let Some(event) = route_wiki_click(text, target) {
                    cx.emit(event);
                }
            }
            EditorEvent::OpenLink(url) => {
                // Link targets are untrusted note text: only web and mail links leave the app.
                if url.starts_with("https://")
                    || url.starts_with("http://")
                    || url.starts_with("mailto:")
                {
                    cx.open_url(url);
                }
            }
            // No structural math or property editor: put the caret in the source so
            // it shows raw and the arrow keys keep moving (zorite waits for the host).
            // zorite asks again on every arrow press inside the block, so from inside
            // we step one line (block) or character (inline) in the press's direction.
            EditorEvent::EditMath {
                range,
                at_end,
                inline,
                ..
            } => {
                let (text, cursor) = {
                    let e = editor.read(cx);
                    (e.text().to_owned(), e.cursor())
                };
                let to = math_caret_target(&text, cursor, range.clone(), *at_end, *inline);
                editor.update(cx, |e, cx| e.set_cursor(to, cx));
            }
            EditorEvent::EditProperties { range, at_end, .. } => {
                let to = if *at_end { range.end } else { range.start };
                editor.update(cx, |e, cx| e.set_cursor(to, cx));
            }
            _ => {}
        }
    }

    // ---- Link suggestions ----

    fn refresh_link_menu(&mut self, cx: &mut Context<Self>) {
        let (text, cursor) = {
            let e = self.editor.read(cx);
            (e.text().to_owned(), e.cursor())
        };
        let Some((query_start, query)) = open_link_query(&text, cursor) else {
            if self.link_menu.take().is_some() {
                cx.notify();
            }
            return;
        };
        let titles: Vec<String> = self
            .library
            .upgrade()
            .and_then(|lib| {
                lib.read(cx)
                    .suggest_note_titles(query, MAX_SUGGESTIONS + 1)
                    .ok()
            })
            .unwrap_or_default()
            .into_iter()
            .filter(|(id, _)| Some(*id) != self.current)
            .take(MAX_SUGGESTIONS)
            .map(|(_, title)| title)
            .collect();
        let selected = self
            .link_menu
            .as_ref()
            .filter(|m| m.query_start == query_start)
            .map_or(0, |m| m.selected.min(titles.len().saturating_sub(1)));
        self.link_menu = (!titles.is_empty()).then_some(LinkMenu {
            query_start,
            titles,
            selected,
        });
        cx.notify();
    }

    /// Replaces the typed `[[query` (and a `]]` right after the caret) with `[[Title]]`.
    fn accept_link(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.link_menu.take() else {
            return;
        };
        let Some(title) = menu.titles.get(index) else {
            return;
        };
        let insert = format!("{title}]]");
        self.editor.update(cx, |e, cx| {
            let cursor = e.cursor();
            let end = if e.text()[cursor..].starts_with("]]") {
                cursor + 2
            } else {
                cursor
            };
            e.replace_range(menu.query_start..end, &insert, cx);
            e.focus(window, cx);
        });
        cx.emit(NoteEditorEvent::Changed);
        cx.notify();
    }

    fn move_link_selection(&mut self, delta: isize, cx: &mut Context<Self>) -> bool {
        let Some(menu) = self.link_menu.as_mut() else {
            return false;
        };
        let n = menu.titles.len() as isize;
        menu.selected = (menu.selected as isize + delta).rem_euclid(n) as usize;
        cx.notify();
        true
    }

    // ---- Selection bar ----

    fn selection(&self, window: &mut Window, cx: &mut Context<Self>) -> Range<usize> {
        self.editor.update(cx, |e, cx| {
            let text = e.text().to_owned();
            e.selected_text_range(false, window, cx).map_or(0..0, |s| {
                utf16_to_byte(&text, s.range.start)..utf16_to_byte(&text, s.range.end)
            })
        })
    }

    fn refresh_selection_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let sel = self.selection(window, cx);
        let bar = (!sel.is_empty() && self.is_focused(window, cx))
            .then(|| self.editor.read(cx).bounds_for_offset(sel.start))
            .flatten();
        if bar != self.selection_bar {
            self.selection_bar = bar;
            cx.notify();
        }
    }

    // ---- Formatting ----

    /// Bold, italic, … are the editor's own commands (they handle hidden markers).
    fn inline(&mut self, style: Inline, window: &mut Window, cx: &mut Context<Self>) {
        let action: Box<dyn Action> = match style {
            Inline::Bold => Box::new(zorite_editor::Bold),
            Inline::Italic => Box::new(zorite_editor::Italic),
            Inline::Strikethrough => Box::new(zorite_editor::Strike),
            Inline::Code => Box::new(zorite_editor::Code),
            Inline::Highlight => Box::new(zorite_editor::Highlight),
        };
        let focus = self.editor.read(cx).focus_handle(cx);
        focus.focus(window, cx);
        focus.dispatch_action(action.as_ref(), window, cx);
        self.refresh_selection_bar(window, cx);
    }

    /// Applies a Markdown text edit at the selection as one undo step.
    fn edit(
        &mut self,
        f: impl FnOnce(&str, Range<usize>) -> format::Edit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sel = self.selection(window, cx);
        let text = self.text(cx);
        let edit = f(&text, sel);
        if text.get(edit.range.clone()) == Some(edit.text.as_str()) {
            return;
        }
        self.editor.update(cx, |e, cx| {
            e.replace_range(edit.range.clone(), &edit.text, cx);
            e.set_cursor(edit.selection.end, cx);
            e.focus(window, cx);
        });
        self.selection_bar = None;
        cx.emit(NoteEditorEvent::Changed);
    }

    fn block(&mut self, block: Block, window: &mut Window, cx: &mut Context<Self>) {
        self.edit(|doc, sel| format::set_block(doc, sel, block), window, cx);
    }

    fn render_selection_bar(&self, anchor: Bounds<Pixels>, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let view = cx.entity().downgrade();
        let tool = |id: &'static str,
                    icon: Lucide,
                    label: &'static str,
                    action: Box<dyn Action>,
                    run: fn(&mut Self, &mut Window, &mut Context<Self>)| {
            let view = view.clone();
            gpui_kit::base::Button::new(id)
                // Pressing must not take focus (or the selection) from the editor.
                .focusable(false)
                .tab_stop(false)
                .accessibility_label(label)
                .size(rems(1.75))
                .flex()
                .items_center()
                .justify_center()
                .rounded(theme.radius)
                .hover(|s| s.bg(theme.list_hover))
                .child(Icon::new(icon).small().text_color(theme.foreground))
                .tooltip(move |window, cx| {
                    Tooltip::new(label)
                        .action(action.as_ref(), Some(NOTE_EDITOR_CONTEXT))
                        .build(window, cx)
                })
                .on_click(move |_, window, cx| {
                    _ = view.update(cx, |this, cx| run(this, window, cx));
                })
        };
        let bar_height = rems(2.25).to_pixels(px(16.));
        deferred(
            anchored()
                .position_mode(AnchoredPositionMode::Window)
                .position(point(anchor.left(), anchor.top() - bar_height - px(6.)))
                .snap_to_window_with_margin(px(8.))
                .child(
                    h_flex()
                        .id("selection-bar")
                        .test_support()
                        .p_0p5()
                        .gap_0p5()
                        .bg(theme.popover)
                        .border_1()
                        .border_color(theme.border)
                        .rounded(theme.radius)
                        // Clicks on the bar's padding must not reach the editor below.
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(tool(
                            "fmt-bold",
                            Lucide::Bold,
                            "Bold",
                            Box::new(FormatBold),
                            |t, w, cx| t.inline(Inline::Bold, w, cx),
                        ))
                        .child(tool(
                            "fmt-italic",
                            Lucide::Italic,
                            "Italic",
                            Box::new(FormatItalic),
                            |t, w, cx| t.inline(Inline::Italic, w, cx),
                        ))
                        .child(tool(
                            "fmt-strike",
                            Lucide::Strikethrough,
                            "Strikethrough",
                            Box::new(FormatStrikethrough),
                            |t, w, cx| t.inline(Inline::Strikethrough, w, cx),
                        ))
                        .child(tool(
                            "fmt-highlight",
                            Lucide::Highlighter,
                            "Highlight",
                            Box::new(FormatHighlight),
                            |t, w, cx| t.inline(Inline::Highlight, w, cx),
                        ))
                        .child(tool(
                            "fmt-code",
                            Lucide::Code,
                            "Code",
                            Box::new(FormatCode),
                            |t, w, cx| t.inline(Inline::Code, w, cx),
                        ))
                        .child(tool(
                            "fmt-link",
                            Lucide::Link2,
                            "Link",
                            Box::new(FormatLink),
                            |t, w, cx| t.edit(format::insert_link, w, cx),
                        )),
                ),
        )
        .with_priority(1)
    }

    fn render_link_menu(&self, menu: &LinkMenu, cx: &Context<Self>) -> Option<impl IntoElement> {
        let caret = self.caret_bounds(cx)?;
        let theme = cx.theme();
        let view = cx.entity().downgrade();
        Some(
            deferred(
                anchored()
                    .position_mode(AnchoredPositionMode::Window)
                    .position(point(caret.left(), caret.bottom() + px(4.)))
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        v_flex()
                            .id("link-menu")
                            .test_support()
                            .w(rems(16.))
                            .p_1()
                            .bg(theme.popover)
                            .border_1()
                            .border_color(theme.border)
                            .rounded(theme.radius)
                            .text_sm()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .children(menu.titles.iter().enumerate().map(|(i, title)| {
                                let view = view.clone();
                                h_flex()
                                    .id(ElementId::NamedInteger("link-suggestion".into(), i as u64))
                                    .test_support()
                                    .gap_2()
                                    .px_2()
                                    .py_1()
                                    .rounded(theme.radius)
                                    .when(i == menu.selected, |s| s.bg(theme.list_active))
                                    .when(i != menu.selected, |s| {
                                        s.hover(|s| s.bg(theme.list_hover))
                                    })
                                    .child(
                                        Icon::new(Lucide::FileText)
                                            .xsmall()
                                            .text_color(theme.muted_foreground),
                                    )
                                    .child(div().flex_1().min_w_0().truncate().child(title.clone()))
                                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                        cx.stop_propagation();
                                        _ = view
                                            .update(cx, |this, cx| this.accept_link(i, window, cx));
                                    })
                            })),
                    ),
            )
            .with_priority(1),
        )
    }
}

impl Focusable for NoteEditor {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.read(cx).focus_handle(cx)
    }
}

impl Render for NoteEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.scale.get() != window.scale_factor() {
            // Moved to a display with another scale: re-rasterize sharply.
            self.math_cache.borrow_mut().clear();
            self.scale.set(window.scale_factor());
            self.diagrams.update(
                crate::views::diagram::DiagramColors::from_theme(cx.theme()),
                window.scale_factor(),
            );
        }
        let theme = cx.theme();
        let link_menu = self
            .link_menu
            .as_ref()
            .and_then(|m| self.render_link_menu(m, cx));
        let selection_bar = self
            .selection_bar
            .filter(|_| self.link_menu.is_none())
            .map(|anchor| self.render_selection_bar(anchor, cx));
        let editor = self.editor.clone();

        v_flex()
            .id("note-body")
            .test_support()
            .key_context(NOTE_EDITOR_CONTEXT)
            // zorite reads the ambient text style: prose in the UI font.
            .font_family(theme.font_family.clone())
            .text_size(theme.font_size)
            .text_color(theme.foreground)
            // The link menu owns these keys while it is open.
            .capture_action(cx.listener(|this, _: &zorite_editor::Up, _, cx| {
                if this.move_link_selection(-1, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &zorite_editor::Down, _, cx| {
                if this.move_link_selection(1, cx) {
                    cx.stop_propagation();
                }
            }))
            .capture_action(cx.listener(|this, _: &zorite_editor::Newline, window, cx| {
                if let Some(i) = this.link_menu.as_ref().map(|m| m.selected) {
                    cx.stop_propagation();
                    this.accept_link(i, window, cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &zorite_editor::Indent, window, cx| {
                if let Some(i) = this.link_menu.as_ref().map(|m| m.selected) {
                    cx.stop_propagation();
                    this.accept_link(i, window, cx);
                }
            }))
            .capture_action(cx.listener(|this, _: &zorite_editor::Dismiss, _, cx| {
                if this.link_menu.is_some() || this.selection_bar.is_some() {
                    this.close_overlays(cx);
                    cx.stop_propagation();
                }
            }))
            // Show the selection bar when a drag ends, not while it moves.
            .capture_any_mouse_down(cx.listener(|this, _, _, cx| {
                this.pointer_down = true;
                if this.selection_bar.take().is_some() {
                    cx.notify();
                }
            }))
            .capture_any_mouse_up(cx.listener(|this, _, window, cx| {
                this.pointer_down = false;
                this.refresh_selection_bar(window, cx);
            }))
            .on_action(cx.listener(|this, _: &FormatBold, w, cx| this.inline(Inline::Bold, w, cx)))
            .on_action(
                cx.listener(|this, _: &FormatItalic, w, cx| this.inline(Inline::Italic, w, cx)),
            )
            .on_action(cx.listener(|this, _: &FormatStrikethrough, w, cx| {
                this.inline(Inline::Strikethrough, w, cx)
            }))
            .on_action(cx.listener(|this, _: &FormatCode, w, cx| this.inline(Inline::Code, w, cx)))
            .on_action(
                cx.listener(|this, _: &FormatHighlight, w, cx| {
                    this.inline(Inline::Highlight, w, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &FormatLink, w, cx| this.edit(format::insert_link, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FormatHeading1, w, cx| this.block(Block::Heading(1), w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FormatHeading2, w, cx| this.block(Block::Heading(2), w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FormatHeading3, w, cx| this.block(Block::Heading(3), w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FormatParagraph, w, cx| this.block(Block::Paragraph, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FormatBullets, w, cx| this.block(Block::Bullet, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FormatNumbers, w, cx| this.block(Block::Numbered, w, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FormatChecklist, w, cx| this.block(Block::Checklist, w, cx)),
            )
            .on_action(cx.listener(|this, _: &FormatQuote, w, cx| this.block(Block::Quote, w, cx)))
            .on_action(cx.listener(|this, _: &ToggleCheckbox, w, cx| {
                this.edit(
                    |doc, sel| {
                        format::toggle_checkbox(doc, sel.start).unwrap_or(format::Edit {
                            text: doc[sel.clone()].to_owned(),
                            range: sel.clone(),
                            selection: sel,
                        })
                    },
                    w,
                    cx,
                );
            }))
            .child(editor)
            // The empty space under the text is part of the page: clicking it
            // continues writing at the end, like a document.
            .child(
                div()
                    .id("note-body-tail")
                    .test_support()
                    .flex_1()
                    .min_h(rems(12.))
                    .cursor_text()
                    .on_click(cx.listener(|this, _, window, cx| {
                        let end = this.editor.read(cx).text().len();
                        this.editor.update(cx, |e, cx| {
                            e.set_cursor(end, cx);
                            e.focus(window, cx);
                        });
                    })),
            )
            .children(link_menu)
            .children(selection_bar)
    }
}

/// Live-preview colors from the theme. Every value is a theme token.
fn syntax_style(theme: &Theme) -> SyntaxStyle {
    SyntaxStyle {
        block_label: None,
        block_label_gen: 0,
        block_ref_count: None,
        marker: theme.muted_foreground.opacity(0.7),
        code: theme.foreground,
        code_bg: theme.muted,
        // The theme's `link` token is the text color (links elsewhere are
        // underlined). In running text a link needs its own hue to be seen.
        link: theme.blue,
        tag: theme.blue,
        quote: theme.muted_foreground,
        alert_note: theme.info,
        alert_tip: theme.success,
        alert_important: theme.magenta,
        alert_warning: theme.warning,
        alert_caution: theme.danger,
        alert_icons: None,
        rule: theme.border,
        mark_bg: theme.yellow.opacity(0.35),
        popover_bg: theme.popover,
        popover_border: theme.border,
        popover_fg: theme.popover_foreground,
        popover_hover: theme.list_hover,
        popover_divider: theme.border,
        popover_danger: theme.danger,
        mono: font(theme.mono_font_family.clone()),
        property_icon: None,
    }
}

/// Turns an editor link click into a note or tag event. zorite reports both
/// `[[Page]]` and `#tag` clicks as a page title, so a target that only exists
/// as a tag in the text is a tag. Block references are not supported.
fn route_wiki_click(text: &str, target: &str) -> Option<NoteEditorEvent> {
    if target.starts_with("#^") || target.starts_with("refs:") {
        return None;
    }
    let page = target.split("#^").next().unwrap_or(target);
    let page = page.split('#').next().unwrap_or(page).trim();
    let wanted = calmfold_core::links::normalize_target(page);
    let is_link = calmfold_core::links::extract_wiki_links(text)
        .iter()
        .any(|l| calmfold_core::links::normalize_target(&l.target) == wanted);
    let tag = calmfold_core::tags::normalize_tag(target);
    let is_tag = calmfold_core::tags::extract_tags(text).contains(&tag);
    match (is_link, is_tag) {
        (false, true) => Some(NoteEditorEvent::OpenTag(tag)),
        _ if page.is_empty() => None,
        _ => Some(NoteEditorEvent::OpenNote(page.to_owned())),
    }
}

/// Where the caret goes when zorite reports it moving into math `range`.
/// `at_end` means it came from after the formula (moving up or left).
fn math_caret_target(
    text: &str,
    cursor: usize,
    range: Range<usize>,
    at_end: bool,
    inline: bool,
) -> usize {
    let inside = cursor >= range.start && cursor <= range.end;
    if !inside {
        let fence = if inline { 1 } else { 2 };
        return if at_end {
            range.end.saturating_sub(fence)
        } else {
            (range.start + fence).min(text.len())
        };
    }
    match (inline, at_end) {
        // Left / right by one character.
        (true, true) => text[..cursor]
            .chars()
            .next_back()
            .map_or(0, |c| cursor - c.len_utf8()),
        (true, false) => text[cursor..]
            .chars()
            .next()
            .map_or(cursor, |c| cursor + c.len_utf8()),
        // Up: end of the previous line. Down: start of the next line.
        (false, true) => text[..cursor].rfind('\n').unwrap_or(0),
        (false, false) => text[cursor..]
            .find('\n')
            .map_or(text.len(), |i| cursor + i + 1),
    }
}

/// Byte offset of UTF-16 offset `units` in `text` (clamped).
fn utf16_to_byte(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (i, ch) in text.char_indices() {
        if seen >= units {
            return i;
        }
        seen += ch.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_and_tag_clicks_are_told_apart() {
        let text = "See [[Car]] and #travel, also [[Trip#Day 1]]";
        assert_eq!(
            route_wiki_click(text, "Car"),
            Some(NoteEditorEvent::OpenNote("Car".into()))
        );
        assert_eq!(
            route_wiki_click(text, "travel"),
            Some(NoteEditorEvent::OpenTag("travel".into()))
        );
        assert_eq!(
            route_wiki_click(text, "Trip#Day 1"),
            Some(NoteEditorEvent::OpenNote("Trip".into()))
        );
        assert_eq!(route_wiki_click(text, "#^abc"), None);
        assert_eq!(route_wiki_click(text, "refs:^abc"), None);
        // A name used both ways opens the note.
        let both = "[[car]] #car";
        assert_eq!(
            route_wiki_click(both, "car"),
            Some(NoteEditorEvent::OpenNote("car".into()))
        );
    }

    #[test]
    fn caret_steps_through_math_instead_of_sticking() {
        let text = "above\n$$\nx^2\n$$\nbelow";
        let block = 6..15; // "$$\nx^2\n$$"
        // Entering from above lands after the opening fence; from below, before the closing one.
        assert_eq!(math_caret_target(text, 0, block.clone(), false, false), 8);
        assert_eq!(math_caret_target(text, 20, block.clone(), true, false), 13);
        // Inside, each press moves one line.
        assert_eq!(math_caret_target(text, 8, block.clone(), false, false), 9);
        assert_eq!(math_caret_target(text, 9, block.clone(), false, false), 13);
        assert_eq!(math_caret_target(text, 13, block.clone(), true, false), 12);
        // Inline: one character at a time, UTF-8 safe.
        let inline = "a $é$ b";
        assert_eq!(math_caret_target(inline, 3, 2..6, false, true), 5);
        assert_eq!(math_caret_target(inline, 5, 2..6, true, true), 3);
    }

    #[test]
    fn utf16_offsets_map_to_bytes() {
        let s = "aé😀b";
        assert_eq!(utf16_to_byte(s, 0), 0);
        assert_eq!(utf16_to_byte(s, 1), 1);
        assert_eq!(utf16_to_byte(s, 2), 3);
        assert_eq!(utf16_to_byte(s, 4), 7);
        assert_eq!(utf16_to_byte(s, 99), s.len());
    }
}
