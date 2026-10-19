//! Building blocks shared by every page: pill buttons with hover and press motion, glass
//! cards, page headers, the start-screen hero with its round action button, the progress
//! ring, the Done check, tags, bars and text helpers. Numbers come from `theme`.

use std::sync::Arc;

use egui::{
    Align2, Color32, CornerRadius, FontId, Frame, Galley, Margin, Pos2, Rect, Response, RichText,
    Sense, Shape, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::icons::{self, Icon};
use crate::theme::{
    self, Section, Tokens, Weight, font, hit, icon as isz, motion, radius, size, space,
};

// ---------------------------------------------------------------------------------------
// Buttons
// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The main action, in the section's gradient.
    Primary,
    /// A quiet glass button.
    Secondary,
    /// Moves files (still never deletes: CleanCraft only moves).
    Danger,
    /// Text only until hovered; for row actions.
    Ghost,
}

/// A pill button. Accessible name = `label`. Respects a disabled `ui`.
pub fn pill(ui: &mut Ui, t: &Tokens, label: &str, kind: Kind, icon: Option<Icon>) -> Response {
    let (h, fs, pad, min_w) = match kind {
        Kind::Primary | Kind::Danger => (hit::PRIMARY, size::BODY, 20.0, 104.0),
        Kind::Secondary => (hit::SECONDARY, size::CALLOUT, 16.0, 76.0),
        Kind::Ghost => (hit::GHOST, size::FOOTNOTE, 11.0, 0.0),
    };
    let weight = match kind {
        Kind::Ghost => Weight::Medium,
        Kind::Secondary => Weight::Medium,
        _ => Weight::SemiBold,
    };
    let fid = font(fs, weight);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), fid, Color32::WHITE);
    let icon_w = if icon.is_some() { fs + 6.0 } else { 0.0 };
    let w = (galley.size().x + icon_w + pad * 2.0).max(min_w);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let enabled = ui.is_enabled();
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let ctx = ui.ctx().clone();
    let hov = anim(
        &ctx,
        resp.id.with("hov"),
        resp.hovered() && enabled,
        motion::HOVER,
    );
    let down = anim(
        &ctx,
        resp.id.with("down"),
        resp.is_pointer_button_down_on() && enabled,
        motion::PRESS,
    );
    let scale = 1.0 - (1.0 - motion::PRESS_SCALE) * down;
    let r = Rect::from_center_size(rect.center(), rect.size() * scale);
    let p = ui.painter().clone();
    let p = if enabled {
        p
    } else {
        let mut q = p;
        q.set_opacity(0.4);
        q
    };
    let round = r.height() / 2.0;
    let text_color = match kind {
        Kind::Primary => {
            if hov > 0.0 {
                icons::glow(
                    &p,
                    r.center(),
                    r.width() * 0.62,
                    t.sec_a.gamma_multiply(0.22 * hov),
                    0.45,
                );
            }
            let a = icons::lerp(t.sec_a, Color32::WHITE, 0.12 * hov);
            let b = icons::lerp(t.sec_b, Color32::WHITE, 0.12 * hov);
            icons::gradient_tile(&p, r, round, a, b);
            top_sheen(&p, r, round);
            t.on_section()
        }
        Kind::Danger => {
            let a = icons::lerp(
                Color32::from_rgb(0xFF, 0x76, 0x6E),
                Color32::WHITE,
                0.1 * hov,
            );
            let b = icons::lerp(
                Color32::from_rgb(0xEE, 0x5A, 0x64),
                Color32::WHITE,
                0.1 * hov,
            );
            icons::gradient_tile(&p, r, round, a, b);
            top_sheen(&p, r, round);
            theme::on_color(icons::lerp(a, b, 0.5))
        }
        Kind::Secondary => {
            let fill = icons::lerp(t.glass, t.glass_hover, hov);
            p.rect_filled(r, round, fill);
            p.rect_stroke(
                r,
                round,
                Stroke::new(
                    1.0,
                    icons::lerp(t.glass_edge, t.sec_a.gamma_multiply(0.7), hov),
                ),
                egui::StrokeKind::Inside,
            );
            t.text
        }
        Kind::Ghost => {
            p.rect_filled(
                r,
                round,
                Color32::from_white_alpha((14.0 + 16.0 * hov) as u8),
            );
            icons::lerp(t.text_dim, t.text, hov)
        }
    };
    let content_w = galley.size().x + icon_w;
    let mut x = r.center().x - content_w / 2.0;
    if let Some(ic) = icon {
        let ir = Rect::from_center_size(pos2(x + fs / 2.0, r.center().y), Vec2::splat(fs + 1.0));
        icons::paint(&p, ir, ic, text_color);
        x += icon_w;
    }
    // The label keeps its size while pressed (only the fill shrinks): re-laying text at a
    // scaled size makes the glyphs shimmer.
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font(fs, weight), text_color);
    p.galley(
        pos2(x, r.center().y - galley.size().y / 2.0),
        galley,
        text_color,
    );
    resp
}

fn top_sheen(p: &egui::Painter, r: Rect, round: f32) {
    let top = Rect::from_min_max(r.min, pos2(r.max.x, r.center().y));
    let _ = round;
    icons::vertical_gradient(
        &p.with_clip_rect(top.shrink2(vec2(round * 0.6, 1.0)).intersect(p.clip_rect())),
        top,
        Color32::from_white_alpha(34),
        Color32::from_white_alpha(0),
    );
}

/// Animated 0..=1 toward `on`, honouring reduced motion.
pub fn anim(ctx: &egui::Context, id: egui::Id, on: bool, secs: f32) -> f32 {
    crate::motion::animate_bool(
        ctx,
        id,
        on,
        crate::motion::Motion::new(secs, crate::motion::GLIDE),
    )
}

/// The main action. Returns true when clicked.
pub fn primary(ui: &mut Ui, t: &Tokens, label: &str) -> bool {
    pill(ui, t, label, Kind::Primary, None).clicked()
}

/// A quiet secondary button.
pub fn secondary(ui: &mut Ui, t: &Tokens, label: &str) -> bool {
    pill(ui, t, label, Kind::Secondary, None).clicked()
}

/// The button that moves files.
pub fn danger(ui: &mut Ui, t: &Tokens, label: &str) -> bool {
    let icon = label.contains("Trash").then_some(Icon::Trash);
    pill(ui, t, label, Kind::Danger, icon).clicked()
}

/// A small row action (Reveal, Ignore).
pub fn ghost(ui: &mut Ui, t: &Tokens, label: &str, icon: Option<Icon>) -> Response {
    pill(ui, t, label, Kind::Ghost, icon)
}

// ---------------------------------------------------------------------------------------
// Surfaces
// ---------------------------------------------------------------------------------------

/// A glass card: translucent over the section's colour field, hairline edge, soft shadow.
pub fn card(t: &Tokens) -> Frame {
    Frame::new()
        .fill(t.glass)
        .stroke(Stroke::new(1.0, t.glass_stroke))
        .corner_radius(CornerRadius::same(radius::L))
        .inner_margin(Margin::same(space::L as i8))
        .shadow(theme::card_shadow())
}

/// A flatter card for content inside cards or long lists.
pub fn well(t: &Tokens) -> Frame {
    Frame::new()
        .fill(Color32::from_black_alpha(60))
        .stroke(Stroke::new(1.0, t.glass_stroke))
        .corner_radius(CornerRadius::same(radius::M))
        .inner_margin(Margin::same(space::M as i8))
}

/// The floating bar at the bottom of a results page.
pub fn bar_frame(t: &Tokens) -> Frame {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(0x1C, 0x22, 0x2E, 235))
        .stroke(Stroke::new(1.0, t.glass_edge))
        .corner_radius(CornerRadius::same(radius::XL))
        .inner_margin(Margin::symmetric(20, 10))
        .shadow(theme::float_shadow())
}

