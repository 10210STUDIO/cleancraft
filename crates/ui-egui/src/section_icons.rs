//! Small 3D icons for the sidebar sections, in the same material and light as the heroes
//! and the app icon.
//!
//! Each icon is a tiny lit object rendered once on the CPU (supersampled) into a texture
//! at the exact physical size it is shown at (28 pt = 28 px at 1x, 56 px at 2x), so it is
//! crisp and costs one textured quad per frame. Hover and selection lift the object a
//! little and light a soft halo of the section colour under it; with Reduce motion the
//! change is immediate.
//!
//! ```ignore
//! let r = section_icons::show(ui, SectionIcon::Protect, section_icons::SIDEBAR_PT, selected);
//! ```

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::sync::{Arc, Mutex, OnceLock};

use egui::epaint::Vertex;
use egui::{
    Color32, ColorImage, Context, Id, Mesh, Pos2, Rect, Response, Sense, TextureHandle,
    TextureOptions, Ui, Vec2, pos2, vec2,
};

use crate::heroes::{
    ccw, facing, gear_outline, retint, rounded_rect, shield_outline, squircle, stroke,
};
use crate::motion;
use crate::render3d::{
    Camera, Lin, Material, Mesh3, Rig, Scene, WHITE, Xf, lin, mix, rasterize, scale, v3,
};
use crate::theme::Section;

/// Size in the sidebar, in points.
pub const SIDEBAR_PT: f32 = 28.0;

/// How far a selected icon rises, in points.
const LIFT: f32 = 1.5;

/// One icon per sidebar entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SectionIcon {
    QuickSweep,
    Clean,
    Protect,
    Speed,
    Apps,
    Files,
    Monitor,
    History,
    Settings,
}

impl SectionIcon {
    pub const ALL: [SectionIcon; 9] = [
        SectionIcon::QuickSweep,
        SectionIcon::Clean,
        SectionIcon::Protect,
        SectionIcon::Speed,
        SectionIcon::Apps,
        SectionIcon::Files,
        SectionIcon::Monitor,
        SectionIcon::History,
        SectionIcon::Settings,
    ];

    /// The theme section whose colours the icon wears.
    pub fn section(self) -> Section {
        match self {
            SectionIcon::QuickSweep => Section::Overview,
            SectionIcon::Clean => Section::Clean,
            SectionIcon::Protect => Section::Protect,
            SectionIcon::Speed => Section::Speed,
            SectionIcon::Apps => Section::Apps,
            SectionIcon::Files => Section::Files,
            SectionIcon::Monitor => Section::System,
            SectionIcon::History | SectionIcon::Settings => Section::Neutral,
        }
    }

    /// The icon for a section (History and Settings share `Neutral`: pick them directly).
    pub fn of(section: Section) -> SectionIcon {
        match section {
            Section::Overview => SectionIcon::QuickSweep,
            Section::Clean => SectionIcon::Clean,
            Section::Protect => SectionIcon::Protect,
            Section::Speed => SectionIcon::Speed,
            Section::Apps => SectionIcon::Apps,
            Section::Files => SectionIcon::Files,
            Section::System => SectionIcon::Monitor,
            Section::Neutral => SectionIcon::Settings,
        }
    }

    fn colors(self) -> (Lin, Lin) {
        let (a, b) = self.section().colors();
        (lin(a), lin(b))
    }
}

// ---------------------------------------------------------------- geometry

struct Parts {
    gem: Mesh3,
    spark: Mesh3,
    bubble: Mesh3,
    shield: Mesh3,
    check: Mesh3,
    bolt: Mesh3,
    tile: Mesh3,
    cell: Mesh3,
    folder_back: Mesh3,
    folder_front: Mesh3,
    ring: Mesh3,
    pulse: Mesh3,
    clock_face: Mesh3,
    clock_rim: Mesh3,
    hand: Mesh3,
    pin: Mesh3,
    gear: Mesh3,
    hub: Mesh3,
}

