//! Keepstone desktop app.
//!
//! Startup order matters: GPUI Kit init → design system (fonts, themes,
//! appearance) → key bindings and commands → open the database → open the
//! window. [`start`] does all of it and is shared by `main` and the UI tests,
//! so tests exercise the real app.

pub mod actions;
pub mod library;
pub mod markdown_io;
pub mod nav;
pub mod paths;
#[cfg(feature = "perf")]
pub mod perf;
pub mod shell;
pub mod ui_state;
pub mod views;

use anyhow::{Context as _, Result};
use gpui_kit::component::TitleBar;
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, WindowBounds, WindowOptions, px, size,
};
use keepstone_storage::Store;

use crate::library::Library;
use crate::paths::AppPaths;
use crate::shell::AppShell;

pub fn run() -> Result<()> {
    let paths = AppPaths::resolve()?;
    // Open the database before the event loop so a broken data folder fails
    // fast with a readable message instead of an empty window.
    let store = Store::open(&paths.database)
        .with_context(|| format!("could not open {}", paths.database.display()))?;

    gpui_kit::application()
        .with_assets(keepstone_ui::assets::KeepstoneAssets)
        .run(move |cx| match start(&paths, store, cx) {
            Ok(_) => cx.activate(true),
            Err(err) => {
                eprintln!("keepstone: failed to open window: {err:#}");
                cx.quit();
            }
        });
    Ok(())
}

/// Initializes everything and opens the main window.
pub fn start(
    paths: &AppPaths,
    store: Store,
    cx: &mut App,
) -> Result<(AnyWindowHandle, Entity<AppShell>)> {
    gpui_kit::init(cx);
    keepstone_ui::init(paths.appearance.clone(), cx);
    // The note editor's own keys first: ours are bound after, so they win on a tie.
    zorite_editor::bind_keys(cx);
    actions::bind(cx);
    actions::set_menus(cx);
    shell::register_commands(cx);

    let library = cx.new(|_| Library::new(store));
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1200.), px(780.)),
            cx,
        ))),
        window_min_size: Some(size(px(720.), px(480.))),
        app_id: Some("app.keepstone.Keepstone".into()),
        ..TitleBar::window_options()
    };
    let (window, shell) = gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| AppShell::new(library, paths, window, cx))
    })?;
    AppShell::install(&shell, window, cx);
    {
        let shell = shell.downgrade();
        window.update(cx, |_, window, cx| {
            window.on_window_should_close(cx, move |_, cx| {
                _ = shell.update(cx, |shell, cx| shell.save_everything(cx));
                true
            });
        })?;
    }

    #[cfg(feature = "perf")]
    perf::install(window, cx);

    // Debug and perf builds only: start on a given screen, for screenshot and perf scripts.
    #[cfg(any(debug_assertions, feature = "perf"))]
    if let Some(route) = std::env::var("KEEPSTONE_DEV_ROUTE")
        .ok()
        .and_then(|r| views::Route::parse(&r))
    {
        window.update(cx, |_, window, cx| {
            shell.update(cx, |s, cx| s.navigate(route, window, cx))
        })?;
        // KEEPSTONE_DEV_SOURCE=1 with route=notes: open the first note as Markdown source.
        if route == views::Route::Notes && std::env::var_os("KEEPSTONE_DEV_SOURCE").is_some() {
            window.update(cx, |_, window, cx| {
                let notes = shell.read(cx).notes().clone();
                notes.update(cx, |n, cx| {
                    n.ensure_selection(window, cx);
                    n.toggle_source_mode(window, cx);
                });
            })?;
        }
        // KEEPSTONE_DEV_NOTE=<title>: open that note with the caret at the end of its body.
        if let Ok(title) = std::env::var("KEEPSTONE_DEV_NOTE") {
            window.update(cx, |_, window, cx| {
                let notes = shell.read(cx).notes().clone();
                notes.update(cx, |n, cx| n.open_note_titled_at_end(&title, window, cx));
            })?;
        }
    }
    Ok((window, shell))
}
