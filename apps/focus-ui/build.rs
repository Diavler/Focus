//! Gives the Windows exe the app icon (see `ico.rs`). Other targets get nothing.

use std::path::PathBuf;

mod ico;
#[path = "src/icon.rs"]
mod icon;

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=src/icon.rs");
    println!("cargo:rerun-if-changed=ico.rs");
    let out = PathBuf::from(std::env::var("OUT_DIR").map_err(std::io::Error::other)?);
    ico::embed(&out)
}
