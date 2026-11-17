//! Settings → Account: Ottero Cloud sign-up, code entry, Emergency Kit,
//! sign-in (with the Secret Key on a new device) and the signed-in page.
//!
//! The screen follows `Library::account().state()` (the state machine in
//! `ottero_core::account`); this view only owns the form fields and which
//! form a signed-out user opened ([`Mode`]). Every step that waits on the
//! network or Argon2 shows the spinner on its own button; errors sit next to
//! the field they are about. Enter submits, Tab moves between fields, Escape
//! goes back.
//!
//! The page is built as Settings groups ([`page`]) so it shares the look,
//! sidebar sections and search of the other Settings pages.

use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::setting::{SettingGroup, SettingItem, SettingPage};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _,
    IntoElement, KeyDownEvent, ParentElement as _, Render, SharedString, Styled as _, Subscription,
    Task as GpuiTask, Window, div, px, rems,
};
use ottero_core::account::{
    AccountState, DeviceKind, Flow, Plan, SecretKeyPresence, active_label, check_password,
    device_label,
};
use ottero_crypto::{SecretString, Zeroizing};

use crate::account::Field;
use crate::library::{Library, LibraryEvent};
use crate::views::passwords::strength_meter;
use crate::views::vault_item::copy_field;

/// Which form a signed-out user is looking at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Intro,
    SignUp,
    SignIn,
}

/// What the screen shows, derived from the account state and [`Mode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Intro,
    SignUp,
    SignIn,
    Code,
    EmergencyKit,
    SignedIn,
}

pub struct AccountView {
    library: Entity<Library>,
    mode: Mode,
    email: Entity<InputState>,
    password: Entity<InputState>,
    confirm: Entity<InputState>,
    secret_key: Entity<InputState>,
    code: Entity<InputState>,
    unlock: Entity<InputState>,
    kit_saved: bool,
    reveal_secret_key: bool,
    unlock_error: Option<SharedString>,
    last_screen: Screen,
    _tick: Option<GpuiTask<()>>,
    _subscriptions: Vec<Subscription>,
}

