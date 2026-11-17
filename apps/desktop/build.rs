//! Windows: embeds the Ottero icon (taskbar, Explorer, Alt-Tab) and the
//! version info into `ottero.exe`. Nothing to do on other platforms
//! (macOS gets its icon from the .app bundle, `scripts/bundle-mac.sh`).

fn main() {
    println!("cargo:rerun-if-changed=../../assets/brand/ottero.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/brand/ottero.ico")
            .set("ProductName", "Ottero")
            .set("FileDescription", "Ottero")
            .set("CompanyName", "Nexdark Labs")
            .set("LegalCopyright", "© Nexdark Labs");
        if let Err(err) = res.compile() {
            println!("cargo:warning=couldn't embed the Windows icon: {err}");
        }
    }
}
