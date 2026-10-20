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
    assert!(!has_text(&h, "items selected"), "nothing can be selected here");
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
    assert!(has_text(&h, "The space comes back when the Trash is emptied."));
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
    assert_eq!(count(&h, "Include"), includes - 1, "no Include on a cleaned tile");
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
    let reveal_rows: Vec<Rect> = h
        .query_all_by_label("Reveal")
        .map(|n| n.rect())
        .collect();
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
    let path = h
        .state()
        .settings
        .ignore
        .last()
        .cloned()
        .expect("ignored");
    let was = before.iter().find(|b| b.0 == path).unwrap().1;
    let id = {
        let p = h.state().plans[&FeatureId::SystemJunk].plan.clone().unwrap();
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
    assert!(it.locked.is_none() && it.selected == was, "row back as it was");
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
    assert!(has_text(&h, "moved to the Trash (Settings)"), "Done says where");
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
    assert_eq!(p, "Could not list local snapshots. This task cannot run now.");
    assert!(!p.contains('`'));
}
