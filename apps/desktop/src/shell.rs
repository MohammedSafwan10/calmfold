//! The main window: title bar, sidebar, the active screen, and the command palette.
//!
//! Commands are app-level action handlers that forward to the shell. That way
//! a shortcut, a menu-bar item, and a palette entry all run the same code no
//! matter which element has focus (the palette lives in an overlay outside the
//! shell's element tree).

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::InteractiveElementExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::resizable::{HandleEdge, resize_handle};
use gpui_kit::component::sidebar::{
    Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
    SidebarMenuItem,
};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _, TitleBar, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyView, AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle,
    Focusable as _, FontWeight, Global, InteractiveElement as _, IntoElement, Keystroke,
    ParentElement as _, Render, Styled as _, Subscription, WeakEntity, Window, div, px, rems,
};
use keepstone_core::NoteId;
use keepstone_ui::appearance::{self, ThemePreference};

use crate::actions::{
    GoNotes, GoSettings, GoTasks, GoToday, NewNote, NewTask, OpenCommandPalette, ToggleSidebar,
    UseDarkTheme, UseLightTheme, UseSystemTheme,
};
use crate::library::{Library, LibraryEvent};
use crate::nav::{self, Badge, NavTarget};
use crate::paths::AppPaths;
use crate::ui_state::{self, LayoutState};
use crate::views::notes::NotesView;
use crate::views::settings::SettingsView;
use crate::views::tasks::TasksView;
use crate::views::today::TodayView;
use crate::views::{Navigate, Route};

const PALETTE_NOTE_LIMIT: usize = 200;

/// Drag payload for the sidebar's resize edge.
#[derive(Clone)]
struct ResizeSidebar;

impl Render for ResizeSidebar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

