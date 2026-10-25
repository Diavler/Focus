//! The app icon, the focus ring: an amber ring and centre dot on a dark rounded tile. Drawn in
//! code so the window and the exe share one source; `build.rs` includes this file for the exe.

/// The tile's colour.
const TILE: [u8; 3] = [0x2B, 0x2A, 0x26];

/// The ring's and the dot's colour.
const AMBER: [u8; 3] = [0xF2, 0xA3, 0x3A];

/// Samples per pixel along each side, for smooth edges.
const SAMPLES: u32 = 4;

/// The icon as `size`×`size` RGBA pixels (not premultiplied), row by row from the top.
pub fn rgba(size: u32) -> Vec<u8> {
    let unit = 64.0 / size as f32;
    let step = 1.0 / SAMPLES as f32;
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (mut sum, mut covered) = ([0u32; 3], 0u32);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let at_x = (x as f32 + (sx as f32 + 0.5) * step) * unit;
                    let at_y = (y as f32 + (sy as f32 + 0.5) * step) * unit;
                    if let Some(colour) = colour_at(at_x, at_y) {
                        covered += 1;
                        for (total, channel) in sum.iter_mut().zip(colour) {
                            *total += u32::from(channel);
                        }
                    }
                }
            }
            if covered == 0 {
                pixels.extend([0; 4]);
                continue;
            }
            // Each channel is an average of bytes and the alpha a share of 255, so all fit a byte.
            pixels.extend(sum.map(|total| (total / covered) as u8));
            pixels.push((255 * covered / (SAMPLES * SAMPLES)) as u8);
        }
    }
    pixels
}

/// The colour at (`x`, `y`) in the 64-unit design, or `None` outside the tile.
fn colour_at(x: f32, y: f32) -> Option<[u8; 3]> {
    // The tile spans 2..62 with corners of radius 14.
    let corner = |at: f32| ((at - 32.0).abs() - 16.0).max(0.0);
    if corner(x).hypot(corner(y)) > 14.0 {
        return None;
    }
    let from_centre = (x - 32.0).hypot(y - 32.0);
    let on_ring = (from_centre - 18.0).abs() <= 3.0;
    Some(if on_ring || from_centre <= 6.0 {
        AMBER
    } else {
        TILE
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(pixels: &[u8], size: u32, x: u32, y: u32) -> &[u8] {
        let at = ((y * size + x) * 4) as usize;
        &pixels[at..at + 4]
    }

    #[test]
    fn corners_are_clear_and_the_centre_is_amber() {
        let pixels = rgba(64);
        assert_eq!(pixels.len(), 64 * 64 * 4);
        assert_eq!(pixel(&pixels, 64, 0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&pixels, 64, 32, 32), [0xF2, 0xA3, 0x3A, 255]);
        // Between the dot and the ring is the tile.
        assert_eq!(pixel(&pixels, 64, 32, 21), [0x2B, 0x2A, 0x26, 255]);
    }

    #[test]
    fn small_sizes_still_show_the_ring() {
        let pixels = rgba(16);
        // The ring passes through (8, 3) at 16 px: mostly amber.
        assert!(pixel(&pixels, 16, 8, 3)[0] > 0xB0);
    }
}
