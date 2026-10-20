//! The flows users hit most, end to end on the real modules (the builders' Quick Sweep tests
//! use stand-ins for the modules other builders owned at the time).

mod common;

use cleancraft_core::FeatureId;
use cleancraft_core::fsutil::{human_size, tree_size};
use cleancraft_core::sweep::Sources;
use cleancraft_core::testkit_cleanup::mail_mac;
use cleancraft_core::{Plan, executor};
use common::*;
use egui_kittest::kittest::Queryable;

fn plan_of(h: &H, id: FeatureId) -> Plan {
    h.state()
        .sweep
        .result
        .as_ref()
        .expect("scanned")
        .tile(id)
        .and_then(|t| t.plan())
        .unwrap_or_else(|| panic!("{} has no plan", id.title()))
        .clone()
}

fn selected_on_disk(p: &Plan) -> u64 {
    p.selected().map(|i| tree_size(&i.path).0).sum()
}

/// Quick Sweep on the registered modules (System Junk, Trash Bins, Mail Downloads, Threat
/// Check): tile numbers against the disk, Run lists exactly the selected files of the runnable
/// modules (Mail Downloads preselects nothing, so none of its attachments), moves exactly
/// those, and Undo puts the module back.
#[test]
fn quick_sweep_runs_the_real_modules_and_undoes_one() {
    let mac = mail_mac();
    mac.home_file("Library/Caches/com.example.Browser/Cache.db", 1_200_000);
    mac.home_file("Library/Caches/com.example.Editor/index", 700_000);
    mac.home_file("Library/Logs/Example/app.log", 300_000);
    mac.home_file(".Trash/old.dmg", 800_000);
    let mut h = harness(&mac);
    click(&mut h, "Scan");
    let ids: Vec<FeatureId> = h
        .state()
        .sweep
        .result
        .as_ref()
        .unwrap()
        .tiles
        .iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, Sources::registered().ids());
    for id in [
        FeatureId::SystemJunk,
        FeatureId::TrashBins,
        FeatureId::MailAttachments,
        FeatureId::Malware,
    ] {
        assert!(ids.contains(&id), "{} tile missing", id.title());
    }
    let junk = plan_of(&h, FeatureId::SystemJunk);
    let mail = plan_of(&h, FeatureId::MailAttachments);
    let trash = plan_of(&h, FeatureId::TrashBins);
    let j = selected_on_disk(&junk);
    assert!(j > 0, "junk {j}");
    assert!(has(&h, &human_size(j)), "junk tile {}", human_size(j));
    // Mail Downloads preselects nothing: its tile says what was found and that none of it is
    // selected, and the header counts only what is selected.
    assert!(!mail.items.is_empty());
    assert_eq!(mail.selected_count(), 0, "a mail attachment is preselected");
    assert!(has(
        &h,
        &format!(
            "0 of {} items selected, {} found",
            mail.items.len(),
            human_size(mail.total_size())
        )
    ));
    assert_eq!(
        h.state()
            .sweep
            .result
            .as_ref()
            .unwrap()
            .ready_bytes(&h.state().settings.sweep_skip),
        j
    );
    // The startup disk's Trash counts what is in it (as the Trash Bins page does).
    let user_trash = mac.env.user_trash();
    let t: u64 = trash
        .items
        .iter()
        .map(|i| {
            if i.path == user_trash {
                cleancraft_core::fsutil::children(&i.path)
                    .iter()
                    .map(|c| tree_size(c).0)
                    .sum()
            } else {
                tree_size(&i.path).0
            }
        })
        .sum();
    assert!(has(&h, &human_size(t)), "trash tile {}", human_size(t));

    click(&mut h, "Review and Clean");
    let listed: Vec<_> = junk.selected().map(|i| i.path.clone()).collect();
    for p in &listed {
        assert_eq!(
            h.query_all_by_label(&mac.env.display(p)).count(),
            1,
            "{} not listed once",
            p.display()
        );
    }
    for i in &mail.items {
        assert_eq!(
            h.query_all_by_label(&mac.env.display(&i.path)).count(),
            0,
            "unticked attachment {} listed",
            i.path.display()
        );
    }
    let n = listed.len();
    assert!(has(&h, &format!("Clean {n} items ({})?", human_size(j))));
    click(&mut h, "Move to Trash");
    let all: Vec<_> = [&junk, &mail, &trash]
        .iter()
        .flat_map(|p| p.items.iter().map(|i| i.path.clone()))
        .collect();
    for p in &all {
        assert_eq!(p.exists(), !listed.contains(p), "{}", p.display());
    }
    assert!(has(&h, &format!("{} moved to the Trash", human_size(j))));
    let moved = executor::history(&mac.env).len();
    assert_eq!(moved, n, "one journal line per moved item");

    // One result card (System Junk); its Undo puts every junk file back, mail untouched.
    assert_eq!(h.query_all_by_label("Undo").count(), 1);
    click(&mut h, "Undo");
    for i in junk.selected() {
        assert!(i.path.exists(), "{} not put back", i.path.display());
    }
    for i in &mail.items {
        assert!(i.path.exists(), "{} touched", i.path.display());
    }
}
