//! A forgotten master password, handled honestly (ADR 0006).
//!
//! "Forgot master password?" (under Unlock) opens one dialog with two
//! clearly different paths:
//!
//! * **Use recovery key** (only if one exists): type the key from the
//!   Emergency Kit, choose a new master password. Everything is kept.
//! * **Start a new vault**: for when there's no key. The current vault is
//!   *archived, still encrypted* (Settings → Passwords → Archived vaults can
//!   open it later with its old password), then setup runs again. Nothing is
//!   deleted silently; the user confirms after reading what happens.
//!
//! Also here: the recovery key box (copy, save Emergency Kit) used by setup
//! and by "Create new recovery key", and opening an archived vault.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _,
    Render, SharedString, Styled as _, Subscription, Window, div, px, rems,
};
use ottero_core::vault::health::{Strength, estimate};
use ottero_core::vault::recovery;
use ottero_crypto::{SecretString, Zeroizing};

use crate::library::Library;
use crate::views::passwords::{MIN_MASTER_CHARS, strength_meter};
use crate::views::vault_item::copy_field;

/// The recovery key in a mono box with Copy and Save Emergency Kit.
pub fn key_box(key: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let copy_value = key.to_owned();
    let kit_value = key.to_owned();
    v_flex()
        .gap_2()
        .child(
            div()
                .px_4()
                .py_4()
                .rounded(theme.radius_lg)
                .bg(theme.muted)
                .text_center()
                .text_lg()
                .font_family(theme.mono_font_family.clone())
                .font_weight(FontWeight::MEDIUM)
                .child(key.to_owned()),
        )
        .child(
            h_flex()
                .gap_2()
                .justify_center()
                .child(
                    Button::new("recovery-copy")
                        .small()
                        .icon(Lucide::Copy)
                        .label("Copy")
                        .on_click(move |_, window, cx| {
                            copy_field("Recovery key", copy_value.clone(), true, window, cx)
                        }),
                )
                .child(
                    Button::new("recovery-kit")
                        .small()
                        .icon(Lucide::Download)
                        .label("Save Emergency Kit…")
                        .on_click(move |_, window, cx| save_kit(kit_value.clone(), window, cx)),
                ),
        )
        .into_any_element()
}

/// Writes the Emergency Kit where the user picks (Documents by default).
fn save_kit(key: String, window: &mut Window, cx: &mut App) {
    let key = Zeroizing::new(key);
    let dir = directories::UserDirs::new()
        .and_then(|d| d.document_dir().map(std::path::Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir);
    let path = cx.prompt_for_new_path(&dir, Some("Ottero Emergency Kit.txt"));
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let created = ottero_core::clock::today()
                .strftime("%B %-d, %Y")
                .to_string();
            let kit = recovery::emergency_kit(&key, &created);
            let written = std::fs::write(&path, kit.as_bytes());
            _ = cx.update(|window, cx| match written {
                Ok(()) => window.push_notification(
                    "Emergency Kit saved. Print it or move it off this computer.",
                    cx,
                ),
                Err(_) => {
                    ottero_ui::feedback::error_toast("Couldn't save the Emergency Kit.", window, cx)
                }
            });
        })
        .detach();
}

/// Opens "Forgot master password?".
pub fn open_forgot(library: Entity<Library>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| Forgot::new(library, window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title("Forgot master password?")
            .w(px(480.))
            .child(view.clone())
    });
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Choose,
    Recover,
    StartOver,
}