impl AccountView {
    pub fn new(library: Entity<Library>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let plain = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
        };
        let masked = |placeholder: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(placeholder)
            })
        };
        let email = plain("name@example.com", window, cx);
        let password = masked("At least 10 characters", window, cx);
        let confirm = masked("Type it again", window, cx);
        let secret_key = plain("O1-XXXXXX-XXXXX-XXXXX-XXXXX-XXXXX", window, cx);
        let code = plain("123 456", window, cx);
        let unlock = masked("Password", window, cx);

        let mut subscriptions =
            vec![
                cx.subscribe_in(&library, window, |this, _, event, window, cx| {
                    if *event == LibraryEvent::AccountChanged {
                        this.on_account_changed(window, cx);
                    }
                }),
            ];
        for input in [&email, &password, &confirm, &secret_key] {
            subscriptions.push(
                cx.subscribe_in(input, window, |this, _, event, window, cx| match event {
                    InputEvent::Change => {
                        this.library.update(cx, Library::clear_account_error);
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.submit_form(window, cx),
                    _ => {}
                }),
            );
        }
        subscriptions.push(cx.subscribe_in(
            &code,
            window,
            |this, _, event, window, cx| match event {
                InputEvent::Change => this.code_changed(window, cx),
                InputEvent::PressEnter { .. } => this.verify(cx),
                _ => {}
            },
        ));
        subscriptions.push(cx.subscribe_in(
            &unlock,
            window,
            |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.unlock_error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.unlock_account(window, cx),
                _ => {}
            },
        ));

        let this = Self {
            library: library.clone(),
            mode: Mode::Intro,
            email,
            password,
            confirm,
            secret_key,
            code,
            unlock,
            kit_saved: false,
            reveal_secret_key: false,
            unlock_error: None,
            last_screen: Screen::Intro,
            _tick: None,
            _subscriptions: subscriptions,
        };
        library.update(cx, Library::refresh_account);
        this
    }

    #[must_use]
    pub fn library(&self) -> &Entity<Library> {
        &self.library
    }

    #[must_use]
    pub fn mode(&self) -> Mode {
        self.mode
    }

    #[must_use]
    pub fn screen(&self, cx: &App) -> Screen {
        match self.library.read(cx).account().state() {
            AccountState::SignedOut => match self.mode {
                Mode::Intro => Screen::Intro,
                Mode::SignUp => Screen::SignUp,
                Mode::SignIn => Screen::SignIn,
            },
            AccountState::SigningUp { .. } => Screen::SignUp,
            AccountState::Error { flow, .. } => match flow {
                Flow::SignUp => Screen::SignUp,
                Flow::SignIn => Screen::SignIn,
            },
            AccountState::SigningIn { .. } | AccountState::NeedsSecretKey { .. } => Screen::SignIn,
            AccountState::AwaitingCode { .. } => Screen::Code,
            AccountState::EmergencyKit { .. } => Screen::EmergencyKit,
            AccountState::SignedIn { .. } => Screen::SignedIn,
        }
    }

    /// Opens a form (from a button or a command).
    pub fn open(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        if self.library.read(cx).account().state().is_signed_in() {
            self.library.update(cx, Library::refresh_account);
            return;
        }
        if matches!(self.screen(cx), Screen::Code | Screen::EmergencyKit) {
            return;
        }
        self.library.update(cx, Library::cancel_account_flow);
        self.mode = mode;
        self.focus_for(self.screen(cx), window, cx);
        cx.notify();
    }

    fn back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.screen(cx) {
            Screen::SignUp | Screen::SignIn if self.library.read(cx).account().is_busy() => {}
            Screen::SignUp | Screen::SignIn => {
                self.library.update(cx, Library::cancel_account_flow);
                self.mode = Mode::Intro;
                self.clear_secrets(window, cx);
            }
            Screen::Code => self.change_email(window, cx),
            Screen::Intro | Screen::EmergencyKit | Screen::SignedIn => {}
        }
        cx.notify();
    }

    fn change_email(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.library.update(cx, Library::cancel_account_flow);
        self.mode = Mode::SignUp;
        self.code.update(cx, |i, cx| i.set_value("", window, cx));
        self.focus_for(Screen::SignUp, window, cx);
        cx.notify();
    }

    fn clear_secrets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [
            &self.password,
            &self.confirm,
            &self.secret_key,
            &self.code,
            &self.unlock,
        ] {
            input.update(cx, |i, cx| i.set_value("", window, cx));
        }
    }

    fn focus_for(&self, screen: Screen, window: &mut Window, cx: &mut Context<Self>) {
        let target = match screen {
            Screen::SignUp | Screen::SignIn => {
                if self.email.read(cx).value().is_empty() {
                    &self.email
                } else if screen == Screen::SignIn
                    && self.needs_secret_key(cx)
                    && !self.password.read(cx).value().is_empty()
                {
                    &self.secret_key
                } else {
                    &self.password
                }
            }
            Screen::Code => &self.code,
            _ => return,
        };
        target.update(cx, |i, cx| i.focus(window, cx));
    }

    fn on_account_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let screen = self.screen(cx);
        let account = self.library.read(cx).account();
        if matches!(account.state(), AccountState::NeedsSecretKey { .. }) {
            self.secret_key.update(cx, |i, cx| i.focus(window, cx));
        }
        if screen != self.last_screen {
            match screen {
                Screen::Code => {
                    self.code.update(cx, |i, cx| i.set_value("", window, cx));
                    self.focus_for(Screen::Code, window, cx);
                }
                Screen::EmergencyKit => {
                    self.kit_saved = false;
                    self.password
                        .update(cx, |i, cx| i.set_value("", window, cx));
                    self.confirm.update(cx, |i, cx| i.set_value("", window, cx));
                }
                Screen::SignedIn => {
                    self.mode = Mode::Intro;
                    self.clear_secrets(window, cx);
                }
                Screen::Intro => {
                    self.reveal_secret_key = false;
                    self.clear_secrets(window, cx);
                }
                Screen::SignUp | Screen::SignIn => {}
            }
            self.last_screen = screen;
        }
        if !self.library.read(cx).account().is_unlocked() {
            self.reveal_secret_key = false;
        }
        // Re-render once a second while "Resend in N s" counts down.
        if screen == Screen::Code && self.library.read(cx).account().resend_wait().is_some() {
            if self._tick.is_none() {
                self._tick = Some(cx.spawn_in(window, async move |this, cx| {
                    loop {
                        cx.background_executor().timer(Duration::from_secs(1)).await;
                        let more = this.update(cx, |this, cx| {
                            cx.notify();
                            this.library.read(cx).account().resend_wait().is_some()
                        });
                        if !matches!(more, Ok(true)) {
                            _ = this.update(cx, |this, _| this._tick = None);
                            break;
                        }
                    }
                }));
            }
        } else {
            self._tick = None;
        }
        cx.notify();
    }

    /// Whether the sign-in form shows the Secret Key field (no valid key
    /// stored on this device for the typed email).
    #[must_use]
    pub fn needs_secret_key(&self, cx: &App) -> bool {
        let account = self.library.read(cx).account();
        if matches!(account.state(), AccountState::NeedsSecretKey { .. }) {
            return true;
        }
        let email = self.email.read(cx).value();
        !email.trim().is_empty()
            && account.secret_key_presence(&email) == SecretKeyPresence::Missing
    }

    fn submit_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.screen(cx) {
            Screen::SignUp => self.create_account(cx),
            Screen::SignIn => self.sign_in(window, cx),
            _ => {}
        }
    }

    fn signup_problem(&self, cx: &App) -> Option<&'static str> {
        let password = self.password.read(cx).value();
        let confirm = self.confirm.read(cx).value();
        let email = self.email.read(cx).value();
        if password.is_empty() {
            return None;
        }
        if let Some(problem) = check_password(&password, &email).problem {
            return Some(problem);
        }
        if !confirm.is_empty() && confirm != password {
            return Some("The passwords don't match.");
        }
        None
    }

    fn create_account(&mut self, cx: &mut Context<Self>) {
        let password = self.password.read(cx).value();
        let confirm = self.confirm.read(cx).value();
        if confirm != password {
            self.library.update(cx, |lib, cx| {
                lib.set_account_error(Field::Password, "The passwords don't match.", cx);
            });
            return;
        }
        let email = self.email.read(cx).value().to_string();
        let password = SecretString::from(password.to_string());
        self.library
            .update(cx, |lib, cx| lib.account_sign_up(&email, password, cx));
    }

    fn sign_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let email = self.email.read(cx).value().to_string();
        let password = SecretString::from(self.password.read(cx).value().to_string());
        let key = Zeroizing::new(self.secret_key.read(cx).value().to_string());
        if self.needs_secret_key(cx)
            && key.trim().is_empty()
            && !self.password.read(cx).value().is_empty()
        {
            self.secret_key.update(cx, |i, cx| i.focus(window, cx));
        }
        self.library.update(cx, |lib, cx| {
            lib.account_sign_in(&email, password, &key, cx)
        });
    }

    /// Digits only, at most six, shown as "123 456"; the sixth digit submits.
    fn code_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.code.read(cx).value();
        let digits: String = value.chars().filter(char::is_ascii_digit).take(6).collect();
        let shown = if digits.len() > 3 {
            format!("{} {}", &digits[..3], &digits[3..])
        } else {
            digits.clone()
        };
        if shown != value.as_ref() {
            self.code.update(cx, |i, cx| i.set_value(shown, window, cx));
        }
        self.library.update(cx, Library::clear_account_error);
        if digits.len() == 6 {
            self.verify(cx);
        }
        cx.notify();
    }

    fn verify(&mut self, cx: &mut Context<Self>) {
        let code = Zeroizing::new(self.code.read(cx).value().to_string());
        self.library
            .update(cx, |lib, cx| lib.verify_account_code(&code, cx));
    }

    fn finish_kit(&mut self, cx: &mut Context<Self>) {
        if self.kit_saved {
            self.library.update(cx, Library::finish_emergency_kit);
        }
    }

    fn unlock_account(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let password = SecretString::from(self.unlock.read(cx).value().to_string());
        let task = self
            .library
            .update(cx, |lib, cx| lib.unlock_account(password, cx));
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(()) => {
                        this.unlock.update(cx, |i, cx| i.set_value("", window, cx));
                        this.unlock_error = None;
                    }
                    Err(message) => this.unlock_error = Some(message.into()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// "View Secret Key" / the Copy Secret Key command: asks for the
    /// password first unless it was entered since Ottero started.
    pub fn show_secret_key(&mut self, copy: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.library.read(cx).account().is_unlocked() {
            self.reveal_secret_key = true;
            if copy {
                self.copy_secret_key(window, cx);
            }
            cx.notify();
            return;
        }
        let view = cx.entity().downgrade();
        open_password_prompt(
            self.library.clone(),
            "View Secret Key",
            "Enter your Ottero Cloud password to show your Secret Key.",
            window,
            cx,
            move |window, cx| {
                _ = view.update(cx, |this, cx| {
                    this.reveal_secret_key = true;
                    if copy {
                        this.copy_secret_key(window, cx);
                    }
                    cx.notify();
                });
            },
        );
    }

    fn copy_secret_key(&self, window: &mut Window, cx: &mut App) {
        if let Some(key) = self.library.read(cx).account().secret_key_text() {
            copy_field("Secret Key", key.to_string(), true, window, cx);
        }
    }

    /// Saves the kit; when signed in it asks for the password first.
    pub fn save_kit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let account = self.library.read(cx).account();
        if matches!(account.state(), AccountState::EmergencyKit { .. }) || account.is_unlocked() {
            save_kit(self.library.clone(), window, cx);
            return;
        }
        if !account.state().is_signed_in() {
            return;
        }
        let library = self.library.clone();
        open_password_prompt(
            self.library.clone(),
            "Save Emergency Kit",
            "Enter your Ottero Cloud password to save a kit with your Secret Key.",
            window,
            cx,
            move |window, cx| save_kit(library.clone(), window, cx),
        );
    }

    pub fn confirm_sign_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.library.read(cx).account().state().is_signed_in() {
            return;
        }
        let library = self.library.clone();
        ottero_ui::feedback::confirm_destructive(
            ottero_ui::feedback::Confirm::new(
                "Sign out of Ottero Cloud?",
                "Everything stays on this computer. Your Secret Key stays too, so signing back in only needs your password.",
                "Sign out",
            ),
            window,
            cx,
            move |window, cx| {
                let result = library.update(cx, Library::account_sign_out);
                ottero_ui::feedback::report(result, window, cx);
            },
        );
    }

    fn confirm_revoke(&self, id: String, label: String, window: &mut Window, cx: &mut App) {
        let library = self.library.clone();
        ottero_ui::feedback::confirm_destructive(
            ottero_ui::feedback::Confirm::new(
                format!("Sign out “{label}”?"),
                "That device stops syncing and needs the password to sign in again.",
                "Sign out",
            ),
            window,
            cx,
            move |window, cx| {
                let task = library.update(cx, |lib, cx| lib.revoke_account_session(&id, cx));
                window
                    .spawn(cx, async move |cx| {
                        if let Err(message) = task.await {
                            _ = cx.update(|window, cx| {
                                ottero_ui::feedback::error_toast(message, window, cx)
                            });
                        }
                    })
                    .detach();
            },
        );
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape"
            && matches!(
                self.screen(cx),
                Screen::SignUp | Screen::SignIn | Screen::Code
            )
            && !window.has_active_dialog(cx)
        {
            self.back(window, cx);
            cx.stop_propagation();
        }
    }

    // ---- Rendering -------------------------------------------------------

    fn render_intro(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let notice = self.library.read(cx).account().notice().map(str::to_owned);
        let point = |icon: Lucide, text: &'static str| {
            h_flex()
                .gap_3()
                .items_start()
                .child(
                    div()
                        .pt(px(2.))
                        .child(Icon::new(icon).small().text_color(theme.muted_foreground)),
                )
                .child(div().text_sm().child(text))
        };
        v_flex()
            .w_full()
            .gap_5()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(icon_tile(Lucide::Cloud, cx))
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Ottero Cloud"),
                            )
                            .child(div().text_sm().text_color(theme.muted_foreground).child(
                                "One account for Ottero on your Mac, Windows PC and phone.",
                            )),
                    ),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(point(
                        Lucide::RefreshCw,
                        "Notes, tasks, calendar and passwords on every device.",
                    ))
                    .child(point(
                        Lucide::LockKeyhole,
                        "End-to-end encrypted. Only you can read your data, not even Ottero.",
                    ))
                    .child(point(
                        Lucide::HardDrive,
                        "Optional. Ottero works fully without an account.",
                    )),
            )
            .children(notice.map(|n| div().text_sm().text_color(theme.warning).child(n)))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("account-create")
                            .primary()
                            .label("Create account")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open(Mode::SignUp, window, cx)
                            })),
                    )
                    .child(Button::new("account-sign-in").label("Sign in").on_click(
                        cx.listener(|this, _, window, cx| this.open(Mode::SignIn, window, cx)),
                    )),
            )
            .child(
                div().text_xs().text_color(theme.muted_foreground).child(
                    "Sync arrives in an upcoming update. Your account will be ready for it.",
                ),
            )
            .into_any_element()
    }

    fn render_sign_up(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let account = self.library.read(cx).account();
        let busy = account.is_busy();
        let email_error = account.error_for(Field::Email).map(str::to_owned);
        let password_error = account.error_for(Field::Password).map(str::to_owned);
        let general = account.error_for(Field::General).map(str::to_owned);
        let password = self.password.read(cx).value();
        let email = self.email.read(cx).value();
        let report = (!password.is_empty()).then(|| check_password(&password, &email));
        let meter = report
            .as_ref()
            .map(|r| strength_meter(r.strength, None, cx));
        let password_note = password_error
            .map(|e| (SharedString::from(e), true))
            .or_else(|| {
                self.signup_problem(cx)
                    .map(|p| (SharedString::from(p), false))
            })
            .or_else(|| {
                report
                    .and_then(|r| r.hint)
                    .map(|h| (SharedString::from(h), false))
            });
        v_flex()
            .w_full()
            .gap_4()
            .child(field("Email", Input::new(&self.email).disabled(busy).into_any_element(), email_error.map(|e| (e.into(), true)), cx))
            .child(
                v_flex()
                    .gap_1p5()
                    .child(label("Password", cx))
                    .child(Input::new(&self.password).mask_toggle().disabled(busy))
                    .children(meter)
                    .children(password_note.map(|(text, error)| note(text, error, cx))),
            )
            .child(field("Confirm password", Input::new(&self.confirm).mask_toggle().disabled(busy).into_any_element(), None, cx))
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Your password also encrypts your data. Ottero can't reset it: if you forget it, only your Emergency Kit gets you back in."),
            )
            .children(general.map(|e| note(e.into(), true, cx)))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("account-sign-up-submit")
                            .primary()
                            .label(if busy { "Creating account…" } else { "Create account" })
                            .loading(busy)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.create_account(cx))),
                    )
                    .child(
                        Button::new("account-sign-up-back")
                            .ghost()
                            .label("Back")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| this.back(window, cx))),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("account-to-sign-in")
                            .ghost()
                            .label("I have an account")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| this.open(Mode::SignIn, window, cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_sign_in(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let account = self.library.read(cx).account();
        let busy = account.is_busy();
        let email_error = account.error_for(Field::Email).map(str::to_owned);
        let password_error = account.error_for(Field::Password).map(str::to_owned);
        let key_error = account.error_for(Field::SecretKey).map(str::to_owned);
        let general = account.error_for(Field::General).map(str::to_owned);
        let needs_key = self.needs_secret_key(cx);
        v_flex()
            .w_full()
            .gap_4()
            .child(field("Email", Input::new(&self.email).disabled(busy).into_any_element(), email_error.map(|e| (e.into(), true)), cx))
            .child(field(
                "Password",
                Input::new(&self.password).mask_toggle().disabled(busy).into_any_element(),
                password_error.map(|e| (e.into(), true)),
                cx,
            ))
            .when(needs_key, |this| {
                this.child(field(
                    "Secret Key",
                    Input::new(&self.secret_key).disabled(busy).into_any_element(),
                    Some(key_error.map_or_else(
                        || ("It's in your Emergency Kit. A signed-in device also shows it in Settings → Account.".into(), false),
                        |e| (e.into(), true),
                    )),
                    cx,
                ))
            })
            .children(general.map(|e| note(e.into(), true, cx)))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("account-sign-in-submit")
                            .primary()
                            .label(if busy { "Signing in…" } else { "Sign in" })
                            .loading(busy)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| this.sign_in(window, cx))),
                    )
                    .child(
                        Button::new("account-sign-in-back")
                            .ghost()
                            .label("Back")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| this.back(window, cx))),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("account-to-sign-up")
                            .ghost()
                            .label("Create account")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| this.open(Mode::SignUp, window, cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_code(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let account = self.library.read(cx).account();
        let email = account.state().email().unwrap_or_default().to_owned();
        let busy = account.code_busy();
        let wait = account.resend_wait();
        let code_error = account.error_for(Field::Code).map(str::to_owned);
        let general = account.error_for(Field::General).map(str::to_owned);
        v_flex()
            .w_full()
            .gap_4()
            .child(
                div()
                    .text_sm()
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_1()
                            .child("Enter the 6-digit code sent to")
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(email.clone()))
                            .child("It expires in 10 minutes."),
                    ),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .child(label("Code", cx))
                    .child(div().w(rems(12.)).child(Input::new(&self.code).large().disabled(busy)))
                    .children(code_error.map(|e| note(e.into(), true, cx))),
            )
            .children(general.map(|e| note(e.into(), true, cx)))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("account-verify")
                            .primary()
                            .label(if busy { "Verifying…" } else { "Verify" })
                            .loading(busy)
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.verify(cx))),
                    )
                    .child(
                        Button::new("account-resend")
                            .ghost()
                            .label(match wait {
                                Some(w) => format!("Resend in {} s", w.as_secs() + 1),
                                None => "Resend code".to_owned(),
                            })
                            .disabled(busy || wait.is_some())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.library.update(cx, Library::resend_account_code);
                            })),
                    )
                    .child(
                        Button::new("account-change-email")
                            .ghost()
                            .label("Change email")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, window, cx| this.change_email(window, cx))),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("No email? Check your spam folder. If this email already has an Ottero account, no code is sent: sign in instead."),
            )
            .into_any_element()
    }

    fn render_kit(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let account = self.library.read(cx).account();
        let email = account.state().email().unwrap_or_default().to_owned();
        let secret = account
            .secret_key_text()
            .map(|k| k.to_string())
            .unwrap_or_default();
        let recovery = account
            .recovery_code_text()
            .map(|k| k.to_string())
            .unwrap_or_default();
        let busy = account.kit_busy();
        let general = account.error_for(Field::General).map(str::to_owned);
        let saved = self.kit_saved;
        v_flex()
            .w_full()
            .gap_4()
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Signing in on a new device needs your password and this Secret Key. If you forget your password, the recovery code is the only way back. Ottero keeps no copy of either."),
            )
            .child(value_row("account-kit-email", "Email", email, None, cx))
            .child(value_row("account-kit-secret", "Secret Key", secret.clone(), Some("Secret Key"), cx))
            .child(value_row("account-kit-recovery", "Recovery code", recovery, Some("Recovery code"), cx))
            .child(
                h_flex().child(
                    Button::new("account-kit-save")
                        .icon(Lucide::Download)
                        .label("Save Emergency Kit…")
                        .on_click(cx.listener(|this, _, window, cx| this.save_kit(window, cx))),
                ),
            )
            .child(
                Checkbox::new("account-kit-saved")
                    .checked(saved)
                    .label("I saved my Emergency Kit")
                    .on_click(cx.listener(|this, checked: &bool, _, cx| {
                        this.kit_saved = *checked;
                        cx.notify();
                    })),
            )
            .children(general.map(|e| note(e.into(), true, cx)))
            .child(
                h_flex().child(
                    Button::new("account-kit-continue")
                        .primary()
                        .label(if busy { "Finishing…" } else { "Continue" })
                        .loading(busy)
                        .disabled(!saved || busy)
                        .on_click(cx.listener(|this, _, _, cx| this.finish_kit(cx))),
                ),
            )
            .into_any_element()
    }

    fn render_profile(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let account = self.library.read(cx).account();
        let (email, plan) = match account.state() {
            AccountState::SignedIn { email, plan } => (email.clone(), *plan),
            _ => (String::new(), Plan::Free),
        };
        let notice = account.notice().map(str::to_owned);
        let initial = email
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default();
        v_flex()
            .w_full()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .size(rems(2.5))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(theme.muted)
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_color(theme.foreground)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(initial),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(email),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.muted_foreground)
                                    .child(plan.label()),
                            ),
                    )
                    .child(
                        Button::new("account-sign-out")
                            .outline()
                            .small()
                            .label("Sign out…")
                            .on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.confirm_sign_out(window, cx)
                                }),
                            ),
                    ),
            )
            .children(notice.map(|n| note(n.into(), false, cx)))
            .into_any_element()
    }

    fn render_cloud(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let account = self.library.read(cx).account();
        let plan = match account.state() {
            AccountState::SignedIn { plan, .. } => *plan,
            _ => Plan::Free,
        };
        let detail = plan.detail(ottero_core::clock::now(), |t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%B %-d, %Y")
                .to_string()
        });
        v_flex()
            .w_full()
            .gap_3()
            .child(info_row("Plan", plan.label().into(), detail.into(), cx))
            .child(divider(cx))
            .child(info_row(
                "Sync",
                "Coming next".into(),
                "Your data stays on this computer until sync is ready.".into(),
                cx,
            ))
            .into_any_element()
    }

    fn render_security(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let account = self.library.read(cx).account();
        let unlocked = account.is_unlocked();
        let busy = account.unlock_busy();
        let secret = (unlocked && self.reveal_secret_key)
            .then(|| account.secret_key_text().map(|k| k.to_string()))
            .flatten();
        v_flex()
            .w_full()
            .gap_3()
            .when(!unlocked, |this| {
                this.child(
                    v_flex()
                        .gap_2()
                        .child(row_text(
                            "Unlock Ottero Cloud",
                            "Enter your password to open your account key on this computer.",
                            cx,
                        ))
                        .child(
                            h_flex()
                                .gap_2()
                                .child(
                                    div().w(rems(16.)).child(
                                        Input::new(&self.unlock).mask_toggle().disabled(busy),
                                    ),
                                )
                                .child(
                                    Button::new("account-unlock")
                                        .label(if busy { "Unlocking…" } else { "Unlock" })
                                        .loading(busy)
                                        .disabled(busy)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.unlock_account(window, cx)
                                        })),
                                ),
                        )
                        .children(self.unlock_error.clone().map(|e| note(e, true, cx))),
                )
                .child(divider(cx))
            })
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(div().text_sm().child("Secret Key"))
                            .child(match &secret {
                                Some(key) => div()
                                    .font_family(theme.mono_font_family.clone())
                                    .text_sm()
                                    .child(key.clone()),
                                None => div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child("With your password, it signs you in on a new device."),
                            }),
                    )
                    .when_some(secret.clone(), |this, key| {
                        this.child(
                            Button::new("account-secret-copy")
                                .ghost()
                                .small()
                                .icon(Lucide::Copy)
                                .tooltip("Copy Secret Key")
                                .on_click(move |_, window, cx| {
                                    copy_field("Secret Key", key.clone(), true, window, cx)
                                }),
                        )
                        .child(
                            Button::new("account-secret-hide")
                                .outline()
                                .small()
                                .label("Hide")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.reveal_secret_key = false;
                                    cx.notify();
                                })),
                        )
                    })
                    .when(secret.is_none(), |this| {
                        this.child(
                            Button::new("account-secret-view")
                                .outline()
                                .small()
                                .label("View…")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.show_secret_key(false, window, cx)
                                })),
                        )
                    }),
            )
            .child(divider(cx))
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .child(row_text(
                        "Emergency Kit",
                        "A printable page with your email and Secret Key.",
                        cx,
                    ))
                    .child(
                        Button::new("account-kit-save-again")
                            .outline()
                            .small()
                            .label("Save…")
                            .on_click(cx.listener(|this, _, window, cx| this.save_kit(window, cx))),
                    ),
            )
            .into_any_element()
    }

    fn render_devices(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let account = self.library.read(cx).account();
        let current = account.current_token();
        let loaded = account.sessions_loaded();
        let error = account.refresh_error().map(str::to_owned);
        let busy = account.refresh_busy();
        let now = ottero_core::clock::now();
        let mut rows: Vec<(String, String, String, DeviceKind, bool)> = account
            .sessions()
            .iter()
            .map(|s| {
                let (name, kind) = device_label(s.user_agent.as_deref());
                let active = s
                    .last_active
                    .or(s.created_at)
                    .map_or_else(String::new, |t| active_label(t, now));
                let this_device = current.as_ref().is_some_and(|t| s.is_current(t));
                (s.id.clone(), name, active, kind, this_device)
            })
            .collect();
        // This device first, then the rest as the server listed them.
        rows.sort_by_key(|r| !r.4);
        let view = cx.entity().downgrade();
        let mut list = v_flex().w_full();
        if !loaded {
            list = list.child(
                h_flex()
                    .gap_2()
                    .py_1()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .map(|this| match &error {
                        Some(e) => this.child(e.clone()),
                        None => this.child(Spinner::new().small()).child("Loading devices…"),
                    })
                    .when(error.is_some(), |this| {
                        this.child(
                            Button::new("account-devices-retry")
                                .ghost()
                                .small()
                                .label("Try again")
                                .loading(busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.library.update(cx, Library::refresh_account)
                                })),
                        )
                    }),
            );
        }
        for (i, (id, name, active, kind, this_device)) in rows.into_iter().enumerate() {
            let icon = match kind {
                DeviceKind::Phone => Lucide::Smartphone,
                DeviceKind::Browser => Lucide::Globe,
                DeviceKind::Computer | DeviceKind::Unknown => Lucide::Laptop,
            };
            let label_for_confirm = name.clone();
            let view = view.clone();
            list = list.child(
                h_flex()
                    .id(SharedString::from(format!("session-{id}")))
                    .w_full()
                    .gap_3()
                    .py_2()
                    .items_center()
                    .when(i > 0, |s| s.border_t_1().border_color(theme.border))
                    .child(Icon::new(icon).text_color(theme.muted_foreground))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(h_flex().gap_2().child(div().text_sm().child(name)).when(
                                this_device,
                                |this| {
                                    this.child(
                                        div()
                                            .text_xs()
                                            .px_1p5()
                                            .rounded(theme.radius)
                                            .bg(theme.muted)
                                            .text_color(theme.muted_foreground)
                                            .child("This device"),
                                    )
                                },
                            ))
                            .when(!active.is_empty(), |this| {
                                this.child(
                                    div()
                                        .text_xs()
                                        .text_color(theme.muted_foreground)
                                        .child(active),
                                )
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("session-sign-out-{id}")))
                            .ghost()
                            .small()
                            .label(if this_device {
                                "Sign out of this device…"
                            } else {
                                "Sign out…"
                            })
                            .on_click(move |_, window, cx| {
                                _ = view.update(cx, |this, cx| {
                                    if this_device {
                                        this.confirm_sign_out(window, cx);
                                    } else {
                                        this.confirm_revoke(
                                            id.clone(),
                                            label_for_confirm.clone(),
                                            window,
                                            cx,
                                        );
                                    }
                                });
                            }),
                    ),
            );
        }
        list.into_any_element()
    }
}

