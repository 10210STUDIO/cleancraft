//! CleanCraft's own look: graphite surfaces, a mint accent, Inter type (SIL OFL).

use craft_ui::theme::WidgetPalette;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Visuals,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub bg: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    pub card_hover: Color32,
    pub border: Color32,
    pub well: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_dim: Color32,
    pub on_accent: Color32,
    pub danger: Color32,
    pub warning: Color32,
    pub success: Color32,
    pub overlay: Color32,
}

impl Tokens {
    pub fn dark() -> Tokens {
        Tokens {
            bg: Color32::from_rgb(0x14, 0x17, 0x1B),
            sidebar: Color32::from_rgb(0x0F, 0x12, 0x15),
            card: Color32::from_rgb(0x1C, 0x21, 0x26),
            card_hover: Color32::from_rgb(0x24, 0x2A, 0x31),
            border: Color32::from_rgba_unmultiplied(255, 255, 255, 22),
            well: Color32::from_rgb(0x11, 0x14, 0x18),
            text: Color32::from_rgb(0xEC, 0xEF, 0xF1),
            text_dim: Color32::from_rgb(0xA9, 0xB1, 0xBA),
            text_faint: Color32::from_rgb(0x6E, 0x78, 0x83),
            accent: Color32::from_rgb(0x2E, 0xC4, 0xA6),
            accent_dim: Color32::from_rgba_unmultiplied(0x2E, 0xC4, 0xA6, 40),
            on_accent: Color32::from_rgb(0x06, 0x1A, 0x16),
            danger: Color32::from_rgb(0xEF, 0x5B, 0x5B),
            warning: Color32::from_rgb(0xF2, 0xB1, 0x3C),
            success: Color32::from_rgb(0x4C, 0xC9, 0x7A),
            overlay: Color32::from_black_alpha(150),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Weight {
    Regular,
    Medium,
    SemiBold,
    Bold,
}

pub fn font(size: f32, w: Weight) -> FontId {
    let family = match w {
        Weight::Regular => FontFamily::Proportional,
        Weight::Medium => FontFamily::Name("medium".into()),
        Weight::SemiBold => FontFamily::Name("semibold".into()),
        Weight::Bold => FontFamily::Name("bold".into()),
    };
    FontId::new(size, family)
}

/// Install Inter (SIL Open Font License, see assets/fonts/OFL-Inter.txt) in four weights.
pub fn install_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let faces: [(&str, &'static [u8]); 4] = [
        (
            "inter-regular",
            include_bytes!("../../../assets/fonts/Inter-Regular.ttf"),
        ),
        (
            "inter-medium",
            include_bytes!("../../../assets/fonts/Inter-Medium.ttf"),
        ),
        (
            "inter-semibold",
            include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"),
        ),
        (
            "inter-bold",
            include_bytes!("../../../assets/fonts/Inter-Bold.ttf"),
        ),
    ];
    for (name, bytes) in faces {
        defs.font_data.insert(
            name.to_string(),
            std::sync::Arc::new(FontData::from_static(bytes)),
        );
    }
    let fallbacks: Vec<String> = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let chain = |first: &str| {
        let mut v = vec![first.to_string()];
        v.extend(fallbacks.iter().cloned());
        v
    };
    defs.families
        .insert(FontFamily::Proportional, chain("inter-regular"));
    defs.families
        .insert(FontFamily::Name("medium".into()), chain("inter-medium"));
    defs.families
        .insert(FontFamily::Name("semibold".into()), chain("inter-semibold"));
    defs.families
        .insert(FontFamily::Name("bold".into()), chain("inter-bold"));
    ctx.set_fonts(defs);
}

pub fn apply(ctx: &egui::Context, t: &Tokens) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = t.bg;
    visuals.window_fill = t.card;
    visuals.window_stroke = Stroke::new(1.0, t.border);
    visuals.extreme_bg_color = t.well;
    visuals.faint_bg_color = t.card;
    visuals.selection.bg_fill = t.accent.gamma_multiply(0.45);
    visuals.selection.stroke = Stroke::new(1.0, t.accent);
    visuals.hyperlink_color = t.accent;
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.override_text_color = Some(t.text);
    WidgetPalette {
        noninteractive: t.card,
        inactive: t.card_hover,
        hovered: t.card_hover,
        active: t.accent.gamma_multiply(0.4),
        open: t.card_hover,
    }
    .apply_to(
        &mut visuals.widgets,
        CornerRadius::same(7),
        Stroke::new(1.0, t.text),
    );
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, t.border);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, t.accent);
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 26.0;
        style.text_styles = [
            (TextStyle::Small, font(11.0, Weight::Regular)),
            (TextStyle::Body, font(13.0, Weight::Regular)),
            (TextStyle::Button, font(13.0, Weight::Medium)),
            (TextStyle::Heading, font(22.0, Weight::Bold)),
            (
                TextStyle::Monospace,
                FontId::new(12.0, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}
