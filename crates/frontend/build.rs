//! Compile the Slint UI markup (`ui/app.slint` and its imports) into Rust,
//! surfaced in `main.rs` via `slint::include_modules!()`.

fn main() {
    slint_build::compile("ui/app.slint").expect("failed to compile Slint UI");
    embed_windows_metadata();
}

#[cfg(target_os = "windows")]
fn embed_windows_metadata() {
    let mut res = winresource::WindowsResource::new();
    res.set("FileDescription", "Swanium");
    res.set("ProductName", "Swanium");
    res.set("OriginalFilename", "Swanium.exe");
    res.set("LegalCopyright", "Copyright © 2026 Bubio");
    res.compile()
        .expect("failed to embed Windows executable metadata");
}

#[cfg(not(target_os = "windows"))]
fn embed_windows_metadata() {}