impl Render for AccountView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        // Drawn by the Settings page (`page`); this entity only owns state.
        div()
    }
}

/// Settings → Account, built from the current screen.
pub fn page(view: &Entity<AccountView>, cx: &App) -> SettingPage {
    let screen = view.read(cx).screen(cx);
    let part = |render: fn(&mut AccountView, &mut Context<AccountView>) -> AnyElement| {
        let view = view.clone();
        SettingItem::render(move |_, _, cx| {
            let content = view.update(cx, render);
            let key_view = view.clone();
            div()
                .id("account-content")
                .w_full()
                .capture_key_down(move |event, window, cx| {
                    key_view.update(cx, |v, cx| v.on_key(event, window, cx));
                })
                .child(content)
        })
        .keywords([
            "account",
            "ottero cloud",
            "cloud",
            "sign in",
            "sign up",
            "log in",
            "sync",
            "secret key",
            "emergency kit",
            "devices",
            "sessions",
        ])
    };
    let page = SettingPage::new("Account").icon(Lucide::CircleUser);
    match screen {
        Screen::Intro => page.group(SettingGroup::new().item(part(AccountView::render_intro))),
        Screen::SignUp => page.group(
            SettingGroup::new()
                .title("Create your Ottero Cloud account")
                .item(part(AccountView::render_sign_up)),
        ),
        Screen::SignIn => page.group(
            SettingGroup::new()
                .title("Sign in to Ottero Cloud")
                .item(part(AccountView::render_sign_in)),
        ),
        Screen::Code => page.group(
            SettingGroup::new()
                .title("Check your email")
                .item(part(AccountView::render_code)),
        ),
        Screen::EmergencyKit => page.group(
            SettingGroup::new()
                .title("Save your Emergency Kit")
                .item(part(AccountView::render_kit)),
        ),
        Screen::SignedIn => page
            .group(SettingGroup::new().item(part(AccountView::render_profile)))
            .group(
                SettingGroup::new()
                    .title("Ottero Cloud")
                    .item(part(AccountView::render_cloud)),
            )
            .group(
                SettingGroup::new()
                    .title("Security")
                    .item(part(AccountView::render_security)),
            )
            .group(
                SettingGroup::new()
                    .title("Devices")
                    .item(part(AccountView::render_devices)),
            ),
    }
}

