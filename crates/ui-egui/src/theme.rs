//! CleanCraft's own look: deep ink surfaces, a mint accent, per-section colour fields, glass
//! cards and Inter type (SIL OFL). Every design token lives here: colours, spacing, radii,
//! shadows, type sizes and motion timings. Pages and `kit` read them, never their own numbers.

use craft_ui::theme::WidgetPalette;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke,
    TextStyle, Visuals,
};

/// Spacing scale (points), on a 4 pt grid. See docs/design/SYSTEM.md.
pub mod space {
    pub const XXS: f32 = 2.0;
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
    pub const L: f32 = 16.0;
    pub const XL: f32 = 24.0;
    pub const XXL: f32 = 32.0;
    pub const XXXL: f32 = 48.0;
    /// Width of the sidebar.
    pub const SIDEBAR: f32 = 232.0;
    /// Inner margin of the sidebar (left/right, top/bottom).
    pub const SIDEBAR_X: i8 = 12;
    pub const SIDEBAR_Y: i8 = 16;
    /// Margin around the content area.
    pub const PAGE_X: i8 = 32;
    pub const PAGE_Y: i8 = 24;
    /// Gap between cards in a grid and between the two columns of a results page.
    pub const GUTTER: f32 = 16.0;
    /// Widest a line of running text may get (hero descriptions, dialogs).
    pub const MEASURE: f32 = 560.0;
}

/// Corner radii.
pub mod radius {
    /// Tags, checkboxes, small tiles.
    pub const XS: u8 = 6;
    pub const S: u8 = 8;
    pub const M: u8 = 12;
    pub const L: u8 = 16;
    pub const XL: u8 = 22;
    /// Fully round ends.
    pub const PILL: u8 = 255;
}

/// The nine sizes of the type scale, largest first (tests check pages use only these).
pub const SCALE: [f32; 9] = [
    size::DISPLAY,
    size::LARGE_TITLE,
    size::TITLE,
    size::TITLE2,
    size::TITLE3,
    size::BODY,
    size::CALLOUT,
    size::FOOTNOTE,
    size::CAPTION,
];

/// The scale size nearest to `x` (for text sized from a shape, like an initial in a tile).
pub fn snap(x: f32) -> f32 {
    SCALE
        .iter()
        .copied()
        .min_by(|a, b| (a - x).abs().total_cmp(&(b - x).abs()))
        .unwrap_or(size::BODY)
}

/// Type scale (points), Inter. Tuned for a Mac utility on Retina: nothing a person must read
/// is below `FOOTNOTE` (12), and `CAPTION` (11.5) is only used for upper-case labels.
/// Weights and line heights per role are in docs/design/SYSTEM.md.
pub mod size {
    /// Big numbers: the total found, freed space. Bold.
    pub const DISPLAY: f32 = 52.0;
    /// Numbers inside cards and tiles. Bold.
    pub const NUMBER: f32 = 30.0;
    /// Numbers in the bottom bar and small stats. Bold. (Same size as `TITLE`: the scale
    /// keeps to nine sizes.)
    pub const NUMBER_S: f32 = 24.0;
    /// Start-screen title, Done headline. Bold.
    pub const LARGE_TITLE: f32 = 30.0;
    /// Page title in the header. Bold.
    pub const TITLE: f32 = 24.0;
    /// Dialog headline, progress headline. SemiBold.
    pub const TITLE2: f32 = 19.0;
    /// Card and section titles. SemiBold.
    pub const TITLE3: f32 = 16.0;
    /// Lead text under a hero title. Regular. (Same size as `TITLE3`.)
    pub const BODY_L: f32 = 16.0;
    /// Body text, row names (SemiBold), sidebar entries (Medium).
    pub const BODY: f32 = 14.0;
    /// Secondary lines: subtitles, meta, buttons. Regular or Medium.
    pub const CALLOUT: f32 = 13.0;
    /// Paths, tags, counts. The smallest text a person must read.
    pub const FOOTNOTE: f32 = 12.0;
    /// Upper-case group labels only (SemiBold, letter spaced).
    pub const CAPTION: f32 = 11.5;

    // Earlier names, kept so code written against round 1 still builds.
    pub const HERO_TITLE: f32 = LARGE_TITLE;
    pub const H2: f32 = TITLE3;
    pub const SMALL: f32 = FOOTNOTE;
    pub const TINY: f32 = CAPTION;
}

/// Icon and picture sizes (points) per context.
pub mod icon {
    /// Small icons inside text, buttons and strips.
    pub const INLINE: f32 = 16.0;
    /// Sidebar entries.
    pub const SIDEBAR: f32 = 18.0;
    /// File-kind icon inside its row tile (the tile is `ROW_TILE`).
    pub const ROW: f32 = 20.0;
    pub const ROW_TILE: f32 = 36.0;
    /// App icons in list rows (App Remover, Startup Items, App Updates).
    pub const APP_ROW: f32 = 32.0;
    /// App icon in the App Remover detail head.
    pub const APP_LARGE: f32 = 64.0;
    /// Module tile in a card head (Quick Sweep tiles, Live Monitor cards).
    pub const CARD: f32 = 36.0;
    /// Module tile in the page header.
    pub const HEADER: f32 = 48.0;
    /// Icon tile in a dialog head.
    pub const DIALOG: f32 = 48.0;
    /// Hero picture on start, scanning and Done screens: between these, by window height.
    pub const HERO_MIN: f32 = 150.0;
    pub const HERO_MAX: f32 = 240.0;
}

