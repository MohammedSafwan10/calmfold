//! Notes screen: searchable list on the left, editor or preview on the right.
//!
//! Editor state machine:
//!
//! ```text
//!  Empty ──open(id)──▶ Clean ──typing──▶ Dirty ──500 ms idle / switch / blur──▶ Clean
//!                        ▲                 │
//!                        └──── save ok ────┘   (save error: stays Dirty, user notified)
//! ```
//!
//! Content is always flushed before another note opens, so switching notes
//! can never lose typing.

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{BufferPoint, Editor, EditorState, Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Icon, Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, ClickEvent, Context, ElementId, Entity, EventEmitter, FocusHandle,
    Focusable, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    Task, TestSupportExt as _, Window, div, px, rems,
};
use keepstone_core::NoteId;
use keepstone_core::format::{self, Block, Inline};
use keepstone_core::links::{note_link_url, parse_note_link_url};
use keepstone_storage::NoteSummary;
use keepstone_ui::appearance;
use keepstone_ui::components::{empty_state, section_label};
use keepstone_ui::feedback::{self, Confirm};
use keepstone_ui::panel::{PanelSide, SidePanel};

use crate::actions::{
    FormatBold, FormatBullets, FormatChecklist, FormatCode, FormatHeading1, FormatHeading2,
    FormatHeading3, FormatHighlight, FormatItalic, FormatLink, FormatNumbers, FormatParagraph,
    FormatQuote, FormatStrikethrough, ToggleCheckbox,
};
use crate::library::{Library, LibraryEvent};
use crate::ui_state::{LayoutStore, NOTES_LIST};
use crate::views::Navigate;
use crate::views::link_completion::NoteLinkCompletions;
use crate::views::task_row::notify_err;

const AUTOSAVE_DELAY: Duration = Duration::from_millis(500);
const SEARCH_LIMIT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Edit,
    Preview,
}

/// The note currently loaded in the editor.
struct OpenNote {
    id: NoteId,
    title: String,
    pinned: bool,
    trashed: bool,
}

