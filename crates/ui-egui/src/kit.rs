//! Small building blocks shared by every page.

use egui::{Align2, Color32, CornerRadius, Frame, Margin, RichText, Sense, Stroke, Ui, Vec2, vec2};

use crate::theme::{Tokens, Weight, font};

/// The accent "primary" button. Returns true when clicked.
pub fn primary(ui: &mut Ui, t: &Tokens, label: &str) -> bool {
    let b = egui::Button::new(
        RichText::new(label)
            .color(t.on_accent)
            .font(font(14.0, Weight::SemiBold)),
    )
    .fill(t.accent)
    .corner_radius(CornerRadius::same(18))
    .min_size(vec2(120.0, 36.0));
    ui.add(b).clicked()
}

/// A quiet secondary button.
pub fn secondary(ui: &mut Ui, t: &Tokens, label: &str) -> bool {
    let b = egui::Button::new(
        RichText::new(label)
            .color(t.text)
            .font(font(13.0, Weight::Medium)),
    )
    .fill(t.card_hover)
    .stroke(Stroke::new(1.0, t.border))
    .corner_radius(CornerRadius::same(14))
    .min_size(vec2(84.0, 30.0));
    ui.add(b).clicked()
}

/// A destructive-looking button (still never deletes: CleanCraft only moves).
pub fn danger(ui: &mut Ui, t: &Tokens, label: &str) -> bool {
    let b = egui::Button::new(
        RichText::new(label)
            .color(Color32::WHITE)
            .font(font(14.0, Weight::SemiBold)),
    )
    .fill(t.danger)
    .corner_radius(CornerRadius::same(18))
    .min_size(vec2(120.0, 36.0));
    ui.add(b).clicked()
}

pub fn card(t: &Tokens) -> Frame {
    Frame::new()
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(14))
}

pub fn title(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(
        RichText::new(text)
            .font(font(24.0, Weight::Bold))
            .color(t.text),
    );
}

pub fn subtitle(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(
        RichText::new(text)
            .font(font(13.5, Weight::Regular))
            .color(t.text_dim),
    );
}

pub fn dim(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(
        RichText::new(text)
            .color(t.text_faint)
            .font(font(11.5, Weight::Regular)),
    );
}

/// A round badge with a glyph, used as each module's picture.
pub fn badge(ui: &mut Ui, color: Color32, glyph: &str, size: f32) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter();
    p.circle_filled(r.center(), size / 2.0, color.gamma_multiply(0.22));
    p.circle_stroke(r.center(), size / 2.0 - 1.0, Stroke::new(2.0, color));
    p.text(
        r.center(),
        Align2::CENTER_CENTER,
        glyph,
        font(size * 0.42, Weight::Bold),
        color,
    );
}

/// Thin horizontal usage bar.
pub fn bar(ui: &mut Ui, t: &Tokens, fraction: f32, color: Color32, width: f32) {
    let (r, _) = ui.allocate_exact_size(vec2(width, 8.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, CornerRadius::same(4), t.well);
    let mut f = r;
    f.set_width(r.width() * fraction.clamp(0.0, 1.0));
    p.rect_filled(f, CornerRadius::same(4), color);
}

pub fn risk_color(t: &Tokens, r: cleancraft_core::Risk) -> Color32 {
    match r {
        cleancraft_core::Risk::Low => t.success,
        cleancraft_core::Risk::Medium => t.warning,
        cleancraft_core::Risk::High => t.danger,
    }
}

/// Glyph and color for each module.
pub fn feature_look(t: &Tokens, f: cleancraft_core::FeatureId) -> (&'static str, Color32) {
    use cleancraft_core::FeatureId as F;
    match f {
        F::SmartCare => ("✦", t.accent),
        F::SystemJunk => ("◍", Color32::from_rgb(0x5B, 0xB6, 0xF5)),
        F::MailAttachments => ("✉", Color32::from_rgb(0x8A, 0xA4, 0xF8)),
        F::TrashBins => ("⌫", Color32::from_rgb(0x9C, 0xA3, 0xAF)),
        F::LargeOldFiles => ("▤", Color32::from_rgb(0xF2, 0xB1, 0x3C)),
        F::Duplicates => ("⧉", Color32::from_rgb(0xF0, 0x8A, 0x5D)),
        F::SimilarPhotos => ("◩", Color32::from_rgb(0xE8, 0x79, 0xC2)),
        F::Uninstaller => ("⊟", Color32::from_rgb(0xEF, 0x5B, 0x5B)),
        F::Updater => ("⟳", Color32::from_rgb(0x4C, 0xC9, 0x7A)),
        F::LoginItems => ("⏻", Color32::from_rgb(0xB4, 0x8C, 0xF2)),
        F::Maintenance => ("⚙", Color32::from_rgb(0x2E, 0xC4, 0xA6)),
        F::Malware => ("⛨", Color32::from_rgb(0xF2, 0x6D, 0x6D)),
        F::Privacy => ("◐", Color32::from_rgb(0x7C, 0xC4, 0xE8)),
        F::SpaceLens => ("◎", Color32::from_rgb(0x6F, 0xD3, 0xE0)),
        F::Monitor => ("∿", Color32::from_rgb(0x9B, 0xE0, 0x6A)),
    }
}