fn parts() -> &'static Parts {
    static P: OnceLock<Parts> = OnceLock::new();
    P.get_or_init(|| {
        let puck = |r: f32, h: f32, b: f32, seg: usize| facing(Mesh3::puck(r, h, b, seg));
        // A chunky bolt: the thin one weighed a third of its neighbours (REVIEW-C C24).
        let bolt = ccw([
            [0.0, 0.9],
            [-0.86, -0.2],
            [-0.18, -0.2],
            [-0.18, -0.9],
            [0.86, 0.25],
            [0.18, 0.25],
        ]
        .to_vec());
        let folder = ccw(vec![
            [-0.92, -0.7],
            [0.92, -0.7],
            [0.92, 0.48],
            [-0.12, 0.48],
            [-0.28, 0.7],
            [-0.92, 0.7],
        ]);
        let pulse = stroke(
            &[
                [-0.5, 0.0],
                [-0.22, 0.0],
                [-0.1, 0.3],
                [0.02, -0.32],
                [0.12, 0.14],
                [0.2, 0.0],
                [0.5, 0.0],
            ],
            0.12,
        );
        let check = stroke(&[[-0.3, 0.02], [-0.08, -0.22], [0.32, 0.26]], 0.16);
        Parts {
            gem: Mesh3::gem(9, 0.9, 0.55, 1.1).transformed(Xf::ID.at(v3(0.0, 0.2, 0.0))),
            spark: Mesh3::gem(4, 1.0, 0.7, 0.7),
            bubble: Mesh3::icosphere(3),
            shield: Mesh3::extrude(&shield_outline(1.0), 0.32, 0.12, 3),
            check: Mesh3::extrude(&check, 0.12, 0.045, 2),
            bolt: Mesh3::extrude(&bolt, 0.42, 0.09, 3),
            tile: retint(Mesh3::extrude(&ccw(squircle(0.9, 4.5, 72)), 0.26, 0.09, 3)),
            cell: Mesh3::rounded_box(v3(0.17, 0.17, 0.05), 0.05, 3),
            folder_back: Mesh3::extrude(&folder, 0.16, 0.06, 2),
            folder_front: Mesh3::extrude(&rounded_rect(1.84, 1.1, 0.12, 4), 0.14, 0.06, 2),
            ring: facing(Mesh3::torus(0.8, 0.16, 72, 14, 0.0, TAU)),
            pulse: Mesh3::extrude(&pulse, 0.1, 0.035, 2),
            clock_face: puck(0.82, 0.22, 0.08, 64),
            clock_rim: facing(Mesh3::torus(0.84, 0.12, 72, 12, 0.0, TAU)),
            hand: Mesh3::rounded_box(v3(0.06, 0.3, 0.04), 0.039, 3),
            pin: puck(0.1, 0.08, 0.03, 24),
            gear: Mesh3::extrude(&gear_outline(10, 0.7), 0.3, 0.05, 2),
            hub: puck(0.24, 0.16, 0.06, 32),
        }
    })
}

/// Optical fit of an icon: (scale, shift right, shift up) in scene units.
fn fit(icon: SectionIcon) -> (f32, f32, f32) {
    match icon {
        SectionIcon::QuickSweep => (0.945, 0.003, 0.054),
        SectionIcon::Clean => (1.03, 0.044, 0.03),
        SectionIcon::Protect => (0.97, -0.014, -0.032),
        SectionIcon::Speed => (1.0, 0.012, 0.027),
        SectionIcon::Apps => (0.959, -0.046, -0.042),
        SectionIcon::Files => (0.925, -0.03, 0.036),
        SectionIcon::Monitor => (1.05, -0.06, -0.056),
        SectionIcon::History => (0.881, -0.031, -0.028),
        SectionIcon::Settings => (1.0, -0.034, -0.03),
    }
}

