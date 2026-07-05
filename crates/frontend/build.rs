//! Compile the Slint UI markup (`ui/app.slint` and its imports) into Rust,
//! surfaced in `main.rs` via `slint::include_modules!()`.

fn main() {
    slint_build::compile("ui/app.slint").expect("failed to compile Slint UI");
}
