//! Reliability review (reviewer "reliability"): the engine under real-world failures on a fake
//! Mac. Files changing between scan and clean, a damaged action log, the Trash emptied before
//! Undo, a plan run twice, agents and the CLI working from saved plans.
//!
//! Ignored tests prove defects (ID in the ignore reason); the others protect behaviour that was
//! checked and found right. Run: `cargo test -p cleancraft-core --test review_reliability_core`
//! and the defects with `-- --ignored`.

use std::fs;
use std::path::{Path, PathBuf};

use cleancraft_core::executor::{self, UndoError, execute, history, journal_path, undo, undo_plan};
use cleancraft_core::features::{self, allow_roots};
use cleancraft_core::fsutil::tree_size;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_perf as tp;
use cleancraft_core::{ActionKind, Fault, FeatureId, Method, Plan, Settings, SkipReason, plans};

fn junk_plan(m: &FakeMac) -> Plan {
    features::scan(&m.env, &Settings::default(), FeatureId::SystemJunk).unwrap()
}

fn run(m: &FakeMac, plan: &Plan) -> executor::Report {
    let allow = allow_roots(&m.env, FeatureId::SystemJunk);
    execute(
        &m.env,
        plan,
        &allow,
        &plan.digest(),
        Method::Trash,
        &mut |_, _| {},
    )
    .unwrap()
}

/// A plan with exactly one selected item: the item at `path`.
fn only(plan: &Plan, path: &Path) -> Plan {
    let mut p = plan.clone();
    for i in 0..p.items.len() {
        let on = p.items[i].path == path;
        p.set_selected(i, on);
    }
    assert_eq!(p.selected_count(), 1, "{} not in the plan", path.display());
    p
}