pub struct AppShell {
    route: Route,
    layout: LayoutState,
    layout_file: std::path::PathBuf,
    pending_layout_save: Option<gpui_kit::Task<()>>,
    due_now: usize,
    library: Entity<Library>,
    today: Entity<TodayView>,
    notes: Entity<NotesView>,
    tasks: Entity<TasksView>,
    settings: Entity<SettingsView>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

/// Lets app-level action handlers reach the shell of the main window.
struct ShellHandle {
    window: AnyWindowHandle,
    shell: WeakEntity<AppShell>,
}

impl Global for ShellHandle {}

impl AppShell {
    pub fn new(
        library: Entity<Library>,
        paths: &AppPaths,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let today = cx.new(|cx| TodayView::new(library.clone(), cx));
        let notes = cx.new(|cx| NotesView::new(library.clone(), window, cx));
        let tasks = cx.new(|cx| TasksView::new(library.clone(), window, cx));
        let settings = cx.new(|cx| SettingsView::new(paths.data_dir.clone(), cx));

        let subscriptions = vec![
            cx.subscribe_in(&today, window, |this, _, nav, window, cx| {
                this.handle_nav(*nav, window, cx)
            }),
            cx.subscribe_in(&notes, window, |this, _, nav, window, cx| {
                this.handle_nav(*nav, window, cx)
            }),
            cx.subscribe(&library, |this, _, event, cx| {
                if *event == LibraryEvent::TasksChanged {
                    this.refresh_badges(cx);
                }
            }),
            cx.observe_window_appearance(window, |_, window, cx| {
                appearance::system_appearance_changed(window, cx);
            }),
            // Refresh "today" when the window comes back to the front (date may have changed).
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.today.update(cx, |today, cx| today.reload(cx));
                }
            }),
        ];

        let layout_file = ui_state::layout_file(&paths.data_dir);
        let mut this = Self {
            route: Route::Today,
            layout: LayoutState::load(&layout_file),
            layout_file,
            pending_layout_save: None,
            due_now: 0,
            library,
            today,
            notes,
            tasks,
            settings,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.refresh_badges(cx);
        this
    }

    fn refresh_badges(&mut self, cx: &mut Context<Self>) {
        let counts = self.library.read(cx).task_counts().unwrap_or_default();
        self.due_now = counts.due_today + counts.overdue;
        cx.notify();
    }

    /// Saves layout shortly after the last change, so a drag writes once.
    fn save_layout_soon(&mut self, cx: &mut Context<Self>) {
        self.pending_layout_save = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(300))
                .await;
            _ = this.update(cx, |this, _| this.layout.save(&this.layout_file));
        }));
    }

    fn set_sidebar_width(&mut self, width: f32, cx: &mut Context<Self>) {
        let width = LayoutState::clamp_width(width);
        if (width - self.layout.sidebar_width).abs() >= 0.5 {
            self.layout.sidebar_width = width;
            self.save_layout_soon(cx);
            cx.notify();
        }
    }

    /// Opens a sidebar deep link.
    pub fn open_target(&mut self, target: NavTarget, window: &mut Window, cx: &mut Context<Self>) {
        match target {
            NavTarget::Route(route) => self.navigate(route, window, cx),
            NavTarget::Tasks(tab) => {
                self.navigate(Route::Tasks, window, cx);
                self.tasks.update(cx, |tasks, cx| tasks.set_tab(tab, cx));
            }
            NavTarget::NotesAll | NavTarget::NotesTrash => {
                self.navigate(Route::Notes, window, cx);
                let trash = target == NavTarget::NotesTrash;
                self.notes
                    .update(cx, |notes, cx| notes.set_show_trash(trash, window, cx));
            }
        }
    }

    fn is_target_active(&self, target: NavTarget, cx: &App) -> bool {
        match target {
            NavTarget::Route(route) => self.route == route,
            NavTarget::Tasks(tab) => self.route == Route::Tasks && self.tasks.read(cx).tab() == tab,
            NavTarget::NotesAll => {
                self.route == Route::Notes && !self.notes.read(cx).is_showing_trash()
            }
            NavTarget::NotesTrash => {
                self.route == Route::Notes && self.notes.read(cx).is_showing_trash()
            }
        }
    }

    fn badge_count(&self, badge: Badge) -> usize {
        match badge {
            Badge::None => 0,
            Badge::TasksDueNow => self.due_now,
        }
    }

    /// Registers this shell as the target of app-level commands.
    pub fn install(shell: &Entity<Self>, window: AnyWindowHandle, cx: &mut App) {
        cx.set_global(ShellHandle {
            window,
            shell: shell.downgrade(),
        });
    }

    pub fn navigate(&mut self, route: Route, window: &mut Window, cx: &mut Context<Self>) {
        self.route = route;
        match route {
            Route::Today => self.today.update(cx, |today, cx| today.reload(cx)),
            Route::Notes => self
                .notes
                .update(cx, |notes, cx| notes.ensure_selection(window, cx)),
            Route::Tasks | Route::Settings => {}
        }
        self.focus_route(window, cx);
        cx.notify();
    }

    fn focus_route(&self, window: &mut Window, cx: &mut App) {
        let handle = match self.route {
            Route::Today => self.today.focus_handle(cx),
            Route::Notes => self.notes.focus_handle(cx),
            Route::Tasks => self.tasks.focus_handle(cx),
            Route::Settings => self.settings.focus_handle(cx),
        };
        window.focus(&handle, cx);
    }

    fn handle_nav(&mut self, nav: Navigate, window: &mut Window, cx: &mut Context<Self>) {
        match nav {
            Navigate::To(route) => self.navigate(route, window, cx),
            Navigate::OpenNote(id) => self.open_note(id, window, cx),
        }
    }

    pub fn open_note(&mut self, id: NoteId, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Notes, window, cx);
        self.notes
            .update(cx, |notes, cx| notes.open_note(id, window, cx));
    }

    pub fn new_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Notes, window, cx);
        self.notes
            .update(cx, |notes, cx| notes.new_note(window, cx));
    }

    pub fn new_task(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Tasks, window, cx);
        self.tasks
            .update(cx, |tasks, cx| tasks.focus_new_task(window, cx));
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.layout.sidebar_collapsed = !self.layout.sidebar_collapsed;
        self.save_layout_soon(cx);
        cx.notify();
    }

    pub fn route(&self) -> Route {
        self.route
    }

    pub fn library(&self) -> &Entity<Library> {
        &self.library
    }

    pub fn notes(&self) -> &Entity<NotesView> {
        &self.notes
    }

    pub fn tasks(&self) -> &Entity<TasksView> {
        &self.tasks
    }

    pub fn layout(&self) -> LayoutState {
        self.layout
    }

    pub fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let state = cx.new(|cx| CommandState::new(window, cx));
        let notes = self.library.read(cx).notes(false).unwrap_or_default();
        let note_ids: Vec<NoteId> = notes
            .iter()
            .take(PALETTE_NOTE_LIMIT)
            .map(|n| n.id)
            .collect();
        let note_items: Vec<CommandItem> = notes
            .into_iter()
            .take(PALETTE_NOTE_LIMIT)
            .map(|n| {
                CommandItem::new()
                    .label(n.title)
                    .icon(Lucide::FileText)
                    .keywords([n.preview])
            })
            .collect();
        let shell = cx.entity().downgrade();

        window.open_dialog(cx, move |dialog, _, _| {
            let state = state.clone();
            let note_items = note_items.clone();
            let note_ids = note_ids.clone();
            let shell = shell.clone();
            dialog
                .close_button(false)
                .p_0()
                // Dialog widths are in pixels; 560 px fits the palette at every interface size.
                .w(px(560.))
                .content(move |content, _, _| {
                    let note_ids = note_ids.clone();
                    let shell = shell.clone();
                    content.child(
                        Command::new(&state)
                            .bordered(false)
                            .placeholder("Search notes or run a command…")
                            .max_h(rems(26.))
                            .group(
                                CommandGroup::new()
                                    .label("Commands")
                                    .items(palette_commands()),
                            )
                            .when(!note_items.is_empty(), |cmd| {
                                cmd.group(
                                    CommandGroup::new().label("Notes").items(note_items.clone()),
                                )
                            })
                            .on_confirm(move |index, window, cx| {
                                window.close_dialog(cx);
                                // Section 0 items carry an Action that already ran.
                                if index.section == 1
                                    && let Some(id) = note_ids.get(index.row).copied()
                                {
                                    _ = shell
                                        .update(cx, |shell, cx| shell.open_note(id, window, cx));
                                }
                            }),
                    )
                })
        });
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = self.layout.sidebar_collapsed;
        let width = px(self.layout.sidebar_width);
        let theme = cx.theme();

        let brand = h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size_6()
                    .flex_shrink_0()
                    .rounded(theme.radius)
                    .bg(theme.sidebar_primary)
                    .text_color(theme.sidebar_primary_foreground)
                    .child(Icon::new(Lucide::Gem).xsmall()),
            )
            .when(!collapsed, |this| {
                this.child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Keepstone"),
                )
            });

        let count_suffix = |count: usize| {
            move |_: &mut Window, cx: &mut App| {
                div().when(count > 0, |this| {
                    this.px_1p5()
                        .rounded(cx.theme().radius)
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(count.to_string())
                })
            }
        };

        let groups: Vec<SidebarGroup<SidebarMenu>> = nav::SECTIONS
            .iter()
            .map(|section| {
                let items =
                    section.entries.iter().map(|entry| {
                        let route = entry.route;
                        let children: Vec<SidebarMenuItem> = entry
                            .children
                            .iter()
                            .map(|child| {
                                let target = child.target;
                                SidebarMenuItem::new(child.label)
                                    .active(self.is_target_active(target, cx))
                                    .suffix(count_suffix(self.badge_count(child.badge)))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_target(target, window, cx)
                                    }))
                            })
                            .collect();
                        let has_children = !children.is_empty();
                        SidebarMenuItem::new(entry.label)
                            .icon(entry.icon)
                            .active(self.route == route && !has_children)
                            .default_open(self.route == route)
                            .suffix(count_suffix(self.badge_count(entry.badge)))
                            .children(children)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.navigate(route, window, cx)
                            }))
                    });
                SidebarGroup::new(section.label).child(SidebarMenu::new().children(items))
            })
            .collect();

        // Settings sits in the footer, aligned with the menu items above.
        // Keyboard paths: the Settings shortcut and the command palette.
        let settings_item = SidebarFooter::new()
            .selected(self.route == Route::Settings)
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.navigate(Route::Settings, window, cx)),
            )
            .child(Icon::new(Lucide::Settings).small())
            .when(!collapsed, |this| {
                this.child(div().text_sm().child("Settings"))
            });

        // The sidebar animates collapse/expand between widths it has seen
        // before. Keying it by width means a drag starts a fresh state at the
        // new width every frame, so resizing tracks the pointer with no easing lag.
        let sidebar = Sidebar::new(gpui_kit::SharedString::from(format!(
            "main-sidebar-{}",
            self.layout.sidebar_width.round() as i32
        )))
        .collapsible(SidebarCollapsible::Icon)
        .collapsed(collapsed)
        .w(width)
        .header(SidebarHeader::new().child(brand))
        .children(groups)
        .footer(settings_item);

        div()
            .relative()
            .h_full()
            .flex_shrink_0()
            .on_drag_move(
                cx.listener(|this, e: &gpui_kit::DragMoveEvent<ResizeSidebar>, _, cx| {
                    let width = f32::from(e.event.position.x - e.bounds.left());
                    this.set_sidebar_width(width, cx);
                }),
            )
            .child(sidebar)
            .when(!collapsed, |this| {
                this.child(
                    resize_handle("sidebar-edge", gpui_kit::Axis::Horizontal)
                        .inside(HandleEdge::Trailing)
                        .on_drag(ResizeSidebar, |drag, _, _, cx| cx.new(|_| drag.clone()))
                        .on_double_click(cx.listener(|this, _, _, cx| {
                            this.set_sidebar_width(LayoutState::DEFAULT_SIDEBAR, cx);
                        })),
                )
            })
    }

    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let shortcut = Keystroke::parse(if cfg!(target_os = "macos") {
            "cmd-k"
        } else {
            "ctrl-k"
        })
        .ok();
        TitleBar::new().child(
            h_flex()
                .w_full()
                .pr_2()
                .justify_between()
                .items_center()
                .child(
                    Button::new("toggle-sidebar")
                        .ghost()
                        .xsmall()
                        .icon(Lucide::PanelLeft)
                        .tooltip("Toggle sidebar")
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
                )
                .child(
                    Button::new("open-palette")
                        .outline()
                        .xsmall()
                        .w(rems(20.))
                        .child(
                            h_flex()
                                .w_full()
                                .justify_between()
                                .gap_2()
                                .text_color(theme.muted_foreground)
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(Icon::new(Lucide::Search).xsmall())
                                        .child("Search or run a command"),
                                )
                                .children(shortcut.map(|k| Kbd::new(k).appearance(false))),
                        )
                        .on_click(
                            cx.listener(|this, _, window, cx| {
                                this.open_command_palette(window, cx)
                            }),
                        ),
                )
                .child(div().w(px(24.))),
        )
    }
}

