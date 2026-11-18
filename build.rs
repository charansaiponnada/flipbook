// Embed the app icon and version info in flipbook.exe. GPUI loads icon resource 1 for the
// window and taskbar, and Explorer / Task Manager read the name and version from here.
fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/windows/flipbook.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/windows/flipbook.ico")
            .set("ProductName", "Flipbook")
            .set("FileDescription", "Flipbook")
            .set("LegalCopyright", "MIT licensed");
        res.compile().expect("could not embed the Windows icon (is the Windows SDK installed?)");
    }
}
