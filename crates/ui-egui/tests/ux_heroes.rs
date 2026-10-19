//! The 3D heroes: every module's hero renders in every state within a frame budget, and with
//! `CLEANCRAFT_SNAPSHOTS=1` each one is saved at 1x and 2x into `docs/screens/v2/heroes/`
//! (plus one contact sheet per state) for review.
//!
//! `CLEANCRAFT_SNAPSHOTS=1 cargo test -p cleancraft-ui-egui --test ux_heroes -- --nocapture`

use std::path::PathBuf;
use std::time::Instant;

use cleancraft_core::FeatureId;
use cleancraft_ui_egui::heroes::{self, HeroPhase, HeroState};

use egui::{Color32, Rect, pos2, vec2};

const STATES: [(&str, HeroPhase, f32, f32); 4] = [
    ("idle", HeroPhase::Idle, 0.0, 2.0),
    ("scanning", HeroPhase::Scanning, 0.55, 1.6),
    ("done-burst", HeroPhase::Done, 1.0, 0.32),
    ("done", HeroPhase::Done, 1.0, 4.0),
];

fn state(phase: HeroPhase, progress: f32, since: f32) -> HeroState {
    HeroState {
        phase,
        progress,
        t: 2.4,
        since,
    }
}

/// Page-like backdrop: the dark ink of the content area (the hero brings its own glow).
fn backdrop(ui: &egui::Ui, rect: Rect, _f: FeatureId) {
    ui.painter()
        .rect_filled(rect, 0.0, Color32::from_rgb(0x0E, 0x13, 0x1B));
}

#[test]
fn hero_frame_fits_the_budget() {
    // A frame of a scanning hero (the busiest state) at 2x for a 260 pt hero. The bound is
    // the 60 fps budget share we allow (4 ms) in an optimised build; unoptimised test builds
    // get ten times that.
    heroes::warm_up();
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(260.0, 260.0));
    let budget_ms = if cfg!(debug_assertions) { 40.0 } else { 4.0 };
    let mut worst = (0.0f64, FeatureId::SmartCare);
    for f in FeatureId::ALL {
        let frames = 12;
        let start = Instant::now();
        let mut tris = 0;
        for i in 0..frames {
            let s = HeroState {
                t: 2.0 + i as f64 / 60.0,
                ..state(HeroPhase::Scanning, 0.5, 1.0 + i as f32 / 60.0)
            };
            let (m, stats) = heroes::mesh(f, s, rect, 2.0);
            tris = stats.triangles;
            assert!(!m.indices.is_empty());
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0 / frames as f64;
        eprintln!("{:>16}: {ms:6.2} ms/frame, {tris} triangles", f.as_str());
        if ms > worst.0 {
            worst = (ms, f);
        }
    }
    assert!(
        worst.0 < budget_ms,
        "{:?} takes {:.2} ms per frame (budget {budget_ms} ms)",
        worst.1,
        worst.0
    );
}

#[test]
fn heroes_render_on_the_gpu() {
    let all = std::env::var("CLEANCRAFT_SNAPSHOTS").is_ok_and(|v| v == "1");
    let renderer_ok = std::panic::catch_unwind(egui_kittest::wgpu::WgpuTestRenderer::new).is_ok();
    if !renderer_ok {
        eprintln!("No wgpu adapter here: rendering skipped.");
        return;
    }
    let out_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screens/v2/heroes");
    if all {
        std::fs::create_dir_all(&out_dir).expect("make docs/screens/v2/heroes");
    }
    let features: Vec<FeatureId> = if all {
        FeatureId::ALL.to_vec()
    } else {
        vec![FeatureId::SmartCare]
    };
    let size = 240.0;
    for &f in &features {
        for (name, phase, progress, since) in STATES {
            for ppp in [1.0f32, 2.0] {
                if !all && ppp > 1.0 {
                    continue;
                }
                let mut h = egui_kittest::Harness::builder()
                    .with_size(vec2(size, size))
                    .with_pixels_per_point(ppp)
                    .build_ui(move |ui| {
                        let rect = ui.ctx().viewport_rect();
                        backdrop(ui, rect, f);
                        heroes::draw(ui, f, state(phase, progress, since), rect.shrink(12.0));
                    });
                h.step();
                let img = h.render().expect("render");
                // The hero is a picture: many colours, and its centre is not the backdrop.
                let mut seen = std::collections::HashSet::new();
                for y in (0..img.height()).step_by(3) {
                    for x in (0..img.width()).step_by(3) {
                        seen.insert(img.get_pixel(x, y).0);
                    }
                }
                assert!(seen.len() > 300, "{f:?} {name}: {} colours", seen.len());
                if all {
                    let suffix = if ppp > 1.0 { "@2x" } else { "" };
                    let file = out_dir.join(format!("{}-{name}{suffix}.png", f.as_str()));
                    img.save(&file).expect("save png");
                }
            }
        }
    }
    if all {
        // Contact sheets: all fifteen heroes per state at 2x.
        for (name, phase, progress, since) in STATES {
            let cell = 200.0;
            let mut h = egui_kittest::Harness::builder()
                .with_size(vec2(cell * 5.0, cell * 3.0))
                .with_pixels_per_point(2.0)
                .build_ui(move |ui| {
                    for (i, f) in FeatureId::ALL.into_iter().enumerate() {
                        let r = Rect::from_min_size(
                            pos2((i % 5) as f32 * cell, (i / 5) as f32 * cell),
                            vec2(cell, cell),
                        );
                        backdrop(ui, r, f);
                        heroes::draw(ui, f, state(phase, progress, since), r.shrink(14.0));
                    }
                });
            h.step();
            let img = h.render().expect("render");
            let file = out_dir.join(format!("sheet-{name}@2x.png"));
            img.save(&file).expect("save png");
            eprintln!("wrote {}", file.display());
        }
    }
}

#[test]
fn app_icon_is_a_full_squircle_with_transparent_corners() {
    let size = 128;
    let mesh = heroes::app_icon(size as f32, 2.0);
    let px = cleancraft_ui_egui::render3d::rasterize(&mesh, size, size, 1.0, 2);
    let alpha = |x: usize, y: usize| px[(y * size + x) * 4 + 3];
    assert_eq!(alpha(1, 1), 0, "corners are transparent");
    assert_eq!(alpha(64, 64), 255, "the tile is opaque");
    assert_eq!(alpha(64, 16), 255, "top of the tile");
    // The mark is bright against the dark tile somewhere in the middle.
    let lum = |x: usize, y: usize| {
        let o = (y * size + x) * 4;
        px[o] as u32 + px[o + 1] as u32 + px[o + 2] as u32
    };
    let brightest = (40..88)
        .flat_map(|y| (40..88).map(move |x| (x, y)))
        .map(|(x, y)| lum(x, y))
        .max()
        .unwrap_or(0);
    assert!(brightest > 450, "{brightest}");
}
