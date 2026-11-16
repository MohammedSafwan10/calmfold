//! Native in-app updates. Sparkle owns macOS installation and Velopack owns
//! Windows installation. No ZIP extraction, file swapping or custom installer.
use anyhow::{Context as _, Result, bail};
use calmfold_core::update;
use gpui_kit::{App, BorrowAppContext as _, Global};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;
#[cfg(target_os = "macos")]
mod sparkle;
mod transport;
#[cfg(windows)]
mod velopack;
const CURRENT: &str = env!("CARGO_PKG_VERSION");
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Unsupported,
    Downloading { version: String },
    Verifying { version: String },
    Installing { version: String },
    Ready { version: String, notes: Vec<String> },
    Available { version: String },
    Failed { message: String },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub automatic: bool,
    /// Milliseconds since the epoch.
    pub last_check: Option<i64>,
    pub retry_after: Option<i64>,
    pub failures: u8,
    pub highest_version: Option<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            automatic: true,
            last_check: None,
            retry_after: None,
            failures: 0,
            highest_version: None,
        }
    }
}

pub struct Updates {
    dir: PathBuf,
    pub prefs: Prefs,
    pub status: Status,
    manual: bool,
}
impl Global for Updates {}
#[cfg(windows)]
impl Global for velopack::Ready {}

pub fn manual_pending(cx: &App) -> bool {
    cx.try_global::<Updates>().is_some_and(|u| u.manual)
}
pub fn status(cx: &App) -> Status {
    cx.try_global::<Updates>()
        .map_or(Status::Idle, |u| u.status.clone())
}
pub fn automatic(cx: &App) -> bool {
    cx.try_global::<Updates>()
        .is_some_and(|u| u.prefs.automatic)
}
pub fn init(data_dir: &Path, cx: &mut App) {
    let dir = data_dir.join("updates");
    let prefs = read_json::<Prefs>(&dir.join("prefs.json")).unwrap_or_default();
    cx.set_global(Updates {
        dir,
        prefs,
        status: Status::Idle,
        manual: false,
    });
    cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_secs(15))
            .await;
        loop {
            cx.update(|cx| {
                if update::platform().is_some()
                    && cx
                        .try_global::<Updates>()
                        .is_some_and(|u| u.prefs.due(calmfold_core::clock::now()))
                {
                    check(false, cx);
                }
            });
            cx.background_executor()
                .timer(Duration::from_secs(60))
                .await;
        }
    })
    .detach();
}
pub fn set_automatic(on: bool, cx: &mut App) {
    let Some(u) = cx.try_global::<Updates>() else {
        return;
    };
    let dir = u.dir.clone();
    let mut prefs = u.prefs.clone();
    prefs.automatic = on;
    if write_json(&dir.join("prefs.json"), &prefs).is_err() {
        set_status(
            Status::Failed {
                message: "Couldn't save update preferences. Try again.".into(),
            },
            cx,
        );
        return;
    }
    cx.update_global::<Updates, _>(|u, _| u.prefs = prefs);
    if on && matches!(status(cx), Status::Idle | Status::Failed { .. }) {
        check(false, cx);
    }
}
fn set_status(status: Status, cx: &mut App) {
    if cx.has_global::<Updates>() {
        cx.update_global::<Updates, _>(|u, _| u.status = status);
    }
}
pub fn check(manual: bool, cx: &mut App) {
    let Some(u) = cx.try_global::<Updates>() else {
        return;
    };
    if matches!(
        u.status,
        Status::Checking
            | Status::Downloading { .. }
            | Status::Verifying { .. }
            | Status::Installing { .. }
            | Status::Ready { .. }
    ) {
        return;
    }
    if update::platform().is_none() {
        set_status(Status::Unsupported, cx);
        return;
    }
    let dir = u.dir.clone();
    let before = u.status.clone();
    cx.update_global::<Updates, _>(|u, _| u.manual = manual);
    set_status(Status::Checking, cx);
    cx.spawn(async move |cx| {
        let result = cx.background_executor().spawn(async { transport::fetch() }).await;
        let feed = match result {
            Ok(feed) => feed,
            Err(_) => {
                cx.update(|cx| {
                    record_failure(&dir, calmfold_core::clock::now(), cx);
                    set_status(if manual { Status::Failed { message: "Couldn't check for updates. Check your connection and try again.".into() } } else { before }, cx);
                }); return;
            }
        };
        let accepted = cx.update(|cx| {
            let highest = &cx.global::<Updates>().prefs.highest_version;
            if highest.as_ref().is_some_and(|v| update::is_newer(v, &feed.version)) {
                record_failure(&dir, calmfold_core::clock::now(), cx);
                set_status(Status::Failed { message: "The update information is older than a verified release. Try again later.".into() }, cx);
                false
            } else { record_check(&dir, &feed.version, calmfold_core::clock::now(), cx); true }
        });
        if !accepted { return; }
        if !update::is_newer(&feed.version, CURRENT) { cx.update(|cx| set_status(Status::UpToDate, cx)); return; }
        #[cfg(target_os = "macos")]
        {
            let result = cx.update(sparkle::check);
            match result {
                Ok(true) => {},
                Ok(false) => cx.update(|cx| set_status(Status::Available { version: feed.version }, cx)),
                Err(_) => cx.update(|cx| {
                    record_failure(&dir, calmfold_core::clock::now(), cx);
                    set_status(Status::Failed { message: "The native updater couldn't start. Reinstall Calmfold or try again.".into() }, cx);
                }),
            }
        }
        #[cfg(windows)]
        {
            let version = feed.version.clone();
            let notes = feed.notes.clone();
            cx.update(|cx| set_status(Status::Downloading { version: version.clone() }, cx));
            let result = cx.background_executor().spawn(async move { velopack::download(&feed) }).await;
            cx.update(|cx| match result {
                Ok(Some(ready)) => { cx.set_global(ready); set_status(Status::Ready { version, notes }, cx); },
                Ok(None) => set_status(Status::Available { version }, cx),
                Err(_) => {
                    record_failure(&dir, calmfold_core::clock::now(), cx);
                    set_status(Status::Failed { message: "The update couldn't be downloaded or verified. Try again.".into() }, cx);
                }
            });
        }
    }).detach();
}

