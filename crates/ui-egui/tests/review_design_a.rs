//! Design review A (docs/design/REVIEW-A.md): the measurable defects as tests.
//! All fixed and on by default since design round 3 (they were `#[ignore]` while open).

use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{FeatureId, testkit_monitor};
use cleancraft_ui_egui::CleanApp;
use cleancraft_ui_egui::theme::{self, Section, Tokens};
use egui::Color32;
use egui::epaint::Shape;

type H = egui_kittest::Harness<'static, CleanApp>;

fn harness(mac: &FakeMac, w: f32, h: f32) -> H {
    let mut app = CleanApp::new(mac.env.clone()).with_sync();
    app.fda = cleancraft_core::permissions::Access::Granted;
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(w, h))
        .build_ui_state(
            |ui, app: &mut CleanApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                let mut root = egui::Ui::new(
                    ctx.clone(),
                    egui::Id::new("window"),
                    egui::UiBuilder::new()
                        .layer_id(egui::LayerId::background())
                        .max_rect(ctx.viewport_rect()),
                );
                app.ui(&mut root);
            },
            app,
        );
    settle(&mut h);
    h
}

fn settle(h: &mut H) {
    for _ in 0..6 {
        h.step();
    }
}

fn lin(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(c: Color32) -> f64 {
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// WCAG 2 contrast ratio of two opaque colours.
fn contrast(a: Color32, b: Color32) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// `top` (possibly translucent, unmultiplied alpha) laid over the opaque `under`.
fn over(top: Color32, under: Color32) -> Color32 {
    let [r, g, b, a] = top.to_srgba_unmultiplied();
    let k = a as f32 / 255.0;
    let mix = |t: u8, u: u8| (t as f32 * k + u as f32 * (1.0 - k)).round() as u8;
    Color32::from_rgb(mix(r, under.r()), mix(g, under.g()), mix(b, under.b()))
}

/// Every text shape on screen: (text, font size).
fn texts(h: &H) -> Vec<(String, f32)> {
    fn walk(s: &Shape, out: &mut Vec<(String, f32)>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
            Shape::Text(t) => {
                for sec in &t.galley.job.sections {
                    let text =
                        t.galley.job.text[sec.byte_range.start.0..sec.byte_range.end.0].to_string();
                    if !text.trim().is_empty() {
                        out.push((text, sec.format.font_id.size));
                    }
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for cs in &h.output().shapes {
        walk(&cs.shape, &mut out);
    }
    out
}

/// Texts egui cut short with an ellipsis (`Label::truncate`).
fn elided(h: &H) -> Vec<String> {
    fn walk(s: &Shape, out: &mut Vec<String>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
            Shape::Text(t) if t.galley.elided => out.push(t.galley.job.text.clone()),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for cs in &h.output().shapes {
        walk(&cs.shape, &mut out);
    }
    out
}

/// The pages a user sees most, with content: (name, harness).
fn tour(w: f32, hgt: f32, mut each: impl FnMut(&str, &H)) {
    let mac = FakeMac::sample();
    let f = mac.env.fake_sys().expect("fake sys");
    f.answer("/usr/bin/open", "");
    f.answer("sh", "");
    let mut h = harness(&mac, w, hgt);
    for page in [
        "system-junk",
        "trash-bins",
        "large-old-files",
        "uninstaller",
        "maintenance",
        "login-items",
        "history",
        "settings",
    ] {
        let name = FeatureId::parse(page).map(|f| f.title()).unwrap_or(page);
        h.state_mut().navigate(page);
        settle(&mut h);
        each(name, &h);
    }
    // System Junk with results.
    h.state_mut().navigate("system-junk");
    settle(&mut h);
    let found = match egui_kittest::kittest::Queryable::query_all_by_label(&h, "Scan").next() {
        Some(n) => {
            n.click();
            true
        }
        None => false,
    };
    if found {
        settle(&mut h);
        each("System Junk results", &h);
    }
    let sm = testkit_monitor::sweep_mac();
    let mut q = harness(&sm, w, hgt);
    q.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
    settle(&mut q);
    each("Quick Sweep", &q);
}

/// REVIEW-A 3: text_faint carries captions; it must reach 4.5:1 on every surface it sits on.
#[test]
fn review_a3_faint_text_meets_aa_on_bg_card_and_glass() {
    let t = Tokens::dark();
    let surfaces = [
        ("bg", t.bg),
        ("sidebar", t.sidebar),
        ("card", t.card),
        ("glass over bg", over(t.glass, t.bg)),
        ("glass_hover over bg", over(t.glass_hover, t.bg)),
    ];
    let mut bad = Vec::new();
    for (name, s) in surfaces {
        let c = contrast(t.text_faint, s);
        if c < 4.5 {
            bad.push(format!("text_faint on {name}: {c:.2}:1"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// REVIEW-A 4: the label on a section-coloured button must reach 4.5:1 at both gradient ends.
#[test]
fn review_a4_text_on_section_gradients_meets_aa() {
    let mut bad = Vec::new();
    for s in [
        Section::Overview,
        Section::Clean,
        Section::Protect,
        Section::Speed,
        Section::Apps,
        Section::Files,
        Section::System,
        Section::Neutral,
    ] {
        let (a, b) = s.colors();
        let ink = Tokens::dark().for_section(s).on_section();
        for (end, c) in [("start", a), ("end", b)] {
            let r = contrast(ink, c);
            if r < 4.5 {
                bad.push(format!("{s:?} {end} {c:?}: label {ink:?} at {r:.2}:1"));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// REVIEW-A 2 and 13: no text under 11 pt anywhere (sidebar headers were 9.5, tags 10.5).
#[test]
fn review_a2_no_text_under_11pt() {
    let mut bad = std::collections::BTreeSet::new();
    tour(1060.0, 680.0, |page, h| {
        for (text, size) in texts(h) {
            if size < 10.95 {
                bad.insert(format!("{page}: {size} pt {text:?}"));
            }
        }
    });
    assert!(
        bad.is_empty(),
        "{} texts under 11 pt:\n{}",
        bad.len(),
        bad.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// REVIEW-A 1: a type scale of at most 9 sizes is what pages actually use.
#[test]
fn review_a1_pages_stay_on_the_type_scale() {
    let mut sizes = std::collections::BTreeMap::<i32, String>::new();
    tour(1280.0, 820.0, |page, h| {
        for (text, size) in texts(h) {
            sizes
                .entry((size * 10.0).round() as i32)
                .or_insert_with(|| format!("{page}: {text:?}"));
        }
    });
    let list: Vec<String> = sizes
        .iter()
        .map(|(s, ex)| format!("{:.1} pt, e.g. {ex}", *s as f32 / 10.0))
        .collect();
    assert!(
        sizes.len() <= 9,
        "{} distinct font sizes:\n{}",
        sizes.len(),
        list.join("\n")
    );
}

/// REVIEW-A 57: page explanations are never cut with an ellipsis at 1060 x 680.
#[test]
fn review_a57_page_headers_are_not_truncated() {
    let mut bad = Vec::new();
    let mac = FakeMac::sample();
    let mut h = harness(&mac, 1060.0, 680.0);
    for f in FeatureId::ALL {
        h.state_mut().navigate(f.as_str());
        settle(&mut h);
        for text in elided(&h) {
            if text.len() > 40 {
                bad.push(format!("{}: {text:?}", f.title()));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Keeps the helpers honest: the contrast function matches WCAG's own examples.
#[test]
fn review_contrast_helper_matches_wcag() {
    let r = contrast(Color32::WHITE, Color32::BLACK);
    assert!((r - 21.0).abs() < 0.01, "{r}");
    let g = contrast(Color32::from_rgb(0x76, 0x76, 0x76), Color32::WHITE);
    assert!((g - 4.54).abs() < 0.02, "{g}");
    let _ = theme::on_color(Color32::WHITE);
}
