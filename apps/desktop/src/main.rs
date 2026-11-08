// Hide the console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    if let Err(err) = keepstone_desktop::run() {
        eprintln!("keepstone: {err:#}");
        std::process::exit(1);
    }
}
