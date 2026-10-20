//! Design round 3 (REVIEW-A / REVIEW-B items closed by design3), as behaviour tests on FakeMac:
//! Danger pill contrast (A4), sparkline (A53), count-up totals and dialog close (A66, A37),
//! Trash rows named after their disk (A34, B10), Quick Sweep module progress (A43), the page
//! header on start screens (A18), the startup switch (A50), plain scan errors (B6), the empty
//! state's Details (B5).

use cleancraft_core::FeatureId;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{testkit_cleanup, testkit_monitor};
use cleancraft_ui_egui::CleanApp;
use cleancraft_ui_egui::jobs::Job;
use cleancraft_ui_egui::pages::{plan_page, sweep};
use cleancraft_ui_egui::theme;
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

fn has(h: &H, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

/// Every text painted on screen.
fn painted(h: &H) -> Vec<String> {
    fn walk(s: &Shape, out: &mut Vec<String>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
            Shape::Text(t) => out.push(t.galley.job.text.clone()),
            _ => {}
        }
    }
    let mut out = Vec::new();
    for cs in &h.output().shapes {
        walk(&cs.shape, &mut out);
    }
    out
}

/// Texts painted at the display size (the results total).
fn display_texts(h: &H) -> Vec<String> {
    fn walk(s: &Shape, out: &mut Vec<String>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
            Shape::Text(t) => {
                let big = t
                    .galley
                    .job
                    .sections
                    .iter()
                    .any(|s| (s.format.font_id.size - theme::size::DISPLAY).abs() < 0.1);
                if big {
                    out.push(t.galley.job.text.clone());
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

/// REVIEW-A 4: the label of the button that moves files reads at least 4.5:1 on both ends of
/// its gradient (it was white at 2.7:1).
#[test]
fn danger_pill_label_reads_on_its_whole_gradient() {
    let ink = theme::danger_label();
    for end in [theme::DANGER_A, theme::DANGER_B] {
        let c = theme::contrast(ink, end);
        assert!(c >= 4.5, "{ink:?} on {end:?}: {c:.2}:1");
    }
    assert_ne!(ink, egui::Color32::WHITE);
}

/// REVIEW-A 66: a results total counts up, but VoiceOver and tests hear the final figure at
/// once, and after the count the screen shows exactly that figure.
#[test]
fn results_total_is_exact_from_the_first_frame() {
    let m = FakeMac::sample();
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0);
    h.state_mut().navigate("system_junk");
    settle(&mut h);
    h.get_by_label("Scan").click();
    // Step until the results are on screen (the scan runs inline; the page polls it).
    for _ in 0..4 {
        h.step();
        if h.state_mut()
            .plan_state(FeatureId::SystemJunk)
            .plan
            .is_some()
        {
            break;
        }
    }
    let total = h
        .state_mut()
        .plan_state(FeatureId::SystemJunk)
        .plan
        .as_ref()
        .expect("plan")
        .total_size();
    let want = cleancraft_core::fsutil::human_size(total);
    // Right after the results arrive the painted total is already exact (round 4, REVIEW-C
    // C10: no frame where the total disagrees with the category list and the bar).
    assert!(has(&h, &want), "accessible total");
    let first = display_texts(&h);
    assert_eq!(first, vec![want.clone()], "exact total on the first frame");
    settle(&mut h);
    assert_eq!(
        display_texts(&h),
        vec![want.clone()],
        "the count ends on {want}"
    );
}

/// REVIEW-A 37 and 66: every confirmation dialog has a close button that cancels (nothing
/// moves), and Esc does the same.
#[test]
fn dialog_close_button_and_esc_cancel() {
    let m = FakeMac::sample();
    base(&m);
    let mut h = harness(&m, 1060.0, 680.0);
    h.state_mut().navigate("system_junk");
    settle(&mut h);
    click(&mut h, "Scan");
    click(&mut h, "Review and Clean");
    assert!(has(&h, "Move to Trash"));
    click(&mut h, "Close dialog");
    assert!(!has(&h, "Move to Trash"), "the X closes the dialog");
    assert!(matches!(
        h.state_mut().plan_state(FeatureId::SystemJunk).phase,
        plan_page::Phase::Results
    ));
    click(&mut h, "Review and Clean");
    assert!(has(&h, "Move to Trash"));
    h.key_press(egui::Key::Escape);
    settle(&mut h);
    assert!(!has(&h, "Move to Trash"), "Esc closes the dialog");
    // Nothing moved.
    let plan = h
        .state_mut()
        .plan_state(FeatureId::SystemJunk)
        .plan
        .clone()
        .expect("plan");
    assert!(plan.items.iter().all(|i| i.path.exists()));
}

/// REVIEW-A 34 and REVIEW-B 10: a Trash on another disk is named after the disk, not "0" or
/// the user id, and its count reads "1 item".
#[test]
fn trash_rows_are_named_after_their_disk() {
    let m = testkit_cleanup::trash_mac();
    base(&m);
    let mut h = harness(&m, 1060.0, 680.0);
    h.state_mut().navigate("trash_bins");
    settle(&mut h);
    click(&mut h, "Scan");
    let uid = cleancraft_core::features::trash::uid(&m.env).to_string();
    // The row's checkbox carries the name.
    assert!(has(&h, "Trash of USB"), "row named after the disk");
    assert!(!has(&h, &uid), "row named after the user id folder");
    let texts = painted(&h);
    assert!(texts.iter().any(|t| t == "1 item"), "{texts:?}");
    assert!(!texts.iter().any(|t| t.contains("1 items")));
}

/// REVIEW-A 43: while Quick Sweep scans, every module shows whether it is done, being
/// scanned or waiting.
#[test]
fn quick_sweep_scan_lists_each_module_with_its_state() {
    let m = testkit_monitor::sweep_mac();
    base(&m);
    let mut h = harness(&m, 1060.0, 680.0);
    h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
    let ids = testkit_monitor::sweep_sources().ids();
    assert!(ids.len() >= 3);
    // The scan has reached the second module.
    h.state_mut().sweep.phase = sweep::Phase::Scanning(Job::waiting(ids[1].title()));
    settle(&mut h);
    assert!(has(&h, &format!("{}: done", ids[0].title())));
    assert!(has(&h, &format!("{}: scanning", ids[1].title())));
    for id in &ids[2..] {
        assert!(has(&h, &format!("{}: waiting", id.title())), "{id:?}");
    }
    // A module reporting its own places keeps the list where it is.
    h.state_mut().sweep.phase = sweep::Phase::Scanning(Job::waiting("~/Library/Caches"));
    settle(&mut h);
    assert!(has(&h, &format!("{}: done", ids[0].title())));
}

/// REVIEW-A 18 (with the lead's section layout): in a section with several tools the
/// switcher at the top stays at the same place from the start screen to scanning and results,
/// so nothing at the top of the page jumps when Scan is clicked.
#[test]
fn section_switcher_stays_in_place_across_phases() {
    let m = FakeMac::sample();
    base(&m);
    let mut h = harness(&m, 1060.0, 680.0);
    h.state_mut().navigate("system_junk");
    settle(&mut h);
    let at = |h: &H| h.get_by_label("System Junk").rect();
    let idle = at(&h);
    h.state_mut().plan_state(FeatureId::SystemJunk).phase =
        plan_page::Phase::Scanning(Job::waiting("~/Library/Caches"));
    settle(&mut h);
    let scanning = at(&h);
    h.state_mut().plan_state(FeatureId::SystemJunk).phase = plan_page::Phase::Idle;
    settle(&mut h);
    click(&mut h, "Scan");
    let results = at(&h);
    for r in [scanning, results] {
        assert!(
            (idle.min - r.min).length() < 0.5,
            "switcher moved from {idle:?} to {r:?}"
        );
    }
}

/// The lead's sidebar: seven sections and History and Settings, 44 pt rows; a section opens
/// the tool it showed last.
#[test]
fn sidebar_sections_open_their_last_tool() {
    let m = FakeMac::sample();
    base(&m);
    let mut h = harness(&m, 1060.0, 680.0);
    for name in [
        "Quick Sweep",
        "Clean",
        "Protect",
        "Speed",
        "Apps",
        "Files",
        "Live Monitor",
        "History",
        "Settings",
    ] {
        let r = h.get_by_label(name).rect();
        assert!((r.height() - 44.0).abs() < 0.5, "{name}: {r:?}");
        assert!(r.max.y <= 680.0, "{name} below the window");
    }
    click(&mut h, "Clean");
    assert_eq!(
        h.state().page,
        cleancraft_ui_egui::Page::Feature(FeatureId::SystemJunk)
    );
    click(&mut h, "Trash Bins");
    assert_eq!(
        h.state().page,
        cleancraft_ui_egui::Page::Feature(FeatureId::TrashBins)
    );
    click(&mut h, "Settings");
    click(&mut h, "Clean");
    assert_eq!(
        h.state().page,
        cleancraft_ui_egui::Page::Feature(FeatureId::TrashBins),
        "the section reopens its last tool"
    );
}

/// REVIEW-A 50: a startup item is one switch, named after the action a click takes.
#[test]
fn startup_items_use_one_switch() {
    let m = cleancraft_core::testkit_perf::startup_mac();
    base(&m);
    let mut h = harness(&m, 1060.0, 680.0);
    h.state_mut().navigate("login_items");
    settle(&mut h);
    let switches: Vec<_> = h
        .get_all_by_role(Role::CheckBox)
        .filter(|n| {
            let l = n.accesskit_node().label().unwrap_or_default();
            l.starts_with("Turn Off ") || l.starts_with("Turn On ")
        })
        .collect();
    assert!(!switches.is_empty(), "no switch");
    let texts = painted(&h);
    assert!(
        !texts.iter().any(|t| t == "On" || t == "Off"),
        "On/Off tags next to the switch"
    );
}

/// REVIEW-B 6: a scan error is a plain sentence with a way to fix it, not raw OS text.
#[test]
fn scan_errors_read_as_plain_sentences() {
    let (text, fda) = plan_page::friendly_error(
        "Could not read ~/Library/Caches: permission denied (os error 13)",
    );
    assert!(fda);
    assert_eq!(
        text,
        "CleanCraft is not allowed to read ~/Library/Caches. Turn on Full Disk Access for CleanCraft, then scan again."
    );
    let (text, fda) = plan_page::friendly_error("the disk is busy (os error 16)");
    assert!(!fda);
    assert_eq!(text, "the disk is busy");

    let m = testkit_cleanup::junk_mac();
    base(&m);
    let mut h = harness(&m, 1060.0, 680.0);
    h.state_mut().navigate("system_junk");
    h.state_mut().plan_state(FeatureId::SystemJunk).error =
        Some("Could not read ~/Library/Caches: permission denied (os error 13)".into());
    settle(&mut h);
    let texts = painted(&h);
    assert!(!texts.iter().any(|t| t.contains("os error")), "{texts:?}");
    let before = h.state().action_count();
    click(&mut h, "Open Privacy Settings");
    assert!(h.state().action_count() > before);
}

/// REVIEW-B 5: a scan that finds nothing shows no 0 total; what could not be read is behind
/// Details.
#[test]
fn empty_result_keeps_notes_behind_details() {
    let m = FakeMac::new();
    base(&m);
    let mut h = harness(&m, 1060.0, 680.0);
    h.state_mut().navigate("system_junk");
    settle(&mut h);
    click(&mut h, "Scan");
    let notes = h
        .state_mut()
        .plan_state(FeatureId::SystemJunk)
        .plan
        .as_ref()
        .expect("plan")
        .notes
        .clone();
    assert!(has(&h, "Nothing to clean here."));
    let texts = painted(&h);
    assert!(!texts.iter().any(|t| t == "0 KB" || t == "Zero KB"));
    if notes.is_empty() {
        return;
    }
    assert!(!texts.iter().any(|t| t.contains(&notes[0])), "notes shown");
    click(&mut h, "Details");
    assert!(has(&h, "Hide Details"));
}

/// REVIEW-A 53: the CPU history spans the box from the first readings (not a stub at the
/// right edge).
#[test]
fn sparkline_spans_the_box_from_two_readings() {
    let ctx = egui::Context::default();
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        let r = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(200.0, 56.0));
        cleancraft_ui_egui::kit::sparkline(
            ui.painter(),
            r,
            &[20.0, 30.0, 25.0],
            120,
            egui::Color32::WHITE,
        );
    });
    out.textures_delta.clear();
    let shapes: Vec<Shape> = out.shapes.into_iter().map(|c| c.shape).collect();
    let mut xs = Vec::new();
    for s in &shapes {
        if let Shape::Path(p) = s {
            xs.extend(p.points.iter().map(|p| p.x));
        }
    }
    let min = xs.iter().cloned().fold(f32::MAX, f32::min);
    let max = xs.iter().cloned().fold(f32::MIN, f32::max);
    assert!(min <= 11.0 && max >= 209.0, "line spans {min}..{max}");
}

/// sound3's Settings group: "Play sounds" is a switch that is saved, the volume is saved and
/// "Preview sound" plays a sound through afplay (FakeSys records it).
#[test]
fn settings_sounds_group_saves_and_previews() {
    let m = FakeMac::new();
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0);
    click(&mut h, "Settings");
    h.get_by_label("Preview sound").scroll_to_me();
    settle(&mut h);
    click(&mut h, "Preview sound");
    let spawned = m.env.fake_sys().unwrap().spawned();
    assert!(
        spawned.iter().any(|l| l.starts_with("/usr/bin/afplay ")),
        "{spawned:?}"
    );
    click(&mut h, "Play sounds");
    assert!(!cleancraft_core::Settings::load(&m.env).sounds);
    click(&mut h, "Play sounds");
    assert!(cleancraft_core::Settings::load(&m.env).sounds);
}
