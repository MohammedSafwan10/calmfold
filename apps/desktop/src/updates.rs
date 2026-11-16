//! In-app updates: check once a day, download in the background, install
//! on restart. The trust rules live in `calmfold_core::update`.
//!
//! State machine (one global, [`Updates`]):
//!
//! ```text
//! Idle ──check──▶ Checking ──newer, installable──▶ Downloading ──verified──▶ Ready
//!                    │  └──newer, not installable (debug, cargo run)──▶ Available
//!                    └──none──▶ UpToDate         any failure ──▶ Failed (keeps the app as is)
//! Ready ──Restart to update──▶ swap files, launch the new copy, quit
//! ```
//!
//! Files (`<data>/updates/`): `prefs.json` (automatic on/off, last check),
//! `.lock` (one check or download at a time, across running copies),
//! `<version>/download.zip` and `<version>/staged/` (the unpacked app),
//! `ready.json` (written last: a staged update is complete only once it exists).
//! The running app is never touched until the user restarts, and a failed
//! swap is rolled back.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context as _, Result, bail};
use calmfold_core::update::{self, Download, Feed};
use gpui_kit::{App, BorrowAppContext as _, Global};
use serde::{Deserialize, Serialize};

const CURRENT: &str = env!("CARGO_PKG_VERSION");
const LOCK_STALE: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Downloading {
        version: String,
    },
    /// Staged and verified; installs on restart.
    Ready {
        version: String,
        notes: Vec<String>,
    },
    /// Newer, but this copy can't replace itself (a development run).
    Available {
        version: String,
    },
    Failed {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub automatic: bool,
    /// Milliseconds since the epoch.
    pub last_check: Option<i64>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            automatic: true,
            last_check: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReadyFile {
    version: String,
    notes: Vec<String>,
}

pub struct Updates {
    dir: PathBuf,
    pub prefs: Prefs,
    pub status: Status,
    /// A check the user asked for, so "up to date" is said out loud.
    manual: bool,
}

/// Whether the last check was asked for by the user.
#[must_use]
pub fn manual_pending(cx: &App) -> bool {
    cx.try_global::<Updates>().is_some_and(|u| u.manual)
}

impl Global for Updates {}

/// Startup: tidy what a past update left, pick up a staged one, and check
/// if a day has passed.
pub fn init(data_dir: &Path, cx: &mut App) {
    let dir = data_dir.join("updates");
    remove_old_copies();
    let prefs = read_json::<Prefs>(&dir.join("prefs.json")).unwrap_or_default();
    let mut status = Status::Idle;
    if let Some(ready) = read_json::<ReadyFile>(&dir.join("ready.json")) {
        if update::is_newer(&ready.version, CURRENT) && staged_app(&dir, &ready.version).is_some() {
            status = Status::Ready {
                version: ready.version,
                notes: ready.notes,
            };
        } else {
            clear_staged(&dir);
        }
    }
    let due = prefs.automatic
        && update::check_due(
            prefs
                .last_check
                .and_then(|ms| jiff::Timestamp::from_millisecond(ms).ok()),
            jiff::Timestamp::now(),
        );
    cx.set_global(Updates {
        dir,
        prefs,
        status,
        manual: false,
    });
    if due {
        check(false, cx);
    }
}

#[must_use]
pub fn status(cx: &App) -> Status {
    cx.try_global::<Updates>()
        .map_or(Status::Idle, |u| u.status.clone())
}

#[must_use]
pub fn automatic(cx: &App) -> bool {
    cx.try_global::<Updates>()
        .is_some_and(|u| u.prefs.automatic)
}

pub fn set_automatic(on: bool, cx: &mut App) {
    let Some(u) = cx.try_global::<Updates>() else {
        return;
    };
    let mut prefs = u.prefs.clone();
    prefs.automatic = on;
    let dir = u.dir.clone();
    write_json(&dir.join("prefs.json"), &prefs);
    cx.update_global::<Updates, _>(|u, _| u.prefs = prefs);
    if on && matches!(status(cx), Status::Idle) {
        check(false, cx);
    }
}

fn set_status(status: Status, cx: &mut App) {
    if cx.has_global::<Updates>() {
        cx.update_global::<Updates, _>(|u, _| u.status = status);
    }
}

/// Checks the feed, and downloads and stages a newer release when this
/// copy can install it. `manual` reports "up to date" and errors.
pub fn check(manual: bool, cx: &mut App) {
    let Some(u) = cx.try_global::<Updates>() else {
        return;
    };
    if matches!(
        u.status,
        Status::Checking | Status::Downloading { .. } | Status::Ready { .. }
    ) {
        return;
    }
    let dir = u.dir.clone();
    let mut prefs = u.prefs.clone();
    let before = u.status.clone();
    cx.update_global::<Updates, _>(|u, _| u.manual = manual);
    set_status(Status::Checking, cx);
    let installable = install_target().is_some();
    cx.spawn(async move |cx| {
        let feed = cx
            .background_executor()
            .spawn({
                let dir = dir.clone();
                async move { fetch_feed(&dir) }
            })
            .await;
        prefs.last_check = Some(jiff::Timestamp::now().as_millisecond());
        write_json(&dir.join("prefs.json"), &prefs);
        cx.update_global::<Updates, _>(|u, _| u.prefs = prefs);
        let feed = match feed {
            Ok(f) => f,
            Err(err) => {
                eprintln!("calmfold: update check failed: {err:#}");
                let status = if manual {
                    Status::Failed {
                        message: "Couldn't check for updates. Check your connection and try again."
                            .into(),
                    }
                } else {
                    before
                };
                cx.update(|cx| set_status(status, cx));
                return;
            }
        };
        let Some(platform) = update::platform() else {
            cx.update(|cx| set_status(Status::UpToDate, cx));
            return;
        };
        let Some(download) = feed.newer_for(CURRENT, platform).cloned() else {
            cx.update(|cx| set_status(Status::UpToDate, cx));
            return;
        };
        if !installable {
            cx.update(|cx| {
                set_status(
                    Status::Available {
                        version: feed.version.clone(),
                    },
                    cx,
                )
            });
            return;
        }
        let version = feed.version.clone();
        cx.update(|cx| {
            set_status(
                Status::Downloading {
                    version: version.clone(),
                },
                cx,
            )
        });
        let staged = cx
            .background_executor()
            .spawn(async move { stage(&dir, &feed, &download) })
            .await;
        cx.update(|cx| match staged {
            Ok(notes) => set_status(Status::Ready { version, notes }, cx),
            Err(err) => {
                eprintln!("calmfold: update download failed: {err:#}");
                set_status(
                    Status::Failed {
                        message:
                            "The update couldn't be downloaded. Calmfold will try again tomorrow."
                                .into(),
                    },
                    cx,
                );
            }
        });
    })
    .detach();
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .https_only(true)
        .user_agent(concat!("Calmfold/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn get(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>> {
    let mut res = agent.get(url).call().context("request failed")?;
    res.body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .context("read failed")
}

fn fetch_feed(dir: &Path) -> Result<Feed> {
    let _lock = Lock::take(dir)?;
    let agent = agent();
    let bytes = get(&agent, update::FEED_URL, update::MAX_FEED_BYTES as u64)?;
    let sig = get(&agent, &format!("{}.minisig", update::FEED_URL), 4096)?;
    let sig = String::from_utf8(sig).context("signature is not text")?;
    Ok(Feed::verified(&bytes, &sig, update::RELEASE_KEY)?)
}

/// Downloads, verifies and unpacks the release; writes `ready.json` last.
fn stage(dir: &Path, feed: &Feed, download: &Download) -> Result<Vec<String>> {
    let _lock = Lock::take(dir)?;
    let vdir = dir.join(&feed.version);
    _ = std::fs::remove_dir_all(&vdir);
    std::fs::create_dir_all(&vdir)?;
    let bytes = get(&agent(), &download.url, update::MAX_DOWNLOAD_BYTES)?;
    update::verify_download(&bytes, download)?;
    let zip = vdir.join("download.zip");
    std::fs::write(&zip, &bytes)?;
    let staged = vdir.join("staged");
    std::fs::create_dir_all(&staged)?;
    unpack(&zip, &staged)?;
    if staged_app(dir, &feed.version).is_none() {
        bail!("the download doesn't contain Calmfold");
    }
    _ = std::fs::remove_file(&zip);
    write_json(
        &dir.join("ready.json"),
        &ReadyFile {
            version: feed.version.clone(),
            notes: feed.notes.clone(),
        },
    );
    Ok(feed.notes.clone())
}

fn unpack(zip: &Path, into: &Path) -> Result<()> {
    // The OS's own unzip keeps the app bundle's permissions and links.
    let status = if cfg!(target_os = "macos") {
        std::process::Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(zip)
            .arg(into)
            .status()?
    } else {
        std::process::Command::new("tar")
            .arg("-xf")
            .arg(zip)
            .arg("-C")
            .arg(into)
            .status()?
    };
    if !status.success() {
        bail!("couldn't unpack the update");
    }
    Ok(())
}

/// The new app inside a staged update.
fn staged_app(dir: &Path, version: &str) -> Option<PathBuf> {
    let staged = dir.join(version).join("staged");
    let path = if cfg!(target_os = "macos") {
        staged.join("Calmfold.app")
    } else {
        staged.join("Calmfold").join("Calmfold.exe")
    };
    path.exists().then_some(path)
}

fn clear_staged(dir: &Path) {
    _ = std::fs::remove_file(dir.join("ready.json"));
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.path().is_dir() {
                _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

/// What a restart replaces: the `.app` bundle on macOS, the `.exe` on
/// Windows. `None` for development runs (debug builds, `cargo run`).
fn install_target() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    if cfg!(target_os = "macos") {
        exe.ancestors()
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .map(Path::to_path_buf)
    } else if cfg!(target_os = "windows") {
        Some(exe)
    } else {
        None
    }
}

fn old_copy(target: &Path) -> PathBuf {
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    target.with_file_name(name)
}

/// Deletes the copy a past update moved aside.
fn remove_old_copies() {
    if let Some(target) = install_target() {
        let old = old_copy(&target);
        if old.is_dir() {
            _ = std::fs::remove_dir_all(&old);
        } else if old.exists() {
            _ = std::fs::remove_file(&old);
        }
    }
}

/// Swaps in the staged release and starts it. Call after saving everything;
/// on success the caller quits. A failure leaves the running copy in place.
pub fn apply(cx: &App) -> Result<()> {
    let Some(Updates {
        dir,
        status: Status::Ready { version, .. },
        ..
    }) = cx.try_global::<Updates>()
    else {
        bail!("No update is ready.");
    };
    let target = install_target().context("This copy of Calmfold can't update itself.")?;
    let new = staged_app(dir, version).context("The update is missing. It will download again.")?;
    let old = old_copy(&target);
    remove_old_copies();
    // Renaming a running app or exe is allowed on both systems; its open
    // files stay valid until it exits.
    std::fs::rename(&target, &old).context("Couldn't move the current version aside")?;
    let placed = if cfg!(target_os = "macos") {
        std::fs::rename(&new, &target)
    } else {
        std::fs::copy(&new, &target).map(|_| ())
    };
    if let Err(err) = placed {
        _ = std::fs::rename(&old, &target);
        return Err(err).context("Couldn't install the update");
    }
    let launched = if cfg!(target_os = "macos") {
        std::process::Command::new("/usr/bin/open")
            .arg("-n")
            .arg(&target)
            .spawn()
    } else {
        std::process::Command::new(&target).spawn()
    };
    if let Err(err) = launched {
        _ = std::fs::remove_dir_all(&target).or_else(|_| std::fs::remove_file(&target));
        _ = std::fs::rename(&old, &target);
        return Err(err).context("Couldn't start the new version");
    }
    clear_staged(dir);
    Ok(())
}

/// One check or download at a time, across every running copy.
struct Lock(PathBuf);

impl Lock {
    fn take(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(".lock");
        let stale = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .is_ok_and(|t| t.elapsed().is_ok_and(|e| e > LOCK_STALE));
        if stale {
            _ = std::fs::remove_file(&path);
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .context("another check is running")?;
        Ok(Self(path))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        _ = std::fs::remove_file(&self.0);
    }
}

fn read_json<T: for<'a> Deserialize<'a>>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Atomic write (temp file + rename) so a crash never leaves half a file.
fn write_json<T: Serialize>(path: &Path, value: &T) {
    let Some(dir) = path.parent() else { return };
    let tmp = path.with_extension("json.tmp");
    let ok = std::fs::create_dir_all(dir).is_ok()
        && serde_json::to_vec_pretty(value)
            .ok()
            .is_some_and(|b| std::fs::write(&tmp, b).is_ok())
        && std::fs::rename(&tmp, path).is_ok();
    if !ok {
        eprintln!("calmfold: couldn't save {}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lock_lets_one_check_run_at_a_time() {
        let dir = tempfile::tempdir().expect("dir");
        let first = Lock::take(dir.path()).expect("free");
        assert!(Lock::take(dir.path()).is_err(), "taken");
        drop(first);
        assert!(Lock::take(dir.path()).is_ok(), "released");
    }

    #[test]
    fn staged_updates_need_their_app_and_clear_cleanly() {
        let dir = tempfile::tempdir().expect("dir");
        assert_eq!(staged_app(dir.path(), "9.0.0"), None);
        let app = if cfg!(target_os = "macos") {
            dir.path().join("9.0.0/staged/Calmfold.app")
        } else {
            dir.path().join("9.0.0/staged/Calmfold/Calmfold.exe")
        };
        std::fs::create_dir_all(&app).expect("mkdir");
        assert_eq!(staged_app(dir.path(), "9.0.0"), Some(app));
        write_json(
            &dir.path().join("ready.json"),
            &ReadyFile {
                version: "9.0.0".into(),
                notes: vec![],
            },
        );
        clear_staged(dir.path());
        assert!(!dir.path().join("ready.json").exists());
        assert!(!dir.path().join("9.0.0").exists());
    }

    #[test]
    fn development_runs_never_replace_themselves() {
        // Tests are debug builds.
        assert_eq!(install_target(), None);
    }
}