/// A dialog's frame.
pub fn dialog_frame(t: &Tokens) -> Frame {
    Frame::new()
        .fill(Color32::from_rgb(0x1A, 0x1F, 0x2A))
        .stroke(Stroke::new(1.0, t.glass_edge))
        .corner_radius(CornerRadius::same(radius::XL))
        .inner_margin(Margin::same(space::XL as i8))
        .shadow(theme::float_shadow())
}

/// The colour field behind the content: a vertical wash of the section colour, a large soft
/// glow at the upper right and a fainter one at the lower left.
pub fn backdrop(p: &egui::Painter, r: Rect, t: &Tokens) {
    p.rect_filled(r, 0.0, t.bg);
    icons::vertical_gradient(p, r, icons::lerp(t.bg, t.sec_b, 0.10), t.bg);
    icons::glow(
        p,
        pos2(r.right() - r.width() * 0.18, r.top() + r.height() * 0.02),
        r.width() * 0.55,
        t.sec_a.gamma_multiply(0.16),
        0.75,
    );
    icons::glow(
        p,
        pos2(r.left() + r.width() * 0.05, r.bottom()),
        r.width() * 0.45,
        t.sec_b.gamma_multiply(0.08),
        0.7,
    );
}

// ---------------------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------------------

pub fn title(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(
        RichText::new(text)
            .font(font(size::TITLE, Weight::Bold))
            .color(t.text),
    );
}

pub fn subtitle(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(
        RichText::new(text)
            .font(font(size::CALLOUT, Weight::Regular))
            .color(t.text_dim),
    );
}

pub fn dim(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(
        RichText::new(text)
            .color(t.text_faint)
            .font(font(size::FOOTNOTE, Weight::Regular)),
    );
}

/// A small upper-case label above a group of controls.
pub fn eyebrow(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(
        RichText::new(text.to_uppercase())
            .font(font(size::CAPTION, Weight::SemiBold))
            .extra_letter_spacing(0.6)
            .color(t.text_faint),
    );
}

/// A big number (display size) in the section colour.
pub fn display(ui: &mut Ui, t: &Tokens, text: &str) -> Response {
    ui.label(
        RichText::new(text)
            .font(font(size::DISPLAY, Weight::Bold))
            .color(icons::lerp(t.sec_a, Color32::WHITE, 0.15)),
    )
}

/// The narrowest width up to `max_w` at which `text` still takes as few lines as at
/// `max_w`: wrapped text gets even lines instead of a one-word last line.
pub fn balanced_width(ui: &Ui, text: &str, fid: &FontId, max_w: f32) -> f32 {
    let lines = |w: f32| {
        let mut job =
            egui::text::LayoutJob::simple(text.to_string(), fid.clone(), Color32::WHITE, w);
        job.wrap.max_width = w;
        ui.ctx().fonts_mut(|f| f.layout_job(job)).rows.len()
    };
    let n = lines(max_w);
    if n <= 1 {
        return max_w;
    }
    let (mut lo, mut hi) = (max_w * 0.4, max_w);
    for _ in 0..12 {
        let mid = (lo + hi) / 2.0;
        if lines(mid) > n {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (hi + 2.0).min(max_w)
}

/// How far below the top of a `big` line a `small` line must start so both share one
/// baseline (Inter's ascender is 0.969 em).
pub fn baseline_offset(big: f32, small: f32) -> f32 {
    ((big - small) * 0.969).round()
}

/// Height of a line of `size::DISPLAY` text (Inter's line is 1.21 em).
pub const DISPLAY_ROW_H: f32 = size::DISPLAY * 1.21;

/// `text` shortened in the middle with an ellipsis so it fits `max_w` in `fid`.
pub fn truncate_middle(ctx: &egui::Context, text: &str, fid: &FontId, max_w: f32) -> String {
    let widths: Vec<f32> = ctx.fonts_mut(|f| text.chars().map(|c| f.glyph_width(fid, c)).collect());
    let total: f32 = widths.iter().sum();
    if total <= max_w {
        return text.to_string();
    }
    let ell = ctx.fonts_mut(|f| f.glyph_width(fid, '…'));
    let budget = (max_w - ell).max(0.0);
    let chars: Vec<char> = text.chars().collect();
    let (mut i, mut j) = (0usize, chars.len());
    let (mut wl, mut wr) = (0.0f32, 0.0f32);
    // Keep a little more of the end: the file name lives there.
    loop {
        let take_right = wr <= wl * 1.4;
        if take_right && j > i {
            let w = widths[j - 1];
            if wl + wr + w > budget {
                break;
            }
            wr += w;
            j -= 1;
        } else if i < j {
            let w = widths[i];
            if wl + wr + w > budget {
                break;
            }
            wl += w;
            i += 1;
        } else {
            break;
        }
    }
    let mut s: String = chars[..i].iter().collect();
    s.push('…');
    s.extend(chars[j..].iter());
    s
}

/// `text` cut at the end with an ellipsis so it fits `max_w` in `fid` (sentences: reasons).
pub fn truncate_end(ctx: &egui::Context, text: &str, fid: &FontId, max_w: f32) -> String {
    let widths: Vec<f32> = ctx.fonts_mut(|f| text.chars().map(|c| f.glyph_width(fid, c)).collect());
    if widths.iter().sum::<f32>() <= max_w {
        return text.to_string();
    }
    let ell = ctx.fonts_mut(|f| f.glyph_width(fid, '…'));
    let mut w = 0.0;
    let mut out = String::new();
    for (c, cw) in text.chars().zip(widths) {
        if w + cw + ell > max_w {
            break;
        }
        w += cw;
        out.push(c);
    }
    let mut out = out.trim_end().to_string();
    out.push('…');
    out
}

/// A label shortened in the middle to the available width.
pub fn path_label(ui: &mut Ui, t: &Tokens, text: &str) -> Response {
    let fid = font(size::FOOTNOTE, Weight::Regular);
    let w = ui.available_width().max(40.0);
    let shown = truncate_middle(ui.ctx(), text, &fid, w);
    // Shows the shortened path, announces the whole one (VoiceOver, tests).
    let sw = galley(ui, &shown, fid.clone(), t.text_faint).size().x + 2.0;
    let (r, _) = ui.allocate_exact_size(vec2(sw.min(w), 18.0), Sense::hover());
    text_at(ui, r, &shown, text, fid, t.text_faint, egui::Align::Min)
}

/// A rounded tag (a file's reason, a state).
pub fn tag(ui: &mut Ui, text: &str, color: Color32) -> Response {
    let fid = font(size::FOOTNOTE, Weight::Medium);
    let galley = ui.painter().layout_no_wrap(text.to_string(), fid, color);
    let size = galley.size() + vec2(16.0, 6.0);
    let (r, resp) = ui.allocate_exact_size(size, Sense::hover());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, text));
    let p = ui.painter();
    p.rect_filled(r, r.height() / 2.0, color.gamma_multiply(0.16));
    p.galley(r.center() - galley.size() / 2.0, galley, color);
    resp
}

// ---------------------------------------------------------------------------------------
// Icons in tiles
// ---------------------------------------------------------------------------------------

/// A rounded tile with the section gradient and a white icon: each module's picture.
pub fn badge(ui: &mut Ui, color: Color32, icon: Icon, side: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    icon_tile(ui.painter(), r, icon, color);
}