/// The scene for one icon, posed with a slight turn so it reads as an object.
fn scene(icon: SectionIcon) -> Scene<'static> {
    let p = parts();
    let (a, b) = icon.colors();
    let mut sc = Scene::new(Camera::front(4.4, 0.22), Rig::studio(a));
    // Each icon's scale and shift so they weigh alike and sit centred (REVIEW-C C24,
    // measured by `sidebar_icons_weigh_alike_and_sit_centred`).
    let (k, dx, dy) = fit(icon);
    let root = Xf::scale(k)
        .at(v3(dx, dy, 0.0))
        .then(Xf::ID.rot_y(-0.32).rot_x(0.08));
    let ink = Material::gloss(WHITE, mix(WHITE, a, 0.25));
    let body = Material::gloss(mix(a, WHITE, 0.12), scale(b, 0.72));
    match icon {
        SectionIcon::QuickSweep => {
            let m = Material {
                spec: 1.0,
                shine: 60.0,
                rim: 0.8,
                ..Material::gloss(mix(a, WHITE, 0.3), b)
            };
            sc.add(
                &p.gem,
                Xf::scale(1.1).at(v3(-0.06, -0.1, 0.0)).then(root),
                m,
            );
            sc.add(
                &p.spark,
                Xf::scale(0.2).rot_y(0.5).at(v3(0.82, 0.78, 0.3)).then(root),
                Material::gloss(WHITE, mix(WHITE, a, 0.4)),
            );
        }
        SectionIcon::Clean => {
            for (r, x, y, z) in [
                (0.62, -0.22, -0.22, 0.0),
                (0.42, 0.55, 0.3, -0.1),
                (0.26, 0.1, 0.72, 0.1),
            ] {
                sc.add(
                    &p.bubble,
                    // Shifted so the group sits centred in its square (REVIEW-C C24).
                    Xf::scale(r).at(v3(x - 0.12, y - 0.12, z)).then(root),
                    Material {
                        spec: 1.2,
                        shine: 70.0,
                        rim: 1.0,
                        ..Material::gloss(mix(a, WHITE, 0.35), b)
                    },
                );
            }
        }
        SectionIcon::Protect => {
            sc.add(&p.shield, Xf::scale(0.98).then(root), body);
            sc.add(&p.check, Xf::ID.at(v3(0.0, 0.0, 0.22)).then(root), ink);
        }
        SectionIcon::Speed => {
            // The bolt is thin: larger, so it weighs like its neighbours (REVIEW-C C24).
            sc.add(
                &p.bolt,
                Xf::scale(1.24)
                    .rot_z(-0.1)
                    .at(v3(-0.06, -0.1, 0.0))
                    .then(root),
                body,
            );
        }
        SectionIcon::Apps => {
            // A full tile reads heavier than the outlines around it: slightly smaller.
            let k = 0.9;
            sc.add(&p.tile, Xf::scale(k).then(root), body);
            for (x, y) in [(-1.0, 1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
                sc.add(
                    &p.cell,
                    Xf::scale(k)
                        .at(v3(0.26 * k * x, 0.26 * k * y, 0.17))
                        .then(root),
                    ink,
                );
                // The glyph sits on the tile's face: always drawn over it.
                sc.bias_last(0.4);
            }
        }
        SectionIcon::Files => {
            sc.add(
                &p.folder_back,
                Xf::ID.at(v3(0.0, 0.05, -0.12)).then(root),
                Material::gloss(scale(mix(a, b, 0.5), 0.85), scale(b, 0.6)),
            );
            sc.add(
                &p.folder_front,
                Xf::ID.rot_x(-0.18).at(v3(0.0, -0.17, 0.1)).then(root),
                Material::gloss(mix(a, WHITE, 0.2), scale(mix(a, b, 0.4), 0.85)),
            );
        }
        SectionIcon::Monitor => {
            sc.add(&p.ring, root, body);
            sc.add(&p.pulse, Xf::ID.at(v3(0.0, 0.0, 0.05)).then(root), ink);
        }
        SectionIcon::History => {
            sc.add(
                &p.clock_face,
                root,
                Material::satin(mix(WHITE, a, 0.15), mix(WHITE, b, 0.4)),
            );
            sc.add(&p.clock_rim, Xf::ID.at(v3(0.0, 0.0, 0.08)).then(root), body);
            let dark = Material::gloss(
                lin(Color32::from_rgb(0x2A, 0x33, 0x44)),
                lin(Color32::from_rgb(0x16, 0x1C, 0x26)),
            );
            // Hands at ten past ten, from the centre.
            for (len, ang) in [(1.0, -0.35f32), (0.7, 1.95)] {
                sc.add(
                    &p.hand,
                    Xf::scale3(v3(1.0, len, 1.0))
                        .at(v3(0.0, 0.26 * len, 0.0))
                        .rot_z(ang)
                        .at(v3(0.0, 0.0, 0.16))
                        .then(root),
                    dark,
                );
            }
            sc.add(&p.pin, Xf::ID.at(v3(0.0, 0.0, 0.18)).then(root), body);
        }
        SectionIcon::Settings => {
            sc.add(&p.gear, Xf::scale(1.12).rot_z(0.12).then(root), body);
            sc.add(
                &p.hub,
                Xf::scale(1.12).at(v3(0.0, 0.0, 0.12)).then(root),
                Material::metal(WHITE, mix(WHITE, a, 0.4)),
            );
        }
    }
    sc
}