fn stored_of(rec: &cleancraft_core::ActionRecord) -> PathBuf {
    match &rec.kind {
        ActionKind::Moved { stored, .. } => stored.clone(),
        k => panic!("not a move: {k:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// Defects
// ---------------------------------------------------------------------------------------------

/// REL-1. A crash or power loss while a journal line is written leaves a last line without its
/// newline. The next cleanup appends its first record right after it, so that record is glued to
/// the damaged line and can no longer be read: the item sits in the Trash with no History entry
/// and no Undo.
#[test]
fn truncated_journal_line_does_not_swallow_the_next_move() {
    let m = FakeMac::sample();
    fs::create_dir_all(m.env.data_dir()).unwrap();
    // An earlier write cut short: no closing brace, no newline.
    fs::write(
        journal_path(&m.env),
        br#"{"id":"system_junk-1#0","plan_id":"system_junk-1","feature":"system_junk","time":1"#,
    )
    .unwrap();
    let plan = junk_plan(&m);
    let rep = run(&m, &plan);
    assert!(rep.moved.len() >= 2, "{:?}", rep.skipped);
    let h = history(&m.env);
    for rec in &rep.moved {
        assert!(
            h.iter().any(|r| r.id == rec.id),
            "{} was moved to {} but History has no line for it",
            rec.id,
            stored_of(rec).display()
        );
    }
    let (n, errs) = undo_plan(&m.env, &rep.plan_id);
    assert_eq!(n, rep.moved.len(), "Undo put back only {n}: {errs:?}");
}

/// REL-2. Undo rewrites the whole journal from the lines it could parse. Lines it cannot read
/// (a damaged line, or a record from a newer CleanCraft) are dropped for good, so one Undo erases
/// part of the action log that History and support would need.
#[test]
fn undo_keeps_journal_lines_it_cannot_read() {
    let m = FakeMac::sample();
    fs::create_dir_all(m.env.data_dir()).unwrap();
    let foreign = r#"{"id":"future-1#0","plan_id":"future-1","feature":null,"time":5,"size":1,"kind":"renamed","from":"/a","to":"/b"}"#;
    fs::write(journal_path(&m.env), format!("{foreign}\n")).unwrap();
    let plan = junk_plan(&m);
    let rep = run(&m, &plan);
    undo(&m.env, &rep.moved[0].id).unwrap();
    let text = fs::read_to_string(journal_path(&m.env)).unwrap();
    assert!(
        text.contains(foreign),
        "the line CleanCraft could not read was deleted by Undo:\n{text}"
    );
}

/// REL-3. Running the same saved plan again after an Undo (CLI `clean <plan> --yes` twice, MCP
/// `execute_plan` twice) logs the second moves under the same action ids ("<plan>#<item>").
/// Undo looks up the first record with that id, finds it already put back, and refuses: the
/// item stays in the Trash and History offers an Undo that cannot work.
#[test]
fn same_plan_run_again_after_undo_can_be_undone_again() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    let first = run(&m, &plan);
    assert!(!first.moved.is_empty());
    let (n, errs) = undo_plan(&m.env, &plan.id);
    assert_eq!((n, errs.len()), (first.moved.len(), 0));
    // Same plan, same digest, same files back in place: it runs again.
    let second = run(&m, &plan);
    assert_eq!(second.moved.len(), first.moved.len());
    let (n, errs) = undo_plan(&m.env, &plan.id);
    assert!(
        errs.is_empty(),
        "second Undo failed for {} items: {:?}",
        errs.len(),
        errs.first()
    );
    assert_eq!(n, second.moved.len());
    for rec in &second.moved {
        if let ActionKind::Moved { original, .. } = &rec.kind {
            assert!(
                original.exists(),
                "{} still in the Trash",
                original.display()
            );
        }
    }
}

/// REL-4. MCP `execute_plan` checks the digest of the saved plan's own selection, then applies
/// `only_items` and runs with the digest of the new selection. Items the user never saw selected
/// (not part of the confirmed digest) can be added by the agent and are moved.
/// This test replays `crates/mcp/src/lib.rs` `execute_plan` step by step (reviewers may add tests
/// only to core and ui-egui).
#[test]
fn mcp_only_items_cannot_reach_beyond_the_confirmed_selection() {
    let m = FakeMac::sample();
    let settings = Settings {
        agents_may_clean: true,
        ..Settings::default()
    };
    settings.save(&m.env).unwrap();
    // Scan: the user is shown one selected item and confirms exactly that list.
    let full = junk_plan(&m);
    let editor = m.env.home_path("Library/Caches/com.example.Editor");
    let shown = only(&full, &editor);
    plans::save(&m.env, &shown).unwrap();
    let confirmed = shown.digest();
    let other = shown
        .items
        .iter()
        .find(|i| !i.selected && i.locked.is_none())
        .expect("an item the user did not select")
        .clone();
    // execute_plan(plan_id, digest = confirmed, only_items = [other]) as written in mcp/lib.rs
    // (fix2, REL-4: it now narrows with features::narrow_selection).
    let mut plan = plans::load(&m.env, &shown.id).unwrap();
    assert_eq!(plan.digest(), confirmed);
    let only_items = [other.id];
    let rep = features::narrow_selection(&mut plan, &only_items)
        .map_err(anyhow::Error::msg)
        .and_then(|()| {
            let digest = plan.digest();
            let s = Settings::load(&m.env);
            features::run_plan(&m.env, &s, &plan, &digest, &mut |_, _| {})
        });
    assert!(
        other.path.exists(),
        "{} was moved although the confirmed list did not contain it ({:?})",
        other.path.display(),
        rep.map(|r| r.moved.len())
    );
}

/// REL-5. The Trash was emptied, then another item with the same name was put in the Trash
/// (by Finder or a later cleanup). Undo finds "something" at the stored place and moves that
/// unrelated file out of the Trash into the original folder, instead of saying the removed copy
/// is gone.
#[test]
fn undo_refuses_a_different_file_at_the_stored_place() {
    let m = FakeMac::sample();
    let editor = m.env.home_path("Library/Caches/com.example.Editor");
    let plan = only(&junk_plan(&m), &editor);
    let rep = run(&m, &plan);
    let stored = stored_of(&rep.moved[0]);
    // Trash emptied (Finder), then a different thing with the same name is trashed.
    fs::remove_dir_all(&stored).unwrap();
    fs::write(&stored, b"someone else's file").unwrap();
    let r = undo(&m.env, &rep.moved[0].id);
    assert!(
        matches!(r, Err(UndoError::StoredMissing(_))),
        "Undo answered {r:?} and moved the stand-in to {}",
        editor.display()
    );
    assert!(stored.exists(), "the unrelated file left the Trash");
}

/// REL-6. Startup Items (and Threat Check) stop the launch agent with `launchctl bootout` before
/// the executor decides whether the plist can move. When the move is then skipped (permission
/// denied, file in use, disk full) the agent is stopped anyway, the file stays, and the journal
/// holds only a Command line, which cannot be undone.
#[test]
fn agent_is_not_stopped_when_its_file_is_not_moved() {
    let m = tp::startup_mac();
    let settings = Settings::default();
    let mut plan = features::scan(&m.env, &settings, FeatureId::LoginItems).unwrap();
    let helper = m
        .env
        .home_path("Library/LaunchAgents/com.example.helper.plist");
    let id = plan.items.iter().find(|i| i.path == helper).unwrap().id;
    plan.set_selected(id, true);
    m.env.inject_fault(&helper, Fault::PermissionDenied);
    let rep = features::run_plan(&m.env, &settings, &plan, &plan.digest(), &mut |_, _| {}).unwrap();
    assert_eq!(rep.moved.len(), 0);
    assert_eq!(rep.skipped[0].1, SkipReason::PermissionDenied);
    assert!(helper.exists());
    let booted = m
        .env
        .fake_sys()
        .unwrap()
        .calls()
        .into_iter()
        .any(|c| c.contains("bootout") && c.contains("com.example.helper"));
    let undoable = history(&m.env).iter().any(|r| r.can_undo());
    assert!(
        !booted || undoable,
        "the agent was stopped (bootout) although its file was not moved, and nothing can undo it"
    );
}

/// REL-9. A name of 255 bytes (the APFS limit) that already exists in the Trash: the free name
/// "<name> 2" is too long, the rename fails with "File name too long" and the item is skipped.
/// It should still go to the Trash (Finder shortens the name) or to the quarantine.
#[test]
fn longest_name_already_in_the_trash_still_moves() {
    let m = FakeMac::new();
    let name = "a".repeat(255);
    let log = m.home_file(&format!("Library/Logs/{name}"), 40_000);
    fs::write(m.env.user_trash().join(&name), b"older").unwrap();
    let plan = junk_plan(&m);
    let plan = only(&plan, &log);
    let rep = run(&m, &plan);
    assert_eq!(
        rep.moved.len(),
        1,
        "not moved: {:?}",
        rep.skipped
            .iter()
            .map(|s| s.1.to_string())
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------------------------
// Checked and found right
// ---------------------------------------------------------------------------------------------

#[test]
fn file_deleted_between_scan_and_clean_is_reported_and_others_move() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    let gone = m.env.home_path("Library/Caches/com.example.Editor");
    fs::remove_dir_all(&gone).unwrap();
    let rep = run(&m, &plan);
    assert!(
        rep.skipped
            .iter()
            .any(|(p, r)| p == &gone && *r == SkipReason::Missing)
    );
    assert_eq!(rep.moved.len(), plan.selected_count() - 1);
    assert_eq!(history(&m.env).len(), rep.moved.len());
}

#[cfg(unix)]
#[test]
fn item_replaced_by_a_symlink_moves_only_the_link() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    let editor = m.env.home_path("Library/Caches/com.example.Editor");
    let plan = only(&plan, &editor);
    // Between scan and clean the folder becomes a link to the user's projects.
    let projects = m.home_file("Projects/thesis.tex", 5_000);
    fs::remove_dir_all(&editor).unwrap();
    std::os::unix::fs::symlink(projects.parent().unwrap(), &editor).unwrap();
    let rep = run(&m, &plan);
    assert_eq!(rep.moved.len(), 1);
    assert!(projects.exists(), "the link target must stay untouched");
    let stored = stored_of(&rep.moved[0]);
    assert!(
        fs::symlink_metadata(&stored)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    // The size logged is the link's own, never the target's.
    assert!(rep.freed < 5_000, "freed {}", rep.freed);
}

#[cfg(unix)]
#[test]
fn parent_replaced_by_a_symlink_is_refused() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    let log = m.env.home_path("Library/Logs/Example");
    let plan = only(&plan, &log);
    // ~/Library/Logs becomes a link to elsewhere: the item is now reached through a link.
    let logs = m.env.home_path("Library/Logs");
    let elsewhere = m.env.home_path("Elsewhere");
    fs::rename(&logs, &elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, &logs).unwrap();
    let rep = run(&m, &plan);
    assert!(rep.moved.is_empty());
    assert!(matches!(rep.skipped[0].1, SkipReason::Refused(_)));
    assert!(elsewhere.join("Example/app.log").exists());
}

#[test]
fn grown_file_counts_its_size_at_move_time() {
    let m = FakeMac::sample();
    let editor = m.env.home_path("Library/Caches/com.example.Editor");
    let plan = only(&junk_plan(&m), &editor);
    m.home_file("Library/Caches/com.example.Editor/new-blob", 2_000_000);
    let now = tree_size(&editor).0;
    let rep = run(&m, &plan);
    assert_eq!(rep.freed, now);
    assert_eq!(rep.moved[0].size, now);
}

#[test]
fn running_a_plan_twice_moves_nothing_the_second_time() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    let first = run(&m, &plan);
    let lines = history(&m.env).len();
    let second = run(&m, &plan);
    assert!(second.moved.is_empty());
    assert_eq!(second.freed, 0);
    assert!(
        second
            .skipped
            .iter()
            .all(|(_, r)| *r == SkipReason::Missing)
    );
    assert_eq!(history(&m.env).len(), lines, "no extra journal lines");
    assert_eq!(lines, first.moved.len());
}

#[test]
fn trash_emptied_before_undo_is_explained_and_changes_nothing() {
    let m = FakeMac::sample();
    let editor = m.env.home_path("Library/Caches/com.example.Editor");
    let plan = only(&junk_plan(&m), &editor);
    let rep = run(&m, &plan);
    fs::remove_dir_all(stored_of(&rep.moved[0])).unwrap();
    let r = undo(&m.env, &rep.moved[0].id);
    assert!(matches!(r, Err(UndoError::StoredMissing(_))), "{r:?}");
    assert!(r.unwrap_err().to_string().contains("Trash emptied"));
    assert!(!editor.exists());
    assert!(
        history(&m.env)[0].can_undo(),
        "still listed, not marked put back"
    );
}

#[test]
fn undo_recreates_a_missing_parent_folder() {
    let m = FakeMac::sample();
    let log = m
        .env
        .home_path("Library/Logs/DiagnosticReports/Example-2026.ips");
    let plan = junk_plan(&m);
    let Some(item) = plan.items.iter().find(|i| i.path == log) else {
        panic!("crash report not proposed")
    };
    let plan = only(&plan, &item.path.clone());
    let rep = run(&m, &plan);
    fs::remove_dir_all(m.env.home_path("Library/Logs/DiagnosticReports")).unwrap();
    undo(&m.env, &rep.moved[0].id).unwrap();
    assert!(log.exists());
}

#[test]
fn undo_twice_is_refused_the_second_time() {
    let m = FakeMac::sample();
    let editor = m.env.home_path("Library/Caches/com.example.Editor");
    let plan = only(&junk_plan(&m), &editor);
    let rep = run(&m, &plan);
    undo(&m.env, &rep.moved[0].id).unwrap();
    assert_eq!(
        undo(&m.env, &rep.moved[0].id),
        Err(UndoError::AlreadyUndone)
    );
    assert!(editor.exists());
}

#[test]
fn undo_when_the_place_is_taken_keeps_both() {
    let m = FakeMac::sample();
    let editor = m.env.home_path("Library/Caches/com.example.Editor");
    let plan = only(&junk_plan(&m), &editor);
    let rep = run(&m, &plan);
    // The app rebuilt its cache meanwhile.
    m.home_file("Library/Caches/com.example.Editor/index", 10);
    let r = undo(&m.env, &rep.moved[0].id);
    assert!(matches!(r, Err(UndoError::OriginalTaken(_))), "{r:?}");
    assert!(stored_of(&rep.moved[0]).exists());
    assert_eq!(fs::read(editor.join("index")).unwrap().len(), 10);
}

#[test]
fn damaged_lines_do_not_hide_the_good_ones() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    let rep = run(&m, &plan);
    let mut text = fs::read_to_string(journal_path(&m.env)).unwrap();
    text.push_str("not json at all\n\u{0}\u{0}\n{\"id\":\n");
    fs::write(journal_path(&m.env), text).unwrap();
    assert_eq!(history(&m.env).len(), rep.moved.len());
    let (n, errs) = undo_plan(&m.env, &rep.plan_id);
    assert_eq!((n, errs.len()), (rep.moved.len(), 0));
}

