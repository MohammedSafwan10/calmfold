// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    velopack::VelopackApp::build().run();
    if let Err(err) = calmfold_desktop::run() {
        eprintln!("calmfold: {err:#}");
        std::process::exit(1);
    }
}
