//! Gives the native messaging host's Windows exe the app icon, so it's recognisable in Task
//! Manager under the browser. The drawing and the `.ico` writer are the UI's (`apps/focus-ui`),
//! included here so there's one copy.

use std::path::PathBuf;

#[path = "../focus-ui/ico.rs"]
mod ico;
#[path = "../focus-ui/src/icon.rs"]
mod icon;

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=../focus-ui/src/icon.rs");
    println!("cargo:rerun-if-changed=../focus-ui/ico.rs");
    let out = PathBuf::from(std::env::var("OUT_DIR").map_err(std::io::Error::other)?);
    ico::embed(&out)
}