/// Paint a gradient tile with an icon in `r`.
pub fn icon_tile(p: &egui::Painter, r: Rect, icon: Icon, color: Color32) {
    let a = icons::lerp(color, Color32::WHITE, 0.10);
    let b = icons::lerp(color, Color32::BLACK, 0.30);
    p.add(Shape::rect_filled(
        r.translate(vec2(0.0, r.height() * 0.06)),
        r.width() * 0.3,
        Color32::from_black_alpha(60),
    ));
    icons::gradient_tile(p, r, r.width() * 0.3, a, b);
    top_sheen(p, r, r.width() * 0.3);
    let ir = r.shrink(r.width() * 0.22);
    icons::paint(p, ir, icon, theme::on_color(color));
}

/// A tile with the first letter of `name` (apps, no icon extraction).
pub fn initial_tile(p: &egui::Painter, r: Rect, name: &str, color: Color32) {
    let a = icons::lerp(color, Color32::WHITE, 0.08);
    let b = icons::lerp(color, Color32::BLACK, 0.35);
    icons::gradient_tile(p, r, r.width() * 0.26, a, b);
    top_sheen(p, r, r.width() * 0.26);
    let letter: String = name
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "A".into());
    p.text(
        r.center(),
        Align2::CENTER_CENTER,
        letter,
        font(theme::snap(r.height() * 0.48), Weight::Bold),
        theme::on_color(icons::lerp(color, Color32::BLACK, 0.12)),
    );
}

/// An app's picture in `r`: the real icon (`images::paint_app_icon`, which shows the initial
/// tile while it loads and fades the icon in), or the coloured initial tile when there is no
/// bundle to read it from.
pub fn app_icon(ui: &Ui, r: Rect, name: &str, bundle: Option<&std::path::Path>) {
    match bundle {
        Some(p) => {
            crate::images::paint_app_icon(ui, r, p);
        }
        None => initial_tile(ui.painter(), r, name, name_color(name)),
    }
}

/// A centred, wrapping row of painted chips (small module tile and name), for "what this
/// covers" lines on start screens. Painted only: no accessible nodes, so names stay unique.
pub fn chips(ui: &mut Ui, t: &Tokens, items: &[(Icon, Color32, &str)], max_w: f32) {
    let fid = font(size::CALLOUT, Weight::Medium);
    let h = 34.0;
    let gap = space::S;
    let tile = 22.0;
    let widths: Vec<f32> = items
        .iter()
        .map(|(_, _, n)| galley(ui, n, fid.clone(), t.text).size().x + tile + 30.0)
        .collect();
    let max_w = max_w.min(ui.available_width());
    // Fewest lines that fit, then the same number of chips on each line (5 chips on two
    // lines read 3 and 2, never 4 and 1).
    let fits = |per: usize| {
        widths
            .chunks(per.max(1))
            .all(|c| c.iter().sum::<f32>() + gap * (c.len() as f32 - 1.0) <= max_w)
    };
    let n = widths.len().max(1);
    let mut n_lines = 1;
    while n_lines < n && !fits(n.div_ceil(n_lines)) {
        n_lines += 1;
    }
    let per = n.div_ceil(n_lines);
    let lines: Vec<Vec<usize>> = (0..widths.len())
        .collect::<Vec<_>>()
        .chunks(per)
        .map(|c| c.to_vec())
        .collect();
    let total_h = lines.len() as f32 * h + (lines.len() as f32 - 1.0) * gap;
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), total_h), Sense::hover());
    let p = ui.painter();
    for (li, line) in lines.iter().enumerate() {
        let lw: f32 =
            line.iter().map(|i| widths[*i]).sum::<f32>() + gap * (line.len() as f32 - 1.0);
        let mut x = area.center().x - lw / 2.0;
        let y = area.top() + li as f32 * (h + gap);
        for i in line {
            let (icon, color, name) = items[*i];
            let r = Rect::from_min_size(pos2(x, y), vec2(widths[*i], h));
            p.rect_filled(r, h / 2.0, t.glass);
            p.rect_stroke(
                r,
                h / 2.0,
                Stroke::new(1.0, t.glass_stroke),
                egui::StrokeKind::Inside,
            );
            let tr = Rect::from_center_size(
                pos2(r.left() + 6.0 + tile / 2.0, r.center().y),
                Vec2::splat(tile),
            );
            icon_tile(p, tr, icon, color);
            p.text(
                pos2(tr.right() + 8.0, r.center().y),
                Align2::LEFT_CENTER,
                name,
                fid.clone(),
                t.text_dim,
            );
            x += widths[*i] + gap;
        }
    }
}

/// A stable colour for a name (app tiles).
pub fn name_color(name: &str) -> Color32 {
    const SET: [Color32; 8] = [
        Color32::from_rgb(0x4E, 0xA8, 0xFF),
        Color32::from_rgb(0x7D, 0x8C, 0xFF),
        Color32::from_rgb(0x34, 0xC8, 0xA4),
        Color32::from_rgb(0xF2, 0x8C, 0x4A),
        Color32::from_rgb(0xE8, 0x6A, 0x9A),
        Color32::from_rgb(0x9A, 0xC8, 0x3A),
        Color32::from_rgb(0xB0, 0x7C, 0xF0),
        Color32::from_rgb(0x3A, 0xB8, 0xD8),
    ];
    let h = name
        .bytes()
        .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    SET[(h as usize) % SET.len()]
}

/// The icon for an item: folder, app, picture or file.
pub fn kind_icon(path: &std::path::Path) -> Icon {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "app" => Icon::App,
        "png" | "jpg" | "jpeg" | "heic" | "gif" | "tiff" | "tif" | "webp" | "bmp" => Icon::Picture,
        _ => {
            if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
                Icon::Folder
            } else {
                Icon::File
            }
        }
    }
}

/// Colours for categories and blocks, in the section's family first.
pub fn series_color(t: &Tokens, i: usize) -> Color32 {
    let set = [
        t.sec_a,
        t.sec_b,
        Color32::from_rgb(0x8F, 0xA2, 0xFF),
        Color32::from_rgb(0xF7, 0xC3, 0x5B),
        Color32::from_rgb(0xE9, 0x8A, 0xC9),
        Color32::from_rgb(0x6F, 0xD6, 0xC5),
        Color32::from_rgb(0xF2, 0x8C, 0x6A),
        Color32::from_rgb(0x9C, 0xD0, 0x6A),
    ];
    set[i % set.len()]
}

// ---------------------------------------------------------------------------------------
// Page parts
// ---------------------------------------------------------------------------------------

/// The compact header at the top of a page: icon tile, title, one line under it.
pub fn header(ui: &mut Ui, t: &Tokens, icon: Icon, title_text: &str, sub: &str) {
    ui.horizontal(|ui| {
        badge(ui, t.sec_a, icon, isz::HEADER);
        ui.add_space(space::XS);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            if sub.is_empty() {
                ui.add_space(8.0);
            }
            title(ui, t, title_text);
            if !sub.is_empty() {
                // Wraps (never cut): a page's explanation is read in full.
                ui.set_max_width(ui.available_width().min(space::MEASURE + 160.0));
                ui.add(
                    egui::Label::new(
                        RichText::new(sub)
                            .font(font(size::CALLOUT, Weight::Regular))
                            .color(t.text_dim),
                    )
                    .wrap(),
                );
            }
        });
    });
    ui.add_space(space::L);
}

/// The head of a card: a small module tile in `color` and a title.
pub fn card_head(ui: &mut Ui, t: &Tokens, icon: Icon, color: Color32, title_text: &str) {
    ui.horizontal(|ui| {
        badge(ui, color, icon, 28.0);
        ui.add_space(space::XXS);
        ui.label(
            RichText::new(title_text)
                .font(font(size::TITLE3, Weight::SemiBold))
                .color(t.text),
        );
    });
}

