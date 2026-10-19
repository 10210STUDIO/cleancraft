//! CleanCraft's own look: deep ink surfaces, a mint accent, per-section colour fields, glass
//! cards and Inter type (SIL OFL). Every design token lives here: colours, spacing, radii,
//! shadows, type sizes and motion timings. Pages and `kit` read them, never their own numbers.

use craft_ui::theme::WidgetPalette;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke,
    TextStyle, Visuals,
};

/// Spacing scale (points).
pub mod space {
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    pub const L: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
    /// Width of the sidebar.
    pub const SIDEBAR: f32 = 236.0;
    /// Margin around the content area.
    pub const PAGE_X: i8 = 28;
    pub const PAGE_Y: i8 = 20;
}

/// Corner radii.
pub mod radius {
    pub const S: u8 = 8;
    pub const M: u8 = 12;
    pub const L: u8 = 16;
    pub const XL: u8 = 22;
    /// Fully round ends.
    pub const PILL: u8 = 255;
}

/// Type scale (points).
pub mod size {
    /// Big numbers: totals, freed space.
    pub const DISPLAY: f32 = 46.0;
    /// Numbers inside cards and tiles.
    pub const NUMBER: f32 = 26.0;
    pub const HERO_TITLE: f32 = 28.0;
    pub const TITLE: f32 = 21.0;
    pub const H2: f32 = 15.0;
    pub const BODY: f32 = 13.5;
    pub const SMALL: f32 = 11.5;
    pub const TINY: f32 = 10.5;
}

/// Motion: every animation's duration and easing. `REDUCE_MOTION` turns movement off
/// (fades become instant, loops stand still).
pub mod motion {
    pub const REDUCE_MOTION: bool = false;
    /// Page change: fade and short slide.
    pub const PAGE: f32 = 0.18;
    /// Distance of the page slide (points).
    pub const PAGE_SLIDE: f32 = 10.0;
    /// Hover highlight in and out.
    pub const HOVER: f32 = 0.12;
    /// Button press scale.
    pub const PRESS: f32 = 0.08;
    pub const PRESS_SCALE: f32 = 0.965;
    /// One breath of the primary action's halo.
    pub const PULSE: f32 = 2.6;
    /// One turn of the progress ring's sweep.
    pub const SPIN: f32 = 1.4;
    /// The check mark drawn on the Done screen.
    pub const CHECK: f32 = 0.6;
    /// Slow drift of the illustration sparkles.
    pub const DRIFT: f32 = 9.0;

    /// Ease-out cubic: fast start, gentle landing.
    pub fn ease_out(x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        1.0 - (1.0 - x).powi(3)
    }

    /// Ease-in-out sine, for loops.
    pub fn ease_in_out(x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        0.5 - 0.5 * (std::f32::consts::PI * x).cos()
    }

    /// Progress (0..=1) of an animation of `duration` that started at `start` (seconds).
    pub fn progress(now: f64, start: f64, duration: f32) -> f32 {
        if REDUCE_MOTION || duration <= 0.0 {
            return 1.0;
        }
        (((now - start) as f32) / duration).clamp(0.0, 1.0)
    }

    /// Phase (0..1) of a loop of `period` seconds; constant with reduced motion.
    pub fn looped(now: f64, period: f32) -> f32 {
        if REDUCE_MOTION {
            return 0.0;
        }
        ((now / period as f64).fract()) as f32
    }
}

/// The sidebar sections. Each one has its own colour field behind the content.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Overview,
    Clean,
    Protect,
    Speed,
    Apps,
    Files,
    System,
    /// History and Settings.
    Neutral,
}

impl Section {
    pub fn of(f: cleancraft_core::FeatureId) -> Section {
        use cleancraft_core::FeatureId as F;
        match f {
            F::SmartCare => Section::Overview,
            F::SystemJunk | F::MailAttachments | F::TrashBins => Section::Clean,
            F::Malware | F::Privacy => Section::Protect,
            F::Maintenance | F::LoginItems => Section::Speed,
            F::Uninstaller | F::Updater => Section::Apps,
            F::LargeOldFiles | F::Duplicates | F::SimilarPhotos | F::SpaceLens => Section::Files,
            F::Monitor => Section::System,
        }
    }

