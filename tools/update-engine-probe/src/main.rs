//! Non-installing API smoke probe. Run as an unbundled binary on the main thread.
//! Never checks a network feed, packages an app or replaces any executable.
use gpui_updater_sparkle::{SparkleDriver, SparkleOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match SparkleDriver::load(SparkleOptions::new())? {
        None => println!("PASS: Sparkle declines an unbundled development process"),
        Some(_) => return Err("Run this probe outside an app bundle".into()),
    }
    #[cfg(target_os = "macos")]
    {
        let rejected = std::thread::spawn(|| {
            matches!(
                SparkleDriver::load(SparkleOptions::new()),
                Err(gpui_updater_sparkle::Error::NotMainThread)
            )
        })
        .join()
        .map_err(|_| "Sparkle worker panicked")?;
        if !rejected {
            return Err("Sparkle must reject loading off the main thread".into());
        }
        println!("PASS: Sparkle enforces main-thread initialization");
    }
    let source = velopack::sources::HttpSource::new("https://example.invalid/calmfold-probe");
    // Construction only locates installation metadata. No feed request is made.
    match velopack::UpdateManager::new(source, None, None) {
        Err(velopack::Error::NotInstalled(_)) => {}
        Err(err) => return Err(err.into()),
        Ok(_) => return Err("Run this probe outside a Velopack installation".into()),
    }
    println!("PASS: Velopack declines an uninstalled development process");
    println!("Not tested: downloading, signatures, installation, relaunch or Windows execution");
    Ok(())
}
