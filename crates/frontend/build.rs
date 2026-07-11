//! Compile the Slint UI markup (`ui/app.slint` and its imports) into Rust,
//! surfaced in `main.rs` via `slint::include_modules!()`.

fn main() {
    slint_build::compile("ui/app.slint").expect("failed to compile Slint UI");
    embed_windows_metadata();
}

#[cfg(target_os = "windows")]
fn embed_windows_metadata() {
    use std::env;
    use std::fs::File;
    use std::path::PathBuf;

    use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
    use image::imageops::FilterType;

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let icon_png = manifest_dir.join("../../assets/icons/AppIcon.png");
    let icon_ico = out_dir.join("swanium.ico");

    println!("cargo:rerun-if-changed={}", icon_png.display());

    let base = image::open(&icon_png)
        .expect("failed to load AppIcon.png")
        .into_rgba8();
    let mut icon_dir = IconDir::new(ResourceType::Icon);
    for size in [256_u32, 128, 64, 48, 32, 16] {
        let resized = image::imageops::resize(&base, size, size, FilterType::Lanczos3);
        let image = IconImage::from_rgba_data(size, size, resized.into_raw());
        let entry = IconDirEntry::encode(&image).expect("failed to encode icon entry");
        icon_dir.add_entry(entry);
    }
    let mut file = File::create(&icon_ico).expect("failed to create .ico file");
    icon_dir
        .write(&mut file)
        .expect("failed to write .ico file");

    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon_ico.to_string_lossy().as_ref());
    res.set("FileDescription", "Swanium");
    res.set("ProductName", "Swanium");
    res.set("OriginalFilename", "Swanium.exe");
    res.set("LegalCopyright", "Copyright © 2026 Bubio");
    res.compile()
        .expect("failed to embed Windows executable metadata");
}

#[cfg(not(target_os = "windows"))]
fn embed_windows_metadata() {}