#[test]
fn journal_disk_full_moves_nothing_and_logs_nothing() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    m.env.inject_fault(&journal_path(&m.env), Fault::DiskFull);
    let allow = allow_roots(&m.env, FeatureId::SystemJunk);
    let r = execute(
        &m.env,
        &plan,
        &allow,
        &plan.digest(),
        Method::Trash,
        &mut |_, _| {},
    );
    assert!(r.is_err());
    for it in plan.selected() {
        assert!(it.path.exists());
    }
    assert!(history(&m.env).is_empty());
}

#[test]
fn startup_journal_unwritable_stops_no_agent() {
    let m = tp::startup_mac();
    let settings = Settings::default();
    let mut plan = features::scan(&m.env, &settings, FeatureId::LoginItems).unwrap();
    let helper = m
        .env
        .home_path("Library/LaunchAgents/com.example.helper.plist");
    let id = plan.items.iter().find(|i| i.path == helper).unwrap().id;
    plan.set_selected(id, true);
    m.env.inject_fault(&journal_path(&m.env), Fault::DiskFull);
    let r = features::run_plan(&m.env, &settings, &plan, &plan.digest(), &mut |_, _| {});
    assert!(r.is_err());
    assert!(helper.exists());
    assert!(
        !m.env
            .fake_sys()
            .unwrap()
            .calls()
            .iter()
            .any(|c| c.contains("bootout"))
    );
}