fn icon_tile(icon: Lucide, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .size(rems(2.5))
        .flex_shrink_0()
        .rounded(theme.radius_lg)
        .bg(theme.muted)
        .flex()
        .items_center()
        .justify_center()
        .child(Icon::new(icon).text_color(theme.foreground))
}

fn label(text: &'static str, cx: &App) -> impl IntoElement {
    let _ = cx;
    div().text_sm().font_weight(FontWeight::MEDIUM).child(text)
}

fn note(text: SharedString, error: bool, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .text_xs()
        .text_color(if error {
            theme.danger
        } else {
            theme.muted_foreground
        })
        .child(text)
        .into_any_element()
}

fn field(
    name: &'static str,
    input: AnyElement,
    message: Option<(SharedString, bool)>,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .gap_1p5()
        .child(label(name, cx))
        .child(input)
        .children(message.map(|(text, error)| note(text, error, cx)))
}

fn divider(cx: &App) -> impl IntoElement {
    div().w_full().h(px(1.)).bg(cx.theme().border)
}

fn row_text(title: &'static str, description: &'static str, cx: &App) -> impl IntoElement {
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_0p5()
        .child(div().text_sm().child(title))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(description),
        )
}

fn info_row(
    title: &'static str,
    value: SharedString,
    detail: SharedString,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    h_flex()
        .w_full()
        .gap_3()
        .items_start()
        .child(
            div()
                .w(rems(6.))
                .flex_shrink_0()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(title),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(value))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(detail),
                ),
        )
}

