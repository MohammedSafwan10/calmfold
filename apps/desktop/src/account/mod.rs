//! Ottero Cloud accounts in the app: the session held by [`Library`], and
//! the `Library` methods views call. Network and Argon2 run on the
//! background executor; results come back here, are checked against the
//! flow's generation (a late answer for a screen the user left is dropped)
//! and against `ottero_core::account::next`, then persisted and announced
//! with [`LibraryEvent::AccountChanged`].
//!
//! Secrets: the account key lives only in memory ([`AccountSession::account_key`])
//! and is dropped on sign-out. Derived keys are held only between the steps
//! of one flow. Nothing secret is logged; errors are fixed sentences.

pub mod client;
pub mod fake;
pub mod flows;
pub mod kit;
pub mod store;

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::Context;
use ottero_core::account::{
    AccountEvent, AccountState, Plan, SecretKeyPresence, check_password, next, validate_email,
};
use ottero_crypto::account::{AccountKdf, AccountSecretKey, decode_salt, encode_salt};
use ottero_crypto::{ExposeSecret as _, SecretKey, SecretString, Zeroizing};

use crate::library::{Library, LibraryEvent};
use client::{AccountApi, ApiError, Me, SessionInfo, SessionToken};
use flows::{Derived, Finish, FlowError, Kit, KitSaved};
use store::{AccountFile, PlanRecord, SignedIn};

/// How long "Resend code" waits after a code was sent.
pub const RESEND_COOLDOWN: Duration = Duration::from_secs(30);

/// Where an error belongs on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    General,
    Email,
    Password,
    SecretKey,
    Code,
}

pub struct AccountSession {
    state: AccountState,
    file: AccountFile,
    api: Arc<dyn AccountApi>,
    kdf_override: Option<AccountKdf>,
    /// Bumped whenever a flow starts or is abandoned.
    generation: u64,
    /// Bumped whenever the signed-in session changes (sign in, sign out,
    /// session ended, lock). Refresh and unlock results carry the value
    /// and the token they started with, and are dropped if either changed,
    /// so a late answer for one account never lands in another.
    epoch: u64,
    derived: Option<Arc<Derived>>,
    verified: bool,
    pending_token: Option<SessionToken>,
    pending_plan: Plan,
    kit: Option<Arc<Kit>>,
    account_key: Option<SecretKey>,
    error: Option<(Field, String)>,
    code_busy: bool,
    kit_busy: bool,
    unlock_busy: bool,
    resend_after: Option<Instant>,
    sessions: Vec<SessionInfo>,
    sessions_loaded: bool,
    refresh_busy: bool,
    refresh_error: Option<String>,
    /// A one-time message for the account page (e.g. "You were signed out").
    notice: Option<String>,
}

impl AccountSession {
    /// No data folder and no server (a bare `Library`).
    #[must_use]
    pub fn detached() -> Self {
        Self::new(AccountFile::in_memory(), Arc::new(client::Unconfigured))
    }

    /// Reads `account.json`; a stored token means signed in (locked).
    #[must_use]
    pub fn new(file: AccountFile, api: Arc<dyn AccountApi>) -> Self {
        let state = match file.signed_in() {
            Some(a) => AccountState::SignedIn {
                email: a.email.clone(),
                plan: a.plan.as_ref().map_or(Plan::Free, PlanRecord::plan),
            },
            None => AccountState::SignedOut,
        };
        Self {
            state,
            file,
            api,
            kdf_override: None,
            generation: 0,
            epoch: 0,
            derived: None,
            verified: false,
            pending_token: None,
            pending_plan: Plan::Free,
            kit: None,
            account_key: None,
            error: None,
            code_busy: false,
            kit_busy: false,
            unlock_busy: false,
            resend_after: None,
            sessions: Vec::new(),
            sessions_loaded: false,
            refresh_busy: false,
            refresh_error: None,
            notice: None,
        }
    }

    #[must_use]
    pub fn state(&self) -> &AccountState {
        &self.state
    }

    #[must_use]
    pub fn error(&self) -> Option<(Field, &str)> {
        self.error.as_ref().map(|(f, m)| (*f, m.as_str()))
    }

    #[must_use]
    pub fn error_for(&self, field: Field) -> Option<&str> {
        self.error
            .as_ref()
            .filter(|(f, _)| *f == field)
            .map(|(_, m)| m.as_str())
    }