/// Commands shown in the palette. Each carries its Action, so the palette also
/// shows the shortcut.
fn palette_commands() -> Vec<CommandItem> {
    let cmd = |label: &str, icon: Lucide, action: Box<dyn Action>, keywords: &[&str]| {
        CommandItem::new()
            .label(label.to_owned())
            .icon(icon)
            .action(action)
            .keywords(keywords.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>())
    };
    vec![
        cmd(
            "New note",
            Lucide::SquarePen,
            Box::new(NewNote),
            &["create", "write"],
        ),
        cmd(
            "New task",
            Lucide::Plus,
            Box::new(NewTask),
            &["create", "todo"],
        ),
        cmd(
            "Go to Today",
            Lucide::House,
            Box::new(GoToday),
            &["dashboard", "home"],
        ),
        cmd("Go to Notes", Lucide::NotebookPen, Box::new(GoNotes), &[]),
        cmd(
            "Go to Tasks",
            Lucide::ListTodo,
            Box::new(GoTasks),
            &["todo"],
        ),
        cmd(
            "Open settings",
            Lucide::Settings,
            Box::new(GoSettings),
            &["preferences"],
        ),
        cmd(
            "Toggle sidebar",
            Lucide::PanelLeft,
            Box::new(ToggleSidebar),
            &[],
        ),
        cmd(
            "Use light theme",
            Lucide::Sun,
            Box::new(UseLightTheme),
            &["appearance"],
        ),
        cmd(
            "Use dark theme",
            Lucide::Moon,
            Box::new(UseDarkTheme),
            &["appearance"],
        ),
        cmd(
            "Use system theme",
            Lucide::SunMoon,
            Box::new(UseSystemTheme),
            &["appearance"],
        ),
    ]
}

