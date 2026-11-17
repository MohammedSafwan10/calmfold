//! The Ottero Cloud account API over HTTPS (ureq, rustls), behind the
//! [`AccountApi`] trait so tests can swap in [`super::fake::FakeAccountApi`].
//!
//! Calls are blocking: run them on the background executor. Nothing here
//! logs. Request bodies (login keys, codes) and tokens live in zeroizing
//! buffers and are never part of an error.

use std::time::Duration;

use jiff::Timestamp;
use ottero_crypto::account::{AccountKdf, decode_salt};
use ottero_crypto::{Salt, Zeroizing};
use serde::Deserialize;
use serde_json::json;

/// Production account server.
pub const PRODUCTION_URL: &str = "https://account.nexdark.com";
/// The local development API (`ottero-cloud/api/dev.sh`).
pub const DEVELOPMENT_URL: &str = "http://127.0.0.1:8787";
/// Overrides the server address.
pub const URL_VAR: &str = "OTTERO_ACCOUNT_URL";

const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;

/// What went wrong, in terms the account screens can explain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// No connection, DNS failure, timeout or TLS failure.
    Offline,
    WrongCode,
    CodeExpired,
    /// Wrong password or Secret Key (the server can't tell which).
    WrongCredentials,
    NotVerified,
    TooManyAttempts,
    /// Blocked by Ottero; the server's message says whom to contact.
    Blocked(String),
    /// The session token is no longer accepted.
    SessionEnded,
    /// `POST /api/keys` when keys exist already.
    KeysExist,
    /// The server refused the request as invalid.
    Rejected,
    /// 5xx.
    Server,
    /// The response wasn't what this version of Ottero understands.
    Unexpected,
    /// `OTTERO_ACCOUNT_URL` is unusable (for example not https in a release build).
    Misconfigured,
}

impl ApiError {
    /// A calm, complete sentence for the user.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::Offline => "Couldn't reach Ottero Cloud. Check your connection and try again.".into(),
            Self::WrongCode => "That code isn't right. Check the email and try again.".into(),
            Self::CodeExpired => "That code has expired. Send a new one.".into(),
            Self::WrongCredentials => "Wrong email, password or Secret Key.".into(),
            Self::NotVerified => "Confirm your email first.".into(),
            Self::TooManyAttempts => "Too many attempts. Wait a minute and try again.".into(),
            Self::Blocked(message) => message.clone(),
            Self::SessionEnded => "You were signed out of Ottero Cloud. Sign in again.".into(),
            Self::KeysExist => "This account's keys are already set up.".into(),
            Self::Rejected => "Ottero Cloud didn't accept that. Update Ottero and try again.".into(),
            Self::Server => "Ottero Cloud had a problem. Try again in a few minutes.".into(),
            Self::Unexpected => "Ottero Cloud sent an answer Ottero doesn't understand. Update Ottero and try again.".into(),
            Self::Misconfigured => "The Ottero Cloud address isn't secure, so Ottero won't use it.".into(),
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// The bearer token for one signed-in device. Never printed.
pub struct SessionToken(Zeroizing<String>);

impl SessionToken {
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(Zeroizing::new(value))
    }

    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The token without its signature, as `list-sessions` reports it.
    #[must_use]
    pub fn session_part(&self) -> &str {
        self.0.split('.').next().unwrap_or_default()
    }
}

impl Clone for SessionToken {
    fn clone(&self) -> Self {
        Self::new(self.0.to_string())
    }
}

impl std::fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionToken([redacted])")
    }
}

/// The public salt and KDF settings for an email (`POST /api/kdf`).
pub struct KdfInfo {
    pub salt: Salt,
    pub kdf: AccountKdf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cloud {
    pub source: String,
    pub status: String,
    pub ends_at: Option<Timestamp>,
}

/// `GET /api/me`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Me {
    pub id: String,
    pub email: String,
    pub role: String,
    pub cloud: Option<Cloud>,
    pub has_keys: bool,
}