pub struct Forgot {
    library: Entity<Library>,
    step: Step,
    has_key: bool,
    key: Entity<InputState>,
    new: Entity<InputState>,
    confirm: Entity<InputState>,
    error: Option<SharedString>,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

impl Forgot {
    fn new(library: Entity<Library>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let key = cx.new(|cx| InputState::new(window, cx).placeholder("XXXX-XXXX-XXXX-…"));
        let masked = |p: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).masked(true).placeholder(p))
        };
        let new = masked("New master password", window, cx);
        let confirm = masked("Type it again", window, cx);
        let subscriptions = [&key, &new, &confirm]
            .into_iter()
            .map(|i| {
                cx.subscribe_in(i, window, |this, _, e, window, cx| match e {
                    InputEvent::Change => {
                        this.error = None;
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.recover(window, cx),
                    _ => {}
                })
            })
            .collect();
        let has_key = library.read(cx).vault_has_recovery();
        Self {
            library,
            step: Step::Choose,
            has_key,
            key,
            new,
            confirm,
            error: None,
            busy: false,
            _subscriptions: subscriptions,
        }
    }

    fn problem(&self, cx: &App) -> Option<&'static str> {
        if recovery::normalize(&self.key.read(cx).value()).is_none() {
            return Some("Type the 32-character key from your Emergency Kit.");
        }
        let new = self.new.read(cx).value();
        if new.chars().count() < MIN_MASTER_CHARS {
            return Some("Use at least 10 characters for the new password.");
        }
        if estimate(&new, &[]).strength < Strength::Fair {
            return Some("Choose something harder to guess.");
        }
        if new != self.confirm.read(cx).value() {
            return Some("The new passwords don't match.");
        }
        None
    }

    fn recover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.step != Step::Recover || self.busy || self.problem(cx).is_some() {
            return;
        }
        self.busy = true;
        cx.notify();
        let key = Zeroizing::new(self.key.read(cx).value().to_string());
        let new = SecretString::from(self.new.read(cx).value().to_string());
        let task = self
            .library
            .update(cx, |lib, cx| lib.recover_vault(&key, new, cx));
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(true) => {
                        for i in [this.key.clone(), this.new.clone(), this.confirm.clone()] {
                            i.update(cx, |i, cx| i.set_value("", window, cx));
                        }
                        window.close_dialog(cx);
                        window.push_notification(
                            "Vault recovered. Your new master password is set.",
                            cx,
                        );
                    }
                    Ok(false) => {
                        this.error = Some("That recovery key doesn't open this vault.".into())
                    }
                    Err(err) => this.error = Some(format!("{err:#}").into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn start_over(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let result = self.library.update(cx, |lib, cx| lib.start_new_vault(cx));
        match result {
            Ok(path) => {
                window.close_dialog(cx);
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                window.push_notification(
                    format!("Old vault archived as {name}. If you remember its password, open it from Settings → Passwords."),
                    cx,
                );
            }
            Err(err) => {
                self.error = Some(format!("{err:#}").into());
                cx.notify();
            }
        }
    }

    fn option(
        id: &'static str,
        icon: Lucide,
        title: &'static str,
        text: &'static str,
        enabled: bool,
        step: Step,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        Button::new(id)
            .outline()
            .w_full()
            .h_auto()
            .disabled(!enabled)
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .py_2()
                    .items_start()
                    .child(Icon::new(icon).text_color(theme.muted_foreground))
                    .child(
                        v_flex()
                            .flex_1()
                            .items_start()
                            .gap_0p5()
                            .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(title))
                            .child(
                                div()
                                    .text_xs()
                                    .text_left()
                                    .whitespace_normal()
                                    .text_color(theme.muted_foreground)
                                    .child(text),
                            ),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.step = step;
                this.error = None;
                if step == Step::Recover {
                    this.key.update(cx, |i, cx| i.focus(window, cx));
                }
                cx.notify();
            }))
    }
}

