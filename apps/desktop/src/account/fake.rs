//! An in-memory account server with the real API's rules, for tests
//! (`Library::set_account_api_for_tests`). Not used by the app itself.
#![doc(hidden)]

use std::collections::HashMap;
use std::sync::Mutex;

use ottero_crypto::Salt;
use ottero_crypto::account::{AccountKdf, decode_salt};

use super::client::{
    AccountApi, ApiError, ApiResult, Cloud, KdfInfo, Me, SessionInfo, SessionToken, WrappedKeys,
};

struct User {
    id: String,
    auth: String,
    salt: String,
    verified: bool,
    code: String,
    attempts: u8,
    keys: Option<WrappedKeys>,
    role: String,
    cloud: Option<Cloud>,
}

#[derive(Default)]
struct State {
    users: HashMap<String, User>,
    /// raw session token → (email, user agent)
    sessions: Vec<(String, String, String)>,
    next: u64,
    offline: bool,
    blocked: Vec<String>,
}

#[derive(Default)]
pub struct FakeAccountApi {
    state: Mutex<State>,
}

impl FakeAccountApi {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The 6-digit code last "emailed" to `email`.
    #[must_use]
    pub fn code_for(&self, email: &str) -> Option<String> {
        self.lock().users.get(email).map(|u| u.code.clone())
    }

    pub fn set_offline(&self, offline: bool) {
        self.lock().offline = offline;
    }

    pub fn grant_owner(&self, email: &str) {
        if let Some(u) = self.lock().users.get_mut(email) {
            u.role = "owner".into();
            u.cloud = Some(Cloud {
                source: "owner".into(),
                status: "active".into(),
                ends_at: None,
            });
        }
    }

    /// Another device signs in (appears in the sessions list).
    pub fn add_session(&self, email: &str, user_agent: &str) {
        let mut s = self.lock();
        s.next += 1;
        let token = format!("other{}", s.next);
        s.sessions
            .push((token, email.to_owned(), user_agent.to_owned()));
    }

    /// Blocks sign-in, as the admin panel does.
    pub fn block(&self, email: &str) {
        let mut s = self.lock();
        s.blocked.push(email.to_owned());
        s.sessions.retain(|(_, e, _)| e != email);
    }

    pub fn end_all_sessions(&self) {
        self.lock().sessions.clear();
    }

    #[must_use]
    pub fn has_keys(&self, email: &str) -> bool {
        self.lock()
            .users
            .get(email)
            .is_some_and(|u| u.keys.is_some())
    }

    #[must_use]
    pub fn session_count(&self) -> usize {
        self.lock().sessions.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn online(&self) -> ApiResult<std::sync::MutexGuard<'_, State>> {
        let s = self.lock();
        if s.offline {
            Err(ApiError::Offline)
        } else {
            Ok(s)
        }
    }

    fn user_of(state: &State, token: &SessionToken) -> ApiResult<String> {
        state
            .sessions
            .iter()
            .find(|(t, _, _)| t == token.session_part())
            .map(|(_, e, _)| e.clone())
            .ok_or(ApiError::SessionEnded)
    }
}

impl AccountApi for FakeAccountApi {
    fn kdf(&self, email: &str) -> ApiResult<KdfInfo> {
        let s = self.online()?;
        let salt = match s.users.get(email) {
            Some(u) => decode_salt(&u.salt).map_err(|_| ApiError::Unexpected)?,
            None => Salt([3; 16]),
        };
        Ok(KdfInfo {
            salt,
            kdf: AccountKdf::V1,
        })
    }