/// Draw a module's hero (heroes.rs). Its glow reaches the edges of its square, so it may
/// paint over the page margin and the scroll area's edge (never over the sidebar): no hard
/// edge where a scroll area would clip it.
pub fn show_hero(
    ui: &mut Ui,
    feature: cleancraft_core::FeatureId,
    phase: crate::heroes::HeroPhase,
    progress: f32,
    size: Vec2,
) -> Response {
    let mut clip = ui.ctx().content_rect();
    clip.min.x += space::SIDEBAR;
    ui.scope(|ui| {
        ui.set_clip_rect(clip);
        crate::heroes::show(ui, feature, phase, progress, size)
    })
    .inner
}

/// Center content vertically: remembers the last height of the block under `id`.
pub fn centered_block<R>(ui: &mut Ui, id: egui::Id, add: impl FnOnce(&mut Ui) -> R) -> R {
    let avail = ui.available_height();
    let known: Option<f32> = ui.data(|d| d.get_temp(id));
    let last = known.unwrap_or(avail);
    let top = ((avail - last) / 2.0).max(0.0) * 0.85;
    ui.add_space(top);
    let start = ui.cursor().min.y;
    let r = ui.vertical_centered(add).inner;
    let h = ui.cursor().min.y - start;
    if (h - last).abs() > 0.5 {
        ui.data_mut(|d| d.insert_temp(id, h));
        // The first time a block shows, its height is unknown: redo this frame unseen
        // instead of showing it one frame at the wrong place.
        if known.is_none() {
            ui.ctx()
                .request_discard("centre a block whose height was unknown");
        } else {
            ui.ctx().request_repaint();
        }
    }
    r
}

/// A module's start screen: picture, title, one line, optional controls, the big round
/// action button and a short note. Returns true when the action is clicked.
pub fn hero(
    ui: &mut Ui,
    t: &Tokens,
    feature: cleancraft_core::FeatureId,
    title_text: &str,
    desc: &str,
    action: &str,
    note: &str,
    extra: impl FnOnce(&mut Ui),
) -> bool {
    let id = ui.id().with(("hero", title_text));
    let avail = ui.available_height();
    // Height of everything but the picture, from the last frame: the picture takes what is
    // left (between 96 and HERO_MAX points), so the action button never needs scrolling.
    let rest: f32 = ui.data(|d| d.get_temp(id.with("rest"))).unwrap_or(320.0);
    let side = ((avail - rest) / 1.12).clamp(96.0, isz::HERO_MAX).round();
    let mut clicked = false;
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            centered_block(ui, id, |ui| {
                // The module's own 3D object (heroes.rs), idle.
                show_hero(
                    ui,
                    feature,
                    crate::heroes::HeroPhase::Idle,
                    0.0,
                    Vec2::splat(side * 1.12),
                );
                let start = ui.cursor().min.y;
                ui.add_space(space::S);
                ui.label(
                    RichText::new(title_text)
                        .font(font(size::LARGE_TITLE, Weight::Bold))
                        .color(t.text),
                );
                ui.add_space(space::XS);
                ui.scope(|ui| {
                    let fid = font(size::BODY_L, Weight::Regular);
                    let w =
                        balanced_width(ui, desc, &fid, space::MEASURE.min(ui.available_width()));
                    ui.set_max_width(w);
                    ui.label(RichText::new(desc).font(fid).color(t.text_dim));
                });
                ui.add_space(space::L);
                extra(ui);
                ui.add_space(space::XS);
                clicked = round_action(ui, t, action, 112.0).clicked();
                ui.add_space(space::S);
                if !note.is_empty() {
                    dim(ui, t, note);
                }
                let h = ui.cursor().min.y - start;
                if (h - rest).abs() > 1.0 {
                    ui.data_mut(|d| d.insert_temp(id.with("rest"), h));
                    ui.ctx().request_repaint();
                }
            });
        });
    // The idle picture drifts slowly: 30 frames a second is plenty and saves energy on
    // 120 Hz displays (heroes::show asks for its own frames; this covers the halo).
    crate::motion::ambient_repaint(ui.ctx());
    clicked
}

/// The big action button with a breathing halo: round for a short label, a wide pill for a
/// long one.
pub fn round_action(ui: &mut Ui, t: &Tokens, label: &str, d: f32) -> Response {
    let fid = font(size::TITLE2, Weight::Bold);
    let label_w = galley(ui, label, fid.clone(), Color32::WHITE).size().x;
    let round = label_w < d * 0.78;
    let size = if round {
        Vec2::splat(d)
    } else {
        vec2(label_w + 72.0, 56.0)
    };
    let pad = 22.0;
    let (outer, _) = ui.allocate_exact_size(size + Vec2::splat(pad * 2.0), Sense::hover());
    let rect = Rect::from_center_size(outer.center(), size);
    let resp = ui.interact(rect, ui.id().with(("round", label)), Sense::click());
    let enabled = ui.is_enabled();
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    let ctx = ui.ctx().clone();
    let hov = anim(&ctx, resp.id.with("hov"), resp.hovered(), motion::HOVER);
    let down = anim(
        &ctx,
        resp.id.with("down"),
        resp.is_pointer_button_down_on(),
        motion::PRESS,
    );
    let time = ui.input(|i| i.time);
    let p = ui.painter();
    let c = rect.center();
    let scale = (1.0 + 0.04 * hov) * (1.0 - (1.0 - motion::PRESS_SCALE) * down);
    let r = Rect::from_center_size(c, size * scale);
    let corner = r.height() / 2.0;
    // Halo: two outlines that grow and fade, half a period apart.
    if !motion::reduced() {
        for k in 0..2 {
            let ph = (motion::looped(time, motion::PULSE) + k as f32 * 0.5).fract();
            let e = motion::ease_out(ph);
            let a = (1.0 - e) * 0.45;
            let grow = 18.0 * e;
            p.rect_stroke(
                r.expand(grow),
                corner + grow,
                Stroke::new(2.0, t.sec_a.gamma_multiply(a)),
                egui::StrokeKind::Middle,
            );
        }
    }
    icons::glow(
        p,
        c,
        r.width().max(r.height()) * 0.85,
        t.sec_a.gamma_multiply(0.26 + 0.1 * hov),
        r.height() / r.width().max(r.height()),
    );
    p.rect_filled(
        r.translate(vec2(0.0, r.height() * 0.08)),
        corner,
        Color32::from_black_alpha(80),
    );
    let a = icons::lerp(t.sec_a, Color32::WHITE, 0.10 * hov);
    let b = icons::lerp(t.sec_b, Color32::WHITE, 0.10 * hov);
    if round {
        icons::gradient_disc(p, c, r.width() / 2.0, a, b);
        icons::glow(
            p,
            c + vec2(-r.width() * 0.15, -r.height() * 0.21),
            r.width() * 0.38,
            Color32::from_white_alpha(40),
            0.6,
        );
    } else {
        icons::gradient_tile(p, r, corner, a, b);
        top_sheen(p, r, corner);
    }
    p.rect_stroke(
        r,
        corner,
        Stroke::new(1.5, Color32::from_white_alpha(70)),
        egui::StrokeKind::Inside,
    );
    p.text(
        c,
        Align2::CENTER_CENTER,
        label,
        font(size::TITLE2, Weight::Bold),
        t.on_section(),
    );
    resp
}

