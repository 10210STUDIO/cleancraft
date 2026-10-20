//! Round 4 polish (design4): click sounds on switches and checkboxes, tooltips that never
//! cover their row, a clear disabled button, Quick Sweep's title row and tile grid.

mod common;

use cleancraft_core::Settings;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{testkit_cleanup, testkit_monitor};
use cleancraft_ui_egui::CleanApp;
use cleancraft_ui_egui::pages::sweep;
use egui::epaint::Shape;
use egui_kittest::kittest::Queryable;

type H = egui_kittest::Harness<'static, CleanApp>;

fn base(m: &FakeMac) {
    let f = m.env.fake_sys().expect("fake sys");
    f.answer("/usr/bin/open", "");
    f.answer("sh", "");
}

fn harness(mac: &FakeMac, w: f32, hgt: f32) -> H {
    let mut app = CleanApp::new(mac.env.clone()).with_sync();
    app.fda = cleancraft_core::permissions::Access::Granted;
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(w, hgt))
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

fn click(h: &mut H, label: &str) {
    h.get_by_label(label).click();
    settle(h);
}

fn go(h: &mut H, page: &str) {
    h.state_mut().navigate(page);
    settle(h);
}

fn taps(mac: &FakeMac) -> usize {
    mac.env
        .fake_sys()
        .expect("fake")
        .spawned()
        .iter()
        .filter(|l| l.starts_with("/usr/bin/afplay ") && l.ends_with("tap.wav"))
        .count()
}

/// "Click sounds on switches" on: toggling a result's checkbox plays the tap once; off (the
/// default): nothing.
#[test]
fn toggles_play_the_tap_only_when_turned_on() {
    for ticks in [true, false] {
        let m = testkit_cleanup::junk_mac();
        base(&m);
        Settings {
            sound_ticks: ticks,
            ..Settings::default()
        }
        .save(&m.env)
        .expect("save");
        let mut h = harness(&m, 1280.0, 820.0);
        go(&mut h, "system_junk");
        click(&mut h, "Scan");
        let before = taps(&m);
        click(&mut h, "Demo-abc");
        settle(&mut h);
        let after = taps(&m);
        assert_eq!(after - before, usize::from(ticks), "ticks {ticks}");
    }
}

/// The tooltip of a result row's path opens under the row (or above it), never over it.
#[test]
fn row_tooltips_do_not_cover_their_row() {
    let m = testkit_cleanup::junk_mac();
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    let cb = h.get_by_label("Demo-abc").rect();
    // The row: from the checkbox across the list, 60 pt tall around it.
    let row = egui::Rect::from_min_max(
        egui::pos2(cb.min.x - 12.0, cb.center().y - 28.0),
        egui::pos2(1240.0, cb.center().y + 28.0),
    );
    // Hover the path (dim text after the name on line 1) and wait past the tooltip delay.
    let path = h
        .output()
        .shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            Shape::Text(t) if t.galley.job.text.contains("DerivedData") => {
                Some(t.visual_bounding_rect())
            }
            _ => None,
        })
        .next()
        .expect("path text");
    let mut tip = None;
    h.hover_at(path.center());
    for _ in 0..80 {
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(10));
        tip = h
            .output()
            .shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                Shape::Text(t)
                    if t.galley.job.text.contains("DerivedData")
                        && t.visual_bounding_rect() != path =>
                {
                    Some(t.visual_bounding_rect())
                }
                _ => None,
            })
            .find(|r| !path.contains_rect(*r));
        if tip.is_some() {
            break;
        }
    }
    let tip = tip.expect("the full path shows in a tooltip");
    assert!(
        !tip.intersects(row.shrink(1.0)),
        "tooltip {tip:?} covers the row {row:?}"
    );
}

/// A disabled primary or danger button is a flat outlined pill with faint text, not a
/// see-through gradient.
#[test]
fn disabled_buttons_have_one_clear_style() {
    let ctx = egui::Context::default();
    cleancraft_ui_egui::theme::install_fonts(&ctx);
    let t = cleancraft_ui_egui::theme::Tokens::dark();
    // Fonts registered in one pass are usable from the next.
    ctx.run_ui(Default::default(), |_| {})
        .textures_delta
        .clear();
    let mut out = ctx.run_ui(Default::default(), |ui| {
        ui.add_enabled_ui(false, |ui| {
            cleancraft_ui_egui::kit::pill(
                ui,
                &t,
                "Uninstall",
                cleancraft_ui_egui::kit::Kind::Danger,
                None,
            );
        });
    });
    out.textures_delta.clear();
    let mut meshes = 0;
    let mut text_ok = false;
    fn walk(s: &Shape, meshes: &mut usize, text_ok: &mut bool, faint: egui::Color32) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|x| walk(x, meshes, text_ok, faint)),
            Shape::Mesh(_) => *meshes += 1,
            Shape::Text(tx) => {
                *text_ok = tx
                    .galley
                    .job
                    .sections
                    .iter()
                    .all(|s| s.format.color == faint)
            }
            _ => {}
        }
    }
    for cs in &out.shapes {
        walk(&cs.shape, &mut meshes, &mut text_ok, t.text_faint);
    }
    assert_eq!(meshes, 0, "no gradient mesh on a disabled button");
    assert!(text_ok, "label in text_faint");
}

/// Quick Sweep: the title row sits at the same place on the start screen, while scanning
/// and on the results; the results grid leaves no empty cell.
#[test]
fn quick_sweep_title_stays_and_the_grid_is_full() {
    let title_y = |h: &H| {
        h.output()
            .shapes
            .iter()
            .filter_map(|cs| match &cs.shape {
                Shape::Text(t)
                    if t.galley.job.text == "Quick Sweep"
                        && t.visual_bounding_rect().min.x > 260.0 =>
                {
                    Some(t.visual_bounding_rect().min.y)
                }
                _ => None,
            })
            .next()
    };
    let m = testkit_monitor::sweep_mac();
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0);
    h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
    settle(&mut h);
    let idle = title_y(&h).expect("title on the start screen");
    h.state_mut().sweep.phase =
        sweep::Phase::Scanning(cleancraft_ui_egui::jobs::Job::waiting("System Junk"));
    settle(&mut h);
    let scanning = title_y(&h).expect("title while scanning");
    h.state_mut().sweep.phase = sweep::Phase::Idle;
    settle(&mut h);
    click(&mut h, "Scan");
    let results = title_y(&h).expect("title on the results");
    assert_eq!(idle, scanning);
    assert_eq!(idle, results);
    // Tiles: every row spans the same width (no empty cell at the end).
    // Each tile paints its drop shadow first: a 16 pt rounded black rect at 50 alpha.
    let tiles: Vec<egui::Rect> = h
        .output()
        .shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            Shape::Rect(r)
                if r.fill == egui::Color32::from_black_alpha(50) && r.rect.width() > 150.0 =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .collect();
    assert!(tiles.len() >= 4, "tiles: {tiles:?}");
    let rights: Vec<f32> = {
        let mut rows: std::collections::BTreeMap<i32, f32> = Default::default();
        for r in &tiles {
            let e = rows.entry(r.center().y as i32 / 40).or_insert(0.0);
            *e = e.max(r.max.x);
        }
        rows.into_values().collect()
    };
    let (lo, hi) = rights
        .iter()
        .fold((f32::MAX, f32::MIN), |(a, b), x| (a.min(*x), b.max(*x)));
    assert!(hi - lo < 2.0, "rows end at {rights:?}");
}
