//! An in-memory account server with the real API's rules, for tests
//! (`Library::set_account_api_for_tests`). Not used by the app itself.
#![doc(hidden)]

use std::collections::HashMap;
use std::sync::Mutex;

use ottero_crypto::Salt;
use ottero_crypto::account::{AccountKdf, decode_salt};

use ottero_crypto::Zeroizing;

use super::client::{
    AccountApi, ApiError, ApiResult, Cloud, KdfInfo, Me, SecondFactor, SessionInfo, SessionToken,
    SignInOutcome, TwoStepChallenge, TwoStepSetup, WrappedKeys,
};

/// Failed codes before two-step verification locks, and for how long (as the server).
const MAX_FAILURES: u8 = 5;
const LOCK_SECONDS: i64 = 15 * 60;
/// Wrong codes one sign-in attempt may try before the password is needed again.
const MAX_CHALLENGE_ATTEMPTS: u8 = 5;

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
    two_factor: Option<TwoFactor>,
}

/// Two-step verification as the server's `twoFactor` plugin keeps it.
struct TwoFactor {
    /// Base32 TOTP secret (RFC 6238, SHA-1, 30 s, 6 digits).
    secret: String,
    /// False until a code from the app confirmed the setup.
    verified: bool,
    backup_codes: Vec<String>,
    failures: u8,
    locked_until: Option<i64>,
}

impl TwoFactor {
    fn uri(&self, email: &str) -> String {
        format!(
            "otpauth://totp/Ottero:{}?secret={}&issuer=Ottero&digits=6&period=30",
            email.replace('@', "%40"),
            self.secret
        )
    }

    /// The code now, as an authenticator app shows it.
    fn code_at(&self, unix_seconds: u64) -> String {
        ottero_core::vault::totp::parse(&self.secret)
            .map(|otp| otp.code_at(unix_seconds).code)
            .unwrap_or_default()
    }

    /// RFC 6238 with one step of clock drift either way, like the server.
    fn accepts(&self, code: &str) -> bool {
        let now = now_seconds().cast_unsigned();
        [now.saturating_sub(30), now, now + 30]
            .iter()
            .any(|t| self.code_at(*t) == code)
    }
}

fn now_seconds() -> i64 {
    ottero_core::clock::now().as_second()
}

/// A pending second step: cookie value → email and wrong tries so far.
struct Challenge {
    cookie: String,
    email: String,
    attempts: u8,
}

#[derive(Default)]
struct State {
    users: HashMap<String, User>,
    /// raw session token → (email, user agent)
    sessions: Vec<(String, String, String)>,
    next: u64,
    offline: bool,
    blocked: Vec<String>,
    challenges: Vec<Challenge>,
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

    /// Whether two-step verification is on (confirmed) for `email`.
    #[must_use]
    pub fn two_factor_on(&self, email: &str) -> bool {
        self.lock()
            .users
            .get(email)
            .and_then(|u| u.two_factor.as_ref())
            .is_some_and(|t| t.verified)
    }

    /// The code an authenticator app would show now for `email`.
    #[must_use]
    pub fn authenticator_code(&self, email: &str) -> Option<String> {
        let s = self.lock();
        let tf = s.users.get(email)?.two_factor.as_ref()?;
        Some(tf.code_at(now_seconds().cast_unsigned()))
    }

    /// The backup codes still unused for `email`.
    #[must_use]
    pub fn backup_codes(&self, email: &str) -> Vec<String> {
        self.lock()
            .users
            .get(email)
            .and_then(|u| u.two_factor.as_ref())
            .map(|t| t.backup_codes.clone())
            .unwrap_or_default()
    }

    /// Whether `token` is a live session (an old token stops working once
    /// turning two-step on or off replaced it).
    #[must_use]
    pub fn is_session(&self, token: &SessionToken) -> bool {
        Self::user_of(&self.lock(), token).is_ok()
    }

    fn new_session(state: &mut State, email: &str) -> SessionToken {
        state.next += 1;
        let raw = format!("tok{}", state.next);
        state
            .sessions
            .push((raw.clone(), email.to_owned(), super::client::user_agent()));
        SessionToken::new(format!("{raw}.sig"))
    }

    /// Ends `token`'s session and starts another for the same user, as the
    /// server does when two-step verification is turned on or off.
    fn replace_session(state: &mut State, token: &SessionToken, email: &str) -> SessionToken {
        state.sessions.retain(|(t, _, _)| t != token.session_part());
        Self::new_session(state, email)
    }