/// Seconds as m:ss.
pub fn clock(secs: u64) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// The scanning screen: a large ring with a sweeping arc, the module icon and the elapsed
/// time inside, a headline, the place being looked at (shortened in the middle) and, when the
/// work can stop, a Stop button. Returns true when Stop is clicked.
pub fn progress(
    ui: &mut Ui,
    t: &Tokens,
    feature: cleancraft_core::FeatureId,
    headline: &str,
    status: &str,
    elapsed: u64,
    can_stop: bool,
) -> bool {
    let id = ui.id().with(("progress", headline));
    let mut stop = false;
    centered_block(ui, id, |ui| {
        let avail = ui.available_height();
        let d = if avail < 460.0 { 170.0 } else { 220.0 };
        // The module's 3D object at work (heroes.rs, scanning).
        show_hero(
            ui,
            feature,
            crate::heroes::HeroPhase::Scanning,
            0.0,
            Vec2::splat(d),
        );
        ui.add_space(space::S);
        ui.label(
            RichText::new(headline)
                .font(font(size::TITLE2, Weight::SemiBold))
                .color(t.text),
        );
        ui.add_space(space::XS);
        let fid = font(size::CALLOUT, Weight::Regular);
        let w = ui.available_width().min(space::MEASURE);
        let shown = truncate_middle(ui.ctx(), status, &fid, w);
        ui.label(RichText::new(shown).font(fid).color(t.text_faint));
        ui.add_space(space::XS);
        ui.label(
            RichText::new(clock(elapsed))
                .font(font(size::TITLE3, Weight::SemiBold))
                .color(t.text_dim),
        );
        if can_stop {
            ui.add_space(space::M);
            stop = pill(ui, t, "Stop", Kind::Secondary, Some(Icon::Stop)).clicked();
        }
    });
    stop
}

/// The ring itself: a faint track and an arc that sweeps and stretches.
pub fn progress_ring(p: &egui::Painter, c: Pos2, d: f32, t: &Tokens, time: f64) {
    let r = d / 2.0;
    let w = (d * 0.055).max(5.0);
    icons::glow(p, c, r * 1.35, t.sec_a.gamma_multiply(0.20), 1.0);
    p.circle_filled(c, r - w * 1.4, Color32::from_black_alpha(70));
    p.circle_stroke(c, r, Stroke::new(w, Color32::from_white_alpha(16)));
    let head = motion::looped(time, motion::SPIN) * 360.0;
    let stretch = motion::ease_in_out((motion::looped(time, motion::SPIN * 2.0) * 2.0 - 1.0).abs());
    let len = 50.0 + 170.0 * stretch;
    let seg = 40;
    for i in 0..seg {
        let f0 = i as f32 / seg as f32;
        let f1 = (i + 1) as f32 / seg as f32;
        let a0 = (head - len + len * f0).to_radians();
        let a1 = (head - len + len * f1).to_radians();
        let col = icons::lerp(t.sec_b, t.sec_a, f1).gamma_multiply(0.25 + 0.75 * f1);
        p.line_segment(
            [
                c + vec2(a0.cos(), a0.sin()) * r,
                c + vec2(a1.cos(), a1.sin()) * r,
            ],
            Stroke::new(w, col),
        );
    }
    let ah = head.to_radians();
    let tip = c + vec2(ah.cos(), ah.sin()) * r;
    icons::glow(p, tip, w * 2.6, t.sec_a.gamma_multiply(0.7), 1.0);
    p.circle_filled(tip, w / 2.0, icons::lerp(t.sec_a, Color32::WHITE, 0.3));
}

/// The Done picture: a disc that grows in and a check that draws itself. `started` is the
/// time the screen first showed.
pub fn done_check(ui: &mut Ui, t: &Tokens, d: f32, started: f64) {
    let (r, _) = ui.allocate_exact_size(vec2(d * 1.4, d), Sense::hover());
    done_check_in(
        ui,
        t,
        Rect::from_center_size(r.center(), Vec2::splat(d)),
        started,
    );
}

/// The growing disc and self-drawing check, painted in `r` (a square).
fn done_check_in(ui: &mut Ui, t: &Tokens, r: Rect, started: f64) {
    let d = r.width();
    let now = ui.input(|i| i.time);
    let k = motion::progress(now, started, motion::CHECK);
    if k < 1.0 {
        ui.ctx().request_repaint();
    }
    let grow = motion::ease_out((k * 1.6).min(1.0));
    let draw = motion::ease_out(((k - 0.35) / 0.65).clamp(0.0, 1.0));
    let p = ui.painter();
    let c = r.center();
    let rad = d / 2.0 * (0.6 + 0.4 * grow);
    icons::glow(p, c, rad * 1.8, t.success.gamma_multiply(0.30 * grow), 1.0);
    icons::gradient_disc(
        p,
        c,
        rad,
        Color32::from_rgb(0x5E, 0xE0, 0x9A),
        Color32::from_rgb(0x1E, 0xA8, 0x84),
    );
    p.circle_stroke(c, rad, Stroke::new(1.5, Color32::from_white_alpha(80)));
    // The check, drawn along its length.
    let pts = [vec2(-0.30, 0.02), vec2(-0.08, 0.24), vec2(0.32, -0.20)];
    let lens = [(pts[1] - pts[0]).length(), (pts[2] - pts[1]).length()];
    let total = lens[0] + lens[1];
    let mut left = total * draw;
    let mut line = vec![c + pts[0] * rad * 2.0];
    for i in 0..2 {
        if left <= 0.0 {
            break;
        }
        let f = (left / lens[i]).min(1.0);
        line.push(c + (pts[i] + (pts[i + 1] - pts[i]) * f) * rad * 2.0);
        left -= lens[i];
    }
    let w = (d * 0.075).max(3.0);
    if line.len() > 1 {
        for q in &line {
            p.circle_filled(*q, w / 2.0, Color32::WHITE);
        }
        p.add(Shape::line(line, Stroke::new(w, Color32::WHITE)));
    }
}

/// The Done picture: the module's object in its finished state with a small check that
/// grows in and draws itself on its lower right. `started` is when the screen first showed.
pub fn done_hero(
    ui: &mut Ui,
    t: &Tokens,
    feature: cleancraft_core::FeatureId,
    d: f32,
    started: f64,
) {
    let r = show_hero(
        ui,
        feature,
        crate::heroes::HeroPhase::Done,
        1.0,
        Vec2::splat(d),
    )
    .rect;
    let c = d * 0.3;
    let badge = Rect::from_center_size(r.center() + vec2(d * 0.27, d * 0.27), Vec2::splat(c));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(badge));
    done_check_in(&mut child, t, badge, started);
}

/// When a screen identified by `key` first showed (for one-time animations).
pub fn first_seen(ui: &Ui, key: impl std::hash::Hash + std::fmt::Debug) -> f64 {
    let now = ui.input(|i| i.time);
    let id = egui::Id::new(("first-seen", key));
    ui.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || now))
}

/// Horizontal bar with rounded ends; `fraction` of it filled with a gradient of `color`.
pub fn bar(ui: &mut Ui, t: &Tokens, fraction: f32, color: Color32, width: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    paint_bar(ui.painter(), r, t, fraction, color);
}

pub fn paint_bar(p: &egui::Painter, r: Rect, _t: &Tokens, fraction: f32, color: Color32) {
    p.rect_filled(r, r.height() / 2.0, Color32::from_white_alpha(14));
    let f = fraction.clamp(0.0, 1.0);
    if f <= 0.0 {
        return;
    }
    let mut fr = r;
    fr.set_width((r.width() * f).max(r.height()));
    icons::gradient_tile(
        p,
        fr,
        r.height() / 2.0,
        icons::lerp(color, Color32::WHITE, 0.15),
        color,
    );
}