impl Me {
    #[must_use]
    pub fn plan(&self) -> ottero_core::account::Plan {
        ottero_core::account::Plan::from_parts(
            &self.role,
            self.cloud.as_ref().map(|c| (c.source.as_str(), c.ends_at)),
        )
    }
}

/// The account key, wrapped twice (opaque to the server).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedKeys {
    pub by_password: String,
    pub by_recovery: String,
}

/// One signed-in device, from `list-sessions`.
pub struct SessionInfo {
    pub id: String,
    /// Needed to revoke it. Never shown.
    token: Zeroizing<String>,
    pub user_agent: Option<String>,
    pub created_at: Option<Timestamp>,
    pub last_active: Option<Timestamp>,
}

impl SessionInfo {
    #[must_use]
    pub fn new(
        id: String,
        token: String,
        user_agent: Option<String>,
        created_at: Option<Timestamp>,
        last_active: Option<Timestamp>,
    ) -> Self {
        Self {
            id,
            token: Zeroizing::new(token),
            user_agent,
            created_at,
            last_active,
        }
    }

    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    #[must_use]
    pub fn is_current(&self, current: &SessionToken) -> bool {
        self.token.as_str() == current.session_part()
    }
}

/// The account server. Every method blocks; call from a background thread.
pub trait AccountApi: Send + Sync {
    fn kdf(&self, email: &str) -> ApiResult<KdfInfo>;
    /// Creates an unverified account; the server emails a code. The same
    /// answer comes back for an email that already has an account.
    fn sign_up(&self, email: &str, name: &str, auth_key_hex: &str, salt: &str) -> ApiResult<()>;
    fn verify_email(&self, email: &str, code: &str) -> ApiResult<()>;
    fn resend_code(&self, email: &str) -> ApiResult<()>;
    fn sign_in(&self, email: &str, auth_key_hex: &str) -> ApiResult<SessionToken>;
    fn me(&self, token: &SessionToken) -> ApiResult<Me>;
    fn put_keys(&self, token: &SessionToken, keys: &WrappedKeys) -> ApiResult<()>;
    /// `None` when the account has no keys yet.
    fn get_keys(&self, token: &SessionToken) -> ApiResult<Option<WrappedKeys>>;
    fn sign_out(&self, token: &SessionToken) -> ApiResult<()>;
    fn sessions(&self, token: &SessionToken) -> ApiResult<Vec<SessionInfo>>;
    fn revoke_session(&self, token: &SessionToken, session_token: &str) -> ApiResult<()>;
}

/// The server address: `OTTERO_ACCOUNT_URL`, else the build's default.
/// Release builds accept only `https://`.
pub fn base_url(value: Option<&str>, release: bool) -> ApiResult<String> {
    let default = if release {
        PRODUCTION_URL
    } else {
        DEVELOPMENT_URL
    };
    let url = value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(default);
    let parsed = url::Url::parse(url).map_err(|_| ApiError::Misconfigured)?;
    let allowed = match parsed.scheme() {
        "https" => true,
        "http" => !release,
        _ => false,
    };
    if !allowed || parsed.host_str().is_none() || !parsed.username().is_empty() {
        return Err(ApiError::Misconfigured);
    }
    Ok(url.trim_end_matches('/').to_owned())
}

/// Used when the address is unusable: every call fails with `Misconfigured`.
pub struct Unconfigured;