    fn new_secret(state: &mut State) -> (String, Vec<String>) {
        const BASE32: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
        const ALNUM: &[u8] = b"abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        state.next += 1;
        let mut seed = state.next.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut pick = |alphabet: &[u8]| {
            // xorshift: deterministic, which is all a fake needs.
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            char::from(alphabet[usize::try_from(seed % alphabet.len() as u64).unwrap_or(0)])
        };
        let secret = (0..32).map(|_| pick(BASE32)).collect();
        let codes = (0..10)
            .map(|_| {
                let a: String = (0..5).map(|_| pick(ALNUM)).collect();
                let b: String = (0..5).map(|_| pick(ALNUM)).collect();
                format!("{a}-{b}")
            })
            .collect();
        (secret, codes)
    }

    fn check_auth(state: &State, email: &str, auth: &str) -> ApiResult<()> {
        match state.users.get(email) {
            Some(u) if u.auth == auth => Ok(()),
            _ => Err(ApiError::WrongCredentials),
        }
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
                two_factor: None,
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

    fn sign_in(&self, email: &str, auth: &str) -> ApiResult<SignInOutcome> {
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
        if u.two_factor.as_ref().is_some_and(|t| t.verified) {
            s.next += 1;
            let cookie = format!("better-auth.two_factor=2fa-{}.sig", s.next);
            s.challenges.push(Challenge {
                cookie: cookie.clone(),
                email: email.to_owned(),
                attempts: 0,
            });
            return Ok(SignInOutcome::TwoStep(TwoStepChallenge::new(cookie)));
        }
        Ok(SignInOutcome::Session(Self::new_session(&mut s, email)))
    }

    fn verify_second_factor(
        &self,
        challenge: &TwoStepChallenge,
        factor: SecondFactor,
        code: &str,
    ) -> ApiResult<SessionToken> {
        let mut guard = self.online()?;
        let s = &mut *guard;
        let ix = s
            .challenges
            .iter()
            .position(|c| c.cookie == challenge.expose())
            .ok_or(ApiError::TwoStepExpired)?;
        let email = s.challenges[ix].email.clone();
        let now = now_seconds();
        let tf = s
            .users
            .get_mut(&email)
            .and_then(|u| u.two_factor.as_mut())
            .filter(|t| t.verified)
            .ok_or(ApiError::TwoStepExpired)?;
        // The server's order: account lockout, then this attempt's budget.
        if tf.locked_until.is_some_and(|t| t > now) {
            return Err(ApiError::TwoStepLocked);
        }
        if s.challenges[ix].attempts >= MAX_CHALLENGE_ATTEMPTS {
            s.challenges.remove(ix);
            return Err(ApiError::TwoStepTooManyCodes);
        }
        let right = match factor {
            SecondFactor::Authenticator => tf.accepts(code),
            SecondFactor::BackupCode => match tf.backup_codes.iter().position(|c| c == code) {
                Some(used) => {
                    tf.backup_codes.remove(used);
                    true
                }
                None => false,
            },
        };
        if right {
            tf.failures = 0;
            tf.locked_until = None;
            s.challenges.remove(ix);
            return Ok(Self::new_session(s, &email));
        }
        tf.failures += 1;
        if tf.failures >= MAX_FAILURES {
            tf.locked_until = Some(now + LOCK_SECONDS);
        }
        s.challenges[ix].attempts += 1;
        Err(match factor {
            SecondFactor::Authenticator => ApiError::WrongTwoStepCode,
            SecondFactor::BackupCode => ApiError::WrongBackupCode,
        })
    }

    fn enable_two_factor(&self, token: &SessionToken, auth: &str) -> ApiResult<TwoStepSetup> {
        let mut s = self.online()?;
        let email = Self::user_of(&s, token)?;
        Self::check_auth(&s, &email, auth)?;
        if s.users
            .get(&email)
            .and_then(|u| u.two_factor.as_ref())
            .is_some_and(|t| t.verified)
        {
            return Err(ApiError::TwoStepChanged);
        }
        let (secret, codes) = Self::new_secret(&mut s);
        let tf = TwoFactor {
            secret,
            verified: false,
            backup_codes: codes.clone(),
            failures: 0,
            locked_until: None,
        };
        let uri = tf.uri(&email);
        if let Some(u) = s.users.get_mut(&email) {
            u.two_factor = Some(tf);
        }
        TwoStepSetup::new(uri, codes)
    }

    fn confirm_two_factor(
        &self,
        token: &SessionToken,
        code: &str,
    ) -> ApiResult<Option<SessionToken>> {
        let mut s = self.online()?;
        let email = Self::user_of(&s, token)?;
        let tf = s
            .users
            .get_mut(&email)
            .and_then(|u| u.two_factor.as_mut())
            .ok_or(ApiError::TwoStepChanged)?;
        if !tf.accepts(code) {
            return Err(ApiError::WrongTwoStepCode);
        }
        if tf.verified {
            return Ok(None);
        }
        tf.verified = true;
        Ok(Some(Self::replace_session(&mut s, token, &email)))
    }

    fn disable_two_factor(
        &self,
        token: &SessionToken,
        auth: &str,
    ) -> ApiResult<Option<SessionToken>> {
        let mut s = self.online()?;
        let email = Self::user_of(&s, token)?;
        Self::check_auth(&s, &email, auth)?;
        let u = s.users.get_mut(&email).ok_or(ApiError::SessionEnded)?;
        if u.two_factor.take().is_none() {
            return Err(ApiError::TwoStepChanged);
        }
        Ok(Some(Self::replace_session(&mut s, token, &email)))
    }

    fn regenerate_backup_codes(
        &self,
        token: &SessionToken,
        auth: &str,
    ) -> ApiResult<Vec<Zeroizing<String>>> {
        let mut s = self.online()?;
        let email = Self::user_of(&s, token)?;
        Self::check_auth(&s, &email, auth)?;
        let (_, codes) = Self::new_secret(&mut s);
        let tf = s
            .users
            .get_mut(&email)
            .and_then(|u| u.two_factor.as_mut())
            .filter(|t| t.verified)
            .ok_or(ApiError::TwoStepChanged)?;
        tf.backup_codes.clone_from(&codes);
        Ok(codes.into_iter().map(Zeroizing::new).collect())
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
            two_factor: u.two_factor.as_ref().is_some_and(|t| t.verified),
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

#[cfg(test)]
mod tests {
    use super::*;

    const AUTH: &str = "ab";

    fn signed_up(api: &FakeAccountApi) -> SessionToken {
        let auth = AUTH.repeat(32);
        api.sign_up("sam@example.com", "sam", &auth, "B465J6Y1apD8t4WQw10Jzg")
            .expect("sign up");
        let code = api.code_for("sam@example.com").expect("code");
        api.verify_email("sam@example.com", &code)
            .expect("verified");
        match api.sign_in("sam@example.com", &auth).expect("in") {
            SignInOutcome::Session(t) => t,
            SignInOutcome::TwoStep(_) => panic!("no two-step yet"),
        }
    }

    #[test]
    fn two_step_follows_the_server_rules() {
        let api = FakeAccountApi::new();
        let auth = AUTH.repeat(32);
        let token = signed_up(&api);
        assert_eq!(
            api.enable_two_factor(&token, "wrong").map(|_| ()),
            Err(ApiError::WrongCredentials)
        );
        let setup = api.enable_two_factor(&token, &auth).expect("setup");
        assert_eq!(setup.backup_codes().len(), 10);
        assert!(
            !api.two_factor_on("sam@example.com"),
            "inactive until confirmed"
        );
        // The link's secret is what the code comes from (RFC 6238).
        let now = now_seconds().cast_unsigned();
        let code = ottero_core::vault::totp::parse(setup.uri())
            .expect("link")
            .code_at(now)
            .code;
        let new = api
            .confirm_two_factor(&token, &code)
            .expect("on")
            .expect("replaced");
        assert!(!api.is_session(&token) && api.is_session(&new));
        assert!(api.me(&new).expect("me").two_factor);

        let SignInOutcome::TwoStep(challenge) = api.sign_in("sam@example.com", &auth).expect("in")
        else {
            panic!("asks for the code");
        };
        let backup = setup.backup_codes()[0].to_string();
        api.verify_second_factor(&challenge, SecondFactor::BackupCode, &backup)
            .expect("backup code works");
        assert_eq!(
            api.verify_second_factor(&challenge, SecondFactor::Authenticator, &code)
                .map(|_| ()),
            Err(ApiError::TwoStepExpired),
            "a challenge is used once"
        );
        let SignInOutcome::TwoStep(again) = api.sign_in("sam@example.com", &auth).expect("in")
        else {
            panic!("asks for the code");
        };
        assert_eq!(
            api.verify_second_factor(&again, SecondFactor::BackupCode, &backup)
                .map(|_| ()),
            Err(ApiError::WrongBackupCode),
            "backup codes are single-use"
        );
        for _ in 0..4 {
            assert_eq!(
                api.verify_second_factor(&again, SecondFactor::Authenticator, "000000")
                    .map(|_| ()),
                Err(ApiError::WrongTwoStepCode)
            );
        }
        assert_eq!(
            api.verify_second_factor(&again, SecondFactor::Authenticator, &code)
                .map(|_| ()),
            Err(ApiError::TwoStepLocked),
            "five failures lock it"
        );
    }
}
