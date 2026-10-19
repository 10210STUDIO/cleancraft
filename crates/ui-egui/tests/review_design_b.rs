//! Design review B (docs/design/REVIEW-B.md): defects found or left open in round B, as tests.
//! All fixed and on by default since design round 3 (they were `#[ignore]` while open).

use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{testkit_cleanup, testkit_monitor};
use cleancraft_ui_egui::CleanApp;
use egui::accesskit::Role;
use egui::epaint::Shape;
use egui_kittest::kittest::{NodeT, Queryable};

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

fn base(m: &FakeMac) {
    let f = m.env.fake_sys().expect("fake sys");
    f.answer("/usr/bin/open", "");
    f.answer("sh", "");
}

fn click(h: &mut H, label: &str) {
    h.get_by_label(label).click();
    settle(h);
}

/// Every text on screen, and whether egui cut it with an ellipsis.
fn texts(h: &H) -> Vec<(String, bool)> {
    fn walk(s: &Shape, out: &mut Vec<(String, bool)>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
            Shape::Text(t) => out.push((t.galley.job.text.clone(), t.galley.elided)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for cs in &h.output().shapes {
        walk(&cs.shape, &mut out);
    }
    out
}

/// Buttons and pop-ups on screen: (label, rect).
fn controls(h: &H) -> Vec<(String, egui::Rect)> {
    let mut out = Vec::new();
    for n in h.root().children_recursive() {
        let a = n.accesskit_node();
        if a.is_hidden() || !matches!(a.role(), Role::Button | Role::ComboBox) {
            continue;
        }
        let r = n.rect();
        if r.width() > 1.0 && r.height() > 1.0 {
            out.push((a.label().unwrap_or_default(), r));
        }
    }
    out
}

/// REVIEW-B 1 (S1): at 1060 x 680 the Privacy Traces toolbar draws "Reset Permissions" over
/// "Sort by size". No two buttons may overlap on any results toolbar.
#[test]
fn review_b1_results_toolbar_buttons_never_overlap() {
    let mut bad = Vec::new();
    for (w, hgt) in [(1060.0, 680.0), (1280.0, 820.0)] {
        for (page, mac) in [
            ("privacy", testkit_cleanup::privacy_mac as fn() -> FakeMac),
            ("system-junk", FakeMac::sample),
            ("trash-bins", FakeMac::sample),
        ] {
            let m = mac();
            base(&m);
            let mut h = harness(&m, w, hgt);
            h.state_mut().navigate(page);
            settle(&mut h);
            click(&mut h, "Scan");
            let cs = controls(&h);
            for (i, (la, ra)) in cs.iter().enumerate() {
                for (lb, rb) in &cs[i + 1..] {
                    let x = ra.intersect(*rb);
                    // Same row only (rows scrolled under the bottom bar report unclipped rects).
                    let same_row = (ra.center().y - rb.center().y).abs() < 4.0;
                    if same_row && x.is_positive() && x.area() > 4.0 {
                        bad.push(format!(
                            "{page} at {w}x{hgt}: {la:?} {ra:?} overlaps {lb:?} {rb:?}"
                        ));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// REVIEW-B 2: Live Monitor's key figures ("7.78 GB of 16.0 GB") are never cut at 1060 x 680.
#[test]
fn review_b2_monitor_figures_are_not_elided() {
    let m = FakeMac::new();
    testkit_monitor::answer_all(m.env.fake_sys().expect("fake sys"));
    base(&m);
    let mut h = harness(&m, 1060.0, 680.0);
    h.state_mut().navigate("monitor");
    for _ in 0..3 {
        h.state_mut().monitor.interval = std::time::Duration::ZERO;
        settle(&mut h);
    }
    h.state_mut().monitor.interval = std::time::Duration::from_secs(3600);
    settle(&mut h);
    let cut: Vec<String> = texts(&h)
        .into_iter()
        .filter(|(t, elided)| *elided && (t.contains("GB") || t.contains('%')))
        .map(|(t, _)| t)
        .collect();
    assert!(cut.is_empty(), "elided figures: {cut:?}");
}

/// REVIEW-A 31 (left open): sizes read "0 KB", never "Zero KB".
#[test]
fn review_a31_no_zero_kb_wording() {
    let mut bad = std::collections::BTreeSet::new();
    let m = FakeMac::sample();
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0);
    for page in ["system-junk", "trash-bins"] {
        h.state_mut().navigate(page);
        settle(&mut h);
        click(&mut h, "Scan");
        for (t, _) in texts(&h) {
            if t.contains("Zero KB") {
                bad.insert(format!("{page}: {t:?}"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{}",
        bad.into_iter().collect::<Vec<_>>().join("\n")
    );
}