/// A fresh, private snapshot before handing off to a native installation engine.
pub fn snapshot_path(cx: &App) -> Result<PathBuf> {
    let u = cx
        .try_global::<Updates>()
        .context("updates aren't initialized")?;
    if !matches!(u.status, Status::Ready { .. }) {
        bail!("No update is ready.");
    }
    std::fs::create_dir_all(&u.dir)?;
    Ok(u.dir.join(format!(
        "before-{}.sqlite3",
        calmfold_core::VaultItemId::new()
    )))
}

/// True means GPUI should quit gracefully; Sparkle initiates its own native
/// termination/relaunch after it receives the already-approved install command.
pub fn apply(cx: &mut App) -> Result<bool> {
    if !matches!(status(cx), Status::Ready { .. }) {
        bail!("No update is ready.");
    }
    #[cfg(target_os = "macos")]
    {
        sparkle::restart(cx)?;
        Ok(false)
    }
    #[cfg(windows)]
    {
        let ready = cx
            .try_global::<velopack::Ready>()
            .context("No verified package is ready.")?
            .clone();
        velopack::restart(&ready)?;
        Ok(true)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        bail!("Automatic updates are not supported on this platform.")
    }
}

/// OS locks are released on crash. Never remove their inode while another
/// process may have it open, and never replace this with stale-file deletion.
pub struct InstanceLock(File);
impl InstanceLock {
    pub fn acquire(data_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(data_dir.join("updates"))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(data_dir.join("updates/app.lock"))?;
        file.try_lock()
            .context("Calmfold is already running for this data folder")?;
        Ok(Self(file))
    }
}
impl Drop for InstanceLock {
    fn drop(&mut self) {
        _ = self.0.unlock();
    }
}

impl Prefs {
    fn due(&self, now: jiff::Timestamp) -> bool {
        if !self.automatic {
            return false;
        }
        if let Some(retry) = self.retry_after {
            return now.as_millisecond() >= retry;
        }
        update::check_due(
            self.last_check
                .and_then(|ms| jiff::Timestamp::from_millisecond(ms).ok()),
            now,
        )
    }
}

fn record_check(dir: &Path, version: &str, now: jiff::Timestamp, cx: &mut App) {
    let mut prefs = cx.global::<Updates>().prefs.clone();
    prefs.last_check = Some(now.as_millisecond());
    prefs.retry_after = None;
    prefs.failures = 0;
    prefs.highest_version = Some(version.into());
    if write_json(&dir.join("prefs.json"), &prefs).is_ok() {
        cx.update_global::<Updates, _>(|u, _| u.prefs = prefs);
    }
}

fn record_failure(dir: &Path, now: jiff::Timestamp, cx: &mut App) {
    let mut prefs = cx.global::<Updates>().prefs.clone();
    prefs.failures = prefs.failures.saturating_add(1);
    let seconds = match prefs.failures {
        0 | 1 => 60,
        2 => 300,
        3 => 900,
        _ => 3600,
    };
    // Bounded jitter prevents every restarted client retrying on the same second.
    prefs.retry_after = Some(
        now.as_millisecond()
            .saturating_add((seconds + now.as_second().rem_euclid(17)) * 1000),
    );
    if write_json(&dir.join("prefs.json"), &prefs).is_ok() {
        cx.update_global::<Updates, _>(|u, _| u.prefs = prefs);
    }
}

fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Atomic write (temp file + rename) so a crash never leaves half a file.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let dir = path.parent().context("missing update state directory")?;
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer_pretty(&mut tmp, value)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    #[cfg(unix)]
    std::fs::File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_lock_has_no_stale_file_lockout() {
        let dir = tempfile::tempdir().expect("dir");
        let first = InstanceLock::acquire(dir.path()).expect("first");
        assert!(InstanceLock::acquire(dir.path()).is_err());
        drop(first);
        assert!(InstanceLock::acquire(dir.path()).is_ok());
    }

    #[gpui_kit::test]
    fn failures_retry_soon_without_overriding_user_preferences(cx: &mut gpui_kit::TestAppContext) {
        let dir = tempfile::tempdir().expect("dir");
        let now = "2026-11-16T09:00:00Z"
            .parse::<jiff::Timestamp>()
            .expect("time");
        cx.update(|cx| {
            cx.set_global(Updates {
                dir: dir.path().to_path_buf(),
                prefs: Prefs::default(),
                status: Status::Checking,
                manual: false,
            });
            record_failure(dir.path(), now, cx);
            let prefs = &cx.global::<Updates>().prefs;
            assert!(!prefs.due(now));
            assert!(prefs.due(now + jiff::SignedDuration::from_secs(80)));
            assert_eq!(
                prefs.last_check, None,
                "network failures aren't successful checks"
            );
            set_automatic(false, cx);
            record_check(dir.path(), "0.1.2", now, cx);
            let prefs = &cx.global::<Updates>().prefs;
            assert!(!prefs.automatic);
            assert!(!prefs.due(now + jiff::SignedDuration::from_hours(48)));
            assert_eq!(prefs.retry_after, None);
            assert_eq!(prefs.highest_version.as_deref(), Some("0.1.2"));
            let saved: Prefs = read_json(&dir.path().join("prefs.json")).expect("saved");
            assert!(!saved.automatic);
        });
    }
}