impl Render for Forgot {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let error = self
            .error
            .clone()
            .map(|e| div().text_xs().text_color(theme.danger).child(e));
        match self.step {
            Step::Choose => v_flex()
                .gap_3()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("Ottero never sees your master password, so it can't reset it. Choose what to do:"),
                )
                .child(Self::option(
                    "forgot-recover",
                    Lucide::KeyRound,
                    "Use recovery key",
                    if self.has_key {
                        "From your Emergency Kit. Keeps everything; you choose a new master password."
                    } else {
                        "This vault has no recovery key."
                    },
                    self.has_key,
                    Step::Recover,
                    cx,
                ))
                .child(Self::option(
                    "forgot-start-over",
                    Lucide::RotateCcw,
                    "Start a new vault",
                    "Begin again with an empty vault. The old one is kept, encrypted, in case you remember its password.",
                    true,
                    Step::StartOver,
                    cx,
                ))
                .into_any_element(),
            Step::Recover => {
                let new = self.new.read(cx).value().to_string();
                let meter = (!new.is_empty()).then(|| {
                    let e = estimate(&new, &[]);
                    strength_meter(e.strength, Some(&e.crack_time), cx)
                });
                let problem = self.problem(cx);
                v_flex()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(div().text_xs().text_color(theme.muted_foreground).child("Recovery key"))
                            .child(Input::new(&self.key).id("forgot-key")),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .child(Input::new(&self.new).id("forgot-new").mask_toggle())
                            .children(meter)
                            .child(Input::new(&self.confirm).id("forgot-confirm").mask_toggle()),
                    )
                    .children(error)
                    .child(
                        h_flex()
                            .justify_between()
                            .child(
                                Button::new("forgot-back")
                                    .ghost()
                                    .small()
                                    .label("Back")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.step = Step::Choose;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("forgot-recover-go")
                                    .primary()
                                    .label(if self.busy { "Recovering…" } else { "Recover vault" })
                                    .loading(self.busy)
                                    .disabled(problem.is_some() || self.busy)
                                    .on_click(cx.listener(|this, _, window, cx| this.recover(window, cx))),
                            ),
                    )
                    .when(self.error.is_none(), |s| {
                        s.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .children(problem),
                        )
                    })
                    .into_any_element()
            }
            Step::StartOver => v_flex()
                .gap_4()
                .child(
                    v_flex()
                        .gap_2()
                        .text_sm()
                        .child("Starting a new vault means:")
                        .child(div().text_color(theme.muted_foreground).child(
                            "• You won't be able to see the passwords in your current vault.",
                        ))
                        .child(div().text_color(theme.muted_foreground).child(
                            "• It isn't deleted: it's moved to Archived vaults, still encrypted.",
                        ))
                        .child(div().text_color(theme.muted_foreground).child(
                            "• If you remember the old password later, open the archive and its items come back.",
                        )),
                )
                .children(error)
                .child(
                    h_flex()
                        .justify_between()
                        .child(
                            Button::new("forgot-back-2")
                                .ghost()
                                .small()
                                .label("Back")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.step = Step::Choose;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("forgot-start-go")
                                .danger()
                                .label("Archive and start over")
                                .on_click(cx.listener(|this, _, window, cx| this.start_over(window, cx))),
                        ),
                )
                .into_any_element(),
        }
    }
}

/// Opens an archived vault with its old password and adds its items.
pub fn open_archive(
    path: std::path::PathBuf,
    library: Entity<Library>,
    window: &mut Window,
    cx: &mut App,
) {
    open_vault_file(path, false, library, window, cx);
}

/// Opens an archive or an exported backup and adds its items. Both are the
/// same file format; only the wording differs.
fn open_vault_file(
    path: std::path::PathBuf,
    backup: bool,
    library: Entity<Library>,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| OpenArchive::new(path, backup, library, window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(if backup {
                "Restore vault backup"
            } else {
                "Open archived vault"
            })
            .w(px(440.))
            .child(view.clone())
    });
}

/// Saves an encrypted backup of the unlocked vault where the user picks.
pub fn export_backup(library: Entity<Library>, window: &mut Window, cx: &mut App) {
    if !library.read(cx).vault_is_unlocked() {
        ottero_ui::feedback::error_toast("Unlock the vault first.", window, cx);
        return;
    }
    let dir = directories::UserDirs::new()
        .and_then(|d| d.document_dir().map(std::path::Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir);
    let name = format!(
        "Ottero Vault {}.otvault",
        ottero_core::clock::today().strftime("%Y-%m-%d")
    );
    let path = cx.prompt_for_new_path(&dir, Some(&name));
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            _ = cx.update(|window, cx| {
                match library.read(cx).export_vault(&path) {
                    Ok(()) => window.push_notification(
                        "Vault backup saved. It's encrypted and opens with your current master password.",
                        cx,
                    ),
                    Err(err) => {
                        ottero_ui::feedback::error_toast(format!("{err:#}"), window, cx);
                    }
                }
            });
        })
        .detach();
}

/// Picks a `.otvault` (or older `.cfvault`/`.ksvault`) backup or archive and opens it to add its items.
pub fn restore_backup(library: Entity<Library>, window: &mut Window, cx: &mut App) {
    if !library.read(cx).vault_is_unlocked() {
        ottero_ui::feedback::error_toast("Unlock the vault first.", window, cx);
        return;
    }
    let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some("Restore".into()),
    });
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            _ = cx.update(|window, cx| {
                // Check the format before asking for a password.
                match crate::vault::ArchiveFile::read(&path) {
                    Ok(_) => open_vault_file(path, true, library, window, cx),
                    Err(err) => ottero_ui::feedback::error_toast(format!("{err:#}"), window, cx),
                }
            });
        })
        .detach();
}

pub struct OpenArchive {
    path: std::path::PathBuf,
    backup: bool,
    library: Entity<Library>,
    password: Entity<InputState>,
    error: Option<SharedString>,
    busy: bool,
    _subscriptions: Vec<Subscription>,
}

