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
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Icon, Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, ClickEvent, Context, ElementId, Entity, EventEmitter, FocusHandle,
    Focusable, FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
    rems,
};
use keepstone_core::NoteId;
use keepstone_core::links::{note_link_url, parse_note_link_url, to_markdown_links};
use keepstone_storage::NoteSummary;
use keepstone_ui::appearance;
use keepstone_ui::components::{empty_state, section_label, segmented};
use keepstone_ui::feedback::{self, Confirm};
use keepstone_ui::panel::{PanelSide, SidePanel};

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
        } else if query.trim().is_empty() {
            library.notes(false)
        } else {
            library.search_notes(&query, SEARCH_LIMIT)
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

    fn reload_backlinks(&mut self, cx: &mut Context<Self>) {
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

    fn toggle_pin(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((id, pinned)) = self.open.as_ref().map(|o| (o.id, o.pinned)) {
            self.set_pinned(id, !pinned, window, cx);
        }
    }

    fn trash_open(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.open.as_ref().map(|o| o.id) {
            self.trash_note(id, window, cx);
        }
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

        let header = h_flex()
            .justify_between()
            .items_center()
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if self.show_trash { "Trash" } else { "Notes" }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .when(self.show_trash && !self.list.is_empty(), |this| {
                        this.child(
                            Button::new("notes-empty-trash")
                                .ghost()
                                .small()
                                .label("Empty")
                                .tooltip("Delete everything in the trash forever")
                                .on_click(cx.listener(Self::empty_trash)),
                        )
                    })
                    .child(
                        Button::new("notes-trash-toggle")
                            .ghost()
                            .small()
                            .icon(if self.show_trash {
                                Lucide::NotebookText
                            } else {
                                Lucide::Trash
                            })
                            .tooltip(if self.show_trash {
                                "Back to notes"
                            } else {
                                "Show trash"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                let show = !this.show_trash;
                                this.set_show_trash(show, window, cx);
                            })),
                    )
                    .child(
                        Button::new("notes-new")
                            .ghost()
                            .small()
                            .icon(Lucide::SquarePen)
                            .tooltip("New note")
                            .on_click(cx.listener(|this, _, window, cx| this.new_note(window, cx))),
                    ),
            );

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
            .child(v_flex().px_3().pt_4().pb_2().gap_2().child(header).when(
                !self.show_trash,
                |this| {
                    this.child(
                        Input::new(&self.search).small().cleanable(true).prefix(
                            Icon::new(Lucide::Search)
                                .small()
                                .text_color(theme.muted_foreground),
                        ),
                    )
                },
            ))
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
        let view = cx.entity().downgrade();

        let mode_switch = segmented(
            "note-mode",
            &[(Mode::Edit, "Edit"), (Mode::Preview, "Preview")],
            self.mode,
            move |mode, window, cx| {
                _ = view.update(cx, |this, cx| {
                    _ = this.flush(window, cx);
                    this.mode = mode;
                    cx.notify();
                });
            },
        );

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
                mode_switch.into_any_element()
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
                    this.child(
                        Button::new("note-pin")
                            .ghost()
                            .small()
                            .icon(if pinned { Lucide::PinOff } else { Lucide::Pin })
                            .selected(pinned)
                            .tooltip(if pinned { "Unpin" } else { "Pin to top" })
                            .on_click(cx.listener(Self::toggle_pin)),
                    )
                    .child(
                        Button::new("note-trash")
                            .ghost()
                            .small()
                            .icon(Lucide::Trash)
                            .tooltip("Move to trash")
                            .on_click(cx.listener(Self::trash_open)),
                    )
                }
            }));

        let preview = self.mode == Mode::Preview || trashed;
        let body_text = self.body.read(cx).value();
        let title_text = self.title.read(cx).value();

        let content = v_flex()
            .w_full()
            .max_w(width)
            .mx_auto()
            .px_8()
            .py_6()
            .gap_4()
            .flex_1()
            .map(|this| {
                if preview {
                    this.child(div().text_2xl().font_weight(FontWeight::SEMIBOLD).child(
                        if title_text.is_empty() {
                            SharedString::from(keepstone_core::note::UNTITLED)
                        } else {
                            title_text.clone()
                        },
                    ))
                    .child(self.render_preview(&body_text, cx))
                } else {
                    this.child(
                        Input::new(&self.title)
                            .appearance(false)
                            .large()
                            .text_2xl()
                            .font_weight(FontWeight::SEMIBOLD),
                    )
                    .child(
                        Editor::new(&self.body)
                            .appearance(false)
                            // Notes are prose: the UI font, not the code font.
                            .font_family(theme.font_family.clone())
                            .text_size(theme.font_size)
                            .flex_1()
                            .min_h(rems(20.)),
                    )
                }
            })
            .when(!self.backlinks.is_empty(), |this| {
                this.child(self.render_backlinks(cx))
            });

        let _ = window;
        v_flex().flex_1().min_w_0().h_full().child(toolbar).child(
            div()
                .id("note-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(content),
        )
    }

    /// Markdown preview where `[[links]]` are clickable and open (or create) the target note.
    fn render_preview(&self, body: &str, cx: &Context<Self>) -> impl IntoElement {
        let source = to_markdown_links(body, |link| note_link_url(&link.target));
        let view = cx.entity().downgrade();
        TextView::markdown("note-preview", source)
            .selectable(true)
            .on_link_click(move |url, _, window, cx| match parse_note_link_url(url) {
                Some(target) => {
                    _ = view.update(cx, |this, cx| this.follow_link(&target, window, cx));
                }
                None => cx.open_url(url),
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