#[test]
fn damaged_settings_fall_back_to_safe_defaults() {
    let m = FakeMac::new();
    let s = Settings {
        agents_may_clean: true,
        ..Settings::default()
    };
    s.save(&m.env).unwrap();
    let full = fs::read(Settings::path(&m.env)).unwrap();
    fs::write(Settings::path(&m.env), &full[..full.len() / 2]).unwrap();
    let loaded = Settings::load(&m.env);
    assert!(
        !loaded.agents_may_clean,
        "a damaged file never lets agents clean"
    );
    assert_eq!(loaded.method, Method::Trash);
}

#[test]
fn damaged_or_forged_saved_plans_are_refused() {
    let m = FakeMac::sample();
    let plan = junk_plan(&m);
    let p = plans::save(&m.env, &plan).unwrap();
    let full = fs::read(&p).unwrap();
    fs::write(&p, &full[..full.len() / 3]).unwrap();
    assert!(plans::load(&m.env, &plan.id).is_err());
    assert!(plans::load(&m.env, "../settings").is_err());
    assert!(plans::load(&m.env, "nope").is_err());
    // A saved plan edited to reach outside the allow-list is refused per item by the fence.
    let mut forged = plan.clone();
    let doc = m.env.home_path("Documents/report.pdf");
    forged.items[0].path = doc.clone();
    forged.items[0].selected = true;
    forged.items[0].locked = None;
    let rep = features::run_plan(
        &m.env,
        &Settings::default(),
        &forged,
        &forged.digest(),
        &mut |_, _| {},
    )
    .unwrap();
    assert!(doc.exists());
    assert!(rep.skipped.iter().any(|(p, _)| p == &doc));
}