impl OpenArchive {
    fn new(
        path: std::path::PathBuf,
        backup: bool,
        library: Entity<Library>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder(if backup {
                    "The master password when it was saved"
                } else {
                    "The old vault's master password"
                })
        });
        let sub = cx.subscribe_in(&password, window, |this, _, e, window, cx| match e {
            InputEvent::PressEnter { .. } => this.submit(window, cx),
            InputEvent::Change => {
                this.error = None;
                cx.notify();
            }
            _ => {}
        });
        password.update(cx, |i, cx| i.focus(window, cx));
        Self {
            path,
            backup,
            library,
            password,
            error: None,
            busy: false,
            _subscriptions: vec![sub],
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.password.read(cx).value().to_string();
        if self.busy || text.is_empty() {
            return;
        }
        self.busy = true;
        cx.notify();
        let path = self.path.clone();
        let task = self.library.update(cx, |lib, cx| {
            lib.import_vault_archive(path, SecretString::from(text), cx)
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(Some(0)) => {
                        window.close_dialog(cx);
                        window.push_notification(
                            "Nothing to add: everything in it is already in your vault.",
                            cx,
                        );
                    }
                    Ok(Some(n)) => {
                        window.close_dialog(cx);
                        window.push_notification(
                            format!(
                                "Added {n} item{} from the {}.",
                                if n == 1 { "" } else { "s" },
                                if this.backup { "backup" } else { "archive" }
                            ),
                            cx,
                        );
                    }
                    Ok(None) => {
                        this.error = Some(if this.backup {
                            "That password doesn't open this backup.".into()
                        } else {
                            "That password doesn't open this archive.".into()
                        })
                    }
                    Err(err) => this.error = Some(format!("{err:#}").into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for OpenArchive {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        v_flex()
            .gap_4()
            .child(div().text_sm().text_color(theme.muted_foreground).child(if self.backup {
                "Its items are added to your current vault as copies. The backup file stays as it is."
            } else {
                "Its items are added to your current vault. The archive file stays as it is."
            }))
            .child(Input::new(&self.password).mask_toggle())
            .children(
                self.error
                    .clone()
                    .map(|e| div().text_xs().text_color(theme.danger).child(e)),
            )
            .child(
                h_flex().justify_end().child(
                    Button::new("archive-open")
                        .primary()
                        .label(if self.busy {
                            "Opening…"
                        } else {
                            "Open and add items"
                        })
                        .loading(self.busy)
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                ),
            )
            .map(|s| s.child(div().h(rems(0.))))
    }
}

/// Lists archived vaults with "Open…" for each.
pub fn open_archives(library: Entity<Library>, window: &mut Window, cx: &mut App) {
    let archives = library.read(cx).vault_archives();
    let unlocked = library.read(cx).vault_state() == ottero_core::vault::lock::LockState::Unlocked;
    window.open_dialog(cx, move |dialog, _, cx| {
        let theme = cx.theme().clone();
        let rows = archives.iter().enumerate().map(|(i, (path, at, n))| {
            let when = at
                .to_zoned(jiff::tz::TimeZone::system())
                .strftime("%b %-d, %Y at %H:%M")
                .to_string();
            let path = path.clone();
            let library = library.clone();
            h_flex()
                .gap_3()
                .py_2()
                .items_center()
                .when(i > 0, |s| s.border_t_1().border_color(theme.border))
                .child(
                    v_flex()
                        .flex_1()
                        .child(div().text_sm().child(format!("Archived {when}")))
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!("{n} item{}", if *n == 1 { "" } else { "s" })),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("archive-{i}")))
                        .small()
                        .label("Open…")
                        .disabled(!unlocked)
                        .on_click(move |_, window, cx| {
                            window.close_dialog(cx);
                            open_archive(path.clone(), library.clone(), window, cx);
                        }),
                )
        });
        dialog
            .title("Archived vaults")
            .w(px(440.))
            .child(
                v_flex()
                    .gap_2()
                    .child(div().text_sm().text_color(theme.muted_foreground).child(if archives.is_empty() {
                        "No archived vaults. One is kept here if you ever start a new vault."
                    } else if unlocked {
                        "Opening one asks for the master password it had, then adds its items to this vault."
                    } else {
                        "Unlock your vault first, then open an archive to add its items."
                    }))
                    .children(rows),
            )
    });
}