    #[must_use]
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.state.is_busy() || self.code_busy || self.kit_busy
    }

    #[must_use]
    pub fn code_busy(&self) -> bool {
        self.code_busy
    }

    #[must_use]
    pub fn kit_busy(&self) -> bool {
        self.kit_busy
    }

    #[must_use]
    pub fn unlock_busy(&self) -> bool {
        self.unlock_busy
    }

    /// Whether the account key is open on this device (password entered
    /// since Ottero started).
    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        self.account_key.is_some()
    }

    #[must_use]
    pub fn secret_key_presence(&self, email: &str) -> SecretKeyPresence {
        let valid = self
            .file
            .secret_key(email)
            .is_some_and(|k| AccountSecretKey::parse(&k).is_ok());
        if valid {
            SecretKeyPresence::Stored
        } else {
            SecretKeyPresence::Missing
        }
    }

    /// Time left before "Resend code" works again.
    #[must_use]
    pub fn resend_wait(&self) -> Option<Duration> {
        self.resend_after
            .and_then(|t| t.checked_duration_since(Instant::now()))
            .filter(|d| !d.is_zero())
    }

    #[must_use]
    pub fn sessions(&self) -> &[SessionInfo] {
        &self.sessions
    }

    #[must_use]
    pub fn sessions_loaded(&self) -> bool {
        self.sessions_loaded
    }

    #[must_use]
    pub fn refresh_error(&self) -> Option<&str> {
        self.refresh_error.as_deref()
    }

    #[must_use]
    pub fn refresh_busy(&self) -> bool {
        self.refresh_busy
    }

    /// The raw part of this device's session token, to mark "This device".
    #[must_use]
    pub fn current_token(&self) -> Option<SessionToken> {
        self.file.token()
    }

    /// The Secret Key for display: during the Emergency Kit step, or once
    /// the password was entered on this device.
    #[must_use]
    pub fn secret_key_text(&self) -> Option<Zeroizing<String>> {
        if let (AccountState::EmergencyKit { .. }, Some(d)) = (&self.state, &self.derived) {
            return Some(d.secret_key.display());
        }
        self.account_key.as_ref()?;
        let email = self.state.email()?;
        self.file.secret_key(email)
    }

    /// Shown once, only on the Emergency Kit step.
    #[must_use]
    pub fn recovery_code_text(&self) -> Option<Zeroizing<String>> {
        match (&self.state, &self.kit) {
            (AccountState::EmergencyKit { .. }, Some(kit)) => Some(kit.recovery.display()),
            _ => None,
        }
    }

    fn kdf(&self) -> AccountKdf {
        self.kdf_override.unwrap_or(AccountKdf::V1)
    }

    fn apply(&mut self, event: AccountEvent) -> bool {
        match next(&self.state, event) {
            Some(state) => {
                self.state = state;
                true
            }
            None => false,
        }
    }

    fn fail(&mut self, field: Field, message: impl Into<String>) {
        let message = message.into();
        self.apply(AccountEvent::Failed {
            message: message.clone(),
        });
        self.error = Some((field, message));
    }

    /// The signed-in session changed: pending refresh/unlock results no longer apply.
    fn new_epoch(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.refresh_busy = false;
        self.unlock_busy = false;
    }

    /// True when `epoch` and `token` still describe the signed-in session.
    fn still_current(&self, epoch: u64, token: Option<&str>) -> bool {
        self.epoch == epoch
            && self.state.is_signed_in()
            && self.file.token().as_ref().map(SessionToken::expose) == token
    }

    fn forget_flow(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.derived = None;
        self.kit = None;
        self.verified = false;
        self.code_busy = false;
        self.kit_busy = false;
        self.resend_after = None;
    }

    fn signed_in(
        &mut self,
        derived: &Derived,
        token: &SessionToken,
        me: &Me,
        wrapped_by_password: String,
        account_key: SecretKey,
    ) -> Option<String> {
        let saved = self
            .file
            .set_secret_key(&derived.email, &derived.secret_key.display())
            .and_then(|()| {
                self.file.set_signed_in(SignedIn {
                    email: derived.email.clone(),
                    token: token.expose().to_owned(),
                    salt: encode_salt(&derived.salt),
                    wrapped_by_password: Some(wrapped_by_password),
                    plan: Some(PlanRecord::from_me(me)),
                })
            });
        self.new_epoch();
        self.account_key = Some(account_key);
        self.sessions.clear();
        self.sessions_loaded = false;
        self.notice = None;
        saved.err().map(|e| format!("{e:#}"))
    }
}