impl AccountApi for Unconfigured {
    fn kdf(&self, _: &str) -> ApiResult<KdfInfo> {
        Err(ApiError::Misconfigured)
    }
    fn sign_up(&self, _: &str, _: &str, _: &str, _: &str) -> ApiResult<()> {
        Err(ApiError::Misconfigured)
    }
    fn verify_email(&self, _: &str, _: &str) -> ApiResult<()> {
        Err(ApiError::Misconfigured)
    }
    fn resend_code(&self, _: &str) -> ApiResult<()> {
        Err(ApiError::Misconfigured)
    }
    fn sign_in(&self, _: &str, _: &str) -> ApiResult<SessionToken> {
        Err(ApiError::Misconfigured)
    }
    fn me(&self, _: &SessionToken) -> ApiResult<Me> {
        Err(ApiError::Misconfigured)
    }
    fn put_keys(&self, _: &SessionToken, _: &WrappedKeys) -> ApiResult<()> {
        Err(ApiError::Misconfigured)
    }
    fn get_keys(&self, _: &SessionToken) -> ApiResult<Option<WrappedKeys>> {
        Err(ApiError::Misconfigured)
    }
    fn sign_out(&self, _: &SessionToken) -> ApiResult<()> {
        Err(ApiError::Misconfigured)
    }
    fn sessions(&self, _: &SessionToken) -> ApiResult<Vec<SessionInfo>> {
        Err(ApiError::Misconfigured)
    }
    fn revoke_session(&self, _: &SessionToken, _: &str) -> ApiResult<()> {
        Err(ApiError::Misconfigured)
    }
}

pub struct HttpAccountClient {
    base: String,
    agent: ureq::Agent,
}

struct Reply {
    status: u16,
    body: Zeroizing<Vec<u8>>,
    auth_token: Option<SessionToken>,
}

#[derive(Deserialize, Default)]
struct ErrorBody {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    code: Option<String>,
}

impl HttpAccountClient {
    /// `base` must come from [`base_url`].
    #[must_use]
    pub fn new(base: String) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            // Never follow a redirect: a login key must go only where it was sent.
            .max_redirects(0)
            .https_only(base.starts_with("https://"))
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(user_agent())
            .build()
            .into();
        Self { base, agent }
    }

    /// From `OTTERO_ACCOUNT_URL` and the build type.
    #[must_use]
    pub fn from_env() -> Box<dyn AccountApi> {
        match base_url(
            std::env::var(URL_VAR).ok().as_deref(),
            !cfg!(debug_assertions),
        ) {
            Ok(base) => Box::new(Self::new(base)),
            Err(_) => {
                eprintln!("ottero: {URL_VAR} is not a usable https address; Ottero Cloud is off");
                Box::new(Unconfigured)
            }
        }
    }

    fn post(
        &self,
        path: &str,
        body: &serde_json::Value,
        token: Option<&SessionToken>,
    ) -> ApiResult<Reply> {
        let bytes = Zeroizing::new(serde_json::to_vec(body).map_err(|_| ApiError::Unexpected)?);
        let mut request = self
            .agent
            .post(format!("{}{path}", self.base))
            .header("content-type", "application/json");
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {}", token.expose()));
        }
        read(request.send(bytes.as_slice()))
    }

    fn get(&self, path: &str, token: &SessionToken) -> ApiResult<Reply> {
        read(
            self.agent
                .get(format!("{}{path}", self.base))
                .header("authorization", format!("Bearer {}", token.expose()))
                .call(),
        )
    }
}

/// `Ottero/0.1.3 (macOS)`: names the device in the sessions list.
#[must_use]
pub fn user_agent() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "windows" => "Windows",
        "linux" => "Linux",
        other => other,
    };
    format!("Ottero/{} ({os})", env!("CARGO_PKG_VERSION"))
}

fn read(result: Result<ureq::http::Response<ureq::Body>, ureq::Error>) -> ApiResult<Reply> {
    let mut response = result.map_err(|err| match err {
        ureq::Error::BodyExceedsLimit(_)
        | ureq::Error::TooManyRedirects
        | ureq::Error::RedirectFailed => ApiError::Unexpected,
        _ => ApiError::Offline,
    })?;
    let status = response.status().as_u16();
    let auth_token = response
        .headers()
        .get("set-auth-token")
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .map(|v| SessionToken::new(v.to_owned()));
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_vec()
        .map_err(|_| ApiError::Offline)?;
    Ok(Reply {
        status,
        body: Zeroizing::new(body),
        auth_token,
    })
}

fn error_of(reply: &Reply) -> ErrorBody {
    serde_json::from_slice(&reply.body).unwrap_or_default()
}