#[test]
fn stale_digest_is_refused_before_anything_runs() {
    let m = tp::startup_mac();
    let settings = Settings::default();
    let mut plan = features::scan(&m.env, &settings, FeatureId::LoginItems).unwrap();
    let helper = m
        .env
        .home_path("Library/LaunchAgents/com.example.helper.plist");
    let id = plan.items.iter().find(|i| i.path == helper).unwrap().id;
    plan.set_selected(id, true);
    let old = plan.digest();
    plan.items[id].size += 1; // the file grew and the plan was refreshed
    let r = features::run_plan(&m.env, &settings, &plan, &old, &mut |_, _| {});
    assert!(r.is_err());
    assert!(helper.exists());
    assert!(
        !m.env
            .fake_sys()
            .unwrap()
            .calls()
            .iter()
            .any(|c| c.contains("bootout")),
        "nothing runs before the digest check"
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_name_is_never_lost() {
    use std::os::unix::ffi::OsStrExt;
    let m = FakeMac::new();
    let name = std::ffi::OsStr::from_bytes(b"bad\xff\xfename.log");
    let p = m.env.home_path("Library/Logs").join(name);
    if fs::write(&p, vec![7u8; 30_000]).is_err() {
        return; // APFS refuses names that are not UTF-8 (EILSEQ): nothing to lose on a Mac
    }
    let plan = junk_plan(&m);
    let Some(item) = plan.items.iter().find(|i| i.path == p) else {
        return; // not proposed at all: fine
    };
    let plan = only(&plan, &item.path.clone());
    let rep = run(&m, &plan);
    if rep.moved.is_empty() {
        assert!(p.exists(), "skipped, so it must still be in place");
    } else {
        let (n, errs) = undo_plan(&m.env, &rep.plan_id);
        assert_eq!((n, errs.len()), (1, 0));
        assert!(p.exists());
    }
}

#[test]
fn many_files_in_one_tree_move_as_one_item_with_exact_size() {
    let m = FakeMac::new();
    let root = m.env.home_path("Library/Caches/com.example.Huge");
    for d in 0..100 {
        for f in 0..100 {
            let p = root.join(format!("d{d}/f{f}"));
            if f == 0 {
                fs::create_dir_all(p.parent().unwrap()).unwrap();
            }
            fs::write(p, b"x").unwrap();
        }
    }
    let expect = tree_size(&root).0;
    let plan = junk_plan(&m);
    let item = plan.items.iter().find(|i| i.path == root).unwrap();
    assert_eq!(item.size, expect);
    let plan = only(&plan, &root);
    let rep = run(&m, &plan);
    assert_eq!(rep.freed, expect);
    assert_eq!(
        cleancraft_core::fsutil::tree_file_count(&stored_of(&rep.moved[0])),
        10_000
    );
}
