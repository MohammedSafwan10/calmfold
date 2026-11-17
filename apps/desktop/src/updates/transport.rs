//! Authenticated update metadata and bounded package transport; no installation logic.
use anyhow::{Context as _, Result};
#[cfg(any(windows, test))]
use ottero_core::update::Download;
use ottero_core::update::{self, Feed};
#[cfg(any(windows, test))]
use std::io::{Read, Write};
use std::time::Duration;
use ureq::ResponseExt as _;

pub const FEED_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(windows)]
pub const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .https_only(true)
        .user_agent(concat!("Ottero/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}
pub fn get(agent: &ureq::Agent, url: &str, limit: u64) -> Result<Vec<u8>> {
    Ok(agent
        .get(url)
        .call()?
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()?)
}
pub fn fetch() -> Result<Feed> {
    let agent = agent(FEED_TIMEOUT);
    let response = agent.head(update::RELEASES_LATEST_URL).call()?;
    let url = update::feed_url_for_release(&response.get_uri().to_string())
        .context("invalid release location")?;
    fetch_signed(&agent, &url, update::RELEASE_KEY)
}
pub fn fetch_signed(agent: &ureq::Agent, url: &str, key: &str) -> Result<Feed> {
    let bytes = get(agent, url, update::MAX_FEED_BYTES as u64)?;
    let signature = String::from_utf8(get(agent, &format!("{url}.minisig"), 4096)?)?;
    Ok(Feed::verified(&bytes, &signature, key)?)
}
#[cfg(any(windows, test))]
pub(crate) fn copy_verified(
    mut reader: impl Read,
    mut writer: impl Write,
    expected: &Download,
) -> Result<()> {
    let mut verifier = update::DownloadVerifier::new(expected)?;
    let mut chunk = [0_u8; 32 * 1024];
    loop {
        let size = match reader.read(&mut chunk) {
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result.context("couldn't read the update")?,
        };
        if size == 0 {
            break;
        }
        verifier.update(&chunk[..size])?;
        writer
            .write_all(&chunk[..size])
            .context("couldn't store the update")?;
    }
    verifier.finish()?;
    writer.flush().context("couldn't flush the update")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hello() -> Download {
        Download {
            url: "https://example.com/package.nupkg".into(),
            size: 5,
            sha256: "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into(),
        }
    }

    #[test]
    fn package_transfer_rejects_corruption_truncation_and_extra_bytes() {
        let mut out = Vec::new();
        copy_verified(b"hello".as_slice(), &mut out, &hello()).expect("verified");
        assert_eq!(out, b"hello");
        for bytes in [b"hell".as_slice(), b"hellO", b"hello!"] {
            let mut out = Vec::new();
            assert!(copy_verified(bytes, &mut out, &hello()).is_err());
            assert!(out.len() <= 5, "never write beyond authenticated size");
        }
    }

    #[test]
    fn package_transfer_propagates_disk_failure() {
        struct Full;
        impl Write for Full {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        assert!(copy_verified(b"hello".as_slice(), Full, &hello()).is_err());
    }
}
