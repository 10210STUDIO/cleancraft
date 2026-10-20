//! Design review C (docs/design/REVIEW-C.md): measurable defects found in round 4, as tests.
//! Each test is `#[ignore]` while its defect is open and fails on main 4e23d58; run them with
//! `cargo test -p cleancraft-ui-egui --test review_design_c -- --ignored`. Remove the
//! `#[ignore]` with the fix. Pixel checks render with wgpu (software adapter here) and return
//! early, passing, when no adapter exists.

use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{testkit_cleanup, testkit_monitor, testkit_perf as tp};
use cleancraft_ui_egui::CleanApp;
use egui::accesskit::Role;
use egui::epaint::Shape;
use egui::{CursorIcon, Pos2, Rect, pos2};
use egui_kittest::kittest::{NodeT, Queryable};

type H = egui_kittest::Harness<'static, CleanApp>;

fn harness(mac: &FakeMac, w: f32, h: f32, ppp: f32) -> H {
    let mut app = CleanApp::new(mac.env.clone()).with_sync();
    app.fda = cleancraft_core::permissions::Access::Granted;
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(w, h))
        .with_pixels_per_point(ppp)
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

/// Long enough for every hover, press and appear animation to finish.
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

/// A pointer press and release at `p` (kittest's node click goes through accesskit, not the
/// pointer, so it cannot tell whether the spot itself is a click target).
fn click_at(h: &mut H, p: Pos2) {
    h.hover_at(p);
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.step();
    }
    settle(h);
}