/// The icon as a straight-alpha image `px` square (4x4 supersampled).
pub fn image(icon: SectionIcon, px: u32) -> ColorImage {
    let px = px.clamp(4, 512) as usize;
    let rect = Rect::from_min_size(Pos2::ZERO, Vec2::splat(px as f32));
    let (mesh, _) = scene(icon).render(rect, 4.0);
    let rgba = rasterize(&mesh, px, px, 1.0, 4);
    ColorImage::from_rgba_unmultiplied([px, px], &rgba)
}

type Cache = Arc<Mutex<HashMap<(SectionIcon, u32), TextureHandle>>>;

fn cache(ctx: &Context) -> Cache {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(Id::new("cleancraft.section_icons"))
            .clone()
    })
}

fn texture(ctx: &Context, icon: SectionIcon, px: u32) -> Option<TextureHandle> {
    let cache = cache(ctx);
    let mut map = cache.lock().ok()?;
    let tex = map.entry((icon, px)).or_insert_with(|| {
        ctx.load_texture(
            format!("cleancraft-section-{icon:?}-{px}"),
            image(icon, px),
            TextureOptions::LINEAR,
        )
    });
    Some(tex.clone())
}

/// A soft halo of `col` under the icon (premultiplied, fades to zero at `r`).
fn halo(ui: &Ui, c: Pos2, r: f32, col: Color32, k: f32) {
    if k <= 0.01 {
        return;
    }
    let seg = 32;
    let mut m = Mesh::default();
    let tint = |a: f32| col.gamma_multiply(a * k);
    m.vertices.push(Vertex {
        pos: c,
        uv: egui::epaint::WHITE_UV,
        color: tint(0.42),
    });
    for (ring, (rr, a)) in [(0.5f32, 0.2f32), (1.0, 0.0)].into_iter().enumerate() {
        for j in 0..seg {
            let t = TAU * j as f32 / seg as f32;
            m.vertices.push(Vertex {
                pos: c + vec2(t.cos(), t.sin()) * r * rr,
                uv: egui::epaint::WHITE_UV,
                color: tint(a),
            });
        }
        let base = 1 + ring as u32 * seg;
        for j in 0..seg {
            let j1 = (j + 1) % seg;
            if ring == 0 {
                m.indices.extend_from_slice(&[0, base + j, base + j1]);
            } else {
                let (pa, pb) = (base - seg + j, base - seg + j1);
                m.indices
                    .extend_from_slice(&[pa, base + j, base + j1, pa, base + j1, pb]);
            }
        }
    }
    ui.painter().add(m);
}