/// A label, a monospace value and (for secrets) a Copy button.
fn value_row(
    id: &'static str,
    title: &'static str,
    value: String,
    copy_as: Option<&'static str>,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let copy_value = value.clone();
    v_flex().gap_1p5().child(label(title, cx)).child(
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .px_3()
            .py_2()
            .rounded(theme.radius)
            .bg(theme.muted)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .when(copy_as.is_some(), |d| {
                        d.font_family(theme.mono_font_family.clone())
                    })
                    .text_sm()
                    .child(value),
            )
            .when_some(copy_as, |this, name| {
                this.child(
                    Button::new(id)
                        .ghost()
                        .xsmall()
                        .icon(Lucide::Copy)
                        .tooltip(format!("Copy {name}"))
                        .on_click(move |_, window, cx| {
                            copy_field(name, copy_value.clone(), true, window, cx)
                        }),
                )
            }),
    )
}

/// Writes the Emergency Kit where the user picks, then opens it in the
/// browser to print. The file is readable only by the user.
pub fn save_kit(library: Entity<Library>, window: &mut Window, cx: &mut App) {
    let created = ottero_core::clock::today()
        .strftime("%B %-d, %Y")
        .to_string();
    let Some(html) = library.read(cx).emergency_kit_html(&created) else {
        ottero_ui::feedback::error_toast(
            "Enter your password first to include your Secret Key.",
            window,
            cx,
        );
        return;
    };
    let dir = directories::UserDirs::new()
        .and_then(|d| d.document_dir().map(std::path::Path::to_path_buf))
        .unwrap_or_else(std::env::temp_dir);
    let path = cx.prompt_for_new_path(&dir, Some("Ottero Emergency Kit.html"));
    window
        .spawn(cx, async move |cx| {
            let Ok(Ok(Some(path))) = path.await else {
                return;
            };
            let written = write_private(&path, html.as_bytes());
            _ = cx.update(|window, cx| match written {
                Ok(()) => {
                    cx.open_with_system(&path);
                    window.push_notification(
                        "Emergency Kit saved. Print it or save it as a PDF, then keep it off this computer.",
                        cx,
                    );
                }
                Err(_) => ottero_ui::feedback::error_toast("Couldn't save the Emergency Kit.", window, cx),
            });
        })
        .detach();
}

