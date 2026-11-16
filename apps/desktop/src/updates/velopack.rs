//! Velopack installation with an authenticated catalog and bounded transport.
//! Never use HttpSource's unsigned feed or its per-install staging-ID query.
use std::path::Path;
use std::sync::mpsc::Sender;

use anyhow::{Context as _, Result, ensure};
use calmfold_core::update::{Download, Feed};
use velopack::bundle::Manifest;
use velopack::sources::UpdateSource;
use velopack::{UpdateCheck, UpdateInfo, UpdateManager, VelopackAsset, VelopackAssetFeed};

use super::transport;

#[derive(Clone)]
pub struct SignedSource {
    asset: VelopackAsset,
    download: Download,
    agent: ureq::Agent,
}

impl SignedSource {
    /// `feed` must come from transport::fetch_signed, never unsigned JSON.
    pub fn new(feed: &Feed, agent: ureq::Agent) -> Result<Self> {
        let download = feed
            .platforms
            .get("windows-x64")
            .context("missing Windows package")?
            .clone();
        ensure!(download.is_valid(), "invalid signed package metadata");
        let uri: ureq::http::Uri = download.url.parse()?;
        let name = uri
            .path()
            .rsplit('/')
            .next()
            .context("missing package name")?;
        ensure!(
            name.ends_with("-full.nupkg")
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
            "invalid package name"
        );
        let asset = VelopackAsset {
            PackageId: "Calmfold".into(),
            Version: feed.version.clone(),
            Type: "Full".into(),
            FileName: name.into(),
            SHA256: download.sha256.clone(),
            Size: download.size,
            NotesMarkdown: feed.notes.join("\n"),
            ..VelopackAsset::default()
        };
        Ok(Self {
            asset,
            download,
            agent,
        })
    }
}

impl UpdateSource for SignedSource {
    fn get_release_feed(
        &self,
        _: &str,
        _: &Manifest,
        _: &str,
    ) -> Result<VelopackAssetFeed, velopack::Error> {
        Ok(VelopackAssetFeed {
            Assets: vec![self.asset.clone()],
        })
    }

    fn download_release_entry(
        &self,
        asset: &VelopackAsset,
        local_file: &Path,
        progress: Option<Sender<i16>>,
    ) -> Result<(), velopack::Error> {
        let result = (|| -> Result<()> {
            ensure!(
                asset.FileName == self.asset.FileName
                    && asset.SHA256 == self.download.sha256
                    && asset.Size == self.download.size
                    && asset.Version == self.asset.Version,
                "package is not in the verified catalog"
            );
            let mut response = self.agent.get(&self.download.url).call()?;
            let mut file = std::fs::File::create(local_file)?;
            transport::copy_verified(response.body_mut().as_reader(), &mut file, &self.download)?;
            file.sync_all()?;
            if let Some(progress) = progress {
                _ = progress.send(100);
            }
            Ok(())
        })();
        if result.is_err() {
            _ = std::fs::remove_file(local_file);
        }
        result.map_err(|_| {
            velopack::Error::Other("The package couldn't be downloaded or verified.".into())
        })
    }
}

#[derive(Clone)]
pub struct Ready {
    manager: UpdateManager,
    info: UpdateInfo,
}

pub fn download(feed: &Feed) -> Result<Option<Ready>> {
    let source = SignedSource::new(feed, transport::agent(transport::DOWNLOAD_TIMEOUT))?;
    let manager = match UpdateManager::new(source, None, None) {
        Ok(manager) => manager,
        Err(velopack::Error::NotInstalled(_)) => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let UpdateCheck::UpdateAvailable(info) = manager.check_for_updates()? else {
        return Ok(None);
    };
    manager.download_updates(&info, None)?;
    Ok(Some(Ready {
        manager,
        info: *info,
    }))
}

pub fn restart(ready: &Ready) -> Result<()> {
    // Let GPUI flush and quit normally; never use the SDK's exit(0) convenience API.
    ready
        .manager
        .wait_exit_then_apply_updates(&ready.info, true, true, Vec::<String>::new())?;
    Ok(())
}
