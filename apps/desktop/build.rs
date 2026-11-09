//! Windows: embeds the Calmfold icon (taskbar, Explorer, Alt-Tab) and the
//! version info into `calmfold.exe`. Nothing to do on other platforms
//! (macOS gets its icon from the .app bundle, `scripts/bundle-mac.sh`).

fn main() {
    println!("cargo:rerun-if-changed=../../assets/brand/calmfold.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/brand/calmfold.ico")
            .set("ProductName", "Calmfold")
            .set("FileDescription", "Calmfold")
            .set("CompanyName", "Nexdark Labs")
            .set("LegalCopyright", "© Nexdark Labs");
        if let Err(err) = res.compile() {
            println!("cargo:warning=couldn't embed the Windows icon: {err}");
        }
    }
}