/// Field for a flow error.
fn field_of(err: &FlowError, typed_secret_key: bool) -> Field {
    match err {
        FlowError::Api(ApiError::WrongCredentials) if typed_secret_key => Field::SecretKey,
        FlowError::Api(ApiError::WrongCredentials) | FlowError::WrongPassword => Field::Password,
        FlowError::Api(ApiError::WrongCode | ApiError::CodeExpired) => Field::Code,
        _ => Field::General,
    }
}

fn wrong_credentials_message(typed_secret_key: bool) -> String {
    if typed_secret_key {
        "Wrong password or Secret Key. Check both and try again.".into()
    } else {
        "Wrong email or password.".into()
    }
}

impl Library {
    #[must_use]
    pub fn account(&self) -> &AccountSession {
        &self.account
    }

    pub fn set_account_session(&mut self, session: AccountSession) {
        self.account = session;
    }

    /// Tests: a fake server and cheap Argon2 parameters.
    pub fn set_account_api_for_tests(&mut self, api: Arc<dyn AccountApi>, kdf: AccountKdf) {
        self.account.api = api;
        self.account.kdf_override = Some(kdf);
    }

    /// Clears the error shown on the account screens (the user is typing).
    pub fn clear_account_error(&mut self, cx: &mut Context<Self>) {
        let mut changed = self.account.error.take().is_some();
        if matches!(self.account.state, AccountState::Error { .. }) {
            changed |= self.account.apply(AccountEvent::Cancel);
        }
        if changed {
            cx.emit(LibraryEvent::AccountChanged);
        }
    }

    /// A form-level check failed in the view (e.g. passwords don't match).
    pub fn set_account_error(&mut self, field: Field, message: &str, cx: &mut Context<Self>) {
        self.account.error = Some((field, message.to_owned()));
        cx.emit(LibraryEvent::AccountChanged);
    }

    pub fn dismiss_account_notice(&mut self, cx: &mut Context<Self>) {
        if self.account.notice.take().is_some() {
            cx.emit(LibraryEvent::AccountChanged);
        }
    }

    /// Leaves the current flow (Back, Change email, Escape).
    pub fn cancel_account_flow(&mut self, cx: &mut Context<Self>) {
        if self.account.state.is_signed_in() {
            return;
        }
        if let Some(token) = self.account.pending_token.take() {
            // Signed in only to save keys; end that session too.
            let api = self.account.api.clone();
            cx.background_executor()
                .spawn(async move { _ = api.sign_out(&token) })
                .detach();
        }
        self.account.forget_flow();
        self.account.error = None;
        self.account.apply(AccountEvent::Cancel);
        cx.emit(LibraryEvent::AccountChanged);
    }

