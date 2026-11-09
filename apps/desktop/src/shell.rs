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
    Sidebar, SidebarCollapsible, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem,
};
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _, TitleBar, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyView, AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle,
    Focusable as _, Global, InteractiveElement as _, IntoElement, Keystroke, ParentElement as _,
    Render, Styled as _, Subscription, TestSupportExt as _, WeakEntity, Window, div, px, rems,
};
use keepstone_core::NoteId;
use keepstone_ui::appearance::{self, ThemePreference};

use crate::actions::{
    ChangeMasterPassword, ExportNotes, FindInNote, FindNext, FindPrevious, GeneratePassword,
    GoCalendar, GoHabits, GoNotes, GoPasswords, GoSettings, GoTasks, GoToday, ImportNotes,
    LockVault, NewEvent, NewHabit, NewLogin, NewNote, NewProject, NewTask, OpenCommandPalette,
    OpenDailyNote, ToggleListPanel, ToggleMarkdownSource, ToggleSidebar, UseDarkTheme,
    UseLightTheme, UseSystemTheme,
};
use crate::library::{Library, LibraryEvent};
use crate::nav::{self, Badge, NavTarget};
use crate::paths::AppPaths;
use crate::ui_state::{self, LayoutState, LayoutStore};
use crate::views::calendar::CalendarView;
use crate::views::habits::HabitsView;
use crate::views::notes::NotesView;
use crate::views::passwords::PasswordsView;
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
    layout: Entity<LayoutStore>,
    /// Set while the command palette is the open dialog.
    palette_open: std::rc::Rc<std::cell::Cell<bool>>,
    due_now: usize,
    habits_left: usize,
    library: Entity<Library>,
    today: Entity<TodayView>,
    notes: Entity<NotesView>,
    tasks: Entity<TasksView>,
    calendar: Entity<CalendarView>,
    habits: Entity<HabitsView>,
    passwords: Entity<PasswordsView>,
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
        let layout = cx.new(|_| LayoutStore::load(&paths.data_dir));
        let today = cx.new(|cx| TodayView::new(library.clone(), cx));
        let notes = cx.new(|cx| NotesView::new(library.clone(), layout.clone(), window, cx));
        let tasks = cx.new(|cx| TasksView::new(library.clone(), window, cx));
        let calendar = cx.new(|cx| CalendarView::new(library.clone(), window, cx));
        let habits = cx.new(|cx| HabitsView::new(library.clone(), window, cx));
        let passwords = cx.new(|cx| PasswordsView::new(library.clone(), window, cx));
        let settings = cx.new(|cx| SettingsView::new(paths.data_dir.clone(), cx));

        let subscriptions = vec![
            cx.observe(&layout, |_, _, cx| cx.notify()),
            cx.subscribe_in(&today, window, |this, _, nav, window, cx| {
                this.handle_nav(*nav, window, cx)
            }),
            cx.subscribe_in(
                &today,
                window,
                |this, _, _: &crate::views::today::OpenDailyNoteRequest, window, cx| {
                    this.open_daily_note(window, cx)
                },
            ),
            cx.subscribe_in(&notes, window, |this, _, nav, window, cx| {
                this.handle_nav(*nav, window, cx)
            }),
            cx.subscribe(&library, |this, _, event, cx| {
                if matches!(
                    event,
                    LibraryEvent::TasksChanged | LibraryEvent::HabitsChanged
                ) {
                    this.refresh_badges(cx);
                }
            }),
            // ⌘Q or closing the window must not drop typing that is still
            // waiting for the autosave timer, or an unsaved sidebar width.
            cx.on_app_quit(|this, cx| {
                this.save_everything(cx);
                async {}
            }),
            cx.observe_window_appearance(window, |_, window, cx| {
                appearance::system_appearance_changed(window, cx);
            }),
            // Refresh "today" when the window comes back to the front (date may have changed).
            // Any key counts as activity for vault auto-lock.
            cx.observe_keystrokes(|this, _, _, cx| {
                this.library.update(cx, |lib, _| lib.touch_vault());
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() && crate::vault_prefs::get(cx).lock_on_hide {
                    this.library.update(cx, |lib, cx| lib.lock_vault(cx));
                }
                if window.is_window_active() {
                    this.today.update(cx, |today, cx| today.reload(cx));
                    this.tasks
                        .update(cx, |tasks, cx| tasks.refresh_if_new_day(cx));
                    this.calendar.update(cx, |calendar, cx| calendar.reload(cx));
                    this.habits.update(cx, |habits, cx| habits.reload(cx));
                    this.refresh_badges(cx);
                }
            }),
        ];

        let mut this = Self {
            route: Route::Today,
            layout,
            palette_open: std::rc::Rc::default(),
            due_now: 0,
            habits_left: 0,
            library,
            today,
            notes,
            tasks,
            calendar,
            habits,
            passwords,
            settings,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.refresh_badges(cx);
        this
    }

    /// Flushes everything that is saved lazily.
    pub fn save_everything(&mut self, cx: &mut Context<Self>) {
        self.layout.update(cx, |layout, _| layout.save_now());
        if let Err(err) = self.notes.update(cx, |notes, cx| notes.save_pending(cx)) {
            eprintln!("keepstone: could not save the open note on exit: {err:#}");
        }
    }

    fn refresh_badges(&mut self, cx: &mut Context<Self>) {
        let counts = self.library.read(cx).task_counts().unwrap_or_default();
        self.due_now = counts.due_today + counts.overdue;
        self.habits_left = self
            .library
            .read(cx)
            .habits(false)
            .map(|hs| hs.iter().filter(|h| h.due_today && !h.done_today()).count())
            .unwrap_or(0);
        cx.notify();
    }

    fn set_sidebar_width(&mut self, width: f32, cx: &mut Context<Self>) {
        self.layout
            .update(cx, |layout, cx| layout.set_sidebar_width(width, cx));
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
            NavTarget::DailyNote => self.open_daily_note(window, cx),
            NavTarget::Project(id) => {
                self.navigate(Route::Tasks, window, cx);
                self.tasks
                    .update(cx, |tasks, cx| tasks.show_project(id, cx));
            }
            NavTarget::Vault(filter) => {
                self.navigate(Route::Passwords, window, cx);
                self.passwords
                    .update(cx, |p, cx| p.set_filter(filter, window, cx));
            }
        }
    }

    fn is_target_active(&self, target: NavTarget, cx: &App) -> bool {
        match target {
            NavTarget::Route(route) => self.route == route,
            NavTarget::Tasks(tab) => {
                self.route == Route::Tasks
                    && self.tasks.read(cx).project().is_none()
                    && self.tasks.read(cx).tab() == tab
            }
            NavTarget::Project(id) => {
                self.route == Route::Tasks && self.tasks.read(cx).project() == Some(id)
            }
            NavTarget::Vault(filter) => {
                self.route == Route::Passwords && self.passwords.read(cx).filter() == filter
            }
            NavTarget::DailyNote => {
                self.route == Route::Notes
                    && self.notes.read(cx).open_note_title(cx).as_deref()
                        == Some(
                            crate::library::daily_note_title(keepstone_core::clock::today())
                                .as_str(),
                        )
            }
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
            Badge::HabitsLeft => self.habits_left,
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
            Route::Calendar => self.calendar.update(cx, |c, cx| c.reload(cx)),
            Route::Habits => self.habits.update(cx, |h, cx| h.reload(cx)),
            Route::Passwords => {}
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
            Route::Calendar => self.calendar.focus_handle(cx),
            Route::Habits => self.habits.focus_handle(cx),
            Route::Passwords => {
                // The field that matters: master password, unlock, or search.
                self.passwords
                    .update(cx, |p, cx| p.focus_primary(window, cx));
                return;
            }
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

    pub fn new_event(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Calendar, window, cx);
        self.calendar
            .update(cx, |calendar, cx| calendar.focus_quick_add(window, cx));
    }

    pub fn new_habit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Habits, window, cx);
        self.habits
            .update(cx, |habits, cx| habits.focus_quick_add(window, cx));
    }

    pub fn open_daily_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.navigate(Route::Notes, window, cx);
        self.notes
            .update(cx, |notes, cx| notes.open_daily_note(window, cx));
    }

    /// Asks for a name, creates the project and shows it.
    pub fn new_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let library = self.library.clone();
        let shell = cx.entity().downgrade();
        keepstone_ui::feedback::prompt_text(
            "New project",
            "Project name",
            "",
            "Create",
            window,
            cx,
            move |name, window, cx| {
                let result = library.update(cx, |lib, cx| lib.create_project(&name, cx));
                if let Some(id) = keepstone_ui::feedback::report(result, window, cx) {
                    _ = shell.update(cx, |s, cx| {
                        s.open_target(NavTarget::Project(id), window, cx)
                    });
                }
            },
        );
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.layout
            .update(cx, |layout, cx| layout.toggle_sidebar(cx));
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

    pub fn calendar(&self) -> &Entity<CalendarView> {
        &self.calendar
    }

    pub fn habits(&self) -> &Entity<HabitsView> {
        &self.habits
    }

    pub fn passwords(&self) -> &Entity<PasswordsView> {
        &self.passwords
    }

    pub fn layout(&self, cx: &App) -> LayoutState {
        self.layout.read(cx).state().clone()
    }

    /// Collapses or expands the list panel of the current screen (Notes today).
    pub fn toggle_list_panel(&mut self, cx: &mut Context<Self>) {
        self.layout.update(cx, |layout, cx| {
            layout.toggle_panel(ui_state::NOTES_LIST, cx)
        });
    }

    /// ⌘K toggles the palette. It never stacks: a second press closes it, and it
    /// doesn't open over another dialog (a confirmation, say).
    pub fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_dialog(cx) {
            if self.palette_open.get() {
                window.close_dialog(cx);
            }
            self.palette_open.set(false);
            return;
        }
        let palette_open = self.palette_open.clone();
        palette_open.set(true);
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
        let (command_items, command_actions): (Vec<_>, Vec<_>) =
            palette_commands().into_iter().unzip();
        let command_actions = std::rc::Rc::new(command_actions);
        let search_focus = state.focus_handle(cx);

        window.open_dialog(cx, move |dialog, _, _| {
            let state = state.clone();
            let command_items = command_items.clone();
            let command_actions = command_actions.clone();
            let note_items = note_items.clone();
            let note_ids = note_ids.clone();
            let shell = shell.clone();
            let closed = palette_open.clone();
            let content_flag = palette_open.clone();
            dialog
                .on_close(move |_, _, _| closed.set(false))
                .close_button(false)
                .p_0()
                // Dialog widths are in pixels; 560 px fits the palette at every interface size.
                .w(px(560.))
                .content(move |content, _, _| {
                    let content_flag = content_flag.clone();
                    let note_ids = note_ids.clone();
                    let shell = shell.clone();
                    let command_actions = command_actions.clone();
                    let palette_flag = content_flag.clone();
                    content.child(
                        Command::new(&state)
                            .bordered(false)
                            .placeholder("Search notes or run a command…")
                            .max_h(rems(26.))
                            .group(
                                CommandGroup::new()
                                    .label("Commands")
                                    .items(command_items.clone()),
                            )
                            .when(!note_items.is_empty(), |cmd| {
                                cmd.group(
                                    CommandGroup::new().label("Notes").items(note_items.clone()),
                                )
                            })
                            .on_confirm(move |index, window, cx| {
                                // Close first: closing restores the previous focus, and
                                // commands like "New note" must be able to move it.
                                palette_flag.set(false);
                                window.close_dialog(cx);
                                if index.section == 0
                                    && let Some(action) = command_actions.get(index.row)
                                {
                                    window.dispatch_action(action.boxed_clone(), cx);
                                } else if index.section == 1
                                    && let Some(id) = note_ids.get(index.row).copied()
                                {
                                    _ = shell
                                        .update(cx, |shell, cx| shell.open_note(id, window, cx));
                                }
                            }),
                    )
                })
        });
        // Type straight away: the search field takes focus when the palette opens.
        window.focus(&search_focus, cx);
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let layout = self.layout.read(cx).state().clone();
        let collapsed = layout.sidebar_collapsed;
        let width = px(layout.sidebar_width);

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
                let items = section.entries.iter().map(|entry| {
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
                    let mut children = children;
                    if entry.dynamic == Some(nav::DynamicChildren::Projects) {
                        for (project, open) in self.library.read(cx).projects() {
                            let target = NavTarget::Project(project.id());
                            children.push(
                                SidebarMenuItem::new(gpui_kit::SharedString::from(
                                    project.name().to_owned(),
                                ))
                                .icon(Lucide::FolderKanban)
                                .active(self.is_target_active(target, cx))
                                .suffix(count_suffix(open))
                                .on_click(cx.listener(
                                    move |this, _, window, cx| this.open_target(target, window, cx),
                                )),
                            );
                        }
                        children.push(
                            SidebarMenuItem::new("New project")
                                .icon(Lucide::Plus)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.new_project(window, cx)),
                                ),
                        );
                    }
                    let has_children = !children.is_empty();
                    SidebarMenuItem::new(entry.label)
                        .icon(entry.icon)
                        .active(self.route == route && !has_children)
                        .default_open(self.route == route)
                        .suffix(count_suffix(self.badge_count(entry.badge)))
                        .children(children)
                        .on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.navigate(route, window, cx)
                            }),
                        )
                });
                SidebarGroup::new(section.label).child(SidebarMenu::new().children(items))
            })
            .collect();

        // Settings: a single icon button in the footer (name in its tooltip,
        // with the shortcut). Also reachable via ⌘, and the command palette.
        let settings_item = h_flex().w_full().child(
            Button::new("sidebar-settings")
                .ghost()
                .small()
                .icon(Lucide::Settings)
                .selected(self.route == Route::Settings)
                .tooltip_with_action("Settings", &GoSettings, None)
                .on_click(
                    cx.listener(|this, _, window, cx| this.navigate(Route::Settings, window, cx)),
                ),
        );

        // The sidebar animates collapse/expand between widths it has seen
        // before. Keying it by width means a drag starts a fresh state at the
        // new width every frame, so resizing tracks the pointer with no easing lag.
        let sidebar = Sidebar::new(gpui_kit::SharedString::from(format!(
            "main-sidebar-{}",
            layout.sidebar_width.round() as i32
        )))
        .collapsible(SidebarCollapsible::Icon)
        .collapsed(collapsed)
        .w(width)
        .header(self.search_entry(collapsed, cx))
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

    /// "Search ⌘K" at the top of the sidebar: the visible way into the command
    /// palette. It is the sidebar header itself, so its hover covers just this
    /// row. (The app name lives in the menu bar and Dock; the sidebar keeps its
    /// space for navigation.)
    fn search_entry(&self, collapsed: bool, cx: &mut Context<Self>) -> SidebarHeader {
        let theme = cx.theme();
        let shortcut = crate::actions::shortcut_for(&OpenCommandPalette)
            .and_then(|k| Keystroke::parse(k).ok());
        SidebarHeader::new()
            .on_mouse_down(
                gpui_kit::MouseButton::Left,
                cx.listener(|this, _, window, cx| this.open_command_palette(window, cx)),
            )
            .child(
                h_flex()
                    .id("open-palette")
                    .w_full()
                    .gap_2()
                    .items_center()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(Icon::new(Lucide::Search).small())
                    .when(!collapsed, |this| {
                        this.child(div().flex_1().child("Search"))
                            .children(shortcut.map(|k| Kbd::new(k).appearance(false)))
                    })
                    .test_support(),
            )
    }

    fn title_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        TitleBar::new().child(
            h_flex()
                .w_full()
                .pr_2()
                .items_center()
                // Panel toggles live here so they never move when a panel collapses.
                .child(
                    h_flex()
                        .gap_1()
                        .child(
                            Button::new("toggle-sidebar")
                                .ghost()
                                .xsmall()
                                .icon(Lucide::PanelLeft)
                                .tooltip_with_action("Toggle sidebar", &ToggleSidebar, None)
                                .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx))),
                        )
                        .when(self.route == Route::Notes, |this| {
                            this.child(
                                Button::new("toggle-list-panel")
                                    .ghost()
                                    .xsmall()
                                    .icon(Lucide::PanelLeftDashed)
                                    .tooltip_with_action("Toggle note list", &ToggleListPanel, None)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.toggle_list_panel(cx)),
                                    ),
                            )
                        }),
                ),
        )
    }
}