/// A semicircle gauge with a value from 0 to 1.
pub fn gauge(p: &egui::Painter, c: Pos2, r: f32, value: f32, color: Color32) {
    let w = (r * 0.16).max(5.0);
    let arc = |from: f32, to: f32| -> Vec<Pos2> {
        let n = 48;
        (0..=n)
            .map(|i| {
                let a = (from + (to - from) * i as f32 / n as f32).to_radians();
                c + vec2(a.cos(), a.sin()) * r
            })
            .collect()
    };
    p.add(Shape::line(
        arc(180.0, 360.0),
        Stroke::new(w, Color32::from_white_alpha(18)),
    ));
    let v = value.clamp(0.0, 1.0);
    if v > 0.0 {
        let pts = arc(180.0, 180.0 + 180.0 * v);
        let n = pts.len();
        for i in 0..n - 1 {
            let col = icons::lerp(color.gamma_multiply(0.55), color, i as f32 / n as f32);
            p.line_segment([pts[i], pts[i + 1]], Stroke::new(w, col));
        }
        p.circle_filled(pts[0], w / 2.0, color.gamma_multiply(0.55));
        p.circle_filled(pts[n - 1], w / 2.0, color);
    }
}

/// A filled sparkline (values 0..=100) in `r`.
pub fn sparkline(p: &egui::Painter, r: Rect, values: &[f64], slots: usize, color: Color32) {
    p.rect_filled(r, radius::S, Color32::from_black_alpha(50));
    if values.len() < 2 {
        // Not enough samples for a line yet: a dashed baseline and a word, not an empty box.
        let y = r.bottom() - 6.0;
        p.add(Shape::dashed_line(
            &[pos2(r.left() + 8.0, y), pos2(r.right() - 8.0, y)],
            Stroke::new(1.0, color.gamma_multiply(0.45)),
            4.0,
            4.0,
        ));
        p.text(
            r.center() - vec2(0.0, 3.0),
            Align2::CENTER_CENTER,
            "Collecting",
            font(size::FOOTNOTE, Weight::Medium),
            Color32::from_rgb(0x93, 0x9D, 0xAA),
        );
        return;
    }
    let n = slots.max(values.len()) as f32;
    let start = n - values.len() as f32;
    let pts: Vec<Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = r.left() + (start + i as f32) / (n - 1.0) * r.width();
            let y = r.bottom() - (*v as f32 / 100.0).clamp(0.0, 1.0) * (r.height() - 4.0) - 2.0;
            pos2(x, y)
        })
        .collect();
    // Area under the line, fading down.
    let mut m = egui::epaint::Mesh::default();
    for q in &pts {
        let top = color.gamma_multiply(0.35);
        m.colored_vertex(*q, top);
        m.colored_vertex(pos2(q.x, r.bottom()), color.gamma_multiply(0.0));
    }
    for i in 0..pts.len() as u32 - 1 {
        let a = i * 2;
        m.add_triangle(a, a + 1, a + 2);
        m.add_triangle(a + 1, a + 3, a + 2);
    }
    p.add(Shape::mesh(m));
    p.add(Shape::line(pts.clone(), Stroke::new(1.8, color)));
    if let Some(last) = pts.last() {
        icons::glow(p, *last, 9.0, color.gamma_multiply(0.6), 1.0);
        p.circle_filled(*last, 2.6, color);
    }
}

pub fn risk_color(t: &Tokens, r: cleancraft_core::Risk) -> Color32 {
    match r {
        cleancraft_core::Risk::Low => t.success,
        cleancraft_core::Risk::Medium => t.warning,
        cleancraft_core::Risk::High => t.danger,
    }
}

/// Icon and colour for each module. Icons are vector shapes (never font glyphs).
pub fn feature_look(_t: &Tokens, f: cleancraft_core::FeatureId) -> (Icon, Color32) {
    use cleancraft_core::FeatureId as F;
    let (a, b) = Section::of(f).colors();
    let icon = match f {
        F::SmartCare => Icon::Sweep,
        F::SystemJunk => Icon::Junk,
        F::MailAttachments => Icon::Mail,
        F::TrashBins => Icon::Trash,
        F::LargeOldFiles => Icon::LargeOld,
        F::Duplicates => Icon::Duplicates,
        F::SimilarPhotos => Icon::Similar,
        F::Uninstaller => Icon::Remover,
        F::Updater => Icon::Updates,
        F::LoginItems => Icon::Startup,
        F::Maintenance => Icon::Tasks,
        F::Malware => Icon::Shield,
        F::Privacy => Icon::Privacy,
        F::SpaceLens => Icon::DiskMap,
        F::Monitor => Icon::Monitor,
    };
    // Alternate the two section colours so neighbours in the sidebar differ a little.
    let color = match f {
        F::MailAttachments | F::Privacy | F::LoginItems | F::Updater | F::Duplicates => b,
        F::SpaceLens => icons::lerp(a, b, 0.5),
        _ => a,
    };
    (icon, color)
}

/// Galley helper for painting a single line.
pub fn galley(ui: &Ui, text: &str, fid: FontId, color: Color32) -> Arc<Galley> {
    ui.painter().layout_no_wrap(text.to_string(), fid, color)
}

/// Center a horizontal row: remembers the row's last width under `id`.
pub fn centered_row<R>(ui: &mut Ui, id: egui::Id, add: impl FnOnce(&mut Ui) -> R) -> R {
    let avail = ui.available_width();
    let last: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(0.0);
    ui.horizontal(|ui| {
        ui.add_space(((avail - last) / 2.0).max(0.0));
        let start = ui.cursor().min.x;
        let r = add(ui);
        let w = ui.min_rect().right() - start;
        if (w - last).abs() > 0.5 {
            ui.data_mut(|d| d.insert_temp(id, w));
            ui.ctx().request_repaint();
        }
        r
    })
    .inner
}

/// A confirmation dialog in CleanCraft's style.
pub fn modal(id: egui::Id, t: &Tokens) -> egui::Modal {
    egui::Modal::new(id)
        .frame(dialog_frame(t))
        .backdrop_color(Color32::from_rgba_unmultiplied(4, 6, 10, 170))
}

/// A dialog's top: icon tile in `color`, headline, one dim line.
pub fn dialog_head(ui: &mut Ui, t: &Tokens, icon: Icon, color: Color32, headline: &str, sub: &str) {
    ui.horizontal(|ui| {
        badge(ui, color, icon, isz::DIALOG);
        ui.add_space(space::S);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = space::XS;
            ui.add(
                egui::Label::new(
                    RichText::new(headline)
                        .font(font(size::TITLE2, Weight::Bold))
                        .color(t.text),
                )
                .wrap(),
            );
            if !sub.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new(sub)
                            .font(font(size::CALLOUT, Weight::Regular))
                            .color(t.text_dim),
                    )
                    .wrap(),
                );
            }
        });
    });
}

/// A coloured strip with an icon and a line of text (warnings inside pages and dialogs).
pub fn strip(ui: &mut Ui, icon: Icon, color: Color32, text: &str) {
    Frame::new()
        .fill(color.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, color.gamma_multiply(0.35)))
        .corner_radius(CornerRadius::same(radius::M))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(isz::INLINE), Sense::hover());
                icons::paint(ui.painter(), r, icon, color);
                ui.add_space(space::XS);
                ui.add(
                    egui::Label::new(
                        RichText::new(text)
                            .font(font(size::CALLOUT, Weight::Regular))
                            .color(icons::lerp(color, Color32::WHITE, 0.55)),
                    )
                    .wrap(),
                );
            });
        });
}

/// A label placed in `r`, showing `shown` but announcing `full` to accessibility (and
/// tests), so shortened paths keep their real text.
pub fn text_at(
    ui: &mut Ui,
    r: Rect,
    shown: &str,
    full: &str,
    fid: FontId,
    color: Color32,
    align: egui::Align,
) -> Response {
    let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_align(align);
    let resp = place_with(ui, r, layout, |ui| {
        ui.add(
            egui::Label::new(RichText::new(shown).font(fid).color(color))
                .selectable(false)
                .truncate(),
        )
    });
    let full = full.to_string();
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, full.clone()));
    resp
}