/// Every text on screen: (text, font size, rect).
fn texts(h: &H) -> Vec<(String, f32, Rect)> {
    fn walk(s: &Shape, out: &mut Vec<(String, f32, Rect)>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
            Shape::Text(t) => {
                let size = t
                    .galley
                    .job
                    .sections
                    .first()
                    .map(|s| s.format.font_id.size)
                    .unwrap_or(0.0);
                out.push((t.galley.job.text.clone(), size, t.visual_bounding_rect()))
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

/// Every text galley on screen with the text of each of its rows.
fn text_rows(h: &H) -> Vec<(f32, Vec<String>)> {
    fn walk(s: &Shape, out: &mut Vec<(f32, Vec<String>)>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|x| walk(x, out)),
            Shape::Text(t) => {
                let size = t
                    .galley
                    .job
                    .sections
                    .first()
                    .map(|s| s.format.font_id.size)
                    .unwrap_or(0.0);
                let chars: Vec<char> = t.galley.job.text.chars().collect();
                let mut at = 0;
                let mut rows = Vec::new();
                for r in &t.galley.rows {
                    let n = r.glyphs.len() + usize::from(r.ends_with_newline);
                    let end = (at + n).min(chars.len());
                    rows.push(chars[at..end].iter().collect::<String>());
                    at = end;
                }
                out.push((size, rows));
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

/// Enabled, visible controls a pointer can click: (role, label, rect).
fn clickables(h: &H) -> Vec<(Role, String, Rect)> {
    let mut out = Vec::new();
    for n in h.root().children_recursive() {
        let a = n.accesskit_node();
        if a.is_hidden() || a.is_disabled() {
            continue;
        }
        if !matches!(
            a.role(),
            Role::Button
                | Role::CheckBox
                | Role::Switch
                | Role::RadioButton
                | Role::ComboBox
                | Role::Link
        ) {
            continue;
        }
        let r = n.rect();
        if r.width() > 4.0 && r.height() > 4.0 {
            out.push((a.role(), a.label().unwrap_or_default(), r));
        }
    }
    out
}

/// The part of `r` the widget actually paints on screen: the union of its own shapes (those
/// inside `r`) cut to their clip rects. `None` when everything it paints is clipped away
/// (a row scrolled out of its list): accessibility rects ignore clipping.
fn painted_visible(h: &H, r: Rect) -> Option<Rect> {
    fn walk(s: &Shape, clip: Rect, r: Rect, out: &mut Option<Rect>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|x| walk(x, clip, r, out)),
            _ => {
                let b = s.visual_bounding_rect();
                if b.is_positive() && r.expand(2.0).contains_rect(b) {
                    let v = b.intersect(clip);
                    if v.is_positive() {
                        *out = Some(out.map_or(v, |o| o.union(v)));
                    }
                }
            }
        }
    }
    let mut out = None;
    for cs in &h.output().shapes {
        walk(&cs.shape, cs.clip_rect, r, &mut out);
    }
    out
}

fn renderer_ok() -> bool {
    std::panic::catch_unwind(egui_kittest::wgpu::WgpuTestRenderer::new).is_ok()
}

fn lin(c: u8) -> f64 {
    let c = c as f64 / 255.0;
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn lum(p: [u8; 4]) -> f64 {
    0.2126 * lin(p[0]) + 0.7152 * lin(p[1]) + 0.0722 * lin(p[2])
}

fn contrast(a: [u8; 4], b: [u8; 4]) -> f64 {
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// C1 (S1): no control changes the pointer. Every enabled button, checkbox, switch, radio,
/// pop-up and link shows the pointing hand on hover.
#[test]
fn review_c1_every_control_shows_the_pointing_hand() {
    type Go = fn(&mut H);
    let screens: Vec<(&str, fn() -> FakeMac, Go)> = vec![
        ("quick-sweep-idle", testkit_monitor::sweep_mac, |h| {
            h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
            // The start screen lays out its chips for these sources first (fix7: the chips are
            // toggles now, so their lines can change).
            for _ in 0..4 {
                h.step();
            }
            settle(h);
        }),
        ("quick-sweep-results", testkit_monitor::sweep_mac, |h| {
            h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
            // The start screen lays out its chips for these sources first (fix7: the chips are
            // toggles now, so their lines can change).
            for _ in 0..4 {
                h.step();
            }
            click(h, "Scan");
        }),
        ("system-junk-results", testkit_cleanup::junk_mac, |h| {
            go(h, "system_junk");
            click(h, "Scan");
        }),
        ("system-junk-confirm", testkit_cleanup::junk_mac, |h| {
            go(h, "system_junk");
            click(h, "Scan");
            click(h, "Review and Clean");
        }),
        ("system-junk-done", testkit_cleanup::junk_mac, |h| {
            go(h, "system_junk");
            click(h, "Scan");
            click(h, "Review and Clean");
            click(h, "Move to Trash");
        }),
        ("startup-items", tp::startup_mac, |h| go(h, "login_items")),
        ("large-old-files-idle", FakeMac::sample, |h| {
            go(h, "large_old_files")
        }),
        ("disk-map", FakeMac::sample, |h| {
            go(h, "disk_map");
            click(h, "Map Home Folder");
        }),
        ("settings", FakeMac::new, |h| go(h, "settings")),
    ];
    let mut bad = Vec::new();
    let mut total = 0;
    for (name, mac, open) in screens {
        let m = mac();
        base(&m);
        let mut h = harness(&m, 1280.0, 820.0, 1.0);
        open(&mut h);
        let modal = name.ends_with("confirm");
        for (role, label, r) in clickables(&h) {
            // Behind an open dialog only the dialog's own controls count.
            if modal && !matches!(label.as_str(), "Cancel" | "Move to Trash" | "Close dialog") {
                continue;
            }
            // Only what is on screen: rows scrolled out of a list keep their rect.
            let Some(seen) = painted_visible(&h, r) else {
                continue;
            };
            if seen.width() < 4.0 || seen.height() < 4.0 {
                continue;
            }
            total += 1;
            h.hover_at(seen.center());
            h.step();
            h.step();
            let c = h.output().platform_output.cursor_icon;
            if c != CursorIcon::PointingHand {
                bad.push(format!("{name}: {role:?} {label:?} at {r:?} shows {c:?}"));
            }
        }
    }
    // The screens above hold about 190 controls on screen; far fewer means the filter broke.
    assert!(total >= 150, "only {total} controls hovered");
    assert!(
        bad.is_empty(),
        "{} of {total} controls do not show the pointing hand:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

/// C2 (S1): keyboard focus must be visible. Focusing a control has to change its pixels.
#[test]

fn review_c2_keyboard_focus_is_visible() {
    if !renderer_ok() {
        return;
    }
    let m = testkit_cleanup::junk_mac();
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0, 1.0);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    h.hover_at(pos2(1275.0, 4.0));
    rest(&mut h);
    let before = h.render().expect("render");
    let mut bad = Vec::new();
    for label in [
        "Scan Again",
        "Select All",
        "Clean",
        "Mail Downloads",
        "Demo-abc",
        "Review and Clean",
    ] {
        let node = h.get_by_label(label);
        let r = node.rect().expand(6.0);
        node.focus();
        rest(&mut h);
        let img = h.render().expect("render");
        let mut changed = 0;
        for y in r.min.y.max(0.0) as u32..(r.max.y as u32).min(img.height()) {
            for x in r.min.x.max(0.0) as u32..(r.max.x as u32).min(img.width()) {
                let (p, q) = (before.get_pixel(x, y).0, img.get_pixel(x, y).0);
                if (0..3).any(|i| (p[i] as i32 - q[i] as i32).abs() > 24) {
                    changed += 1;
                }
            }
        }
        // A 2 pt ring around a 100 x 32 pt pill is several hundred pixels.
        if changed < 60 {
            bad.push(format!("{label}: {changed} pixels change on focus"));
        }
    }
    assert!(bad.is_empty(), "no visible focus:\n{}", bad.join("\n"));
}

/// C3 (S2): a click on a result row (its name, not the 22 pt checkbox) toggles it.
#[test]

fn review_c3_clicking_a_result_row_toggles_it() {
    let m = testkit_cleanup::junk_mac();
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0, 1.0);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    rest(&mut h);
    let cb = h.get_by_label("Demo-abc");
    let was = cb.accesskit_node().is_selected().unwrap_or(false)
        || format!("{:?}", cb.accesskit_node().toggled()).contains("True");
    let r = cb.rect();
    // The row's name sits about 80 pt right of the checkbox.
    click_at(&mut h, pos2(r.center().x + 100.0, r.center().y));
    let cb = h.get_by_label("Demo-abc");
    let now = cb.accesskit_node().is_selected().unwrap_or(false)
        || format!("{:?}", cb.accesskit_node().toggled()).contains("True");
    assert_ne!(was, now, "clicking the row's name did not toggle the row");
}

/// C4 (S2): category names are never shortened in the middle ("Deve...aches") at 1060 x 680.
#[test]

fn review_c4_category_names_are_whole_at_the_smallest_window() {
    let mut bad = std::collections::BTreeSet::new();
    for (page, mac) in [
        ("system_junk", testkit_cleanup::junk_mac as fn() -> FakeMac),
        ("privacy", testkit_cleanup::privacy_mac),
        ("trash_bins", testkit_cleanup::trash_mac),
    ] {
        let m = mac();
        base(&m);
        let mut h = harness(&m, 1060.0, 680.0, 1.0);
        go(&mut h, page);
        click(&mut h, "Scan");
        for (t, size, r) in texts(&h) {
            // Category names: 15 pt, in the left column (x < 520), cut with an ellipsis.
            if (size - 15.0).abs() < 0.1
                && r.min.x < 520.0
                && (t.contains('\u{2026}') || t.contains("..."))
            {
                bad.insert(format!("{page}: {t:?}"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "{} names cut:\n{}",
        bad.len(),
        bad.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// C5 (S2): behind the confirmation dialog the page is inert (no I-beam over its search
/// field), and the first Tab lands on Cancel, never on the destructive button.
#[test]

fn review_c5_confirm_dialog_is_modal_for_pointer_and_keyboard() {
    let m = testkit_cleanup::junk_mac();
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0, 1.0);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    let search = h.get_by_label("Search").rect();
    click(&mut h, "Review and Clean");
    let mut bad = Vec::new();
    h.hover_at(search.center());
    h.step();
    h.step();
    let c = h.output().platform_output.cursor_icon;
    if c == CursorIcon::Text {
        bad.push(format!(
            "I-beam over the Search field behind the dialog ({c:?})"
        ));
    }
    // design4 round 4: the target's "Cancel focused when the dialog opens" (lead), so the
    // keyboard starts on Cancel without a Tab; Tab then reaches "Move to Trash".
    let focused = h
        .root()
        .children_recursive()
        .find(|n| n.accesskit_node().is_focused())
        .and_then(|n| n.accesskit_node().label());
    if focused.as_deref() != Some("Cancel") {
        bad.push(format!(
            "the dialog opens with {focused:?} focused, not Cancel"
        ));
    }
    h.key_press(egui::Key::Tab);
    settle(&mut h);
    let next = h
        .root()
        .children_recursive()
        .find(|n| n.accesskit_node().is_focused())
        .and_then(|n| n.accesskit_node().label());
    if next.as_deref() != Some("Move to Trash") {
        bad.push(format!(
            "Tab from Cancel focuses {next:?}, not Move to Trash"
        ));
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// C6 (S2): an unchecked checkbox's border reaches 3:1 against the list card (WCAG 1.4.11).
#[test]

fn review_c6_unchecked_checkbox_border_reaches_3_to_1() {
    if !renderer_ok() {
        return;
    }
    let m = testkit_cleanup::junk_mac();
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0, 2.0);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    h.hover_at(pos2(1275.0, 4.0));
    rest(&mut h);
    let r = h.get_by_label("models").rect();
    let img = h.render().expect("render");
    let y = (r.center().y * 2.0) as u32;
    // Card just outside the box, then the brightest pixel of the box's left edge.
    let card = img.get_pixel(((r.min.x - 3.0) * 2.0) as u32, y).0;
    let mut border = card;
    for x in ((r.min.x - 1.0) * 2.0) as u32..((r.min.x + 6.0) * 2.0) as u32 {
        let p = img.get_pixel(x, y).0;
        if lum(p) > lum(border) {
            border = p;
        }
    }
    let c = contrast(border, card);
    assert!(
        c >= 3.0,
        "unchecked border {border:?} on card {card:?} = {c:.2}:1, needs 3:1"
    );
}

/// C7 (S2): Live Monitor's "Free Up RAM" and "Free Up Space" can only be clicked on the pill
/// itself (they claim a 277 pt wide row today).
#[test]
fn review_c7_monitor_buttons_hit_area_matches_the_pill() {
    let m = FakeMac::new();
    testkit_monitor::answer_all(m.env.fake_sys().expect("fake sys"));
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0, 1.0);
    go(&mut h, "monitor");
    let mut bad = Vec::new();
    for label in ["Free Up RAM", "Free Up Space"] {
        let r = h.get_by_label(label).rect();
        if r.width() > 160.0 {
            bad.push(format!("{label}: hit area {:.0} pt wide", r.width()));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// C8 (S2): the primary pill's fill has no hard step (the top sheen is a rectangle inset in
/// the pill today: 16 levels in one pixel on both sides).
#[test]

fn review_c8_primary_pill_fill_has_no_hard_edge() {
    if !renderer_ok() {
        return;
    }
    let m = testkit_cleanup::junk_mac();
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0, 2.0);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    h.hover_at(pos2(1275.0, 4.0));
    rest(&mut h);
    let r = h.get_by_label("Review and Clean").rect();
    let img = h.render().expect("render");
    // A row through the top quarter of the pill, above the label, away from the round ends.
    let y = ((r.min.y + r.height() * 0.22) * 2.0) as u32;
    let (x0, x1) = (
        ((r.min.x + 5.0) * 2.0) as u32,
        ((r.max.x - 5.0) * 2.0) as u32,
    );
    let mut worst = (0, 0);
    for x in x0..x1 {
        let (p, q) = (img.get_pixel(x, y).0, img.get_pixel(x + 1, y).0);
        let d = (0..3)
            .map(|i| (p[i] as i32 - q[i] as i32).abs())
            .max()
            .unwrap_or(0);
        if d > worst.0 {
            worst = (d, x);
        }
    }
    assert!(
        worst.0 <= 4,
        "fill step of {} levels at x {} px (y {y} px)",
        worst.0,
        worst.1
    );
}

/// C9 (S2): on two-column start screens the round Scan disc starts on the text column's
/// left edge (21 pt to the right today).
#[test]

fn review_c9_scan_disc_aligns_with_the_text_column() {
    let m = testkit_cleanup::junk_mac();
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0, 1.0);
    go(&mut h, "system_junk");
    let title = texts(&h)
        .into_iter()
        .find(|(t, size, _)| t == "System Junk" && *size >= 30.0)
        .expect("34 pt title")
        .2;
    let scan = h.get_by_label("Scan").rect();
    let d = scan.min.x - title.min.x;
    assert!(
        d.abs() <= 3.0,
        "Scan disc at x {:.0}, title at x {:.0}: {d:.0} pt off",
        scan.min.x,
        title.min.x
    );
}

/// C10 (S2): the first results frame never shows a total that disagrees with the figures
/// beside it ("0 KB found in 43 items" next to "All items 52.2 MB").
#[test]

fn review_c10_first_results_frame_is_consistent() {
    let m = testkit_cleanup::junk_mac();
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0, 1.0);
    go(&mut h, "system_junk");
    h.get_by_label("Scan").click();
    h.step();
    h.step();
    let first: Vec<String> = texts(&h)
        .into_iter()
        .filter(|(_, size, _)| *size >= 50.0)
        .map(|(t, _, _)| t)
        .collect();
    rest(&mut h);
    let last: Vec<String> = texts(&h)
        .into_iter()
        .filter(|(_, size, _)| *size >= 50.0)
        .map(|(t, _, _)| t)
        .collect();
    let all_items_final = texts(&h).iter().any(|(t, _, _)| t == "52.2 MB");
    assert!(all_items_final, "fixture: 52.2 MB total");
    assert_eq!(
        first, last,
        "display total on the first results frame differs from the final figure shown in the category list"
    );
}

/// C11 (S2): every slider and value field in Settings has an accessible name.
#[test]
fn review_c11_settings_value_fields_are_named() {
    let m = FakeMac::new();
    base(&m);
    let mut h = harness(&m, 1280.0, 1400.0, 1.0);
    go(&mut h, "settings");
    let mut bad = Vec::new();
    for n in h.root().children_recursive() {
        let a = n.accesskit_node();
        if matches!(a.role(), Role::SpinButton | Role::Slider | Role::TextInput)
            && a.label().unwrap_or_default().trim().is_empty()
        {
            bad.push(format!("{:?} at {:?}", a.role(), n.rect()));
        }
    }
    assert!(bad.is_empty(), "unnamed controls:\n{}", bad.join("\n"));
}

/// C13 (S2): when a row's Reveal and Ignore appear, the path is re-shortened to end before
/// them (today it runs under them and is cut mid-word with no ellipsis).
#[test]

fn review_c13_hovered_row_path_ends_before_its_buttons() {
    let m = testkit_cleanup::junk_mac();
    base(&m);
    let mut h = harness(&m, 1280.0, 820.0, 1.0);
    go(&mut h, "system_junk");
    click(&mut h, "Scan");
    rest(&mut h);
    let cb = h.get_by_label("AAA").rect();
    h.hover_at(pos2(cb.center().x + 300.0, cb.center().y));
    rest(&mut h);
    let buttons_left = clickables(&h)
        .into_iter()
        .filter(|(_, l, r)| {
            (l == "Reveal" || l == "Ignore") && (r.center().y - cb.center().y).abs() < 20.0
        })
        .map(|(_, _, r)| r.min.x)
        .fold(f32::INFINITY, f32::min);
    assert!(buttons_left.is_finite(), "Reveal/Ignore on the AAA row");
    let path = texts(&h)
        .into_iter()
        .find(|(t, _, r)| {
            t.contains("MobileSync/Backup/AAA")
                || (t.contains("MobileSy") && (r.center().y - cb.center().y).abs() < 20.0)
        })
        .expect("AAA path text");
    // The solid under the buttons starts 10 pt left of them.
    assert!(
        path.2.max.x <= buttons_left - 10.0,
        "path {:?} ends at x {:.0}, the buttons start at {:.0}",
        path.0,
        path.2.max.x,
        buttons_left
    );
}

/// C16 (S3): start-screen descriptions never end on a one-word line ("...and your / apps.").
#[test]

fn review_c16_start_screen_descriptions_have_no_widow() {
    let mut bad = Vec::new();
    for (name, mac, page) in [
        (
            "quick-sweep",
            testkit_monitor::sweep_mac as fn() -> FakeMac,
            "",
        ),
        ("system-junk", testkit_cleanup::junk_mac, "system_junk"),
        ("disk-map", FakeMac::sample, "disk_map"),
        ("large-old-files", FakeMac::sample, "large_old_files"),
    ] {
        for (w, hgt) in [(1060.0, 680.0), (1280.0, 820.0), (1600.0, 1000.0)] {
            let m = mac();
            base(&m);
            let mut h = harness(&m, w, hgt, 1.0);
            if page.is_empty() {
                h.state_mut().sweep.sources = Some(testkit_monitor::sweep_sources());
                // The start screen lays out its chips for these sources first (fix7: the chips are
                // toggles now, so their lines can change).
                for _ in 0..4 {
                    h.step();
                }
                settle(&mut h);
            } else {
                go(&mut h, page);
            }
            for (size, rows) in text_rows(&h) {
                // Lead text under a start-screen title is BODY_L (16 pt).
                if (size - 16.0).abs() > 0.1 || rows.len() < 2 {
                    continue;
                }
                let last = rows.last().cloned().unwrap_or_default();
                if last.split_whitespace().count() < 2 {
                    bad.push(format!(
                        "{name} {w}x{hgt}: last line {:?} of {:?}",
                        last.trim(),
                        rows.concat()
                    ));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
