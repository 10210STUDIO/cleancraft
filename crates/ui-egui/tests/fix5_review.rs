//! Round 5 fixes of the last review items (docs/design/REVIEW-C.md D1-D6 and the open C
//! items, docs/design/REVIEW-U.md N1-N4 and the open U items). Each test failed before its
//! fix. Fake Mac only.

mod common;

use cleancraft_core::executor::{self, Method};
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::permissions::Access;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{FeatureId, Settings, testkit_cleanup, testkit_monitor};
use cleancraft_ui_egui::CleanApp;
use egui::accesskit::Role;
use egui::epaint::Shape;
use egui::{Pos2, Rect};
use egui_kittest::kittest::{NodeT, Queryable};

type H = egui_kittest::Harness<'static, CleanApp>;

fn harness_at(mac: &FakeMac, w: f32, h: f32) -> H {
    let mut app = CleanApp::new(mac.env.clone()).with_sync();
    app.fda = Access::Granted;
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

fn harness(mac: &FakeMac) -> H {
    harness_at(mac, 1280.0, 820.0)
}

fn settle(h: &mut H) {
    for _ in 0..6 {
        h.step();
    }
}

fn rest(h: &mut H) {
    for _ in 0..40 {
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

fn go(h: &mut H, page: &str) {
    h.state_mut().navigate(page);
    settle(h);
}

fn has(h: &H, label: &str) -> bool {
    h.query_all_by_label(label).next().is_some()
}

fn has_text(h: &H, needle: &str) -> bool {
    h.query_all_by_label_contains(needle).next().is_some()
}

fn count(h: &H, label: &str) -> usize {
    h.query_all_by_label(label).count()
}

/// Enabled, visible buttons by label.
fn buttons(h: &H) -> Vec<String> {
    h.root()
        .children_recursive()
        .filter_map(|n| {
            let a = n.accesskit_node();
            (!a.is_hidden()
                && !a.is_disabled()
                && matches!(a.role(), Role::Button | Role::CheckBox))
            .then(|| a.label().unwrap_or_default())
        })
        .filter(|l| !l.is_empty())
        .collect()
}

/// Every painted text: (text, font family, rect).
fn texts(h: &H) -> Vec<(String, egui::FontFamily, Rect)> {
    fn walk(s: &Shape, out: &mut Vec<(String, egui::FontFamily, Rect)>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
            Shape::Text(t) => {
                let fam = t
                    .galley
                    .job
                    .sections
                    .first()
                    .map(|s| s.format.font_id.family.clone())
                    .unwrap_or_default();
                out.push((t.galley.job.text.clone(), fam, t.visual_bounding_rect()))
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

fn junk() -> FakeMac {
    let m = testkit_cleanup::junk_mac();
    base(&m);
    m
}

fn clean_here(h: &mut H) {
    click(h, "Review and Clean");
    let label = if has(h, "Move to Quarantine") {
        "Move to Quarantine"
    } else {
        "Move to Trash"
    };
    click(h, label);
}

fn sweep_mac() -> FakeMac {
    let m = testkit_monitor::sweep_mac();
    base(&m);
    m
}

fn sweep_scan(h: &mut H) {
    h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
    click(h, "Scan");
}

fn trash_bytes(m: &FakeMac) -> u64 {
    cleancraft_core::fsutil::children(&m.env.user_trash())
        .iter()
        .map(|c| tree_size(c).0)
        .sum()
}

/// D1 (S1): at 1060 x 680 the Trash Bins bar showed "0 items selected" under "Finder empties
/// the Trash..."; now it shows what the Trash holds and the hint never runs into it.
#[test]
fn d1_trash_bins_bar_has_no_overlap_at_the_smallest_window() {
    let mac = testkit_cleanup::trash_mac();
    base(&mac);
    let mut h = harness_at(&mac, 1060.0, 680.0);
    go(&mut h, "trash_bins");
    click(&mut h, "Scan");
    rest(&mut h);
    assert!(
        !has_text(&h, "items selected"),
        "nothing can be selected here"
    );
    let all = texts(&h);
    let bar: Vec<_> = all.iter().filter(|(_, _, r)| r.top() > 590.0).collect();
    assert!(
        bar.iter().any(|(s, _, _)| s == "in the Trash"),
        "bar texts: {:?}",
        bar.iter().map(|b| &b.0).collect::<Vec<_>>()
    );
    for (i, a) in bar.iter().enumerate() {
        for b in &bar[i + 1..] {
            let inter = a.2.intersect(b.2);
            assert!(
                !(inter.width() > 0.5 && inter.height() > 0.5),
                "{:?} overlaps {:?}",
                a.0,
                b.0
            );
        }
    }
}

/// N2 (S2) and U4: the Quick Sweep Trash tile follows what a cleanup moved into the Trash,
/// and offers Empty Trash in place of "Emptied from its own page".
#[test]
fn n2_sweep_trash_tile_follows_moves_into_the_trash() {
    let mac = sweep_mac();
    let mut h = harness(&mac);
    sweep_scan(&mut h);
    let before = trash_bytes(&mac);
    assert!(has(&h, &human_size(before)), "Trash tile before");
    assert!(!has_text(&h, "Emptied from its own page"));
    assert!(buttons(&h).iter().any(|b| b == "Empty Trash..."));
    // Clean System Junk from its page, come back.
    h.get_all_by_label("Review").next().expect("Review").click();
    settle(&mut h);
    clean_here(&mut h);
    click(&mut h, "Done");
    click(&mut h, "Quick Sweep");
    let after = trash_bytes(&mac);
    assert!(after > before);
    assert!(
        has(&h, &human_size(after)),
        "the Trash tile still says {} after the cleanup moved files into the Trash",
        human_size(before)
    );
}

/// U4: the Quick Sweep Done screen offers Finder's Empty Trash when items went to the Trash.
#[test]
fn u04_quick_sweep_done_offers_empty_trash() {
    let mac = sweep_mac();
    let mut h = harness(&mac);
    sweep_scan(&mut h);
    click(&mut h, "Review and Clean");
    click(&mut h, "Move to Trash");
    assert!(has_text(&h, "moved to the Trash"));
    let b = buttons(&h);
    assert!(b.iter().any(|l| l == "Empty Trash..."), "{b:?}");
    assert!(has_text(
        &h,
        "The space comes back when the Trash is emptied."
    ));
    assert!(!has_text(&h, "quarantine is cleared"));
}

/// N1 (S3): a module emptied by a cleanup elsewhere says so, with no Include.
#[test]
fn n1_cleaned_tile_says_so_and_offers_a_scan() {
    let mac = sweep_mac();
    let mut h = harness(&mac);
    sweep_scan(&mut h);
    let includes = count(&h, "Include");
    let mail: Vec<_> = h
        .state()
        .sweep
        .result
        .as_ref()
        .and_then(|r| r.tile(FeatureId::MailAttachments))
        .and_then(|t| t.plan())
        .map(|p| p.items.iter().map(|i| i.path.clone()).collect())
        .unwrap_or_default();
    assert!(!mail.is_empty());
    h.state_mut().files_moved(&mail);
    settle(&mut h);
    assert!(has_text(&h, "Cleaned just now."));
    assert!(!has_text(&h, "0 of 0"));
    assert_eq!(
        count(&h, "Include"),
        includes - 1,
        "no Include on a cleaned tile"
    );
    assert!(has(&h, "Scan Mail Downloads again"));
}

/// D2 (S2) and C17: a hovered Privacy row keeps its reason tag whole on one side of Reveal
/// and Ignore, and tags drop their asides in brackets before being cut.
#[test]
fn d2_hovered_privacy_row_keeps_its_tag_before_the_buttons() {
    let mac = testkit_cleanup::privacy_mac();
    base(&mac);
    let mut h = harness_at(&mac, 1060.0, 680.0);
    go(&mut h, "privacy");
    click(&mut h, "Scan");
    rest(&mut h);
    // C17: no tag cut while a shorter form fits.
    let cut: Vec<String> = texts(&h)
        .into_iter()
        .filter(|(s, _, _)| s.contains("profile)") && s.ends_with('…'))
        .map(|t| t.0)
        .collect();
    assert!(cut.is_empty(), "tags cut with their asides kept: {cut:?}");
    // Hover the first row's name.
    let reveal_rows: Vec<Rect> = h.query_all_by_label("Reveal").map(|n| n.rect()).collect();
    let first = reveal_rows
        .iter()
        .copied()
        .min_by(|a, b| a.top().total_cmp(&b.top()))
        .expect("a row with Reveal");
    h.hover_at(Pos2::new(700.0, first.center().y));
    rest(&mut h);
    let reveal = h
        .query_all_by_label("Reveal")
        .map(|n| n.rect())
        .find(|r| (r.center().y - first.center().y).abs() < 2.0)
        .expect("Reveal of the hovered row");
    let row_band = reveal.expand2(egui::vec2(0.0, 30.0));
    for (s, _, r) in texts(&h) {
        if s == "Reveal" || s == "Ignore" || !row_band.intersects(r) {
            continue;
        }
        // Nothing on the row starts left of the buttons and runs on past them.
        assert!(
            !(r.left() < reveal.left() - 4.0 && r.right() > reveal.left() + 2.0),
            "{s:?} ({r:?}) runs under Reveal ({reveal:?})"
        );
    }
}

/// N3 (S3): the Ignore notice has an Undo that puts the row back as it was.
#[test]
fn n3_ignore_notice_has_undo() {
    let mac = junk();
    let mut h = harness(&mac);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    let before: Vec<(std::path::PathBuf, bool)> = h.state().plans[&FeatureId::SystemJunk]
        .plan
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| (i.path.clone(), i.selected))
        .collect();
    h.get_all_by_label("Ignore").next().unwrap().click();
    settle(&mut h);
    let path = h.state().settings.ignore.last().cloned().expect("ignored");
    let was = before.iter().find(|b| b.0 == path).unwrap().1;
    let id = {
        let p = h.state().plans[&FeatureId::SystemJunk]
            .plan
            .clone()
            .unwrap();
        let it = p.items.iter().find(|i| i.path == path).unwrap();
        assert!(it.locked.is_some() && !it.selected);
        it.id
    };
    assert!(has_text(&h, "is on the ignore list now."));
    click(&mut h, "Undo Ignore");
    assert!(!h.state().settings.ignore.contains(&path));
    assert!(!Settings::load(&mac.env).ignore.contains(&path), "saved");
    let it = &h.state().plans[&FeatureId::SystemJunk]
        .plan
        .as_ref()
        .unwrap()
        .items[id];
    assert!(
        it.locked.is_none() && it.selected == was,
        "row back as it was"
    );
    assert!(has_text(&h, "is no longer ignored"));
}

/// U9 (S2): a file open in an app names the app and the next step.
#[test]
fn u09_skipped_item_names_the_app_holding_it() {
    let mac = junk();
    let mut h = harness(&mac);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    let held = h.state().plans[&FeatureId::SystemJunk]
        .plan
        .as_ref()
        .unwrap()
        .selected()
        .next()
        .unwrap()
        .path
        .clone();
    mac.env.fake_sys().unwrap().set_open_by(&held, "Safari");
    clean_here(&mut h);
    assert!(has_text(&h, "open in Safari"));
    assert!(has_text(&h, "Quit Safari, then Try Again."));
}

/// History: once Undo All is on a card, the per-file Undo buttons wait behind a disclosure.
#[test]
fn history_hides_per_file_undo_behind_a_disclosure() {
    let mac = junk();
    let mut h = harness(&mac);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    clean_here(&mut h);
    let moved = executor::history(&mac.env).len();
    assert!(moved > 1);
    go(&mut h, "history");
    assert!(has(&h, "Undo All"));
    assert_eq!(count(&h, "Undo"), 0, "per-file Undo shown next to Undo All");
    let show = format!("Show {moved} items of System Junk");
    click(&mut h, &show);
    assert_eq!(count(&h, "Undo"), moved);
    click(&mut h, "Hide Items of System Junk");
    assert_eq!(count(&h, "Undo"), 0);
}

/// D5 (S3): an empty History says it once.
#[test]
fn d5_empty_history_says_it_once() {
    let mac = FakeMac::new();
    base(&mac);
    let mut h = harness(&mac);
    go(&mut h, "history");
    assert_eq!(count(&h, "No actions yet"), 1);
    assert!(!has_text(&h, "Nothing yet"));
}

/// U5: Move Quarantine to Trash, from Settings, with History's record of it.
#[test]
fn u05_quarantine_to_trash_is_listed_in_history_with_undo_all() {
    let mac = junk();
    Settings {
        method: Method::Quarantine,
        ..Default::default()
    }
    .save(&mac.env)
    .unwrap();
    let mut h = harness(&mac);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    clean_here(&mut h);
    assert!(
        has_text(&h, "moved to the Trash (Settings)"),
        "Done says where"
    );
    go(&mut h, "settings");
    click(&mut h, "Move Quarantine to Trash...");
    assert!(has_text(&h, "Move the quarantine to the Trash"));
    // Cancel moves nothing.
    click(&mut h, "Cancel");
    assert!(executor::quarantine_size(&mac.env).1 > 0);
    click(&mut h, "Move Quarantine to Trash...");
    click(&mut h, "Move to Trash");
    assert_eq!(executor::quarantine_size(&mac.env), (0, 0));
    go(&mut h, "history");
    assert!(
        texts(&h)
            .iter()
            .any(|(s, _, _)| s == "Quarantine moved to the Trash"),
        "History names the move"
    );
    assert_eq!(count(&h, "Undo All"), 2, "the cleanup and the move");
}

/// C14: commands are set in the bundled monospace face.
#[test]
fn c14_commands_are_monospace() {
    let m = FakeMac::new();
    let f = m.env.fake_sys().unwrap();
    f.answer("id -u", "501\n");
    f.answer("sw_vers -productVersion", "14.6.1\n");
    base(&m);
    let mut h = harness(&m);
    go(&mut h, "maintenance");
    let code: Vec<String> = texts(&h)
        .into_iter()
        .filter(|(_, fam, _)| *fam == egui::FontFamily::Name("code".into()))
        .map(|t| t.0)
        .collect();
    assert!(
        code.iter().any(|s| s.contains('/')),
        "no command in the monospace face: {code:?}"
    );
}

/// D4: a task that cannot run says so in a sentence; the tool's words go to the tooltip.
#[test]
fn d4_task_errors_are_plain_sentences() {
    let p = cleancraft_ui_egui::pages::tasks::plain_reason(
        "Could not list local snapshots: fake: no answer for `/usr/bin/tmutil listlocalsnapshots /`",
    );
    assert_eq!(
        p,
        "Could not list local snapshots. This task cannot run now."
    );
    assert!(!p.contains('`'));
}

/// C10 (S2): on the first results frame the list already shows its rows (they fade in, they
/// are not missing) and the bottom bar is where it stays.
#[test]
fn c10_first_results_frame_has_rows_and_the_bar_in_place() {
    let m = junk();
    let mut h = harness(&m);
    go(&mut h, "system_junk");
    h.get_by_label("Scan").click();
    h.step();
    h.step();
    let probe = std::env::var("FIX5_PROBE").is_ok();
    if probe {
        let img = h.render().expect("render");
        img.save("/tmp/claude-0/-home-claude/cb89f4a3-398a-5ab4-a872-bb5977d98224/scratchpad/c10-frame1.png").unwrap();
    }
    let bar = |h: &H| {
        texts(h)
            .into_iter()
            .find(|(s, _, _)| s.ends_with("selected"))
            .map(|t| t.2.top())
    };
    let rows = |h: &H| {
        texts(h)
            .into_iter()
            .filter(|(s, _, r)| s == "Demo-abc" && r.left() > 560.0)
            .count()
    };
    let (bar1, rows1) = (bar(&h), rows(&h));
    if probe {
        for i in 0..6 {
            eprintln!("frame {i}: bar {:?}", bar(&h));
            h.step();
        }
    }
    rest(&mut h);
    if probe {
        let img = h.render().expect("render");
        img.save("/tmp/claude-0/-home-claude/cb89f4a3-398a-5ab4-a872-bb5977d98224/scratchpad/c10-final.png").unwrap();
    }
    assert_eq!(rows1, rows(&h), "rows missing on the first results frame");
    assert!(rows1 > 0);
    let (a, b) = (bar1.expect("bar on frame 1"), bar(&h).unwrap());
    assert!(
        (a - b).abs() < 1.0,
        "bottom bar at {a} on frame 1, {b} after"
    );
}

/// C15 (S2): Quick Sweep names the same modules in every phase: the idle chips, the
/// scanning chips and the result tiles.
#[test]
fn c15_quick_sweep_lists_the_same_modules_in_every_phase() {
    let mac = sweep_mac();
    let mut h = harness(&mac);
    let src = testkit_monitor::sweep_sources();
    h.state_mut().sweep.sources = Some(src.clone());
    settle(&mut h);
    let titles: Vec<&str> = src.ids().iter().map(|i| i.title()).collect();
    let shown = |h: &H| -> Vec<String> {
        let t = texts(h);
        titles
            .iter()
            .filter(|n| t.iter().any(|(s, _, _)| s == *n))
            .map(|n| n.to_string())
            .collect()
    };
    let idle = shown(&h);
    h.state_mut().sweep.phase = cleancraft_ui_egui::pages::sweep::Phase::Scanning(
        cleancraft_ui_egui::jobs::Job::waiting("System Junk"),
    );
    settle(&mut h);
    let scanning = shown(&h);
    h.state_mut().sweep.phase = cleancraft_ui_egui::pages::sweep::Phase::Idle;
    settle(&mut h);
    click(&mut h, "Scan");
    rest(&mut h);
    let tiles: Vec<FeatureId> = h
        .state()
        .sweep
        .result
        .as_ref()
        .unwrap()
        .tiles
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(idle.len(), titles.len(), "idle chips {idle:?}");
    assert_eq!(idle, scanning);
    assert_eq!(tiles, src.ids());
}

/// C21 (S2): on a results page Tab follows reading order: Search and Sort before the
/// bottom bar's Review and Clean, and one stop per category.
#[test]
fn c21_tab_follows_reading_order_with_one_stop_per_category() {
    let m = junk();
    let mut h = harness(&m);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    rest(&mut h);
    let mut order: Vec<String> = Vec::new();
    for _ in 0..60 {
        h.key_press(egui::Key::Tab);
        h.step();
        h.step();
        let f = h
            .root()
            .children_recursive()
            .find(|n| n.accesskit_node().is_focused())
            .and_then(|n| n.accesskit_node().label())
            .unwrap_or_default();
        if order.last() == Some(&f) {
            continue;
        }
        if f == "Review and Clean" {
            order.push(f);
            break;
        }
        order.push(f);
    }
    let pos = |l: &str| order.iter().position(|x| x == l);
    let search = pos("Search").expect("Search reached");
    let review = pos("Review and Clean").expect("Review and Clean reached");
    assert!(search < review, "order: {order:?}");
    // Each category is one stop (its checkbox), not the row and then its checkbox.
    let plan = h.state().plans[&FeatureId::SystemJunk]
        .plan
        .clone()
        .unwrap();
    for c in &plan.categories {
        let n = order.iter().filter(|l| l.contains(&c.title)).count();
        assert!(n <= 1, "{} reached {n} times: {order:?}", c.title);
    }
}

fn remover_mac() -> FakeMac {
    use cleancraft_core::testkit_apps::{AppSpec, FakeMacApps};
    let mac = FakeMac::new();
    let editor = mac.app_spec(&AppSpec {
        size: 3_000_000,
        ..AppSpec::new("Editor", "com.example.Editor", "2.0")
    });
    mac.app("Safari", "com.apple.Safari", "18.0");
    mac.last_opened(&editor, 2);
    mac.spotlight_finds_nothing();
    mac.home_file(
        "Library/Application Support/com.example.Editor/db.sqlite",
        400_000,
    );
    mac.home_file("Library/Caches/com.example.Editor/blob", 900_000);
    base(&mac);
    mac
}

fn x_of(h: &H, label: &str) -> f32 {
    h.get_by_label(label).rect().left()
}

/// D3 (S3): the App Remover Done screen orders its buttons like every module Done screen:
/// Empty Trash..., Undo, Show History, Done.
#[test]
fn d3_app_remover_done_buttons_in_the_module_order() {
    let mac = remover_mac();
    let mut h = harness(&mac);
    go(&mut h, "uninstaller");
    click(&mut h, "Editor");
    click(&mut h, "Uninstall");
    click(&mut h, "Move to Trash");
    let xs: Vec<f32> = ["Empty Trash...", "Undo", "Show History", "Done"]
        .iter()
        .map(|l| x_of(&h, l))
        .collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
}

/// C19 and C20: App Remover uses the shared bottom bar (figure, then "N items selected") and
/// its list has the headline every results page has.
#[test]
fn c19_c20_app_remover_bar_and_headline() {
    let mac = remover_mac();
    let mut h = harness(&mac);
    go(&mut h, "uninstaller");
    assert!(has_text(&h, "in 2 apps"), "App Remover list headline");
    click(&mut h, "Editor");
    let n = h.state().apps.detail.as_ref().unwrap().selected_count();
    // Shown "4 items selected" after the 24 pt figure; announced with the size.
    assert!(
        texts(&h)
            .iter()
            .any(|t| t.0 == format!("{n} items selected"))
    );
    assert!(has_text(&h, &format!("{n} selected, ")));
    // U15: plain action names.
    assert!(has(&h, "Reset App...") && has(&h, "Hide from List"));
}

/// D6 (S3): row names are whole at 1060 when the path can shrink instead.
#[test]
fn d6_trash_row_names_are_whole_at_1060() {
    let mac = testkit_cleanup::trash_mac();
    base(&mac);
    let mut h = harness_at(&mac, 1060.0, 680.0);
    go(&mut h, "trash_bins");
    click(&mut h, "Scan");
    rest(&mut h);
    let painted: Vec<String> = texts(&h).into_iter().map(|t| t.0).collect();
    assert!(
        painted.iter().any(|s| s == "Trash of the startup disk"),
        "{painted:?}"
    );
}

/// C39 (S3): the Duplicates headline counts what can be freed (every copy but the kept one).
#[test]
fn c39_duplicates_headline_counts_the_copies_that_can_go() {
    let m = FakeMac::sample();
    let dup: Vec<u8> = (0..300_000u32)
        .map(|i| (i.wrapping_mul(7919) >> 3) as u8)
        .collect();
    for p in ["Downloads/a.bin", "Music/old/a.bin", "Projects/a.bin"] {
        m.write(&m.env.home_path(p), &dup);
    }
    base(&m);
    let mut h = harness(&m);
    go(&mut h, "duplicates");
    click(&mut h, "Scan");
    let plan = h.state().plans[&FeatureId::Duplicates]
        .plan
        .clone()
        .unwrap();
    let (bytes, n) = cleancraft_ui_egui::pages::plan_page::duplicate_copies(&plan);
    assert_eq!(n, 2);
    assert_eq!(bytes, 2 * plan.items[0].size);
    assert!(has_text(&h, "in 2 duplicate copies"));
    assert!(has(&h, &human_size(bytes)));
}

/// C37, U25: Disk Map says "Measuring..." with nothing after it, and its Done summary
/// mentions skipped items only when there are some.
#[test]
fn c37_disk_map_measuring_has_no_space_before_the_dots() {
    let m = FakeMac::new();
    base(&m);
    let mut h = harness(&m);
    go(&mut h, "disk_map");
    h.state_mut().disk_map.phase = cleancraft_ui_egui::pages::disk_map::Phase::Scanning(
        cleancraft_ui_egui::jobs::Job::waiting("x"),
    );
    settle(&mut h);
    assert!(!texts(&h).iter().any(|t| t.0.contains(" ...")));
    assert!(texts(&h).iter().any(|t| t.0 == "Measuring..."));
}

/// C29: the Map Folder button is live; with nothing typed it says what to do.
#[test]
fn c29_map_folder_is_live_and_explains_an_empty_field() {
    let m = FakeMac::new();
    base(&m);
    let mut h = harness(&m);
    go(&mut h, "disk_map");
    assert!(buttons(&h).iter().any(|b| b == "Map Folder"));
    click(&mut h, "Map Folder");
    assert!(has_text(&h, "Type a folder first"));
}

/// C30 (S3): empty states say it once, without a full stop.
#[test]
fn c30_empty_states_have_no_full_stop() {
    let m = FakeMac::new();
    base(&m);
    let mut h = harness(&m);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    assert!(has(&h, "Nothing to clean here"));
    let mac = remover_mac();
    let mut h = harness(&mac);
    go(&mut h, "uninstaller");
    h.state_mut().apps.search = "zzz".into();
    settle(&mut h);
    assert!(has(&h, "No app matches your search"));
}

/// C33 (S3): switches announce as switches; a result checkbox tells size and reason too.
#[test]
fn c33_switch_role_and_row_descriptions() {
    let m = junk();
    let mut h = harness(&m);
    go(&mut h, "settings");
    let n = h.get_by_label("Play sounds");
    assert_eq!(n.accesskit_node().role(), Role::Switch);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    let n = h.get_by_label("Demo-abc");
    let d = n.accesskit_node().description().unwrap_or_default();
    assert!(d.contains("MB") && d.contains("Derived data"), "{d:?}");
}

/// U26: AI assistants is a switch in plain words; the quarantine path is a dim line.
#[test]
fn u26_settings_assistants_switch_and_quarantine_line() {
    let m = FakeMac::new();
    base(&m);
    let mut h = harness(&m);
    go(&mut h, "settings");
    let n = h.get_by_label("Let AI assistants clean after you confirm");
    assert_eq!(n.accesskit_node().role(), Role::Switch);
    assert!(has(&h, "CleanCraft quarantine"));
    assert!(has_text(
        &h,
        "Library/Application Support/CleanCraft/Quarantine"
    ));
}

/// U21, C27: startup rows say who signed in plain words, and Remove shows on hover only.
#[test]
fn u21_c27_startup_rows() {
    let m = cleancraft_core::testkit_perf::startup_mac();
    base(&m);
    let mut h = harness(&m);
    go(&mut h, "login_items");
    rest(&mut h);
    assert!(!has_text(&h, "Ad hoc"));
    let painted_remove = |h: &H| texts(h).iter().filter(|t| t.0 == "Remove").count();
    assert!(
        h.query_all_by_label_contains("Remove ").next().is_some(),
        "still there for VoiceOver"
    );
    let at_rest = painted_remove(&h);
    let r = h
        .query_all_by_label_contains("Remove ")
        .next()
        .unwrap()
        .rect();
    h.hover_at(r.center() - egui::vec2(300.0, 0.0));
    rest(&mut h);
    assert!(
        painted_remove(&h) > at_rest,
        "Remove shows when its row is hovered"
    );
}

/// U22, U4: Live Monitor empties the Trash from its Storage card.
#[test]
fn u22_live_monitor_offers_empty_trash() {
    let m = FakeMac::new();
    testkit_monitor::answer_all(m.env.fake_sys().unwrap());
    m.home_file(".Trash/old.zip", 2_500_000);
    base(&m);
    let mut h = harness(&m);
    go(&mut h, "monitor");
    rest(&mut h);
    assert!(buttons(&h).iter().any(|b| b == "Empty Trash..."));
}

/// U24: the locked Library row on Disk Map links to the tools that handle app data.
#[test]
fn u24_disk_map_library_row_links_to_the_tools() {
    let m = FakeMac::sample();
    base(&m);
    let mut h = harness(&m);
    go(&mut h, "disk_map");
    click(&mut h, "Map Home Folder");
    rest(&mut h);
    click(&mut h, "Open System Junk");
    assert_eq!(
        h.state().page,
        cleancraft_ui_egui::Page::Feature(FeatureId::SystemJunk)
    );
}