/// A secondary line that shows `shown` and announces `full` (the bottom bar shows "27 items
/// selected" next to the big size, and VoiceOver still hears the size).
pub fn quiet_label(ui: &mut Ui, t: &Tokens, shown: &str, full: &str) -> Response {
    let fid = font(size::CALLOUT, Weight::Medium);
    let w = galley(ui, shown, fid.clone(), t.text_dim).size().x + 2.0;
    let (r, _) = ui.allocate_exact_size(vec2(w, 20.0), Sense::hover());
    text_at(ui, r, shown, full, fid, t.text_dim, egui::Align::Min)
}

/// One line of a confirmation list: kind icon, path (shortened in the middle), reason, size.
pub fn confirm_line(
    ui: &mut Ui,
    t: &Tokens,
    path: &std::path::Path,
    shown: &str,
    reason: &str,
    size_text: &str,
) {
    let w = ui.available_width();
    let (r, _) = ui.allocate_exact_size(vec2(w, hit::CONFIRM_LINE), Sense::hover());
    let ir = Rect::from_center_size(
        pos2(r.left() + 10.0, r.center().y),
        Vec2::splat(isz::INLINE),
    );
    crate::images::paint_file_icon(ui, ir, path, crate::file_icons::FileKind::of(path));
    let size_font = font(size::CALLOUT, Weight::SemiBold);
    // Fixed columns, so reasons start on one line from row to row.
    let size_w = galley(ui, size_text, size_font.clone(), t.text)
        .size()
        .x
        .max(72.0);
    let avail = r.width() - 28.0 - size_w - 16.0;
    let path_font = font(size::CALLOUT, Weight::Regular);
    let reason_font = font(size::FOOTNOTE, Weight::Regular);
    let reason_w = (avail * 0.38).min(260.0);
    let path_w = avail - reason_w - 12.0;
    let path_s = truncate_middle(ui.ctx(), shown, &path_font, path_w);
    let reason_s = truncate_end(ui.ctx(), reason, &reason_font, reason_w);
    let x0 = r.left() + 26.0;
    text_at(
        ui,
        Rect::from_min_size(pos2(x0, r.top()), vec2(path_w, r.height())),
        &path_s,
        shown,
        path_font,
        t.text,
        egui::Align::Min,
    );
    text_at(
        ui,
        Rect::from_min_size(
            pos2(x0 + path_w + 12.0, r.top()),
            vec2(reason_w, r.height()),
        ),
        &reason_s,
        reason,
        reason_font,
        t.text_dim,
        egui::Align::Min,
    );
    text_at(
        ui,
        Rect::from_min_size(
            pos2(r.right() - size_w - 6.0, r.top()),
            vec2(size_w + 4.0, r.height()),
        ),
        size_text,
        size_text,
        size_font,
        t.text,
        egui::Align::Max,
    );
}

/// CleanCraft's checkbox: a rounded box that fills with the section gradient and draws a
/// check. Accessible as a checkbox named `name`. Returns the response; `changed()` tells
/// when the user toggled it (`on` holds the new value).
pub fn check(ui: &mut Ui, t: &Tokens, on: &mut bool, name: &str) -> Response {
    let (r, mut resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
    let enabled = ui.is_enabled();
    if resp.clicked() && enabled {
        *on = !*on;
        resp.mark_changed();
    }
    let state = *on;
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, state, name));
    paint_check(ui, t, r, &resp, state, enabled);
    resp
}

/// A checkbox that can also show a mixed state (a dash): a group where only some members are
/// on. Clicking a mixed box turns it on. Accessible as a checkbox named `name`.
pub fn check_mixed(ui: &mut Ui, t: &Tokens, on: &mut bool, mixed: bool, name: &str) -> Response {
    let (r, mut resp) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::click());
    let enabled = ui.is_enabled();
    if resp.clicked() && enabled {
        *on = !*on;
        resp.mark_changed();
    }
    let state = *on;
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, state, name));
    paint_box(ui, t, r, &resp, state, mixed && !state, enabled);
    resp
}

/// Paint a checkbox box in `r` for `resp`.
pub fn paint_check(ui: &Ui, t: &Tokens, r: Rect, resp: &Response, on: bool, enabled: bool) {
    paint_box(ui, t, r, resp, on, false, enabled);
}

fn paint_box(ui: &Ui, t: &Tokens, r: Rect, resp: &Response, on: bool, mixed: bool, enabled: bool) {
    let ctx = ui.ctx();
    let k = anim(ctx, resp.id.with("on"), on || mixed, motion::HOVER);
    let hov = anim(
        ctx,
        resp.id.with("hov"),
        resp.hovered() && enabled,
        motion::HOVER,
    );
    let mut p = ui.painter().clone();
    if !enabled {
        p.set_opacity(0.35);
    }
    let b = Rect::from_center_size(r.center(), Vec2::splat(hit::CHECK));
    p.rect_filled(b, 5.0, Color32::from_black_alpha(60));
    p.rect_stroke(
        b,
        5.0,
        Stroke::new(
            1.3,
            icons::lerp(Color32::from_white_alpha(70), t.sec_a, hov.max(k)),
        ),
        egui::StrokeKind::Inside,
    );
    if k > 0.0 {
        let fill = Rect::from_center_size(b.center(), b.size() * (0.6 + 0.4 * k));
        icons::gradient_tile(
            &p,
            fill,
            5.0,
            t.sec_a.gamma_multiply(k),
            t.sec_b.gamma_multiply(k),
        );
        if mixed {
            let y = b.center().y;
            p.line_segment(
                [pos2(b.left() + 4.5, y), pos2(b.right() - 4.5, y)],
                Stroke::new(2.0, t.on_section().gamma_multiply(k)),
            );
        } else {
            let ir = b.shrink(2.5);
            icons::paint_with(&p, ir, Icon::Check, t.on_section().gamma_multiply(k), 2.0);
        }
    }
}

/// A checkbox with its text to the right; the text is its accessible name.
pub fn check_text(ui: &mut Ui, t: &Tokens, on: &mut bool, text: &str) -> Response {
    check_text_font(ui, t, on, text, font(size::CALLOUT, Weight::Medium), false)
}

/// `check_text` with a chosen font; `strong` keeps the text bright.
pub fn check_text_font(
    ui: &mut Ui,
    t: &Tokens,
    on: &mut bool,
    text: &str,
    fid: FontId,
    strong: bool,
) -> Response {
    let g = galley(ui, text, fid, t.text_dim);
    let (r, mut resp) = ui.allocate_exact_size(vec2(28.0 + g.size().x, hit::MIN), Sense::click());
    let enabled = ui.is_enabled();
    if resp.clicked() && enabled {
        *on = !*on;
        resp.mark_changed();
    }
    let state = *on;
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, state, text));
    let br = Rect::from_min_size(pos2(r.min.x, r.center().y - 11.0), Vec2::splat(22.0));
    paint_check(ui, t, br, &resp, state, enabled);
    let color = if resp.hovered() || strong {
        t.text
    } else {
        t.text_dim
    };
    let color = if enabled { color } else { t.text_faint };
    ui.painter().galley(
        pos2(r.left() + 28.0, r.center().y - g.size().y / 2.0),
        g,
        color,
    );
    resp
}

/// The floating action bar pinned to the bottom of the page (stays visible at 680 pt).
pub fn bottom_bar(ui: &mut Ui, t: &Tokens, id: egui::Id, add: impl FnOnce(&mut Ui)) {
    egui::Panel::bottom(id)
        .show_separator_line(false)
        .frame(Frame::new().inner_margin(Margin {
            left: 0,
            right: 0,
            top: 10,
            bottom: 0,
        }))
        .show(ui, |ui| {
            // The bar's shadow falls into the page margin; without this the panel's clip
            // cuts it into a square slab under the rounded corners.
            ui.set_clip_rect(ui.ctx().content_rect());
            bar_frame(t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let w = ui.available_width();
                ui.allocate_ui_with_layout(
                    vec2(w, 44.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_height(44.0);
                        add(ui);
                    },
                );
            });
        });
}

