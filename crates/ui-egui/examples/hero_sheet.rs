//! Contact sheet of every hero in one state, rasterised on the CPU (no GPU needed), for quick
//! design iteration. The GPU snapshots come from `tests/ux_heroes.rs`.
//!
//! `cargo run -p cleancraft-ui-egui --example hero_sheet -- <idle|scanning|burst|done> out.png [ppp]`

use cleancraft_core::FeatureId;
use cleancraft_ui_egui::heroes::{self, HeroPhase, HeroState};
use cleancraft_ui_egui::render3d;
use egui::{Rect, pos2, vec2};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let which = args.get(1).map(String::as_str).unwrap_or("idle");
    let out = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "hero-sheet.png".into());
    let ppp: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let (phase, progress, since) = match which {
        "scanning" => (HeroPhase::Scanning, 0.55, 1.6),
        "burst" => (HeroPhase::Done, 1.0, 0.32),
        "done" => (HeroPhase::Done, 1.0, 4.0),
        _ => (HeroPhase::Idle, 0.0, 2.0),
    };
    let cell = 200.0;
    let (w, h) = ((cell * 5.0 * ppp) as usize, (cell * 3.0 * ppp) as usize);
    let mut all = egui::Mesh::default();
    // Opaque page ink first, so additive glows have something to add to.
    all.add_colored_rect(
        Rect::from_min_size(pos2(0.0, 0.0), vec2(cell * 5.0, cell * 3.0)),
        egui::Color32::from_rgb(0x0E, 0x13, 0x1B),
    );
    for (i, f) in FeatureId::ALL.into_iter().enumerate() {
        let r = Rect::from_min_size(
            pos2((i % 5) as f32 * cell, (i / 5) as f32 * cell),
            vec2(cell, cell),
        );
        let state = HeroState {
            phase,
            progress,
            t: 2.4,
            since,
        };
        let (m, _) = heroes::mesh(f, state, r.shrink(14.0), ppp);
        all.append(m);
    }
    let px = render3d::rasterize(&all, w, h, ppp, 1);
    let img = image::RgbaImage::from_raw(w as u32, h as u32, px).expect("image size");
    img.save(&out).expect("write png");
    println!("wrote {out}");
}