    fn sign_up(&self, email: &str, _: &str, auth: &str, salt: &str) -> ApiResult<()> {
        let mut s = self.online()?;
        if auth.len() != 64 || decode_salt(salt).is_err() {
            return Err(ApiError::Rejected);
        }
        if s.users.contains_key(email) {
            return Ok(()); // same answer, no email: no enumeration
        }
        s.next += 1;
        let code = format!("{:06}", 100_000 + s.next * 7_919 % 900_000);
        let id = format!("user{}", s.next);
        s.users.insert(
            email.to_owned(),
            User {
                id,
                auth: auth.to_owned(),
                salt: salt.to_owned(),
                verified: false,
                code,
                attempts: 0,
                keys: None,
                role: "user".into(),
                cloud: None,
            },
        );
        Ok(())
    }

    fn verify_email(&self, email: &str, code: &str) -> ApiResult<()> {
        let mut s = self.online()?;
        let u = s.users.get_mut(email).ok_or(ApiError::WrongCode)?;
        if u.attempts >= 5 {
            return Err(ApiError::TooManyAttempts);
        }
        if u.code != code {
            u.attempts += 1;
            return Err(ApiError::WrongCode);
        }
        u.verified = true;
        Ok(())
    }

    fn resend_code(&self, email: &str) -> ApiResult<()> {
        let mut s = self.online()?;
        if let Some(u) = s.users.get_mut(email) {
            u.attempts = 0;
        }
        Ok(())
    }

    fn sign_in(&self, email: &str, auth: &str) -> ApiResult<SessionToken> {
        let mut s = self.online()?;
        if s.blocked.iter().any(|b| b == email) {
            return Err(ApiError::Blocked("This account is blocked.".into()));
        }
        let u = s.users.get(email).ok_or(ApiError::WrongCredentials)?;
        if u.auth != auth {
            return Err(ApiError::WrongCredentials);
        }
        if !u.verified {
            return Err(ApiError::NotVerified);
        }
        s.next += 1;
        let raw = format!("tok{}", s.next);
        s.sessions
            .push((raw.clone(), email.to_owned(), super::client::user_agent()));
        Ok(SessionToken::new(format!("{raw}.sig")))
    }

    fn me(&self, token: &SessionToken) -> ApiResult<Me> {
        let s = self.online()?;
        let email = Self::user_of(&s, token)?;
        let u = s.users.get(&email).ok_or(ApiError::SessionEnded)?;
        Ok(Me {
            id: u.id.clone(),
            email,
            role: u.role.clone(),
            cloud: u.cloud.clone(),
            has_keys: u.keys.is_some(),
        })
    }

    fn put_keys(&self, token: &SessionToken, keys: &WrappedKeys) -> ApiResult<()> {
        let mut s = self.online()?;
        let email = Self::user_of(&s, token)?;
        let u = s.users.get_mut(&email).ok_or(ApiError::SessionEnded)?;
        if u.keys.is_some() {
            return Err(ApiError::KeysExist);
        }
        u.keys = Some(keys.clone());
        Ok(())
    }

    fn get_keys(&self, token: &SessionToken) -> ApiResult<Option<WrappedKeys>> {
        let s = self.online()?;
        let email = Self::user_of(&s, token)?;
        Ok(s.users.get(&email).and_then(|u| u.keys.clone()))
    }

    fn sign_out(&self, token: &SessionToken) -> ApiResult<()> {
        let mut s = self.online()?;
        s.sessions.retain(|(t, _, _)| t != token.session_part());
        Ok(())
    }

    fn sessions(&self, token: &SessionToken) -> ApiResult<Vec<SessionInfo>> {
        let s = self.online()?;
        let email = Self::user_of(&s, token)?;
        Ok(s.sessions
            .iter()
            .filter(|(_, e, _)| *e == email)
            .map(|(t, _, agent)| {
                SessionInfo::new(
                    format!("id-{t}"),
                    t.clone(),
                    Some(agent.clone()),
                    None,
                    None,
                )
            })
            .collect())
    }

    fn revoke_session(&self, token: &SessionToken, session_token: &str) -> ApiResult<()> {
        let mut s = self.online()?;
        Self::user_of(&s, token)?;
        s.sessions.retain(|(t, _, _)| t != session_token);
        Ok(())
    }
}