/// Maps a failed reply for calls that need no special handling.
fn general(reply: &Reply, authed: bool) -> ApiError {
    let body = error_of(reply);
    match reply.status {
        401 if authed => ApiError::SessionEnded,
        401 => ApiError::WrongCredentials,
        429 => ApiError::TooManyAttempts,
        403 if body.code.as_deref() == Some("BANNED_USER")
            || body
                .message
                .as_deref()
                .is_some_and(|m| m.contains("blocked")) =>
        {
            ApiError::Blocked(blocked_message(&body))
        }
        400..=499 => ApiError::Rejected,
        500..=599 => ApiError::Server,
        _ => ApiError::Unexpected,
    }
}

fn blocked_message(body: &ErrorBody) -> String {
    // The server's own sentence names the contact; keep it short and plain.
    body.message
        .as_deref()
        .filter(|m| m.len() <= 200 && m.contains("blocked"))
        .map_or_else(|| "This account is blocked.".to_owned(), str::to_owned)
}

fn ok(reply: &Reply) -> bool {
    (200..300).contains(&reply.status)
}

fn parse<T: for<'de> Deserialize<'de>>(reply: &Reply) -> ApiResult<T> {
    serde_json::from_slice(&reply.body).map_err(|_| ApiError::Unexpected)
}

fn timestamp(value: Option<&str>) -> Option<Timestamp> {
    value.and_then(|v| v.parse().ok())
}

impl AccountApi for HttpAccountClient {
    fn kdf(&self, email: &str) -> ApiResult<KdfInfo> {
        #[derive(Deserialize)]
        struct Body {
            salt: String,
            kdf: String,
        }
        let reply = self.post("/api/kdf", &json!({ "email": email }), None)?;
        if !ok(&reply) {
            return Err(general(&reply, false));
        }
        let body: Body = parse(&reply)?;
        Ok(KdfInfo {
            salt: decode_salt(&body.salt).map_err(|_| ApiError::Unexpected)?,
            kdf: AccountKdf::parse(&body.kdf).map_err(|_| ApiError::Unexpected)?,
        })
    }

    fn sign_up(&self, email: &str, name: &str, auth_key_hex: &str, salt: &str) -> ApiResult<()> {
        let body = json!({
            "email": email,
            "name": name,
            "password": auth_key_hex,
            "kdf": { "salt": salt, "params": ottero_crypto::account::KDF_V1 },
        });
        let reply = self.post("/api/auth/sign-up/email", &body, None)?;
        if ok(&reply) {
            Ok(())
        } else {
            Err(general(&reply, false))
        }
    }

    fn verify_email(&self, email: &str, code: &str) -> ApiResult<()> {
        let reply = self.post(
            "/api/auth/email-otp/verify-email",
            &json!({ "email": email, "otp": code }),
            None,
        )?;
        if ok(&reply) {
            return Ok(());
        }
        let body = error_of(&reply);
        Err(match (reply.status, body.code.as_deref()) {
            (_, Some("OTP_EXPIRED")) => ApiError::CodeExpired,
            (_, Some("TOO_MANY_ATTEMPTS")) | (429, _) => ApiError::TooManyAttempts,
            (400, _) => ApiError::WrongCode,
            _ => general(&reply, false),
        })
    }

    fn resend_code(&self, email: &str) -> ApiResult<()> {
        let reply = self.post(
            "/api/auth/email-otp/send-verification-otp",
            &json!({ "email": email, "type": "email-verification" }),
            None,
        )?;
        if ok(&reply) {
            Ok(())
        } else {
            Err(general(&reply, false))
        }
    }

    fn sign_in(&self, email: &str, auth_key_hex: &str) -> ApiResult<SessionToken> {
        let reply = self.post(
            "/api/auth/sign-in/email",
            &json!({ "email": email, "password": auth_key_hex }),
            None,
        )?;
        if ok(&reply) {
            return reply.auth_token.ok_or(ApiError::Unexpected);
        }
        let body = error_of(&reply);
        Err(match (reply.status, body.code.as_deref()) {
            (403, Some("EMAIL_NOT_VERIFIED")) => ApiError::NotVerified,
            (401, _) => ApiError::WrongCredentials,
            _ => general(&reply, false),
        })
    }