fn write_private(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Asks for the account password, then runs `then`. Escape cancels.
fn open_password_prompt(
    library: Entity<Library>,
    title: &'static str,
    description: &'static str,
    window: &mut Window,
    cx: &mut App,
    then: impl Fn(&mut Window, &mut App) + 'static,
) {
    if window.has_active_dialog(cx) {
        return;
    }
    let prompt =
        cx.new(|cx| PasswordPrompt::new(library, description, std::rc::Rc::new(then), window, cx));
    window.open_dialog(cx, move |dialog, _, _| {
        dialog.title(title).w(px(420.)).child(prompt.clone())
    });
}

/// What runs after the password was accepted.
type AfterUnlock = std::rc::Rc<dyn Fn(&mut Window, &mut App)>;

struct PasswordPrompt {
    library: Entity<Library>,
    description: &'static str,
    input: Entity<InputState>,
    error: Option<SharedString>,
    busy: bool,
    then: AfterUnlock,
    _subscription: Subscription,
}

impl PasswordPrompt {
    fn new(
        library: Entity<Library>,
        description: &'static str,
        then: AfterUnlock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Password")
        });
        let subscription =
            cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                InputEvent::Change => {
                    this.error = None;
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.submit(window, cx),
                _ => {}
            });
        input.update(cx, |i, cx| i.focus(window, cx));
        Self {
            library,
            description,
            input,
            error: None,
            busy: false,
            then,
            _subscription: subscription,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        cx.notify();
        let password = SecretString::from(self.input.read(cx).value().to_string());
        let task = self
            .library
            .update(cx, |lib, cx| lib.unlock_account(password, cx));
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            _ = this.update_in(cx, |this, window, cx| {
                this.busy = false;
                match result {
                    Ok(()) => {
                        window.close_dialog(cx);
                        (this.then)(window, cx);
                    }
                    Err(message) => this.error = Some(message.into()),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for PasswordPrompt {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        v_flex()
            .gap_3()
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(self.description),
            )
            .child(Input::new(&self.input).mask_toggle().disabled(self.busy))
            .children(self.error.clone().map(|e| note(e, true, cx)))
            .child(
                h_flex().justify_end().child(
                    Button::new("account-prompt-submit")
                        .primary()
                        .label(if self.busy { "Checking…" } else { "Continue" })
                        .loading(self.busy)
                        .disabled(self.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
                ),
            )
    }
}
