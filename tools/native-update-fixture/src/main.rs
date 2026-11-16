//! Real disposable Velopack app; never packaged as Calmfold or given real data.
#![allow(dead_code)] // Shared production modules also contain entry points the fixture does not use.
#[cfg(windows)]
#[path = "../../../apps/desktop/src/updates/velopack.rs"]
mod native_windows;
#[path = "../../../apps/desktop/src/updates/transport.rs"]
mod transport;

#[cfg(windows)]
use anyhow::Context as _;
use anyhow::Result;

fn main() -> Result<()> {
    velopack::VelopackApp::build().run();
    #[cfg(windows)]
    {
        windows()
    }
    #[cfg(not(windows))]
    {
        println!("Installed Velopack tests require a native Windows runner.");
        Ok(())
    }
}

#[cfg(windows)]
fn windows() -> Result<()> {
    use std::io::Write as _;
    let args = std::env::args().collect::<Vec<_>>();
    let root = args
        .windows(2)
        .find(|a| a[0] == "--test-root")
        .map(|a| a[1].clone())
        .or_else(|| std::env::var("CALMFOLD_NATIVE_TEST_ROOT").ok())
        .context("fixture root")?;
    let root = std::path::PathBuf::from(root);
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("app.lock"))?;
    lock.try_lock().context("fixture is already running")?;
    let manager = velopack::UpdateManager::new(velopack::sources::NoneSource {}, None, None)?;
    let version = manager.get_current_version_as_string();
    let mut log = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(root.join("started.log"))?;
    writeln!(log, "{version} {}", std::process::id())?;
    log.flush()?;
    let mut attempted = false;
    while !root.join("stop").exists() {
        if !attempted && root.join("check").exists() {
            attempted = true;
            // Only this disposable fixture permits its own self-signed loopback
            // certificate. Production uses the default strict TLS agent.
            let tls = ureq::tls::TlsConfig::builder()
                .disable_verification(true)
                .build();
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .tls_config(tls)
                .https_only(true)
                .build()
                .into();
            let url = std::fs::read_to_string(root.join("feed-url"))?;
            let key = std::fs::read_to_string(root.join("feed-key"))?;
            let result = (|| -> Result<()> {
                let feed = transport::fetch_signed(&agent, url.trim(), key.trim())?;
                if !calmfold_core::update::is_newer(&feed.version, &version) {
                    return Ok(());
                }
                let source = native_windows::SignedSource::new(&feed, agent)?;
                let manager = velopack::UpdateManager::new(source, None, None)?;
                let velopack::UpdateCheck::UpdateAvailable(info) = manager.check_for_updates()?
                else {
                    anyhow::bail!("fixture update missing");
                };
                manager.download_updates(&info, None)?;
                std::fs::write(root.join("ready"), b"verified")?;
                while !root.join("install").exists() {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                manager.wait_exit_then_apply_updates(
                    &info,
                    true,
                    true,
                    ["--test-root", root.to_str().context("fixture path")?],
                )?;
                std::process::exit(0);
            })();
            if let Err(err) = result {
                std::fs::write(root.join("rejected"), err.to_string())?;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(())
}