    fn me(&self, token: &SessionToken) -> ApiResult<Me> {
        #[derive(Deserialize)]
        struct CloudBody {
            source: String,
            status: String,
            ends_at: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Body {
            id: String,
            email: String,
            role: String,
            cloud: Option<CloudBody>,
            has_keys: bool,
        }
        let reply = self.get("/api/me", token)?;
        if !ok(&reply) {
            return Err(general(&reply, true));
        }
        let body: Body = parse(&reply)?;
        Ok(Me {
            id: body.id,
            email: body.email,
            role: body.role,
            cloud: body.cloud.map(|c| Cloud {
                source: c.source,
                status: c.status,
                ends_at: timestamp(c.ends_at.as_deref()),
            }),
            has_keys: body.has_keys,
        })
    }

    fn put_keys(&self, token: &SessionToken, keys: &WrappedKeys) -> ApiResult<()> {
        let reply = self.post(
            "/api/keys",
            &json!({ "wrapped_by_password": keys.by_password, "wrapped_by_recovery": keys.by_recovery }),
            Some(token),
        )?;
        match reply.status {
            201 | 200 => Ok(()),
            409 => Err(ApiError::KeysExist),
            _ => Err(general(&reply, true)),
        }
    }

    fn get_keys(&self, token: &SessionToken) -> ApiResult<Option<WrappedKeys>> {
        #[derive(Deserialize)]
        struct Body {
            wrapped_by_password: String,
            wrapped_by_recovery: String,
        }
        let reply = self.get("/api/keys", token)?;
        match reply.status {
            404 => Ok(None),
            s if (200..300).contains(&s) => {
                let body: Body = parse(&reply)?;
                Ok(Some(WrappedKeys {
                    by_password: body.wrapped_by_password,
                    by_recovery: body.wrapped_by_recovery,
                }))
            }
            _ => Err(general(&reply, true)),
        }
    }

    fn sign_out(&self, token: &SessionToken) -> ApiResult<()> {
        let reply = self.post("/api/auth/sign-out", &json!({}), Some(token))?;
        // Already signed out on the server counts as done.
        if ok(&reply) || reply.status == 401 {
            Ok(())
        } else {
            Err(general(&reply, true))
        }
    }

    fn sessions(&self, token: &SessionToken) -> ApiResult<Vec<SessionInfo>> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Body {
            id: String,
            token: String,
            user_agent: Option<String>,
            created_at: Option<String>,
            updated_at: Option<String>,
        }
        let reply = self.get("/api/auth/list-sessions", token)?;
        if !ok(&reply) {
            return Err(general(&reply, true));
        }
        let rows: Vec<Body> = parse(&reply)?;
        Ok(rows
            .into_iter()
            .map(|r| {
                SessionInfo::new(
                    r.id,
                    r.token,
                    r.user_agent,
                    timestamp(r.created_at.as_deref()),
                    timestamp(r.updated_at.as_deref()),
                )
            })
            .collect())
    }

    fn revoke_session(&self, token: &SessionToken, session_token: &str) -> ApiResult<()> {
        let reply = self.post(
            "/api/auth/revoke-session",
            &json!({ "token": session_token }),
            Some(token),
        )?;
        if ok(&reply) {
            Ok(())
        } else {
            Err(general(&reply, true))
        }
    }
}

#[cfg(test)]
mod tests {
    //! Against a tiny in-process HTTP server that replays the real API's
    //! answers (captured from `ottero-cloud/api`), so these run in check.sh.
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    struct Seen {
        method: String,
        path: String,
        authorization: Option<String>,
        body: String,
    }

