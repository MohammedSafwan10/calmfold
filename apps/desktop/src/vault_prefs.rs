//! Vault preferences (`vault.json` in the data folder): auto-lock delay,
//! clipboard clearing and the generator's last options. No secrets here.

use std::path::{Path, PathBuf};

use gpui_kit::{App, Global};
use keepstone_core::vault::generator::Options;
use keepstone_core::vault::lock::{AutoLock, ClipboardClear};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VaultPrefs {
    pub auto_lock: AutoLock,
    pub clipboard: ClipboardClear,
    /// Lock when Keepstone goes to the background.
    pub lock_on_hide: bool,
    pub generator: Options,
}

struct PrefsState {
    prefs: VaultPrefs,
    file: PathBuf,
}

impl Global for PrefsState {}

fn load(path: &Path) -> VaultPrefs {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<VaultPrefs>(&t).ok())
        .map(|mut p| {
            p.generator = p.generator.clamped();
            p
        })
        .unwrap_or_default()
}

pub fn init(data_dir: &Path, cx: &mut App) {
    let file = data_dir.join("vault.json");
    cx.set_global(PrefsState {
        prefs: load(&file),
        file,
    });
}

#[must_use]
pub fn get(cx: &App) -> VaultPrefs {
    cx.try_global::<PrefsState>()
        .map(|s| s.prefs)
        .unwrap_or_default()
}

pub fn update(cx: &mut App, edit: impl FnOnce(&mut VaultPrefs)) {
    let Some(state) = cx.try_global::<PrefsState>() else {
        return;
    };
    let mut prefs = state.prefs;
    edit(&mut prefs);
    prefs.generator = prefs.generator.clamped();
    let file = state.file.clone();
    cx.set_global(PrefsState {
        prefs,
        file: file.clone(),
    });
    let write = serde_json::to_vec_pretty(&prefs)
        .map_err(std::io::Error::other)
        .and_then(|json| {
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir)?;
            }
            let tmp = file.with_extension("json.tmp");
            std::fs::write(&tmp, json)?;
            std::fs::rename(tmp, &file)
        });
    if let Err(err) = write {
        eprintln!("keepstone: couldn't save vault settings: {err}");
    }
    cx.refresh_windows();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_bad_files_give_defaults() {
        let dir = tempfile::tempdir().expect("dir");
        assert_eq!(load(&dir.path().join("nope.json")), VaultPrefs::default());
        let bad = dir.path().join("vault.json");
        std::fs::write(&bad, "{ nope").expect("write");
        assert_eq!(load(&bad), VaultPrefs::default());
        std::fs::write(
            &bad,
            r#"{"auto_lock":"one_hour","generator":{"length":200}}"#,
        )
        .expect("write");
        let p = load(&bad);
        assert_eq!(p.auto_lock, AutoLock::OneHour);
        assert_eq!(p.generator.length, 64, "clamped");
    }
}