/// Runs `f` on the main shell, if it is still open.
///
/// Deferred: app-level action handlers run while the window is still borrowed
/// by the event dispatch, so updating it directly would silently fail.
fn with_shell(
    cx: &mut App,
    f: impl FnOnce(&mut AppShell, &mut Window, &mut Context<AppShell>) + 'static,
) {
    let Some(handle) = cx.try_global::<ShellHandle>() else {
        return;
    };
    let (window, shell) = (handle.window, handle.shell.clone());
    cx.defer(move |cx| {
        let updated = window.update(cx, |_, window, cx| {
            shell.update(cx, |shell, cx| f(shell, window, cx))
        });
        if updated.is_err() {
            eprintln!("keepstone: command ignored, main window is unavailable");
        }
    });
}

/// App-level command handlers. Call once at startup.
pub fn register_commands(cx: &mut App) {
    cx.on_action(|_: &OpenCommandPalette, cx| {
        with_shell(cx, |s, w, cx| s.open_command_palette(w, cx))
    });
    cx.on_action(|_: &NewNote, cx| with_shell(cx, |s, w, cx| s.new_note(w, cx)));
    cx.on_action(|_: &NewTask, cx| with_shell(cx, |s, w, cx| s.new_task(w, cx)));
    cx.on_action(|_: &GoToday, cx| with_shell(cx, |s, w, cx| s.navigate(Route::Today, w, cx)));
    cx.on_action(|_: &GoNotes, cx| with_shell(cx, |s, w, cx| s.navigate(Route::Notes, w, cx)));
    cx.on_action(|_: &GoTasks, cx| with_shell(cx, |s, w, cx| s.navigate(Route::Tasks, w, cx)));
    cx.on_action(|_: &GoSettings, cx| {
        with_shell(cx, |s, w, cx| s.navigate(Route::Settings, w, cx))
    });
    cx.on_action(|_: &ToggleSidebar, cx| with_shell(cx, |s, _, cx| s.toggle_sidebar(cx)));
    cx.on_action(|_: &UseLightTheme, cx| {
        appearance::update(cx, |s| s.theme = ThemePreference::Light)
    });
    cx.on_action(|_: &UseDarkTheme, cx| {
        appearance::update(cx, |s| s.theme = ThemePreference::Dark)
    });
    cx.on_action(|_: &UseSystemTheme, cx| {
        appearance::update(cx, |s| s.theme = ThemePreference::System)
    });
}

impl Render for AppShell {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content: AnyView = match self.route {
            Route::Today => self.today.clone().into(),
            Route::Notes => self.notes.clone().into(),
            Route::Tasks => self.tasks.clone().into(),
            Route::Settings => self.settings.clone().into(),
        };
        let theme = cx.theme();
        v_flex()
            .track_focus(&self.focus)
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(theme.font_family.clone())
            .child(self.title_bar(cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar(cx))
                    .child(div().flex_1().min_w_0().h_full().child(content)),
            )
    }
}