/// Hit targets and fixed control heights (points).
pub mod hit {
    /// The smallest clickable thing (HIG: 28 pt on macOS).
    pub const MIN: f32 = 28.0;
    /// Sidebar row: grows to `NAV_MAX` in tall windows.
    pub const NAV: f32 = 30.0;
    pub const NAV_MAX: f32 = 36.0;
    /// Item row in a results list.
    pub const ROW: f32 = 60.0;
    /// Category row in the left column of a results page.
    pub const CATEGORY: f32 = 50.0;
    /// Line in a confirmation list.
    pub const CONFIRM_LINE: f32 = 30.0;
    /// Button heights. Primary, secondary and danger share one height so a row of buttons
    /// lines up; the primary stands out by its fill. Only the start screen's round action
    /// is bigger.
    pub const PRIMARY: f32 = 32.0;
    pub const SECONDARY: f32 = 32.0;
    /// Row actions (Reveal, Ignore, Dismiss).
    pub const GHOST: f32 = 28.0;
    /// Text fields and pop-up menus in toolbars: the same height as the buttons beside them.
    pub const FIELD: f32 = 32.0;
    /// Checkbox box (the hit area is `MIN`).
    pub const CHECK: f32 = 18.0;
}

/// Motion lives in `crate::motion` (easing, springs, durations, Reduce motion); this path
/// stays for the pages that name `theme::motion`.
pub mod motion {
    pub use crate::motion::*;
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
            // Ends chosen so the dark ink label reads at 4.5:1 or more on both (WCAG).
            Section::Overview => (rgb(0x34, 0xD8, 0xB4), rgb(0x26, 0x90, 0xB4)),
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
            Section::Neutral => (rgb(0x8E, 0x9B, 0xB0), rgb(0x7A, 0x8A, 0xA6)),
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

    /// The glass card colour as it looks over the backdrop (opaque), for things that must
    /// hide what is under them inside a card.
    pub fn glass_solid(&self) -> Color32 {
        Color32::from_rgb(0x21, 0x27, 0x33)
    }

    /// Text colour that reads well on the section gradient.
    pub fn on_section(&self) -> Color32 {
        on_color(self.sec_a)
    }
}

/// The Danger pill's gradient (the button that moves files): coral to rose.
pub const DANGER_A: Color32 = Color32::from_rgb(0xFF, 0x76, 0x6E);
pub const DANGER_B: Color32 = Color32::from_rgb(0xEE, 0x5A, 0x64);

/// The Danger pill's label: ink, which reads at least 4.5:1 on both ends of its gradient
/// (white would read 2.7:1 at the light end).
pub fn danger_label() -> Color32 {
    on_color(icons_mid(DANGER_A, DANGER_B))
}

fn icons_mid(a: Color32, b: Color32) -> Color32 {
    Color32::from_rgb(
        ((a.r() as u16 + b.r() as u16) / 2) as u8,
        ((a.g() as u16 + b.g() as u16) / 2) as u8,
        ((a.b() as u16 + b.b() as u16) / 2) as u8,
    )
}

/// Dark ink for text on colour fills.
pub const INK: Color32 = Color32::from_rgb(0x08, 0x16, 0x1C);

/// WCAG relative luminance of a colour (0 black, 1 white).
pub fn luminance(c: Color32) -> f32 {
    fn lin(v: u8) -> f32 {
        let v = v as f32 / 255.0;
        if v <= 0.039_28 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// WCAG contrast ratio of two opaque colours (1 to 21).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Text colour for a fill: white or dark ink, whichever has more WCAG contrast. Every section
/// colour, the danger gradient and the treemap blocks get ink (at least 5.5:1); only deep
/// fills get white.
pub fn on_color(c: Color32) -> Color32 {
    if contrast(Color32::WHITE, c) >= contrast(INK, c) {
        Color32::WHITE
    } else {
        INK
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
            // WCAG contrast on glass (#1F2530): text 13.9, dim 8.9, faint 5.6.
            text: Color32::from_rgb(0xF0, 0xF2, 0xF5),
            text_dim: Color32::from_rgb(0xBE, 0xC6, 0xD0),
            text_faint: Color32::from_rgb(0x93, 0x9D, 0xAA),
            accent: Color32::from_rgb(0x2E, 0xC4, 0xA6),
            accent_dim: Color32::from_rgba_unmultiplied(0x2E, 0xC4, 0xA6, 40),
            on_accent: Color32::from_rgb(0x06, 0x1A, 0x16),
            danger: Color32::from_rgb(0xFF, 0x7A, 0x74),
            warning: Color32::from_rgb(0xF2, 0xB1, 0x3C),
            success: Color32::from_rgb(0x5A, 0xD6, 0x8A),
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
        style.spacing.button_padding = egui::vec2(14.0, 7.0);
        style.spacing.interact_size.y = hit::MIN;
        style.spacing.icon_width = 18.0;
        style.spacing.icon_width_inner = 10.0;
        style.spacing.combo_height = 320.0;
        style.animation_time = motion::HOVER;
        style.text_styles = [
            (TextStyle::Small, font(size::FOOTNOTE, Weight::Regular)),
            (TextStyle::Body, font(size::BODY, Weight::Regular)),
            (TextStyle::Button, font(size::CALLOUT, Weight::Medium)),
            (TextStyle::Heading, font(size::TITLE, Weight::Bold)),
            (
                TextStyle::Monospace,
                FontId::new(size::FOOTNOTE, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}