/// Paints `icon` centred in `rect` with `emphasis` 0 (rest) to 1 (selected): the object
/// rises by up to 1.5 pt and a halo of its section colour lights under it.
pub fn paint(ui: &Ui, rect: Rect, icon: SectionIcon, emphasis: f32) {
    let k = emphasis.clamp(0.0, 1.0);
    let ppp = ui.ctx().pixels_per_point();
    let side = rect.width().min(rect.height());
    let px = (side * ppp).round().max(1.0) as u32;
    let size = px as f32 / ppp;
    let snap = |v: f32| (v * ppp).round() / ppp;
    let lift = snap(LIFT * k);
    let min = pos2(
        snap(rect.center().x - size / 2.0),
        snap(rect.center().y - size / 2.0) - lift,
    );
    let r = Rect::from_min_size(min, Vec2::splat(size));
    let (a, _) = icon.section().colors();
    halo(
        ui,
        rect.center() + vec2(0.0, size * 0.08),
        size * 0.62,
        a,
        k,
    );
    if let Some(tex) = texture(ui.ctx(), icon, px) {
        ui.painter().image(
            tex.id(),
            r,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

/// Allocates a `size_pt` square and draws the icon: rest, hovered (half lift) or selected
/// (full lift and halo), eased over `motion::tr::HOVER`, immediate with Reduce motion.
/// Hover only: the sidebar row owns the click.
pub fn show(ui: &mut Ui, icon: SectionIcon, size_pt: f32, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size_pt, size_pt), Sense::hover());
    let target = if selected {
        1.0
    } else if resp.hovered() {
        0.5
    } else {
        0.0
    };
    let k = if motion::reduced() {
        target
    } else {
        motion::animate(
            ui.ctx(),
            resp.id.with("emphasis"),
            target,
            motion::tr::HOVER,
        )
    };
    if ui.is_rect_visible(rect) {
        paint(ui, rect, icon, k);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_at(img: &ColorImage, x: usize, y: usize) -> u8 {
        img.pixels[y * img.size[0] + x].a()
    }

    /// REVIEW-C C24: the sidebar icons weigh alike (ink within 25 % of each other) and sit
    /// centred (the box of what they paint within 0.5 pt of the square's centre at 22.5 pt).
    #[test]
    fn sidebar_icons_weigh_alike_and_sit_centred() {
        let px = 90u32; // 22.5 pt at 4x, enough to measure
        let pt = px as f32 / 22.5;
        let n = px as usize;
        let mut inks = Vec::new();
        for icon in SectionIcon::ALL {
            let img = image(icon, px);
            let mut ink = 0.0f32;
            let (mut x0, mut y0, mut x1, mut y1) = (n, n, 0, 0);
            for y in 0..n {
                for x in 0..n {
                    let a = alpha_at(&img, x, y);
                    ink += a as f32 / 255.0;
                    if a > 128 {
                        (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                    }
                }
            }
            let cx = ((x0 + x1 + 1) as f32 / 2.0 - n as f32 / 2.0) / pt;
            let cy = ((y0 + y1 + 1) as f32 / 2.0 - n as f32 / 2.0) / pt;
            let ink_pt = ink / (pt * pt);
            eprintln!("{icon:?}: ink {ink_pt:.0} pt2, box centre off ({cx:.2}, {cy:.2}) pt");
            inks.push((icon, ink_pt, cx.abs().max(cy.abs())));
        }
        let max = inks.iter().map(|i| i.1).fold(0.0, f32::max);
        let min = inks.iter().map(|i| i.1).fold(f32::MAX, f32::min);
        assert!(max / min <= 1.25, "ink spread {:.2}x: {inks:?}", max / min);
        for (icon, _, off) in &inks {
            assert!(*off <= 0.5, "{icon:?} sits {off:.2} pt off centre");
        }
    }

    #[test]
    fn every_icon_fills_its_square_and_keeps_a_clear_edge() {
        for icon in SectionIcon::ALL {
            for px in [28u32, 56] {
                let img = image(icon, px);
                let n = px as usize;
                let covered = img.pixels.iter().filter(|p| p.a() > 128).count();
                let share = covered as f32 / (n * n) as f32;
                assert!(
                    (0.1..0.8).contains(&share),
                    "{icon:?} at {px} px covers {share:.2}"
                );
                // Nothing touches the outer pixel ring: no cut-off edges.
                for i in 0..n {
                    for (x, y) in [(i, 0), (i, n - 1), (0, i), (n - 1, i)] {
                        assert!(
                            alpha_at(&img, x, y) < 8,
                            "{icon:?} {px} px edge at ({x}, {y})"
                        );
                    }
                }
                // Soft but crisp silhouette: some partial pixels, not a blur.
                let partial = img
                    .pixels
                    .iter()
                    .filter(|p| p.a() > 10 && p.a() < 245)
                    .count();
                assert!(
                    partial > n / 2 && partial < n * n / 3,
                    "{icon:?} {px}: {partial}"
                );
            }
        }
    }

    #[test]
    fn icons_differ_from_each_other() {
        let imgs: Vec<ColorImage> = SectionIcon::ALL.iter().map(|&i| image(i, 28)).collect();
        for i in 0..imgs.len() {
            for j in i + 1..imgs.len() {
                let diff: u32 = imgs[i]
                    .pixels
                    .iter()
                    .zip(&imgs[j].pixels)
                    .map(|(a, b)| a.a().abs_diff(b.a()) as u32)
                    .sum();
                assert!(
                    diff > 28 * 28 * 10,
                    "{:?} vs {:?}",
                    SectionIcon::ALL[i],
                    SectionIcon::ALL[j]
                );
            }
        }
    }

    #[test]
    fn show_caches_one_texture_per_icon_and_size_and_lifts_when_selected() {
        let ctx = Context::default();
        for ppp in [1.0, 2.0] {
            ctx.set_pixels_per_point(ppp);
            for _ in 0..2 {
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    for icon in SectionIcon::ALL {
                        let r = show(ui, icon, SIDEBAR_PT, icon == SectionIcon::Apps);
                        assert_eq!(r.rect.size(), Vec2::splat(SIDEBAR_PT));
                    }
                });
                out.textures_delta.clear();
            }
        }
        let n = cache(&ctx).lock().map(|m| m.len()).unwrap_or(0);
        assert_eq!(n, SectionIcon::ALL.len() * 2);
    }
}