    /// (main colour, second colour) of the section's gradient.
    pub fn colors(self) -> (Color32, Color32) {
        match self {
            // Mint into deep sea green.
            Section::Overview => (rgb(0x34, 0xD8, 0xB4), rgb(0x1E, 0x7F, 0xA8)),
            // Glacier blue into cyan.
            Section::Clean => (rgb(0x4E, 0xA8, 0xFF), rgb(0x2B, 0xC8, 0xD8)),
            // Coral into ember.
            Section::Protect => (rgb(0xFF, 0x6B, 0x6B), rgb(0xE8, 0x8A, 0x3A)),
            // Lime into teal.
            Section::Speed => (rgb(0x9A, 0xE2, 0x4A), rgb(0x2E, 0xB8, 0x8A)),
            // Periwinkle into azure.
            Section::Apps => (rgb(0x7D, 0x8C, 0xFF), rgb(0x3E, 0xA6, 0xF2)),
            // Amber into tangerine.
            Section::Files => (rgb(0xFF, 0xB8, 0x3E), rgb(0xF2, 0x77, 0x4A)),
            // Sky into spring green.
            Section::System => (rgb(0x52, 0xD6, 0xF0), rgb(0x6C, 0xE0, 0x8C)),
            Section::Neutral => (rgb(0x8E, 0x9B, 0xB0), rgb(0x5B, 0x6B, 0x84)),
        }
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// The soft shadow under cards.
pub fn card_shadow() -> Shadow {
    Shadow {
        offset: [0, 6],
        blur: 22,
        spread: 0,
        color: Color32::from_black_alpha(70),
    }
}

/// The deeper shadow under dialogs and the bottom action bar.
pub fn float_shadow() -> Shadow {
    Shadow {
        offset: [0, 10],
        blur: 36,
        spread: 2,
        color: Color32::from_black_alpha(120),
    }
}

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
    /// Translucent card surface laid over the section gradient.
    pub glass: Color32,
    pub glass_hover: Color32,
    pub glass_stroke: Color32,
    /// Hairline highlight along the top edge of glass.
    pub glass_edge: Color32,
    /// The colours of the section on screen (set by the shell every frame).
    pub sec_a: Color32,
    pub sec_b: Color32,
}

impl Tokens {
    /// These tokens with the colours of `s`.
    pub fn for_section(mut self, s: Section) -> Tokens {
        let (a, b) = s.colors();
        self.sec_a = a;
        self.sec_b = b;
        self
    }

    /// Text colour that reads well on the section gradient.
    pub fn on_section(&self) -> Color32 {
        on_color(self.sec_a)
    }
}

/// Dark ink on light colours, white on deep ones.
pub fn on_color(c: Color32) -> Color32 {
    let l = 0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32;
    if l > 150.0 {
        Color32::from_rgb(0x08, 0x16, 0x1C)
    } else {
        Color32::WHITE
    }
}

impl Tokens {
    pub fn dark() -> Tokens {
        Tokens {
            bg: Color32::from_rgb(0x10, 0x13, 0x1A),
            sidebar: Color32::from_rgb(0x0B, 0x0E, 0x14),
            card: Color32::from_rgb(0x1A, 0x1F, 0x29),
            card_hover: Color32::from_rgb(0x23, 0x29, 0x35),
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
            glass: Color32::from_rgba_unmultiplied(0x2A, 0x31, 0x40, 150),
            glass_hover: Color32::from_rgba_unmultiplied(0x34, 0x3C, 0x4E, 175),
            glass_stroke: Color32::from_rgba_unmultiplied(255, 255, 255, 20),
            glass_edge: Color32::from_rgba_unmultiplied(255, 255, 255, 34),
            sec_a: Section::Overview.colors().0,
            sec_b: Section::Overview.colors().1,
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

/// Inter's four weights (SIL Open Font License, see assets/fonts/OFL-Inter.txt).
fn inter_faces() -> [(&'static str, &'static [u8]); 4] {
    [
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
    ]
}

/// Fonts with Inter only, no fallback: tests use it to prove every character on screen is one
/// Inter can draw.
pub fn inter_only_fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::empty();
    for (name, bytes) in inter_faces() {
        defs.font_data.insert(
            name.to_string(),
            std::sync::Arc::new(FontData::from_static(bytes)),
        );
    }
    defs.families
        .insert(FontFamily::Proportional, vec!["inter-regular".into()]);
    defs.families
        .insert(FontFamily::Monospace, vec!["inter-regular".into()]);
    for (fam, face) in [
        ("medium", "inter-medium"),
        ("semibold", "inter-semibold"),
        ("bold", "inter-bold"),
    ] {
        defs.families
            .insert(FontFamily::Name(fam.into()), vec![face.to_string()]);
    }
    defs
}

/// Install Inter in four weights, with egui's fonts as fallback.
pub fn install_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    for (name, bytes) in inter_faces() {
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
    visuals.window_corner_radius = CornerRadius::same(radius::XL);
    visuals.menu_corner_radius = CornerRadius::same(radius::M);
    visuals.window_shadow = float_shadow();
    visuals.slider_trailing_fill = true;
    visuals.handle_shape = egui::style::HandleShape::Circle;
    visuals.popup_shadow = card_shadow();
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
        CornerRadius::same(radius::S),
        Stroke::new(1.0, t.text),
    );
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, t.glass_stroke);
    visuals.widgets.inactive.weak_bg_fill = t.glass_hover;
    visuals.widgets.inactive.bg_fill = t.card_hover;
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x30, 0x38, 0x48);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, t.accent.gamma_multiply(0.8));
    visuals.widgets.active.weak_bg_fill = t.accent.gamma_multiply(0.35);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.glass_stroke);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text_dim);
    visuals.text_edit_bg_color = Some(Color32::from_rgba_unmultiplied(0, 0, 0, 70));
    visuals.selection.bg_fill = t.accent.gamma_multiply(0.35);
    ctx.set_visuals(visuals);
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(space::S, space::S);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 26.0;
        style.spacing.icon_width = 16.0;
        style.spacing.icon_width_inner = 9.0;
        style.animation_time = motion::HOVER;
        style.text_styles = [
            (TextStyle::Small, font(size::SMALL, Weight::Regular)),
            (TextStyle::Body, font(size::BODY, Weight::Regular)),
            (TextStyle::Button, font(13.0, Weight::Medium)),
            (TextStyle::Heading, font(size::TITLE, Weight::Bold)),
            (
                TextStyle::Monospace,
                FontId::new(12.0, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}