/// CleanCraft's radio button: a ring that fills with the section gradient and an ink dot,
/// with its text to the right (the accessible name). Returns the response; `clicked()` means
/// the user picked it.
pub fn radio(ui: &mut Ui, t: &Tokens, on: bool, text: &str) -> Response {
    let fid = font(size::CALLOUT, Weight::Medium);
    let g = galley(ui, text, fid, t.text_dim);
    let (r, resp) = ui.allocate_exact_size(vec2(28.0 + g.size().x, hit::MIN), Sense::click());
    let enabled = ui.is_enabled();
    resp.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, enabled, on, text));
    let ctx = ui.ctx();
    let k = anim(ctx, resp.id.with("on"), on, motion::HOVER);
    let hov = anim(
        ctx,
        resp.id.with("hov"),
        resp.hovered() && enabled,
        motion::HOVER,
    );
    let mut p = ui.painter().clone();
    if !enabled {
        p.set_opacity(0.4);
    }
    let c = pos2(r.left() + 11.0, r.center().y);
    let rad = hit::CHECK / 2.0;
    p.circle_filled(c, rad, Color32::from_black_alpha(60));
    // Ring at 3.5:1 against the card (WCAG 1.4.11), section colour on hover.
    p.circle_stroke(
        c,
        rad - 0.75,
        Stroke::new(
            1.5,
            icons::lerp(Color32::from_rgb(0x70, 0x7A, 0x8A), t.sec_a, hov.max(k)),
        ),
    );
    if k > 0.0 {
        icons::gradient_disc(
            &p,
            c,
            rad * (0.6 + 0.4 * k),
            t.sec_a.gamma_multiply(k),
            t.sec_b.gamma_multiply(k),
        );
        p.circle_filled(c, 3.2 * k, t.on_section());
    }
    let color = if resp.hovered() || on {
        t.text
    } else {
        t.text_dim
    };
    p.galley(
        pos2(r.left() + 28.0, r.center().y - g.size().y / 2.0),
        g,
        color,
    );
    resp
}

/// A search field in a toolbar: rounded, a magnifier, `FIELD` tall. The caller names it for
/// accessibility (`widget_info`) and reacts to `changed()`.
pub fn search_field(
    ui: &mut Ui,
    t: &Tokens,
    text: &mut String,
    hint: &str,
    width: f32,
) -> Response {
    let (r, _) = ui.allocate_exact_size(vec2(width, hit::FIELD), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, r.height() / 2.0, Color32::from_black_alpha(70));
    p.rect_stroke(
        r,
        r.height() / 2.0,
        Stroke::new(1.0, Color32::from_white_alpha(26)),
        egui::StrokeKind::Inside,
    );
    let ir = Rect::from_center_size(pos2(r.left() + 18.0, r.center().y), Vec2::splat(15.0));
    icons::paint(p, ir, Icon::Search, t.text_faint);
    let field = Rect::from_min_max(
        pos2(r.left() + 30.0, r.top()),
        pos2(r.right() - 10.0, r.bottom()),
    );
    let mut out = None;
    place_with(
        ui,
        field,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            out = Some(
                ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text(RichText::new(hint).color(t.text_faint))
                        .font(font(size::CALLOUT, Weight::Regular))
                        .frame(egui::Frame::NONE)
                        .background_color(Color32::TRANSPARENT)
                        .desired_width(field.width())
                        .margin(Margin::symmetric(2, 0)),
                ),
            );
        },
    );
    let resp = out.unwrap_or_else(|| ui.interact(r, ui.id().with(hint), Sense::hover()));
    if resp.has_focus() {
        ui.painter().rect_stroke(
            r,
            r.height() / 2.0,
            Stroke::new(1.5, t.sec_a.gamma_multiply(0.8)),
            egui::StrokeKind::Inside,
        );
    }
    resp
}

/// A pop-up menu for choosing a sort order (or any small set of values): a pill with the
/// current choice and up/down chevrons, `FIELD` tall. Accessible name "Sort" plus `id`'s
/// own salt for egui.
pub fn sort_menu<T: Copy + PartialEq>(ui: &mut Ui, id: &str, value: &mut T, choices: &[(T, &str)]) {
    let current = choices
        .iter()
        .find(|(v, _)| v == value)
        .map(|(_, l)| *l)
        .unwrap_or("");
    ui.scope(|ui| {
        let v = &mut ui.style_mut().visuals.widgets;
        for w in [&mut v.inactive, &mut v.hovered, &mut v.active, &mut v.open] {
            w.corner_radius = CornerRadius::same((hit::FIELD / 2.0) as u8);
        }
        ui.spacing_mut().interact_size.y = hit::FIELD;
        ui.spacing_mut().button_padding = vec2(14.0, 7.0);
        egui::ComboBox::from_id_salt(id)
            .selected_text(RichText::new(current).font(font(size::CALLOUT, Weight::Medium)))
            .icon(|ui, rect, visuals, _open| {
                let r = Rect::from_center_size(rect.center(), Vec2::splat(13.0));
                icons::paint(ui.painter(), r, Icon::UpDown, visuals.fg_stroke.color);
            })
            .show_ui(ui, |ui| {
                for (v, l) in choices {
                    ui.selectable_value(value, *v, *l);
                }
            })
            .response
            .widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, "Sort"));
    });
}

/// A pop-up menu behind a secondary pill (`FIELD` tall, glass, rounded ends).
pub fn menu_pill<R>(
    ui: &mut Ui,
    t: &Tokens,
    label: &str,
    add: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<Option<R>> {
    ui.scope(|ui| {
        let v = &mut ui.style_mut().visuals.widgets;
        for w in [&mut v.inactive, &mut v.hovered, &mut v.active, &mut v.open] {
            w.corner_radius = CornerRadius::same((hit::FIELD / 2.0) as u8);
        }
        v.inactive.weak_bg_fill = t.glass;
        v.inactive.bg_stroke = Stroke::new(1.0, t.glass_edge);
        ui.spacing_mut().interact_size.y = hit::FIELD;
        ui.spacing_mut().button_padding = vec2(16.0, 7.0);
        ui.menu_button(
            RichText::new(label).font(font(size::CALLOUT, Weight::Medium)),
            add,
        )
    })
    .inner
}

/// A plain text field in CleanCraft's style (`FIELD` tall, rounded). The caller names it.
pub fn text_field(ui: &mut Ui, t: &Tokens, text: &mut String, hint: &str, width: f32) -> Response {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(RichText::new(hint).color(t.text_faint))
            .font(font(size::CALLOUT, Weight::Regular))
            .desired_width(width)
            .min_size(vec2(width, hit::FIELD))
            .vertical_align(egui::Align::Center)
            .margin(Margin::symmetric(12, 6)),
    )
}

/// A secondary pill, returning its response (to rename it for accessibility).
pub fn secondary_resp(ui: &mut Ui, t: &Tokens, label: &str) -> Response {
    pill(ui, t, label, Kind::Secondary, None)
}

/// Lay out `add` inside `r` without taking space in `ui` (for things drawn over a row or a
/// tile that already has its place). The default layout is top-down.
pub fn place<R>(ui: &mut Ui, r: Rect, add: impl FnOnce(&mut Ui) -> R) -> R {
    place_with(ui, r, egui::Layout::top_down(egui::Align::Min), add)
}

/// `place` with a layout.
pub fn place_with<R>(
    ui: &mut Ui,
    r: Rect,
    layout: egui::Layout,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r).layout(layout));
    add(&mut child)
}