/// Commands shown in the palette. Each carries its Action, so the palette also
/// shows the shortcut.
fn palette_commands() -> Vec<(CommandItem, Box<dyn Action>)> {
    let cmd = |label: &'static str, icon: Lucide, action: Box<dyn Action>, keywords: &[&str]| {
        let shortcut = crate::actions::shortcut_for(&*action);
        // A custom row so the shortcut hint shows without attaching the Action:
        // the palette runs the command itself, after it has closed (see on_confirm).
        let item = CommandItem::new()
            .label(label)
            .icon(icon)
            .keywords(keywords.iter().map(|k| (*k).to_owned()).collect::<Vec<_>>())
            .child(move |_, cx| {
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(icon)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(div().flex_1().child(label))
                    .children(
                        shortcut
                            .and_then(|s| Keystroke::parse(s).ok())
                            .map(|k| Kbd::new(k).appearance(false)),
                    )
            });
        (item, action)
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
            "New event",
            Lucide::CalendarPlus,
            Box::new(NewEvent),
            &["create", "calendar", "meeting", "appointment"],
        ),
        cmd(
            "New habit",
            Lucide::Sprout,
            Box::new(NewHabit),
            &["create", "routine", "streak"],
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
            "Go to Calendar",
            Lucide::CalendarDays,
            Box::new(GoCalendar),
            &["events", "schedule", "agenda", "month", "week"],
        ),
        cmd(
            "Go to Habits",
            Lucide::Sprout,
            Box::new(GoHabits),
            &["routine", "streak", "check in"],
        ),
        cmd(
            "Go to Passwords",
            Lucide::KeyRound,
            Box::new(GoPasswords),
            &["vault", "password manager", "logins", "cards"],
        ),
        cmd(
            "New login",
            Lucide::KeyRound,
            Box::new(NewLogin),
            &["password", "vault", "account", "create"],
        ),
        cmd(
            "Generate password",
            Lucide::Dices,
            Box::new(GeneratePassword),
            &["random", "passphrase", "vault"],
        ),
        cmd(
            "Lock vault",
            Lucide::LockKeyhole,
            Box::new(LockVault),
            &["passwords", "secure"],
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
            "Toggle note list",
            Lucide::PanelLeftDashed,
            Box::new(ToggleListPanel),
            &["notes", "panel", "focus"],
        ),
        cmd(
            "Find in note",
            Lucide::Search,
            Box::new(FindInNote),
            &["search", "replace", "match"],
        ),
        cmd(
            "Show or hide Markdown source",
            Lucide::Code,
            Box::new(ToggleMarkdownSource),
            &["raw", "markdown", "source", "preview"],
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
    cx.on_action(|_: &GoCalendar, cx| {
        with_shell(cx, |s, w, cx| s.navigate(Route::Calendar, w, cx))
    });
    cx.on_action(|_: &NewEvent, cx| with_shell(cx, |s, w, cx| s.new_event(w, cx)));
    cx.on_action(|_: &GoHabits, cx| with_shell(cx, |s, w, cx| s.navigate(Route::Habits, w, cx)));
    cx.on_action(|_: &NewHabit, cx| with_shell(cx, |s, w, cx| s.new_habit(w, cx)));
    cx.on_action(|_: &GoPasswords, cx| {
        with_shell(cx, |s, w, cx| s.navigate(Route::Passwords, w, cx))
    });
    cx.on_action(|_: &LockVault, cx| {
        with_shell(cx, |s, _, cx| {
            s.library.update(cx, |lib, cx| lib.lock_vault(cx))
        })
    });
    cx.on_action(|_: &NewLogin, cx| {
        with_shell(cx, |s, w, cx| {
            s.navigate(Route::Passwords, w, cx);
            s.passwords.update(cx, |p, cx| p.new_login(w, cx));
        })
    });
    cx.on_action(|_: &ChangeMasterPassword, cx| {
        with_shell(cx, |s, w, cx| {
            crate::views::passwords::open_change_password(s.library.clone(), w, cx)
        })
    });
    cx.on_action(|_: &GeneratePassword, cx| {
        with_shell(cx, |s, w, cx| {
            let library = s.library.clone();
            crate::views::vault_generator::open_generator(None, &library, w, cx);
        })
    });
    cx.on_action(|_: &GoSettings, cx| {
        with_shell(cx, |s, w, cx| s.navigate(Route::Settings, w, cx))
    });
    cx.on_action(|_: &ToggleSidebar, cx| with_shell(cx, |s, _, cx| s.toggle_sidebar(cx)));
    cx.on_action(|_: &ToggleListPanel, cx| {
        with_shell(cx, |s, w, cx| {
            // The list belongs to Notes: show the screen it toggles.
            if s.route() != Route::Notes {
                s.navigate(Route::Notes, w, cx);
            }
            s.toggle_list_panel(cx);
        });
    });
    cx.on_action(|_: &OpenDailyNote, cx| with_shell(cx, |s, w, cx| s.open_daily_note(w, cx)));
    cx.on_action(|_: &FindInNote, cx| {
        // Find belongs to the open note; other screens have their own search.
        with_shell(cx, |s, w, cx| {
            if s.route() == Route::Notes {
                s.notes.update(cx, |n, cx| n.open_find(w, cx));
            }
        })
    });
    cx.on_action(|_: &FindNext, cx| {
        with_shell(cx, |s, w, cx| {
            if s.route() == Route::Notes {
                s.notes.update(cx, |n, cx| n.step_find(1, w, cx));
            }
        })
    });
    cx.on_action(|_: &FindPrevious, cx| {
        with_shell(cx, |s, w, cx| {
            if s.route() == Route::Notes {
                s.notes.update(cx, |n, cx| n.step_find(-1, w, cx));
            }
        })
    });
    cx.on_action(|_: &ToggleMarkdownSource, cx| {
        with_shell(cx, |s, w, cx| {
            if s.route() == Route::Notes {
                s.notes.update(cx, |n, cx| n.toggle_source_mode(w, cx));
            }
        })
    });
    cx.on_action(|_: &NewProject, cx| with_shell(cx, |s, w, cx| s.new_project(w, cx)));
    cx.on_action(|_: &ImportNotes, cx| {
        with_shell(cx, |s, w, cx| {
            s.navigate(Route::Notes, w, cx);
            s.notes.update(cx, |n, cx| n.import_notes(w, cx));
        })
    });
    cx.on_action(|_: &ExportNotes, cx| {
        with_shell(cx, |s, w, cx| {
            s.notes.update(cx, |n, cx| n.export_notes(w, cx))
        })
    });
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
            Route::Calendar => self.calendar.clone().into(),
            Route::Habits => self.habits.clone().into(),
            Route::Passwords => self.passwords.clone().into(),
            Route::Settings => self.settings.clone().into(),
        };
        let theme = cx.theme();
        v_flex()
            .track_focus(&self.focus)
            // Pointer activity also postpones auto-lock (no re-render).
            .on_mouse_move(cx.listener(|this, _, _, cx| {
                this.library.update(cx, |lib, _| lib.touch_vault());
            }))
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