    /// One canned answer: status, extra headers, body.
    type Canned = (u16, Vec<(&'static str, &'static str)>, &'static str);

    /// Serves `replies` in order and records the requests.
    fn server(replies: Vec<Canned>) -> (String, Arc<Mutex<Vec<Seen>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let base = format!("http://{}", listener.local_addr().expect("addr"));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            for (status, headers, body) in replies {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut line = String::new();
                reader.read_line(&mut line).expect("request line");
                let mut parts = line.split_whitespace();
                let method = parts.next().unwrap_or_default().to_owned();
                let path = parts.next().unwrap_or_default().to_owned();
                let (mut length, mut authorization) = (0usize, None);
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).expect("header");
                    let header = header.trim_end();
                    if header.is_empty() {
                        break;
                    }
                    let (name, value) = header.split_once(':').unwrap_or_default();
                    match name.to_ascii_lowercase().as_str() {
                        "content-length" => length = value.trim().parse().unwrap_or(0),
                        "authorization" => authorization = Some(value.trim().to_owned()),
                        _ => {}
                    }
                }
                let mut buf = vec![0; length];
                reader.read_exact(&mut buf).expect("body");
                log.lock().expect("log").push(Seen {
                    method,
                    path,
                    authorization,
                    body: String::from_utf8_lossy(&buf).into_owned(),
                });
                let mut out = stream;
                let mut head = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
                    body.len()
                );
                for (k, v) in headers {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                head.push_str("\r\n");
                _ = out.write_all(head.as_bytes());
                _ = out.write_all(body.as_bytes());
            }
        });
        (base, seen)
    }

    const HEX: &str = "ab";

    #[test]
    fn base_url_rules() {
        assert_eq!(base_url(None, false).as_deref(), Ok(DEVELOPMENT_URL));
        assert_eq!(base_url(None, true).as_deref(), Ok(PRODUCTION_URL));
        assert_eq!(
            base_url(Some("https://x.example/"), true).as_deref(),
            Ok("https://x.example")
        );
        assert_eq!(
            base_url(Some("http://127.0.0.1:9"), false).as_deref(),
            Ok("http://127.0.0.1:9")
        );
        assert_eq!(
            base_url(Some("http://account.nexdark.com"), true),
            Err(ApiError::Misconfigured)
        );
        assert_eq!(
            base_url(Some("ftp://x"), false),
            Err(ApiError::Misconfigured)
        );
        assert_eq!(
            base_url(Some("https://u:p@x.example"), true),
            Err(ApiError::Misconfigured)
        );
        assert_eq!(base_url(Some("  "), true).as_deref(), Ok(PRODUCTION_URL));
    }

    #[test]
    fn sign_in_reads_the_token_header_and_sends_the_key() {
        let (base, seen) = server(vec![
            (
                200,
                vec![("set-auth-token", "raw123.sig=")],
                r#"{"redirect":false,"token":"raw123"}"#,
            ),
            (
                403,
                vec![],
                r#"{"message":"Email not verified","code":"EMAIL_NOT_VERIFIED"}"#,
            ),
            (
                401,
                vec![],
                r#"{"message":"Invalid email or password","code":"INVALID_EMAIL_OR_PASSWORD"}"#,
            ),
            (429, vec![], r#"{"message":"Too many requests"}"#),
            (
                403,
                vec![],
                r#"{"message":"This account is blocked. Contact safwan@nexdark.com.","code":"BANNED_USER"}"#,
            ),
            (500, vec![], r#"{"error":"Something went wrong"}"#),
        ]);
        let api = HttpAccountClient::new(base);
        let token = api.sign_in("sam@example.com", HEX).expect("token");
        assert_eq!(token.expose(), "raw123.sig=");
        assert_eq!(token.session_part(), "raw123");
        assert_eq!(format!("{token:?}"), "SessionToken([redacted])");
        assert_eq!(
            api.sign_in("a@b.co", HEX).map(|_| ()),
            Err(ApiError::NotVerified)
        );
        assert_eq!(
            api.sign_in("a@b.co", HEX).map(|_| ()),
            Err(ApiError::WrongCredentials)
        );
        assert_eq!(
            api.sign_in("a@b.co", HEX).map(|_| ()),
            Err(ApiError::TooManyAttempts)
        );
        assert_eq!(
            api.sign_in("a@b.co", HEX).map(|_| ()),
            Err(ApiError::Blocked(
                "This account is blocked. Contact safwan@nexdark.com.".into()
            ))
        );
        assert_eq!(
            api.sign_in("a@b.co", HEX).map(|_| ()),
            Err(ApiError::Server)
        );
        let seen = seen.lock().expect("seen");
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].path, "/api/auth/sign-in/email");
        let body: serde_json::Value = serde_json::from_str(&seen[0].body).expect("json");
        assert_eq!(body, json!({ "email": "sam@example.com", "password": HEX }));
    }

    #[test]
    fn kdf_accepts_only_v1_and_valid_salts() {
        let (base, _) = server(vec![
            (
                200,
                vec![],
                r#"{"salt":"B465J6Y1apD8t4WQw10Jzg","kdf":"argon2id:m=262144,t=3,p=1:v1"}"#,
            ),
            (
                200,
                vec![],
                r#"{"salt":"B465J6Y1apD8t4WQw10Jzg","kdf":"argon2id:m=1024,t=1,p=1:v1"}"#,
            ),
            (
                200,
                vec![],
                r#"{"salt":"short","kdf":"argon2id:m=262144,t=3,p=1:v1"}"#,
            ),
            (400, vec![], r#"{"error":"Enter a valid email"}"#),
        ]);
        let api = HttpAccountClient::new(base);
        let info = api.kdf("sam@example.com").expect("kdf");
        assert_eq!(info.kdf, AccountKdf::V1);
        assert_eq!(
            api.kdf("x").map(|_| ()),
            Err(ApiError::Unexpected),
            "weaker KDF refused"
        );
        assert_eq!(
            api.kdf("x").map(|_| ()),
            Err(ApiError::Unexpected),
            "bad salt refused"
        );
        assert_eq!(api.kdf("x").map(|_| ()), Err(ApiError::Rejected));
    }

    #[test]
    fn sign_up_sends_the_kdf_and_codes_map_to_errors() {
        let (base, seen) = server(vec![
            (200, vec![], r#"{"token":null,"user":{}}"#),
            (
                400,
                vec![],
                r#"{"message":"Invalid OTP","code":"INVALID_OTP"}"#,
            ),
            (
                400,
                vec![],
                r#"{"message":"OTP expired","code":"OTP_EXPIRED"}"#,
            ),
            (
                403,
                vec![],
                r#"{"message":"Too many attempts","code":"TOO_MANY_ATTEMPTS"}"#,
            ),
            (200, vec![], r#"{"status":true}"#),
            (200, vec![], r#"{"success":true}"#),
        ]);
        let api = HttpAccountClient::new(base);
        api.sign_up("sam@example.com", "sam", HEX, "B465J6Y1apD8t4WQw10Jzg")
            .expect("sign up");
        assert_eq!(
            api.verify_email("sam@example.com", "000000"),
            Err(ApiError::WrongCode)
        );
        assert_eq!(
            api.verify_email("sam@example.com", "000000"),
            Err(ApiError::CodeExpired)
        );
        assert_eq!(
            api.verify_email("sam@example.com", "000000"),
            Err(ApiError::TooManyAttempts)
        );
        api.verify_email("sam@example.com", "123456")
            .expect("verified");
        api.resend_code("sam@example.com").expect("resent");
        let seen = seen.lock().expect("seen");
        let body: serde_json::Value = serde_json::from_str(&seen[0].body).expect("json");
        assert_eq!(body["kdf"]["params"], "argon2id:m=262144,t=3,p=1:v1");
        assert_eq!(body["kdf"]["salt"], "B465J6Y1apD8t4WQw10Jzg");
        assert_eq!(body["password"], HEX);
        let resend: serde_json::Value = serde_json::from_str(&seen[5].body).expect("json");
        assert_eq!(
            resend,
            json!({ "email": "sam@example.com", "type": "email-verification" })
        );
    }

    #[test]
    fn authenticated_calls_send_the_bearer_token() {
        let (base, seen) = server(vec![
            (
                200,
                vec![],
                r#"{"id":"u1","email":"sam@example.com","role":"owner","cloud":{"source":"owner","status":"active","ends_at":null},"hasKeys":true}"#,
            ),
            (404, vec![], r#"{"error":"No keys yet"}"#),
            (
                200,
                vec![],
                r#"{"wrapped_by_password":"aaaa","wrapped_by_recovery":"bbbb","version":1}"#,
            ),
            (201, vec![], r#"{"ok":true}"#),
            (409, vec![], r#"{"error":"Keys already set up"}"#),
            (
                200,
                vec![],
                r#"[{"expiresAt":"2027-01-16T12:53:42.460Z","token":"raw123","createdAt":"2026-11-17T12:53:42.460Z","updatedAt":"2026-11-17T12:54:00.000Z","ipAddress":"127.0.0.1","userAgent":"Ottero/0.1.3 (macOS)","userId":"u1","impersonatedBy":null,"id":"s1"},{"token":"other","id":"s2","userAgent":null,"createdAt":null,"updatedAt":null}]"#,
            ),
            (200, vec![], r#"{"status":true}"#),
            (401, vec![], r#"{"error":"Sign in again"}"#),
            (401, vec![], r#"{"error":"Sign in again"}"#),
        ]);
        let api = HttpAccountClient::new(base);
        let token = SessionToken::new("raw123.sig".into());
        let me = api.me(&token).expect("me");
        assert_eq!(me.plan(), ottero_core::account::Plan::Owner);
        assert!(me.has_keys);
        assert_eq!(api.get_keys(&token), Ok(None));
        assert_eq!(
            api.get_keys(&token).expect("keys").map(|k| k.by_password),
            Some("aaaa".into())
        );
        let keys = WrappedKeys {
            by_password: "p".into(),
            by_recovery: "r".into(),
        };
        api.put_keys(&token, &keys).expect("put");
        assert_eq!(api.put_keys(&token, &keys), Err(ApiError::KeysExist));
        let sessions = api.sessions(&token).expect("sessions");
        assert_eq!(sessions.len(), 2);
        assert!(sessions[0].is_current(&token));
        assert!(!sessions[1].is_current(&token));
        assert_eq!(
            sessions[0].user_agent.as_deref(),
            Some("Ottero/0.1.3 (macOS)")
        );
        assert!(sessions[0].last_active.is_some());
        api.revoke_session(&token, sessions[1].token())
            .expect("revoke");
        assert_eq!(api.me(&token).map(|_| ()), Err(ApiError::SessionEnded));
        api.sign_out(&token).expect("already signed out is fine");
        let seen = seen.lock().expect("seen");
        assert!(
            seen.iter()
                .all(|s| s.authorization.as_deref() == Some("Bearer raw123.sig"))
        );
        assert_eq!(seen[6].path, "/api/auth/revoke-session");
        assert_eq!(seen[6].body, r#"{"token":"other"}"#);
    }

    #[test]
    fn unreachable_server_reads_as_offline() {
        // Bind then drop: nothing listens on that port any more.
        let port = TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("addr")
            .port();
        let api = HttpAccountClient::new(format!("http://127.0.0.1:{port}"));
        assert_eq!(api.kdf("a@b.co").map(|_| ()), Err(ApiError::Offline));
        assert!(ApiError::Offline.message().contains("connection"));
    }

    #[test]
    fn redirects_are_not_followed() {
        let (base, seen) = server(vec![(302, vec![("location", "http://evil.example/")], "")]);
        let api = HttpAccountClient::new(base);
        assert!(api.sign_in("a@b.co", HEX).is_err());
        assert_eq!(seen.lock().expect("seen").len(), 1);
    }
}
