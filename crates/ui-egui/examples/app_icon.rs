//! Renders the CleanCraft app icon with the 3D renderer into `assets/AppIcon.png`
//! (1024 x 1024, 4 x 4 supersampled on the CPU).
//!
//! `cargo run -p cleancraft-ui-egui --example app_icon [-- out.png]`

use cleancraft_ui_egui::{heroes, render3d};

fn main() {
    let size = 1024usize;
    let ss = 4;
    let out = std::env::args().nth(1).unwrap_or_else(|| {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/AppIcon.png").to_string()
    });
    let mesh = heroes::app_icon(size as f32, ss as f32);
    let px = render3d::rasterize(&mesh, size, size, 1.0, ss);
    let img = image::RgbaImage::from_raw(size as u32, size as u32, px).expect("image size");
    img.save(&out).expect("write png");
    println!("wrote {out}");
}
