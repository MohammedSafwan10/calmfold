//! Writing files that hold secrets (the Emergency Kit, `account.json`).
//!
//! The bytes go to a new, uniquely named temp file in the same folder,
//! created readable only by the user (0600 on Unix; `tempfile` creates it
//! exclusively), then flushed and renamed over the target. The rename
//! replaces the old file rather than writing into it, so an existing target
//! with broader permissions never ends up holding the secrets
//! (docs/reviews TWO-STEP-REVIEW-2026-11-17 P2). If anything fails, the temp
//! file is removed and the target is left as it was.

use std::io::Write as _;
use std::path::Path;

pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    std::fs::create_dir_all(dir)?;
    let mut tmp = tempfile::Builder::new()
        .prefix(".ottero-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::metadata(path).expect("meta").permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn a_new_file_is_private() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("kit.html");
        write_private(&path, b"secret").expect("write");
        assert_eq!(mode(&path), 0o600);
        assert_eq!(std::fs::read(&path).expect("read"), b"secret");
    }

    #[cfg(unix)]
    #[test]
    fn overwriting_a_world_readable_file_makes_it_private() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("kit.html");
        std::fs::write(&path, b"old public file").expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        write_private(&path, b"secret").expect("write");
        assert_eq!(mode(&path), 0o600, "old 0644 permissions don't carry over");
        assert_eq!(std::fs::read(&path).expect("read"), b"secret");
    }

    #[test]
    fn no_temp_file_is_left_behind() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("account.json");
        write_private(&path, b"{}").expect("write");
        write_private(&path, b"{\"a\":1}").expect("write");
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .expect("list")
            .filter_map(Result::ok)
            .map(|e| e.file_name())
            .collect();
        assert_eq!(names, vec![std::ffi::OsString::from("account.json")]);
    }

    #[test]
    fn a_failed_write_leaves_the_target_alone() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("kit.html");
        std::fs::write(&path, b"keep me").expect("write");
        // A target that is a folder can't be replaced by a file.
        let folder = dir.path().join("taken");
        std::fs::create_dir(&folder).expect("mkdir");
        std::fs::write(folder.join("x"), b"x").expect("write");
        assert!(write_private(&folder, b"secret").is_err());
        assert_eq!(std::fs::read(&path).expect("read"), b"keep me");
        let leftovers = std::fs::read_dir(dir.path())
            .expect("list")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().starts_with(".ottero-"))
            .count();
        assert_eq!(leftovers, 0);
    }
}