pub struct NotesView {
    library: Entity<Library>,
    layout: Entity<LayoutStore>,
    search: Entity<InputState>,
    title: Entity<InputState>,
    body: Entity<EditorState>,
    list: Vec<NoteSummary>,
    load_error: bool,
    show_trash: bool,
    /// Shows only notes with this tag (lower-case, no `#`).
    tag_filter: Option<String>,
    mentions: Vec<NoteSummary>,
    /// The note page's single scroll area (the editor never scrolls itself).
    page_scroll: ScrollHandle,
    /// Body rows and caret row from the last frame, to size the editor to its
    /// content and keep the caret in view.
    body_rows: usize,
    caret_row: usize,
    open: Option<OpenNote>,
    backlinks: Vec<NoteSummary>,
    mode: Mode,
    dirty: bool,
    pending_save: Option<Task<()>>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Navigate> for NotesView {}

impl NotesView {
    pub fn new(
        library: Entity<Library>,
        layout: Entity<LayoutStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search notes")
                .clean_on_escape()
        });
        let title = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Untitled")
                // Rejects keystrokes past the domain limit, so a save can never fail on length.
                .validate(|text, _| text.chars().count() <= keepstone_core::MAX_TITLE_CHARS)
        });
        // A text editor (not a plain textarea) so `[[` can offer note titles.
        let body = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .line_number(false)
                .folding(false)
                // Prose, not code: no vertical indent guide lines.
                .indent_guides(false)
                .soft_wrap(true)
                .placeholder("Start writing. Type [[ to link another note.");
            state.lsp_mut().completion_provider = Some(Rc::new(NoteLinkCompletions {
                library: library.downgrade(),
                current: None,
            }));
            state
        });

        let subscriptions = vec![
            cx.observe(&layout, |_, _, cx| cx.notify()),
            // Typing changes rows or the caret line: resize the editor and follow
            // the caret. Other editor notifications (blink) don't re-render this view.
            cx.observe(&body, |this, _, cx| {
                if this.body_metrics(cx) != (this.body_rows, this.caret_row) {
                    cx.notify();
                }
            }),
            cx.subscribe(&library, |this, _, event, cx| {
                if *event == LibraryEvent::NotesChanged {
                    this.reload_list(cx);
                }
            }),
            cx.subscribe(&search, |this, _, event, cx| {
                if matches!(event, InputEvent::Change) {
                    this.reload_list(cx);
                }
            }),
            cx.subscribe_in(&title, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.schedule_save(window, cx),
                InputEvent::PressEnter { .. } => {
                    this.body.update(cx, |body, cx| body.focus(window, cx));
                }
                InputEvent::Blur => {
                    _ = this.flush(window, cx);
                }
                _ => {}
            }),
            cx.subscribe_in(&body, window, |this, _, event, window, cx| match event {
                InputEvent::Change => this.schedule_save(window, cx),
                InputEvent::Blur => {
                    _ = this.flush(window, cx);
                }
                _ => {}
            }),
        ];

        let mut this = Self {
            layout,
            library,
            search,
            title,
            body,
            list: Vec::new(),
            load_error: false,
            show_trash: false,
            tag_filter: None,
            mentions: Vec::new(),
            page_scroll: ScrollHandle::new(),
            body_rows: 0,
            caret_row: 0,
            open: None,
            backlinks: Vec::new(),
            mode: Mode::Edit,
            dirty: false,
            pending_save: None,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.reload_list(cx);
        this
    }

    /// Creates a note and opens it with the title focused.
    pub fn new_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.flush(window, cx) {
            return;
        }
        self.show_trash = false;
        match self.library.update(cx, |lib, cx| lib.create_note("", cx)) {
            Ok(id) => {
                self.open_note(id, window, cx);
                self.mode = Mode::Edit;
                self.title.update(cx, |t, cx| t.focus(window, cx));
            }
            Err(err) => notify_err::<()>(Err(err), window, cx),
        }
    }

    /// Opens `id`, saving the current note first.
    pub fn open_note(&mut self, id: NoteId, window: &mut Window, cx: &mut Context<Self>) {
        if !self.flush(window, cx) {
            return;
        }
        let note = match self.library.read(cx).note(id) {
            Ok(Some(note)) => note,
            Ok(None) => {
                notify_err::<()>(
                    Err(anyhow::anyhow!("That note no longer exists")),
                    window,
                    cx,
                );
                return;
            }
            Err(err) => return notify_err::<()>(Err(err), window, cx),
        };
        if note.is_trashed() != self.show_trash {
            self.show_trash = note.is_trashed();
            self.reload_list(cx);
        }
        // An untitled note shows an empty title field with the placeholder.
        let title_text = if note.title() == keepstone_core::note::UNTITLED {
            ""
        } else {
            note.title()
        };
        self.title
            .update(cx, |t, cx| t.set_value(title_text.to_owned(), window, cx));
        let library = self.library.downgrade();
        self.body.update(cx, |b, cx| {
            b.set_value(note.body().to_owned(), window, cx);
            b.lsp_mut().completion_provider = Some(Rc::new(NoteLinkCompletions {
                library,
                current: Some(id),
            }));
        });
        self.open = Some(OpenNote {
            id,
            title: note.title().to_owned(),
            pinned: note.is_pinned(),
            trashed: note.is_trashed(),
        });
        self.dirty = false;
        self.page_scroll.set_offset(gpui_kit::point(px(0.), px(0.)));
        self.caret_row = 0;
        self.reload_backlinks(cx);
        cx.notify();
    }

    /// Opens the first listed note if nothing is open (called when the screen is shown).
    pub fn ensure_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.is_none()
            && !self.show_trash
            && let Some(first) = self.list.first().map(|n| n.id)
        {
            self.open_note(first, window, cx);
        }
    }

    pub fn focus_body(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.body.update(cx, |b, cx| b.focus(window, cx));
    }

    /// The body editor (tests inspect its completion menu).
    pub fn body_editor(&self) -> &Entity<EditorState> {
        &self.body
    }

    /// Title of the open note, if any.
    pub fn open_note_title(&self, _: &App) -> Option<String> {
        self.open.as_ref().map(|o| o.title.clone())
    }

    /// The open note, if any.
    pub fn open_note_id(&self) -> Option<NoteId> {
        self.open.as_ref().map(|o| o.id)
    }

    /// Current editor body text (for tests and diagnostics).
    pub fn body_text(&self, cx: &App) -> String {
        self.body.read(cx).value().to_string()
    }

    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search.update(cx, |s, cx| s.focus(window, cx));
    }

    fn reload_list(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().to_string();
        let library = self.library.read(cx);
        let result = if self.show_trash {
            library.notes(true)
        } else {
            let base = if query.trim().is_empty() {
                library.notes(false)
            } else {
                library.search_notes(&query, SEARCH_LIMIT)
            };
            match &self.tag_filter {
                None => base,
                Some(tag) => {
                    let tagged = library.note_ids_with_tag(tag);
                    base.map(|list| {
                        list.into_iter()
                            .filter(|n| tagged.contains(&n.id))
                            .collect()
                    })
                }
            }
        };
        match result {
            Ok(list) => {
                self.list = list;
                self.load_error = false;
            }
            Err(err) => {
                eprintln!("keepstone: failed to load notes: {err:#}");
                self.list.clear();
                self.load_error = true;
            }
        }
        cx.notify();
    }

    /// Shows only notes tagged `tag` (or all notes for `None`).
    pub fn set_tag_filter(
        &mut self,
        tag: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tag = tag
            .map(|t| keepstone_core::tags::normalize_tag(&t))
            .filter(|t| !t.is_empty());
        if tag == self.tag_filter && !self.show_trash {
            return;
        }
        if self.show_trash {
            self.set_show_trash(false, window, cx);
        }
        self.tag_filter = tag;
        self.reload_list(cx);
    }

    pub fn tag_filter(&self) -> Option<&str> {
        self.tag_filter.as_deref()
    }

    /// Applies a Markdown edit to the body at the current selection.
    pub fn format_with(
        &mut self,
        f: impl FnOnce(&str, std::ops::Range<usize>) -> format::Edit,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.mode != Mode::Edit || self.open.as_ref().is_none_or(|o| o.trashed) {
            return;
        }
        let (doc, sel) = {
            let body = self.body.read(cx);
            (body.value().to_string(), body.selected_range())
        };
        let edit = f(&doc, sel);
        if edit.range.start == edit.range.end && edit.text.is_empty() {
            return;
        }
        self.body.update(cx, |b, cx| {
            b.set_selected_range(edit.range.clone(), cx);
            b.replace(edit.text.clone(), window, cx);
            b.set_selected_range(edit.selection.clone(), cx);
            b.focus(window, cx);
        });
        self.schedule_save(window, cx);
    }

    pub fn format_inline(&mut self, style: Inline, window: &mut Window, cx: &mut Context<Self>) {
        self.format_with(
            |doc, sel| format::toggle_inline(doc, sel, style),
            window,
            cx,
        );
    }

    pub fn format_block(&mut self, block: Block, window: &mut Window, cx: &mut Context<Self>) {
        self.format_with(|doc, sel| format::set_block(doc, sel, block), window, cx);
    }

    /// "Aa": the six everyday styles first, then Headings ▸ and Lists ▸.
    /// Items dispatch the same actions as the shortcuts, into the editor, so the
    /// menu also teaches the keys.
    fn format_menu(&self, cx: &Context<Self>) -> impl IntoElement {
        let editor = self.body.focus_handle(cx);
        Button::new("note-format")
            .ghost()
            .small()
            .icon(Lucide::Type)
            .tooltip("Format")
            .dropdown_menu(move |menu, window, cx| {
                menu.action_context(editor.clone())
                    .menu_with_icon("Bold", Lucide::Bold, Box::new(FormatBold))
                    .menu_with_icon("Italic", Lucide::Italic, Box::new(FormatItalic))
                    .menu_with_icon(
                        "Strikethrough",
                        Lucide::Strikethrough,
                        Box::new(FormatStrikethrough),
                    )
                    .menu_with_icon("Highlight", Lucide::Highlighter, Box::new(FormatHighlight))
                    .menu_with_icon("Code", Lucide::Code, Box::new(FormatCode))
                    .menu_with_icon("Link", Lucide::Link2, Box::new(FormatLink))
                    .separator()
                    .submenu_with_icon(
                        Some(Lucide::Heading.into()),
                        "Headings",
                        window,
                        cx,
                        |sub, _, _| {
                            sub.menu_with_icon(
                                "Heading 1",
                                Lucide::Heading1,
                                Box::new(FormatHeading1),
                            )
                            .menu_with_icon("Heading 2", Lucide::Heading2, Box::new(FormatHeading2))
                            .menu_with_icon("Heading 3", Lucide::Heading3, Box::new(FormatHeading3))
                            .separator()
                            .menu_with_icon(
                                "Body text",
                                Lucide::Pilcrow,
                                Box::new(FormatParagraph),
                            )
                        },
                    )
                    .submenu_with_icon(
                        Some(Lucide::List.into()),
                        "Lists",
                        window,
                        cx,
                        |sub, _, _| {
                            sub.menu_with_icon(
                                "Bulleted list",
                                Lucide::List,
                                Box::new(FormatBullets),
                            )
                            .menu_with_icon(
                                "Numbered list",
                                Lucide::ListOrdered,
                                Box::new(FormatNumbers),
                            )
                            .menu_with_icon(
                                "Checklist",
                                Lucide::ListChecks,
                                Box::new(FormatChecklist),
                            )
                            .separator()
                            .menu_with_icon(
                                "Quote",
                                Lucide::Quote,
                                Box::new(FormatQuote),
                            )
                        },
                    )
            })
    }

    /// "…" for the open note: pin, export, trash (same order as every row menu).
    fn note_menu(&self, pinned: bool, cx: &Context<Self>) -> impl IntoElement {
        let view = cx.entity().downgrade();
        Button::new("note-more")
            .ghost()
            .small()
            .icon(Lucide::Ellipsis)
            .tooltip("More")
            .dropdown_menu(move |menu, _, _| {
                let run = |label: &'static str, icon: Lucide, f: fn(&mut NotesView, &mut Window, &mut Context<NotesView>)| {
                    let view = view.clone();
                    PopupMenuItem::new(label).icon(icon).on_click(move |_, window, cx| {
                        _ = view.update(cx, |this, cx| f(this, window, cx));
                    })
                };
                menu.item(if pinned {
                    run("Unpin", Lucide::PinOff, |this, w, cx| this.toggle_pin_open(w, cx))
                } else {
                    run("Pin to top", Lucide::Pin, |this, w, cx| this.toggle_pin_open(w, cx))
                })
                .item(run("Export all as Markdown…", Lucide::FolderOutput, |this, w, cx| this.export_notes(w, cx)))
                .separator()
                .item(run("Move to trash", Lucide::Trash, |this, w, cx| {
                    if let Some(id) = this.open.as_ref().map(|o| o.id) {
                        this.trash_note(id, w, cx);
                    }
                }))
            })
    }

    fn toggle_pin_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((id, pinned)) = self.open.as_ref().map(|o| (o.id, o.pinned)) {
            self.set_pinned(id, !pinned, window, cx);
        }
    }

    /// Switches between editing and the rendered preview (⌘E).
    pub fn toggle_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.as_ref().is_none_or(|o| o.trashed) {
            return;
        }
        _ = self.flush(window, cx);
        self.mode = match self.mode {
            Mode::Edit => Mode::Preview,
            Mode::Preview => Mode::Edit,
        };
        if self.mode == Mode::Edit {
            self.body.update(cx, |b, cx| b.focus(window, cx));
        } else {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    pub fn is_previewing(&self) -> bool {
        self.mode == Mode::Preview
    }

    /// Opens (creating if needed) today's daily note.
    pub fn open_daily_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.flush(window, cx) {
            return;
        }
        let today = keepstone_core::clock::today();
        let result = self
            .library
            .update(cx, |lib, cx| lib.open_daily_note(today, cx));
        if let Some(id) = feedback::report(result, window, cx) {
            self.show_trash = false;
            self.tag_filter = None;
            self.reload_list(cx);
            self.open_note(id, window, cx);
            self.body.update(cx, |b, cx| b.focus(window, cx));
        }
    }

    fn link_mention(&mut self, source: NoteId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(title) = self.open.as_ref().map(|o| o.title.clone()) else {
            return;
        };
        let result = self
            .library
            .update(cx, |lib, cx| lib.link_mention(source, &title, cx));
        if feedback::report(result, window, cx).is_some() {
            self.reload_backlinks(cx);
            cx.notify();
        }
    }

    fn reload_backlinks(&mut self, cx: &mut Context<Self>) {
        self.mentions = match &self.open {
            Some(open) if !open.trashed => self
                .library
                .read(cx)
                .unlinked_mentions(open.id, &open.title),
            _ => Vec::new(),
        };
        self.backlinks = match &self.open {
            Some(open) if !open.trashed => self
                .library
                .read(cx)
                .backlinks(&open.title)
                .unwrap_or_default()
                .into_iter()
                .filter(|b| b.id != open.id)
                .collect(),
            _ => Vec::new(),
        };
    }

    fn schedule_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open.as_ref().is_none_or(|o| o.trashed) {
            return;
        }
        self.dirty = true;
        // Replacing the task cancels the previous timer: a debounce.
        self.pending_save = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(AUTOSAVE_DELAY).await;
            _ = this.update_in(cx, |this, window, cx| this.flush(window, cx));
        }));
    }

    /// Writes pending edits now. Returns `false` if saving failed: the editor
    /// stays Dirty and callers must not replace its content (that would lose typing).
    fn flush(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.save_pending(cx) {
            Ok(()) => true,
            Err(err) => {
                notify_err::<()>(Err(err), window, cx);
                false
            }
        }
    }

    /// Saves pending edits without UI feedback (used on quit and window close).
    pub fn save_pending(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        self.pending_save = None;
        if !self.dirty {
            return Ok(());
        }
        let Some(open) = &self.open else {
            self.dirty = false;
            return Ok(());
        };
        let id = open.id;
        let title = self.title.read(cx).value().to_string();
        let body = self.body.read(cx).value().to_string();
        self.library
            .update(cx, |lib, cx| lib.edit_note(id, &title, &body, cx))?;
        self.dirty = false;
        let saved_title = keepstone_core::normalize_title(&title)
            .unwrap_or_else(|_| keepstone_core::note::UNTITLED.to_owned());
        if let Some(open) = &mut self.open
            && open.title != saved_title
        {
            open.title = saved_title;
            self.reload_backlinks(cx);
        }
        Ok(())
    }

    fn set_pinned(
        &mut self,
        id: NoteId,
        pinned: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let result = self
            .library
            .update(cx, |lib, cx| lib.set_note_pinned(id, pinned, cx));
        if feedback::report(result, window, cx).is_some()
            && let Some(open) = self.open.as_mut().filter(|o| o.id == id)
        {
            open.pinned = pinned;
        }
    }

    fn restore_note(&mut self, id: NoteId, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.library.update(cx, |lib, cx| lib.restore_note(id, cx));
        if feedback::report(result, window, cx).is_some() {
            self.show_trash = false;
            self.reload_list(cx);
            self.open_note(id, window, cx);
        }
    }

    /// Moves any note to the trash (the open one or one from the list) and offers Undo.
    fn trash_note(&mut self, id: NoteId, window: &mut Window, cx: &mut Context<Self>) {
        let is_open = self.open.as_ref().is_some_and(|o| o.id == id);
        if is_open && !self.flush(window, cx) {
            return;
        }
        let result = self.library.update(cx, |lib, cx| lib.trash_note(id, cx));
        if feedback::report(result, window, cx).is_none() {
            return;
        }
        if is_open {
            self.open = None;
        }
        let library = self.library.clone();
        let view = cx.entity().downgrade();
        feedback::undo_toast("Note moved to trash.", window, cx, move |window, cx| {
            if feedback::report(
                library.update(cx, |lib, cx| lib.restore_note(id, cx)),
                window,
                cx,
            )
            .is_some()
            {
                _ = view.update(cx, |this, cx| this.open_note(id, window, cx));
            }
        });
        cx.notify();
    }

    fn restore_open(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.open.as_ref().map(|o| o.id) {
            self.restore_note(id, window, cx);
        }
    }

    fn delete_open(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.open.as_ref() else {
            return;
        };
        let id = open.id;
        let library = self.library.clone();
        let view = cx.entity().downgrade();
        feedback::confirm_destructive(
            Confirm::new(
                format!("Delete \u{201c}{}\u{201d} forever?", open.title),
                "The note and its links will be removed. This can't be undone.",
                "Delete",
            ),
            window,
            cx,
            move |window, cx| {
                if feedback::report(
                    library.update(cx, |lib, cx| lib.delete_note_permanently(id, cx)),
                    window,
                    cx,
                )
                .is_some()
                {
                    _ = view.update(cx, |this, cx| {
                        this.open = None;
                        cx.notify();
                    });
                }
            },
        );
    }

    fn empty_trash(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let library = self.library.clone();
        let view = cx.entity().downgrade();
        feedback::confirm_destructive(
            Confirm::new(
                "Empty note trash?",
                format!(
                    "{} note(s) will be deleted forever. This can't be undone.",
                    self.list.len()
                ),
                "Empty trash",
            ),
            window,
            cx,
            move |window, cx| {
                if feedback::report(
                    library.update(cx, |lib, cx| lib.empty_note_trash(cx)),
                    window,
                    cx,
                )
                .is_some()
                {
                    _ = view.update(cx, |this, cx| {
                        this.open = None;
                        cx.notify();
                    });
                }
            },
        );
    }

    pub fn is_showing_trash(&self) -> bool {
        self.show_trash
    }

    pub fn set_show_trash(&mut self, show: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.show_trash == show {
            return;
        }
        if !self.flush(window, cx) {
            return;
        }
        self.show_trash = show;
        self.open = None;
        self.reload_list(cx);
    }

    fn render_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let selected = self.open.as_ref().map(|o| o.id);
        let searching = !self.search.read(cx).value().trim().is_empty();
        let view = cx.entity().downgrade();

        // One calm row: search, new note, and "…" for everything else
        // (tags, trash, import/export). The sidebar already says "Notes".
        let header = if self.show_trash {
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    Button::new("notes-trash-back")
                        .ghost()
                        .small()
                        .icon(Lucide::ArrowLeft)
                        .tooltip("Back to notes")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.set_show_trash(false, window, cx)
                        })),
                )
                .child(
                    div()
                        .flex_1()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Trash"),
                )
                .when(!self.list.is_empty(), |this| {
                    this.child(
                        Button::new("notes-empty-trash")
                            .ghost()
                            .small()
                            .label("Empty")
                            .tooltip("Delete everything in the trash forever")
                            .on_click(cx.listener(Self::empty_trash)),
                    )
                })
        } else {
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    div().flex_1().min_w_0().child(
                        Input::new(&self.search).small().cleanable(true).prefix(
                            Icon::new(Lucide::Search)
                                .small()
                                .text_color(theme.muted_foreground),
                        ),
                    ),
                )
                .child(
                    Button::new("notes-new")
                        .ghost()
                        .small()
                        .icon(Lucide::SquarePen)
                        .tooltip_with_action("New note", &crate::actions::NewNote, None)
                        .on_click(cx.listener(|this, _, window, cx| this.new_note(window, cx))),
                )
                .child(self.list_menu_button(cx))
        };

        let in_trash = self.show_trash;
        let rows = self.list.iter().map(|note| {
            let id = note.id;
            let pinned = note.pinned;
            let active = selected == Some(id);
            let view = view.clone();
            let menu_view = view.clone();
            v_flex()
                .id(ElementId::Name(SharedString::from(format!("note-{id}"))))
                .w_full()
                .px_3()
                .py_2()
                .gap_0p5()
                .rounded(theme.radius)
                .when(active, |this| this.bg(theme.list_active))
                .when(!active, |this| this.hover(|this| this.bg(theme.list_hover)))
                .on_click(move |_, window, cx| {
                    _ = view.update(cx, |this, cx| this.open_note(id, window, cx));
                })
                .test_support()
                .child(
                    h_flex()
                        .gap_1()
                        .when(note.pinned, |this| {
                            this.child(
                                Icon::new(Lucide::Pin)
                                    .xsmall()
                                    .text_color(theme.muted_foreground),
                            )
                        })
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_sm()
                                .font_weight(FontWeight::MEDIUM)
                                .child(note.title.clone()),
                        ),
                )
                .child(
                    div()
                        .truncate()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(if note.preview.is_empty() {
                            "No content".to_owned()
                        } else {
                            note.preview.clone()
                        }),
                )
                .context_menu(move |menu, _, _| {
                    note_row_menu(menu, &menu_view, id, pinned, in_trash)
                })
        });

        let empty = match (self.show_trash, searching) {
            _ if self.load_error => empty_state(
                Lucide::TriangleAlert,
                "Couldn't load notes",
                "Your notes are still on disk. Restart Keepstone; if this keeps happening, check the data folder in Settings.",
                cx,
            ),
            (true, _) => empty_state(
                Lucide::Trash,
                "Trash is empty",
                "Deleted notes wait here until you remove them.",
                cx,
            ),
            (false, true) => empty_state(
                Lucide::Search,
                "No matches",
                "Try fewer or different words.",
                cx,
            ),
            (false, false) => empty_state(
                Lucide::NotebookPen,
                "No notes yet",
                format!("Create one with {}N.", crate::actions::modifier_label()),
                cx,
            ),
        };

        v_flex()
            .size_full()
            .border_r_1()
            .border_color(theme.border)
            .child(
                v_flex()
                    .px_3()
                    .pt_4()
                    .pb_2()
                    .gap_2()
                    .child(header)
                    .when_some(
                        self.tag_filter.clone().filter(|_| !self.show_trash),
                        |this, tag| {
                            this.child(
                                h_flex().child(
                                    Button::new("clear-tag-filter")
                                        .outline()
                                        .xsmall()
                                        .label(format!("#{tag}"))
                                        .icon(Lucide::X)
                                        .tooltip("Show all notes")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.set_tag_filter(None, window, cx)
                                        })),
                                ),
                            )
                        },
                    ),
            )
            .child(
                div()
                    .id("notes-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .pb_2()
                    .map(|this| {
                        if self.list.is_empty() {
                            this.child(empty)
                        } else {
                            this.child(v_flex().gap_0p5().children(rows))
                        }
                    }),
            )
    }

    /// "…" next to New note: filter by tag, today's note, trash, import/export.
    fn list_menu_button(&self, cx: &Context<Self>) -> impl IntoElement {
        let tags = self.library.read(cx).note_tags();
        let current = self.tag_filter.clone();
        let view = cx.entity().downgrade();
        Button::new("notes-more")
            .ghost()
            .small()
            .icon(Lucide::Ellipsis)
            .selected(current.is_some())
            .tooltip("More")
            .dropdown_menu(move |menu, window, cx| {
                let v = view.clone();
                let run = move |label: &'static str, icon: Lucide, f: fn(&mut NotesView, &mut Window, &mut Context<NotesView>)| {
                    let v = v.clone();
                    PopupMenuItem::new(label).icon(icon).on_click(move |_, window, cx| {
                        _ = v.update(cx, |this, cx| f(this, window, cx));
                    })
                };
                let tags = tags.clone();
                let current = current.clone();
                let tag_view = view.clone();
                menu.item(run("Today's note", Lucide::CalendarDays, |this, w, cx| this.open_daily_note(w, cx)))
                    .separator()
                    .submenu_with_icon(
                        Some(Lucide::Hash.into()),
                        match &current {
                            Some(tag) => format!("Tag: #{tag}"),
                            None => "Filter by tag".to_owned(),
                        },
                        window,
                        cx,
                        move |sub, _, _| {
                            if tags.is_empty() {
                                return sub.label("No tags yet. Type #tag in a note.");
                            }
                            let all = tag_view.clone();
                            let sub = sub
                                .item(PopupMenuItem::new("All notes").checked(current.is_none()).on_click(move |_, window, cx| {
                                    _ = all.update(cx, |this, cx| this.set_tag_filter(None, window, cx));
                                }))
                                .separator();
                            tags.iter().fold(sub, |sub, (tag, count)| {
                                let view = tag_view.clone();
                                let value = tag.clone();
                                sub.item(
                                    PopupMenuItem::new(format!("#{tag}  ·  {count}"))
                                        .checked(current.as_deref() == Some(tag.as_str()))
                                        .on_click(move |_, window, cx| {
                                            _ = view.update(cx, |this, cx| this.set_tag_filter(Some(value.clone()), window, cx));
                                        }),
                                )
                            })
                        },
                    )
                    .separator()
                    .item(run("Import Markdown folder…", Lucide::FolderInput, |this, w, cx| this.import_notes(w, cx)))
                    .item(run("Export as Markdown…", Lucide::FolderOutput, |this, w, cx| this.export_notes(w, cx)))
                    .separator()
                    .item(run("Trash", Lucide::Trash, |this, w, cx| this.set_show_trash(true, w, cx)))
            })
    }

    /// Asks for a folder and imports its `.md` files (Obsidian vaults work).
    pub fn import_notes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let library = self.library.clone();
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Import".into()),
        });
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(paths))) = paths.await else { return };
            let Some(dir) = paths.into_iter().next() else { return };
            _ = cx.update(|window, cx| {
                match library.update(cx, |lib, cx| lib.import_notes(&dir, cx)) {
                    Ok((imported, skipped)) => {
                        let msg = match skipped {
                            0 => format!("Imported {imported} note(s)."),
                            n => format!("Imported {imported} note(s). Skipped {n} with titles that already exist."),
                        };
                        window.push_notification(msg, cx);
                    }
                    Err(err) => feedback::error_toast(format!("{err:#}"), window, cx),
                }
            });
        })
        .detach();
    }

    /// Asks for a folder and writes every note there as `Title.md`.
    pub fn export_notes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.flush(window, cx) {
            return;
        }
        let library = self.library.clone();
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Export here".into()),
        });
        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            _ = cx.update(|window, cx| match library.read(cx).export_notes(&dir) {
                Ok(n) => window
                    .push_notification(format!("Exported {n} note(s) to {}.", dir.display()), cx),
                Err(err) => feedback::error_toast(format!("{err:#}"), window, cx),
            });
        })
        .detach();
    }

    fn render_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let width = rems(appearance::settings(cx).content_width.max_width_rems());
        let Some(open) = &self.open else {
            return v_flex().flex_1().child(empty_state(
                Lucide::FileText,
                "No note selected",
                "Pick a note from the list, or create a new one.",
                cx,
            ));
        };
        let trashed = open.trashed;
        let pinned = open.pinned;

        // One icon toggle instead of an Edit/Preview label pair: it shows what you
        // switch *to* (eye = preview, pencil = edit). ⌘E does the same.
        let previewing = self.mode == Mode::Preview;
        let mode_toggle = Button::new("note-mode")
            .ghost()
            .small()
            .icon(if previewing {
                Lucide::Pencil
            } else {
                Lucide::Eye
            })
            .tooltip_with_action(
                if previewing { "Edit" } else { "Preview" },
                &crate::actions::TogglePreview,
                None,
            )
            .on_click(cx.listener(|this, _, window, cx| this.toggle_mode(window, cx)));

        let toolbar = h_flex()
            .justify_between()
            .items_center()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .child(if trashed {
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("In trash · read-only")
                    .into_any_element()
            } else {
                div().into_any_element()
            })
            .child(h_flex().gap_1().map(|this| {
                if trashed {
                    this.child(
                        Button::new("note-restore")
                            .small()
                            .outline()
                            .icon(Lucide::ArchiveRestore)
                            .label("Restore")
                            .on_click(cx.listener(Self::restore_open)),
                    )
                    .child(
                        Button::new("note-delete")
                            .small()
                            .danger()
                            .label("Delete forever")
                            .on_click(cx.listener(Self::delete_open)),
                    )
                } else {
                    // Three controls: format, edit/preview, and "…" for the rest.
                    this.when(self.mode == Mode::Edit, |this| {
                        this.child(self.format_menu(cx))
                    })
                    .child(mode_toggle)
                    .child(self.note_menu(pinned, cx))
                }
            }));

        let preview = self.mode == Mode::Preview || trashed;
        let body_text = self.body.read(cx).value();
        let title_text = self.title.read(cx).value();

        // One centered column per child, so the scroll handle knows where the
        // editor (child 1) is and can keep the caret in view.
        let column = || div().w_full().max_w(width).mx_auto().px_8();
        let line_height = theme.font_size * 1.5; // gpui-kit editor rows are 1.5× the font size
        let editor_height = self.editor_height(line_height);

        let page = v_flex()
            .id("note-scroll")
            .track_scroll(&self.page_scroll)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .pt_6()
            .pb_8()
            .map(|this| {
                if preview {
                    this.child(column().pb_4().child(
                        div().text_2xl().font_weight(FontWeight::SEMIBOLD).child(
                            if title_text.is_empty() {
                                SharedString::from(keepstone_core::note::UNTITLED)
                            } else {
                                title_text.clone()
                            },
                        ),
                    ))
                    .child(column().child(self.render_preview(&body_text, cx)))
                } else {
                    this.child(
                        column().pb_2().child(
                            Input::new(&self.title)
                                .appearance(false)
                                .large()
                                .text_2xl()
                                .font_weight(FontWeight::SEMIBOLD),
                        ),
                    )
                    .child(
                        column().child(
                            // Format shortcuts (⌘B, ⌘⌥1, …) are bound to this context.
                            div()
                                .id("note-body")
                                .test_support()
                                .key_context(crate::actions::NOTE_EDITOR_CONTEXT)
                                .on_action(cx.listener(|this, _: &FormatBold, w, cx| {
                                    this.format_inline(Inline::Bold, w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &FormatItalic, w, cx| {
                                    this.format_inline(Inline::Italic, w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &FormatStrikethrough, w, cx| {
                                    this.format_inline(Inline::Strikethrough, w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &FormatCode, w, cx| {
                                    this.format_inline(Inline::Code, w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &FormatHighlight, w, cx| {
                                    this.format_inline(Inline::Highlight, w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &FormatLink, w, cx| {
                                    this.format_with(format::insert_link, w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &FormatHeading1, w, cx| {
                                    this.format_block(Block::Heading(1), w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &FormatHeading2, w, cx| {
                                    this.format_block(Block::Heading(2), w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &FormatHeading3, w, cx| {
                                    this.format_block(Block::Heading(3), w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &FormatParagraph, w, cx| {
                                    this.format_block(Block::Paragraph, w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &FormatBullets, w, cx| {
                                    this.format_block(Block::Bullet, w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &FormatNumbers, w, cx| {
                                    this.format_block(Block::Numbered, w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &FormatChecklist, w, cx| {
                                    this.format_block(Block::Checklist, w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &FormatQuote, w, cx| {
                                    this.format_block(Block::Quote, w, cx)
                                }))
                                .on_action(cx.listener(|this, _: &ToggleCheckbox, w, cx| {
                                    this.format_with(
                                        |doc, sel| {
                                            format::toggle_checkbox(doc, sel.start).unwrap_or(
                                                format::Edit {
                                                    range: sel.clone(),
                                                    text: doc[sel.clone()].to_owned(),
                                                    selection: sel,
                                                },
                                            )
                                        },
                                        w,
                                        cx,
                                    )
                                }))
                                .child(
                                    Editor::new(&self.body)
                                        .appearance(false)
                                        // Notes are prose: the UI font, not the code font.
                                        .font_family(theme.font_family.clone())
                                        .text_size(theme.font_size)
                                        // Exactly as tall as the text: the page scrolls, not the editor.
                                        .h(editor_height),
                                ),
                        ),
                    )
                }
            })
            .when(!self.backlinks.is_empty(), |this| {
                this.child(column().pt_4().child(self.render_backlinks(cx)))
            })
            .when(!self.mentions.is_empty() && !trashed, |this| {
                this.child(column().child(self.render_mentions(cx)))
            })
            .test_support();

        if !preview {
            self.follow_caret(line_height, cx);
        }
        let _ = window;
        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(toolbar)
            .child(page)
    }

    /// Height that fits every wrapped row, so the editor never scrolls on its
    /// own. Re-renders once more when the row count changed (wrapping happens
    /// during layout), which settles on the next frame.
    fn editor_height(&mut self, line_height: Pixels) -> Pixels {
        const MIN_ROWS: usize = 12; // a comfortable empty page to click into
        const PADDING: f32 = 16.; // gpui-kit input padding, top + bottom
        let rows = self.body_rows.max(1);
        line_height * (rows.max(MIN_ROWS) as f32 + 1.) + px(PADDING)
    }

    /// (wrapped row count, caret row) of the body editor.
    fn body_metrics(&self, cx: &App) -> (usize, usize) {
        let body = self.body.read(cx);
        let text = body.value();
        let offset = body.cursor().min(text.len());
        let line = text[..offset].matches('\n').count();
        let line_start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
        let display = body.display_map();
        let caret = display
            .buffer_pos_to_display_pos(BufferPoint::new(line, offset - line_start))
            .row;
        (display.wrap_row_count(), caret)
    }

    /// Scrolls the page so the caret stays visible while typing or moving.
    fn follow_caret(&mut self, line_height: Pixels, cx: &mut Context<Self>) {
        let (rows, caret) = self.body_metrics(cx);
        if rows != self.body_rows {
            self.body_rows = rows;
            cx.notify();
        }
        if caret == self.caret_row {
            return;
        }
        self.caret_row = caret;
        let (Some(editor), viewport) = (
            self.page_scroll.bounds_for_item(1),
            self.page_scroll.bounds(),
        ) else {
            return;
        };
        let margin = line_height * 2.;
        let top = editor.top() + px(8.) + line_height * caret as f32;
        let bottom = top + line_height;
        let mut offset = self.page_scroll.offset();
        if bottom + margin > viewport.bottom() {
            offset.y -= bottom + margin - viewport.bottom();
        } else if top - margin < viewport.top() {
            offset.y += viewport.top() - (top - margin);
        }
        offset.y = offset.y.min(px(0.));
        self.page_scroll.set_offset(offset);
    }

    /// Markdown preview where `[[links]]` are clickable and open (or create) the target note.
    fn render_preview(&self, body: &str, cx: &Context<Self>) -> impl IntoElement {
        let source = preview_markdown(body);
        let view = cx.entity().downgrade();
        TextView::markdown("note-preview", source)
            .selectable(true)
            .on_link_click(move |url, _, window, cx| {
                if let Some(tag) = parse_tag_url(url) {
                    _ = view.update(cx, |this, cx| this.set_tag_filter(Some(tag), window, cx));
                } else if let Some(target) = parse_note_link_url(url) {
                    _ = view.update(cx, |this, cx| this.follow_link(&target, window, cx));
                } else if url.starts_with("https://")
                    || url.starts_with("http://")
                    || url.starts_with("mailto:")
                {
                    cx.open_url(url);
                }
            })
    }

    /// Opens the note a link points to, creating it if needed.
    pub fn follow_link(&mut self, target: &str, window: &mut Window, cx: &mut Context<Self>) {
        if !self.flush(window, cx) {
            return;
        }
        let result = self
            .library
            .update(cx, |lib, cx| lib.open_or_create_by_title(target, cx));
        if let Some(id) = feedback::report(result, window, cx) {
            self.open_note(id, window, cx);
        }
    }

    fn render_backlinks(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let view = cx.entity().downgrade();
        v_flex()
            .gap_1()
            .pt_4()
            .border_t_1()
            .border_color(theme.border)
            .child(section_label(
                format!(
                    "Linked from {} note{}",
                    self.backlinks.len(),
                    if self.backlinks.len() == 1 { "" } else { "s" }
                ),
                cx,
            ))
            .children(self.backlinks.iter().map(|b| {
                let id = b.id;
                let view = view.clone();
                h_flex()
                    .id(ElementId::Name(SharedString::from(format!(
                        "backlink-{id}"
                    ))))
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(theme.radius)
                    .hover(|this| this.bg(theme.list_hover))
                    .on_click(move |_, window, cx| {
                        _ = view.update(cx, |this, cx| this.open_note(id, window, cx));
                    })
                    .child(
                        Icon::new(Lucide::Link2)
                            .xsmall()
                            .text_color(theme.muted_foreground),
                    )
                    .child(div().text_sm().child(b.title.clone()))
            }))
    }

    fn render_mentions(&self, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let view = cx.entity().downgrade();
        v_flex()
            .gap_1()
            .pt_4()
            .child(section_label(
                format!(
                    "Mentioned in {} note{} without a link",
                    self.mentions.len(),
                    if self.mentions.len() == 1 { "" } else { "s" }
                ),
                cx,
            ))
            .children(self.mentions.iter().map(|m| {
                let id = m.id;
                let open_view = view.clone();
                let link_view = view.clone();
                h_flex()
                    .id(ElementId::Name(SharedString::from(format!("mention-{id}"))))
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded(theme.radius)
                    .hover(|this| this.bg(theme.list_hover))
                    .on_click(move |_, window, cx| {
                        _ = open_view.update(cx, |this, cx| this.open_note(id, window, cx));
                    })
                    .child(
                        Icon::new(Lucide::FileText)
                            .xsmall()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .child(m.title.clone()),
                    )
                    .child(
                        Button::new(SharedString::from(format!("link-mention-{id}")))
                            .ghost()
                            .xsmall()
                            .label("Link")
                            .tooltip("Turn the first mention into a [[link]]")
                            .on_click(move |_, window, cx| {
                                cx.stop_propagation();
                                _ = link_view
                                    .update(cx, |this, cx| this.link_mention(id, window, cx));
                            }),
                    )
            }))
    }
}

const TAG_URL_PREFIX: &str = "keepstone://tag/";

/// Markdown for the preview: `[[links]]` and `#tags` become clickable links.
fn preview_markdown(body: &str) -> String {
    let links = keepstone_core::links::wiki_link_spans(body);
    let tags: Vec<_> = keepstone_core::tags::tag_spans(body)
        .into_iter()
        .filter(|(r, _)| {
            !links
                .iter()
                .any(|(l, _)| l.start < r.end && r.start < l.end)
        })
        .collect();
    let mut spans: Vec<(std::ops::Range<usize>, String)> = links
        .into_iter()
        .map(|(range, link)| {
            let text = link.alias.clone().unwrap_or_else(|| match &link.heading {
                Some(h) => format!("{} › {h}", link.target),
                None => link.target.clone(),
            });
            (
                range,
                format!(
                    "[{}]({})",
                    escape_link_text(&text),
                    note_link_url(&link.target)
                ),
            )
        })
        .chain(tags.into_iter().map(|(range, tag)| {
            let url = note_link_url(&tag).replacen(
                keepstone_core::links::NOTE_LINK_PREFIX,
                TAG_URL_PREFIX,
                1,
            );
            let text = format!("#{}", &body[range.start + 1..range.end]);
            (range, format!("[{}]({url})", escape_link_text(&text)))
        }))
        .collect();
    // ==highlight== → <mark>…</mark> (the renderer understands the HTML tag).
    // Delimiters inside a link or tag are left as typed.
    let marks: Vec<_> = keepstone_core::format::highlight_spans(body)
        .into_iter()
        .filter(|(open, close)| {
            !spans.iter().any(|(r, _)| {
                (r.start < open.end && open.start < r.end)
                    || (r.start < close.end && close.start < r.end)
            })
        })
        .flat_map(|(open, close)| [(open, "<mark>".to_owned()), (close, "</mark>".to_owned())])
        .collect();
    spans.extend(marks);
    spans.sort_by_key(|(r, _)| r.start);
    let mut out = String::with_capacity(body.len() + 64);
    let mut last = 0;
    for (range, replacement) in spans {
        out.push_str(&body[last..range.start]);
        out.push_str(&replacement);
        last = range.end;
    }
    out.push_str(&body[last..]);
    out
}

fn escape_link_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '[' | ']' | '\\' | '*' | '_' | '`') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn parse_tag_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix(TAG_URL_PREFIX)?;
    parse_note_link_url(&format!(
        "{}{rest}",
        keepstone_core::links::NOTE_LINK_PREFIX
    ))
}

/// Right-click menu for a note in the list. Same order as task menus:
/// open → change → trash.
fn note_row_menu(
    menu: PopupMenu,
    view: &gpui_kit::WeakEntity<NotesView>,
    id: NoteId,
    pinned: bool,
    in_trash: bool,
) -> PopupMenu {
    let run = |label: &'static str,
               icon: Lucide,
               f: fn(&mut NotesView, NoteId, &mut Window, &mut Context<NotesView>)| {
        let view = view.clone();
        PopupMenuItem::new(label)
            .icon(icon)
            .on_click(move |_, window, cx| {
                _ = view.update(cx, |this, cx| f(this, id, window, cx));
            })
    };
    if in_trash {
        return menu
            .item(run("Open", Lucide::FileText, |this, id, window, cx| {
                this.open_note(id, window, cx)
            }))
            .item(run(
                "Restore",
                Lucide::ArchiveRestore,
                |this, id, window, cx| this.restore_note(id, window, cx),
            ));
    }
    menu.item(run("Open", Lucide::FileText, |this, id, window, cx| {
        this.open_note(id, window, cx)
    }))
    .separator()
    .item(if pinned {
        run("Unpin", Lucide::PinOff, |this, id, window, cx| {
            this.set_pinned(id, false, window, cx)
        })
    } else {
        run("Pin to top", Lucide::Pin, |this, id, window, cx| {
            this.set_pinned(id, true, window, cx)
        })
    })
    .separator()
    .item(run(
        "Move to trash",
        Lucide::Trash,
        |this, id, window, cx| this.trash_note(id, window, cx),
    ))
}

impl Focusable for NotesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for NotesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let layout = self.layout.read(cx);
        let panel = layout.panel(NOTES_LIST);
        let toggles = layout.panel_toggles(NOTES_LIST);
        let store = self.layout.clone();
        h_flex()
            .track_focus(&self.focus)
            .size_full()
            .child(
                SidePanel::new(NOTES_LIST.id, PanelSide::Left)
                    .width(panel.width, NOTES_LIST.default)
                    .collapsed(panel.collapsed, toggles)
                    .on_resize(move |width, _, cx| {
                        store.update(cx, |layout, cx| {
                            layout.set_panel_width(NOTES_LIST, width, cx)
                        });
                    })
                    .child(self.render_list(cx)),
            )
            .child(self.render_editor(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::preview_markdown;

    #[test]
    fn preview_turns_highlights_into_mark_tags() {
        assert_eq!(
            preview_markdown("Pay ==rent== by `==code==`"),
            "Pay <mark>rent</mark> by `==code==`"
        );
        let linked = preview_markdown("==See [[Car]]== now");
        assert!(linked.starts_with("<mark>See ["), "{linked}");
        assert!(linked.ends_with("</mark> now"), "{linked}");
    }
}