    /// Creates an account: checks the form, derives keys in the background,
    /// asks the server for the code email.
    pub fn account_sign_up(&mut self, email: &str, password: SecretString, cx: &mut Context<Self>) {
        let session = &mut self.account;
        if session.is_busy() {
            return;
        }
        let email = match validate_email(email) {
            Ok(e) => e,
            Err(p) => {
                session.error = Some((Field::Email, p.message().into()));
                cx.emit(LibraryEvent::AccountChanged);
                return;
            }
        };
        if let Some(problem) = check_password(password.expose_secret(), &email).problem {
            session.error = Some((Field::Password, problem.into()));
            cx.emit(LibraryEvent::AccountChanged);
            return;
        }
        if !session.apply(AccountEvent::StartSignUp {
            email: email.clone(),
        }) {
            return;
        }
        session.forget_flow();
        session.error = None;
        let generation = session.generation;
        let existing = session
            .file
            .secret_key(&email)
            .and_then(|k| AccountSecretKey::parse(&k).ok());
        let (api, kdf) = (session.api.clone(), session.kdf());
        cx.emit(LibraryEvent::AccountChanged);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { flows::sign_up(&*api, kdf, &email, &password, existing) })
                .await;
            _ = this.update(cx, |lib, cx| {
                let session = &mut lib.account;
                if session.generation != generation {
                    return;
                }
                match result {
                    Ok(derived) => {
                        // Saved now: the Secret Key is the only copy until the kit is saved.
                        if let Err(err) = session
                            .file
                            .set_secret_key(&derived.email, &derived.secret_key.display())
                        {
                            session.error = Some((Field::General, format!("{err:#}")));
                        }
                        session.derived = Some(Arc::new(derived));
                        session.resend_after = Some(Instant::now() + RESEND_COOLDOWN);
                        session.apply(AccountEvent::CodeSent);
                    }
                    Err(err) => session.fail(Field::General, err.message()),
                }
                cx.emit(LibraryEvent::AccountChanged);
            });
        })
        .detach();
    }

    /// Checks the emailed code, then signs in and prepares the Emergency Kit.
    pub fn verify_account_code(&mut self, code: &str, cx: &mut Context<Self>) {
        let session = &mut self.account;
        if !matches!(session.state, AccountState::AwaitingCode { .. }) || session.code_busy {
            return;
        }
        let code: String = code.chars().filter(char::is_ascii_digit).collect();
        if code.len() != 6 {
            session.error = Some((Field::Code, "Enter the 6-digit code from the email.".into()));
            cx.emit(LibraryEvent::AccountChanged);
            return;
        }
        let Some(derived) = session.derived.clone() else {
            session.error = Some((Field::Code, "Start again: go back and sign in.".into()));
            cx.emit(LibraryEvent::AccountChanged);
            return;
        };
        session.code_busy = true;
        session.error = None;
        let (api, verified, generation) =
            (session.api.clone(), session.verified, session.generation);
        cx.emit(LibraryEvent::AccountChanged);
        cx.spawn(async move |this, cx| {
            let code = Zeroizing::new(code);
            let task_derived = derived.clone();
            let (verified, result) = cx
                .background_executor()
                .spawn(async move { flows::verify(&*api, &task_derived, &code, verified) })
                .await;
            _ = this.update(cx, |lib, cx| {
                if lib.account.generation != generation {
                    return;
                }
                lib.account.code_busy = false;
                lib.account.verified = verified;
                lib.apply_finish(&derived, result, false, cx);
            });
        })
        .detach();
    }

    /// Sends a new code (after the cooldown).
    pub fn resend_account_code(&mut self, cx: &mut Context<Self>) {
        let session = &mut self.account;
        let AccountState::AwaitingCode { email } = &session.state else {
            return;
        };
        if session.resend_wait().is_some() {
            return;
        }
        let email = email.clone();
        session.resend_after = Some(Instant::now() + RESEND_COOLDOWN);
        session.error = None;
        let (api, generation) = (session.api.clone(), session.generation);
        cx.emit(LibraryEvent::AccountChanged);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { api.resend_code(&email) })
                .await;
            _ = this.update(cx, |lib, cx| {
                if lib.account.generation != generation {
                    return;
                }
                if let Err(err) = result {
                    lib.account.resend_after = None;
                    lib.account.error = Some((Field::Code, err.message()));
                }
                cx.emit(LibraryEvent::AccountChanged);
            });
        })
        .detach();
    }

    /// Signs in. `secret_key` is what the user typed; empty uses the one
    /// stored for this email, and if there is none, asks for it.
    pub fn account_sign_in(
        &mut self,
        email: &str,
        password: SecretString,
        secret_key: &str,
        cx: &mut Context<Self>,
    ) {
        let session = &mut self.account;
        if session.is_busy() {
            return;
        }
        let email = match validate_email(email) {
            Ok(e) => e,
            Err(p) => {
                session.error = Some((Field::Email, p.message().into()));
                cx.emit(LibraryEvent::AccountChanged);
                return;
            }
        };
        if password.expose_secret().is_empty() {
            session.error = Some((Field::Password, "Enter your password.".into()));
            cx.emit(LibraryEvent::AccountChanged);
            return;
        }
        let typed = !secret_key.trim().is_empty();
        let key = if typed {
            match flows::parse_secret_key(secret_key) {
                Ok((key, _)) => Some(key),
                Err(message) => {
                    session.error = Some((Field::SecretKey, message));
                    cx.emit(LibraryEvent::AccountChanged);
                    return;
                }
            }
        } else {
            session
                .file
                .secret_key(&email)
                .and_then(|k| AccountSecretKey::parse(&k).ok())
        };
        if !session.apply(AccountEvent::StartSignIn {
            email: email.clone(),
        }) {
            return;
        }
        session.forget_flow();
        session.error = None;
        let Some(key) = key else {
            session.apply(AccountEvent::SecretKeyMissing);
            cx.emit(LibraryEvent::AccountChanged);
            return;
        };
        let generation = session.generation;
        let (api, kdf) = (session.api.clone(), session.kdf_override);
        cx.emit(LibraryEvent::AccountChanged);
        cx.spawn(async move |this, cx| {
            let (derived, result) = cx
                .background_executor()
                .spawn(async move { flows::sign_in(&*api, kdf, &email, &password, key) })
                .await;
            _ = this.update(cx, |lib, cx| {
                if lib.account.generation != generation {
                    return;
                }
                match derived {
                    Some(derived) => lib.apply_finish(&derived, result, typed, cx),
                    None => {
                        let err = result.err().unwrap_or(FlowError::Crypto);
                        lib.account.fail(field_of(&err, typed), err.message());
                        cx.emit(LibraryEvent::AccountChanged);
                    }
                }
            });
        })
        .detach();
    }

    /// Applies the end of sign-in or code entry.
    fn apply_finish(
        &mut self,
        derived: &Arc<Derived>,
        result: flows::FlowResult<Finish>,
        typed_secret_key: bool,
        cx: &mut Context<Self>,
    ) {
        let session = &mut self.account;
        match result {
            Ok(Finish::SignedIn {
                token,
                me,
                account_key,
                wrapped_by_password,
            }) => {
                let plan = me.plan();
                if let Some(err) =
                    session.signed_in(derived, &token, &me, wrapped_by_password, account_key)
                {
                    session.notice = Some(err);
                }
                session.forget_flow();
                session.error = None;
                session.apply(AccountEvent::SignedIn { plan });
                cx.emit(LibraryEvent::AccountChanged);
                self.refresh_account(cx);
                return;
            }
            Ok(Finish::NeedsKit { token, me, kit }) => {
                // The server accepted the keys, so a typed Secret Key is right: keep it.
                if let Err(err) = session
                    .file
                    .set_secret_key(&derived.email, &derived.secret_key.display())
                {
                    session.error = Some((Field::General, format!("{err:#}")));
                } else {
                    session.error = None;
                }
                session.pending_token = Some(token);
                session.pending_plan = me.plan();
                session.kit = Some(Arc::new(kit));
                session.derived = Some(derived.clone());
                session.apply(AccountEvent::KeysMissing);
            }
            Err(FlowError::Api(ApiError::NotVerified)) => {
                session.derived = Some(derived.clone());
                session.verified = false;
                session.resend_after = Some(Instant::now() + RESEND_COOLDOWN);
                session.error = None;
                session.apply(AccountEvent::NotVerified);
            }
            Err(err) => {
                let field = field_of(&err, typed_secret_key);
                let message = match err {
                    FlowError::Api(ApiError::WrongCredentials) => {
                        wrong_credentials_message(typed_secret_key)
                    }
                    ref other => other.message(),
                };
                if matches!(session.state, AccountState::AwaitingCode { .. }) {
                    session.error = Some((field, message));
                } else {
                    session.fail(field, message);
                }
            }
        }
        cx.emit(LibraryEvent::AccountChanged);
    }

    /// "I saved my Emergency Kit" → upload the wrapped keys and finish.
    pub fn finish_emergency_kit(&mut self, cx: &mut Context<Self>) {
        let session = &mut self.account;
        if !matches!(session.state, AccountState::EmergencyKit { .. }) || session.kit_busy {
            return;
        }
        let (Some(token), Some(derived), Some(kit)) = (
            session.pending_token.clone(),
            session.derived.clone(),
            session.kit.clone(),
        ) else {
            return;
        };
        session.kit_busy = true;
        session.error = None;
        let (api, generation) = (session.api.clone(), session.generation);
        cx.emit(LibraryEvent::AccountChanged);
        cx.spawn(async move |this, cx| {
            let (t, d, k) = (token.clone(), derived.clone(), kit.clone());
            let result = cx
                .background_executor()
                .spawn(async move { flows::save_kit(&*api, &t, &d, &k) })
                .await;
            _ = this.update(cx, |lib, cx| {
                let session = &mut lib.account;
                if session.generation != generation {
                    return;
                }
                session.kit_busy = false;
                let (key, wrapped, notice) = match result {
                    Ok(KitSaved::Saved) => {
                        let mut bytes = *kit.account_key.expose();
                        (
                            SecretKey::from_bytes(&mut bytes),
                            kit.wrapped.by_password.clone(),
                            None,
                        )
                    }
                    Ok(KitSaved::AlreadySet(key, wrapped)) => (
                        key,
                        wrapped,
                        Some("Another device finished setting up this account first, so the recovery code shown here wasn't saved. Use the Emergency Kit from that device.".to_owned()),
                    ),
                    Err(err) => {
                        session.error = Some((Field::General, err.message()));
                        cx.emit(LibraryEvent::AccountChanged);
                        return;
                    }
                };
                let me = Me {
                    id: String::new(),
                    email: derived.email.clone(),
                    role: match session.pending_plan {
                        Plan::Owner => "owner".into(),
                        _ => "user".into(),
                    },
                    cloud: match session.pending_plan {
                        Plan::Cloud { source, ends_at } => Some(client::Cloud {
                            source: source.as_str().into(),
                            status: "active".into(),
                            ends_at,
                        }),
                        _ => None,
                    },
                    has_keys: true,
                };
                let plan = session.pending_plan;
                let saved = session.signed_in(&derived, &token, &me, wrapped, key);
                session.pending_token = None;
                session.forget_flow();
                session.notice = notice.or(saved);
                session.apply(AccountEvent::KitSaved { plan });
                cx.emit(LibraryEvent::AccountChanged);
                lib.refresh_account(cx);
            });
        })
        .detach();
    }

    /// Reloads the plan and the device list. A revoked or blocked session
    /// signs this device out.
    pub fn refresh_account(&mut self, cx: &mut Context<Self>) {
        let session = &mut self.account;
        if !session.state.is_signed_in() || session.refresh_busy {
            return;
        }
        let Some(token) = session.file.token() else {
            return;
        };
        session.refresh_busy = true;
        let api = session.api.clone();
        let (epoch, started_with) = (session.epoch, Zeroizing::new(token.expose().to_owned()));
        cx.emit(LibraryEvent::AccountChanged);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let me = api.me(&token)?;
                    let sessions = api.sessions(&token)?;
                    Ok::<_, ApiError>((me, sessions))
                })
                .await;
            _ = this.update(cx, |lib, cx| {
                let session = &mut lib.account;
                if !session.still_current(epoch, Some(&started_with)) {
                    return; // another session took over; its own refresh decides
                }
                session.refresh_busy = false;
                match result {
                    Ok((me, sessions)) => {
                        let record = PlanRecord::from_me(&me);
                        _ = session.file.update_signed_in(|a| a.plan = Some(record));
                        session.apply(AccountEvent::PlanChanged { plan: me.plan() });
                        session.sessions = sessions;
                        session.sessions_loaded = true;
                        session.refresh_error = None;
                    }
                    Err(ApiError::SessionEnded) => lib.end_account_session(cx),
                    Err(ApiError::Blocked(message)) => {
                        lib.end_account_session(cx);
                        lib.account.notice = Some(message);
                    }
                    Err(err) => session.refresh_error = Some(err.message()),
                }
                cx.emit(LibraryEvent::AccountChanged);
            });
        })
        .detach();
    }

    fn end_account_session(&mut self, cx: &mut Context<Self>) {
        let session = &mut self.account;
        _ = session.file.sign_out();
        session.new_epoch();
        session.account_key = None;
        session.sessions.clear();
        session.sessions_loaded = false;
        session.apply(AccountEvent::SessionEnded);
        session.notice = Some(ApiError::SessionEnded.message());
        cx.emit(LibraryEvent::AccountChanged);
    }

    /// Signs this device out: forgets the token and account key at once,
    /// then tells the server (best effort; offline still signs out here).
    /// The Secret Key stays so signing in again needs only the password.
    pub fn account_sign_out(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let session = &mut self.account;
        if !session.state.is_signed_in() {
            return Ok(());
        }
        let token = session.file.token();
        let saved = session.file.sign_out();
        session.new_epoch();
        session.account_key = None;
        session.sessions.clear();
        session.sessions_loaded = false;
        session.refresh_error = None;
        session.notice = None;
        session.forget_flow();
        session.apply(AccountEvent::SignOut);
        if let Some(token) = token {
            let api = session.api.clone();
            cx.background_executor()
                .spawn(async move { _ = api.sign_out(&token) })
                .detach();
        }
        cx.emit(LibraryEvent::AccountChanged);
        saved
    }

    /// Signs another device out.
    pub fn revoke_account_session(
        &mut self,
        session_id: &str,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Task<Result<(), String>> {
        let session = &mut self.account;
        let (Some(token), Some(target)) = (
            session.file.token(),
            session
                .sessions
                .iter()
                .find(|s| s.id == session_id)
                .map(|s| Zeroizing::new(s.token().to_owned())),
        ) else {
            return gpui_kit::Task::ready(Err("That device is no longer signed in.".into()));
        };
        let api = session.api.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { api.revoke_session(&token, &target) })
                .await;
            _ = this.update(cx, |lib, cx| lib.refresh_account(cx));
            result.map_err(|e| e.message())
        })
    }

    /// Proves the password on this device and opens the account key
    /// ("Unlock Ottero Cloud", "View Secret Key").
    pub fn unlock_account(
        &mut self,
        password: SecretString,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Task<Result<(), String>> {
        let session = &mut self.account;
        let Some(stored) = session.file.signed_in().cloned() else {
            return gpui_kit::Task::ready(Err("Sign in first.".into()));
        };
        if session.account_key.is_some() {
            return gpui_kit::Task::ready(Ok(()));
        }
        if password.expose_secret().is_empty() {
            return gpui_kit::Task::ready(Err("Enter your password.".into()));
        }
        let Some(secret_key) = session
            .file
            .secret_key(&stored.email)
            .and_then(|k| AccountSecretKey::parse(&k).ok())
        else {
            return gpui_kit::Task::ready(Err(
                "This computer doesn't have your Secret Key. Sign out and sign in again with your Emergency Kit.".into(),
            ));
        };
        let Ok(salt) = decode_salt(&stored.salt) else {
            return gpui_kit::Task::ready(Err(FlowError::DamagedKeys.message()));
        };
        session.unlock_busy = true;
        let (api, kdf) = (session.api.clone(), session.kdf());
        let token = session.file.token();
        let epoch = session.epoch;
        let started_with = token
            .as_ref()
            .map(|t| Zeroizing::new(t.expose().to_owned()));
        cx.emit(LibraryEvent::AccountChanged);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let wrapped = match stored.wrapped_by_password {
                        Some(w) => w,
                        None => {
                            let token = token.ok_or(FlowError::Api(ApiError::SessionEnded))?;
                            api.get_keys(&token)?
                                .ok_or(FlowError::Api(ApiError::Unexpected))?
                                .by_password
                        }
                    };
                    let key =
                        flows::unlock(kdf, &password, &stored.email, salt, secret_key, &wrapped)?;
                    Ok::<_, FlowError>((key, wrapped))
                })
                .await;
            this.update(cx, |lib, cx| {
                let session = &mut lib.account;
                if !session.still_current(epoch, started_with.as_deref().map(String::as_str)) {
                    // Signed out, locked or another account since: never apply A's key to B.
                    cx.emit(LibraryEvent::AccountChanged);
                    return Err("The account changed while unlocking. Try again.".to_owned());
                }
                session.unlock_busy = false;
                let outcome = match result {
                    Ok((key, wrapped)) => {
                        _ = session
                            .file
                            .update_signed_in(|a| a.wrapped_by_password = Some(wrapped));
                        session.account_key = Some(key);
                        Ok(())
                    }
                    Err(err) => Err(err.message()),
                };
                cx.emit(LibraryEvent::AccountChanged);
                outcome
            })
            .map_err(|_| "Ottero is closing.".to_owned())?
        })
    }

    /// Locks the account key again (it is re-derived from the password).
    pub fn lock_account(&mut self, cx: &mut Context<Self>) {
        let was_open = self.account.account_key.take().is_some();
        // An unlock still deriving must not reopen what was just locked.
        let pending = self.account.unlock_busy;
        if pending {
            self.account.new_epoch();
        }
        if was_open || pending {
            cx.emit(LibraryEvent::AccountChanged);
        }
    }

    /// Kit contents for saving, or `None` when the Secret Key isn't open.
    #[must_use]
    pub fn emergency_kit_html(&self, created: &str) -> Option<Zeroizing<String>> {
        let session = &self.account;
        let email = session.state.email()?.to_owned();
        let secret = session.secret_key_text()?;
        let recovery = session.recovery_code_text();
        Some(kit::html(&kit::KitContents {
            email: &email,
            secret_key: &secret,
            recovery_code: recovery.as_deref().map(String::as_str),
            created,
        }))
    }
}
