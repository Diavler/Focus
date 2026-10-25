//! Writes the app icon as an `.ico` and embeds it in an exe. Shared by the UI's, the daemon's and
//! the native messaging host's build scripts (each includes this file and `src/icon.rs` with
//! `#[path]`), so every exe carries the same icon from one drawing.

use std::path::Path;

/// The sizes Windows picks from: tray and title bar up to Explorer's large icons.
const SIZES: [u32; 6] = [16, 24, 32, 48, 64, 256];

/// Embeds the icon in the exe being built, on Windows only. `out` is Cargo's `OUT_DIR`.
pub fn embed(out: &Path) -> std::io::Result<()> {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }
    let ico = out.join("focus.ico");
    std::fs::write(&ico, ico_file())?;
    winresource::WindowsResource::new()
        .set_icon(&ico.to_string_lossy())
        .compile()
}

/// An `.ico` holding the icon at every size in `SIZES`.
fn ico_file() -> Vec<u8> {
    let images: Vec<Vec<u8>> = SIZES.iter().map(|&size| bitmap(size)).collect();
    // Header: reserved, type 1 (icon), image count.
    let mut file = Vec::new();
    for word in [0u16, 1, SIZES.len() as u16] {
        file.extend(word.to_le_bytes());
    }
    let mut offset = 6 + 16 * SIZES.len() as u32;
    for (&size, image) in SIZES.iter().zip(&images) {
        // A side of 256 is written as 0.
        let side = if size >= 256 { 0 } else { size as u8 };
        file.extend([side, side, 0, 0]);
        file.extend(1u16.to_le_bytes()); // colour planes
        file.extend(32u16.to_le_bytes()); // bits per pixel
        file.extend((image.len() as u32).to_le_bytes());
        file.extend(offset.to_le_bytes());
        offset += image.len() as u32;
    }
    for image in images {
        file.extend(image);
    }
    file
}

/// One icon image: a 32-bit bitmap (rows bottom up, BGRA) and an all-clear AND mask.
fn bitmap(size: u32) -> Vec<u8> {
    let mut image = Vec::new();
    // BITMAPINFOHEADER; the height counts the mask too, so it's doubled.
    for field in [40, size, size * 2] {
        image.extend(field.to_le_bytes());
    }
    image.extend(1u16.to_le_bytes());
    image.extend(32u16.to_le_bytes());
    image.extend([0u8; 24]); // no compression, default sizes and palette
    let pixels = crate::icon::rgba(size);
    for row in pixels.chunks(size as usize * 4).rev() {
        for pixel in row.chunks(4) {
            image.extend([pixel[2], pixel[1], pixel[0], pixel[3]]);
        }
    }
    // The alpha channel decides transparency; the mask's rows are padded to 4 bytes.
    let mask_row = size.div_ceil(32) * 4;
    image.extend(std::iter::repeat_n(0u8, (mask_row * size) as usize));
    image
}
