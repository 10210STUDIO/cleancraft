//! Test helpers for the file features: generated pictures that look alike or different.
//! Writes only where the caller says (a FakeMac path).

use std::path::Path;

pub use image::ImageFormat;
use image::{Rgb, RgbImage};

/// A scene: smooth shapes plus a little noise, so files are big and the difference hash is
/// stable. `kind` 0 and 1 are two different scenes; `brighten` and `seed` vary a copy.
pub fn scene(w: u32, h: u32, kind: u32, brighten: i32, seed: u32) -> RgbImage {
    let mut x = seed.wrapping_mul(2654435761).wrapping_add(1);
    RgbImage::from_fn(w, h, |px, py| {
        let u = px as f32 / w as f32;
        let v = py as f32 / h as f32;
        let base = match kind {
            0 => 128.0 + 90.0 * (u * 9.0 + v * 4.0).sin() * (v * 3.0).cos(),
            _ => 128.0 + 90.0 * (v * 11.0 - u * 2.0).cos() * (u * 7.0).sin(),
        };
        x = x.wrapping_mul(1103515245).wrapping_add(12345);
        let noise = ((x >> 16) % 7) as i32 - 3;
        let c = (base as i32 + brighten + noise).clamp(0, 255) as u8;
        Rgb([c, c.wrapping_add(((x >> 8) % 5) as u8), c])
    })
}

/// Save a scene at `p` in `fmt`, creating folders.
pub fn save_scene(p: &Path, w: u32, h: u32, kind: u32, brighten: i32, seed: u32, fmt: ImageFormat) {
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = scene(w, h, kind, brighten, seed).save_with_format(p, fmt);
}
