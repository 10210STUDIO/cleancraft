//! The CleanCraft logo for the sidebar, drawn from the 3D app icon (REVIEW-A 16).
//!
//! `assets/AppIcon.png` is decoded once, cropped to its squircle (the drop shadow of the
//! Dock icon is left out), and resampled into one texture per physical pixel size, so
//! the logo is pixel exact at 1x and 2x. A soft shadow is painted under it.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use egui::{
    Color32, ColorImage, Context, CornerRadius, Id, Rect, Response, Sense, Shadow, TextureHandle,
    TextureOptions, Ui, Vec2, pos2, vec2,
};

const PNG: &[u8] = include_bytes!("../../../assets/AppIcon.png");

/// The squircle inside the 1024 px icon (Apple's icon grid: 824 px tile, 100 px margin).
const TILE_MIN: u32 = 100;
const TILE_SIZE: u32 = 824;

/// Corner radius of the squircle relative to its side, for the shadow under it.
const CORNER: f32 = 0.2237;

/// Sidebar size, in points.
pub const SIDEBAR_PT: f32 = 32.0;

/// The squircle, premultiplied RGBA, `TILE_SIZE` square. `None` if the PNG is broken.
fn source() -> Option<&'static image::Rgba32FImage> {
    static SRC: OnceLock<Option<image::Rgba32FImage>> = OnceLock::new();
    SRC.get_or_init(|| {
        let img = image::load_from_memory_with_format(PNG, image::ImageFormat::Png).ok()?;
        let tile = img
            .crop_imm(TILE_MIN, TILE_MIN, TILE_SIZE, TILE_SIZE)
            .to_rgba32f();
        let mut tile = tile;
        for p in tile.pixels_mut() {
            let a = p.0[3];
            p.0[0] *= a;
            p.0[1] *= a;
            p.0[2] *= a;
        }
        Some(tile)
    })
    .as_ref()
}

/// The logo resampled to `px` square physical pixels, premultiplied.
pub fn image(px: u32) -> Option<ColorImage> {
    let px = px.clamp(1, TILE_SIZE);
    let src = source()?;
    let small = image::imageops::resize(src, px, px, image::imageops::FilterType::Lanczos3);
    let bytes: Vec<u8> = small
        .pixels()
        .flat_map(|p| {
            let a = p.0[3].clamp(0.0, 1.0);
            // Lanczos rings a little: keep colour within alpha (valid premultiplied).
            let c = |v: f32| (v.clamp(0.0, a) * 255.0).round() as u8;
            [c(p.0[0]), c(p.0[1]), c(p.0[2]), (a * 255.0).round() as u8]
        })
        .collect();
    Some(ColorImage::from_rgba_premultiplied(
        [px as usize, px as usize],
        &bytes,
    ))
}

type Cache = Arc<std::sync::Mutex<HashMap<u32, TextureHandle>>>;

/// The texture for `px` physical pixels, made on first use and kept.
fn texture(ctx: &Context, px: u32) -> Option<TextureHandle> {
    let cache: Cache = ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Cache>(Id::new("cleancraft.logo"))
            .clone()
    });
    let mut map = cache.lock().ok()?;
    if let Some(t) = map.get(&px) {
        return Some(t.clone());
    }
    let img = image(px)?;
    let tex = ctx.load_texture(format!("cleancraft-logo-{px}"), img, TextureOptions::LINEAR);
    map.insert(px, tex.clone());
    Some(tex)
}

/// Paints the logo with its squircle exactly on `rect` (snapped to physical pixels).
pub fn paint(ui: &Ui, rect: Rect) {
    let ppp = ui.ctx().pixels_per_point();
    // Snap to whole physical pixels so the texture maps 1:1 to the screen.
    let snap = |v: f32| (v * ppp).round() / ppp;
    let side = rect.width().min(rect.height());
    let px = (side * ppp).round().max(1.0) as u32;
    let size = px as f32 / ppp;
    let min = pos2(
        snap(rect.center().x - size / 2.0),
        snap(rect.center().y - size / 2.0),
    );
    let r = Rect::from_min_size(min, Vec2::splat(size));
    let shadow = Shadow {
        offset: [0, (size * 0.06).round() as i8],
        blur: (size * 0.22).round().clamp(2.0, 60.0) as u8,
        spread: 0,
        color: Color32::from_black_alpha(90),
    };
    ui.painter()
        .add(shadow.as_shape(r, CornerRadius::same((size * CORNER).round() as u8)));
    if let Some(tex) = texture(ui.ctx(), px) {
        ui.painter().image(
            tex.id(),
            r,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
}

/// Allocates a `size_pt` square and draws the logo in it. Hover only: wrap it in your
/// own interaction if the logo should do something.
pub fn show(ui: &mut Ui, size_pt: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size_pt, size_pt), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint(ui, rect);
    }
    resp.on_hover_text(crate::tr("CleanCraft"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn px(img: &ColorImage, x: usize, y: usize) -> Color32 {
        img.pixels[y * img.size[0] + x]
    }

    #[test]
    fn decodes_and_resamples_at_1x_and_2x() {
        for side in [32, 64] {
            let img = image(side).expect("the bundled icon decodes");
            assert_eq!(img.size, [side as usize, side as usize]);
            let c = side as usize / 2;
            // Opaque middle, transparent squircle corners.
            assert_eq!(px(&img, c, c).a(), 255);
            assert!(px(&img, 0, 0).a() < 40, "{:?}", px(&img, 0, 0));
            // Opaque along the middle of each edge: the crop is the squircle, no margin.
            assert!(px(&img, c, 0).a() > 200, "{:?}", px(&img, c, 0));
            assert!(px(&img, 0, c).a() > 200, "{:?}", px(&img, 0, c));
        }
    }

    #[test]
    fn premultiplied_colour_never_exceeds_alpha() {
        let img = image(32).expect("decodes");
        for p in &img.pixels {
            let a = p.a();
            assert!(p.r() <= a && p.g() <= a && p.b() <= a, "{p:?}");
        }
    }

    #[test]
    fn show_allocates_its_size_and_caches_one_texture_per_pixel_size() {
        let ctx = Context::default();
        for ppp in [1.0, 2.0] {
            ctx.set_pixels_per_point(ppp);
            for _ in 0..2 {
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let r = show(ui, SIDEBAR_PT);
                    assert_eq!(r.rect.size(), Vec2::splat(SIDEBAR_PT));
                });
                out.textures_delta.clear();
            }
        }
        let cache: Cache = ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<Cache>(Id::new("cleancraft.logo"))
                .clone()
        });
        let mut sizes: Vec<u32> = cache
            .lock()
            .map(|m| m.keys().copied().collect())
            .unwrap_or_default();
        sizes.sort();
        assert_eq!(sizes, vec![32, 64]);
    }
}
