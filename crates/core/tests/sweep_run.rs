//! Quick Sweep on a fake Mac: only registered modules get tiles, tile numbers equal each
//! module's plan, Run moves exactly the confirmed selection of included modules, skipped and
//! Trash tiles are never run, a failing module does not stop the others, and undo per module.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use cleancraft_core::env::Env;
use cleancraft_core::executor::undo_plan;
use cleancraft_core::features::{self, Feature, ScanCtx};
use cleancraft_core::fsutil::{children, tree_size};
use cleancraft_core::sweep::{self, CountSource, Sources, TileData};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{FeatureId, Plan, Risk, Settings};

/// A stand-in Mail module for tests, used until the real one is registered: proposes every
/// entry of `~/Library/Mail Downloads`, the ones named `keep*` unselected.
struct TestMail;

impl Feature for TestMail {
    fn id(&self) -> FeatureId {
        FeatureId::MailAttachments
    }
    fn allow_roots(&self, env: &Env) -> Vec<PathBuf> {
        vec![env.home_path("Library/Mail Downloads")]
    }
    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let mut p = Plan::new(FeatureId::MailAttachments);
        p.add_category("dl", "Mail downloads", "Copies of attachments.", Risk::Medium);
        for c in children(&ctx.env.home_path("Library/Mail Downloads")) {
            let keep = c.file_name().unwrap().to_string_lossy().starts_with("keep");
            p.push(
                c.clone(),
                tree_size(&c).0,
                "dl",
                "attachment copy",
                Risk::Medium,
                !keep,
            );
        }
        Ok(p)
    }
}

struct Broken;

impl Feature for Broken {
    fn id(&self) -> FeatureId {
        FeatureId::Malware
    }
    fn allow_roots(&self, _env: &Env) -> Vec<PathBuf> {
        Vec::new()
    }
    fn scan(&self, _ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        anyhow::bail!("rules could not be loaded")
    }
}

/// Trash stand-in: proposes what is in ~/.Trash.
struct TestTrash;

impl Feature for TestTrash {
    fn id(&self) -> FeatureId {
        FeatureId::TrashBins
    }
    fn allow_roots(&self, env: &Env) -> Vec<PathBuf> {
        vec![env.user_trash()]
    }
    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let mut p = Plan::new(FeatureId::TrashBins);
        p.add_category("t", "Trash", "In the Trash.", Risk::Low);
        for c in children(&ctx.env.user_trash()) {
            p.push(c.clone(), tree_size(&c).0, "t", "in the Trash", Risk::Low, true);
        }
        Ok(p)
    }
}

fn three_updates(_: &Env, _: &Settings) -> anyhow::Result<usize> {
    Ok(3)
}

fn sources() -> Sources {
    Sources {
        plans: vec![
            Arc::from(features::get(FeatureId::SystemJunk).unwrap()),
            Arc::new(TestTrash),
            Arc::new(TestMail),
            Arc::new(Broken),
        ],
        counts: vec![CountSource {
            id: FeatureId::Updater,
            noun: "updates",
            count: three_updates,
        }],
    }
}

fn mac() -> FakeMac {
    let m = FakeMac::sample();
    m.home_file("Library/Mail Downloads/a/report.pdf", 400_000);
    m.home_file("Library/Mail Downloads/b.zip", 250_000);
    m.home_file("Library/Mail Downloads/keep-c.doc", 90_000);
    m.home_file(".Trash/old.dmg", 800_000);
    m
}

fn scan(m: &FakeMac, s: &Sources) -> sweep::SweepResult {
    sweep::scan(
        &m.env,
        &Settings::default(),
        s,
        &AtomicBool::new(false),
        &|_| {},
    )
}

#[test]
fn registered_sources_skip_missing_modules() {
    let s = Sources::registered();
    for f in &s.plans {
        assert!(sweep::PLAN_MODULES.contains(&f.id()));
        assert!(features::get(f.id()).is_some());
    }
    // System Junk is registered in every build.
    assert!(s.feature(FeatureId::SystemJunk).is_some());
    let m = mac();
    let r = scan(&m, &s);
    assert_eq!(
        r.tiles.iter().map(|t| t.id).collect::<Vec<_>>(),
        s.ids(),
        "one tile per registered module, nothing else"
    );
}

#[test]
fn tiles_match_each_plan() {
    let m = mac();
    let r = scan(&m, &sources());
    assert_eq!(r.tiles.len(), 5);
    for t in &r.tiles {
        if let TileData::Plan(p) = &t.data {
            let fresh = match t.id {
                FeatureId::SystemJunk => {
                    features::scan(&m.env, &Settings::default(), t.id).unwrap()
                }
                FeatureId::MailAttachments => TestMail
                    .scan(&ScanCtx {
                        env: &m.env,
                        settings: &Settings::default(),
                        cancel: &AtomicBool::new(false),
                        progress: &|_| {},
                    })
                    .unwrap(),
                _ => continue,
            };
            assert_eq!(p.selected_size(), fresh.selected_size());
            let disk: u64 = p.selected().map(|i| tree_size(&i.path).0).sum();
            assert_eq!(p.selected_size(), disk);
        }
    }
    let mail = r.tile(FeatureId::MailAttachments).unwrap().plan().unwrap();
    assert_eq!(mail.selected_count(), 2);
    assert!(matches!(
        &r.tile(FeatureId::Malware).unwrap().data,
        TileData::Failed(e) if e.contains("rules could not be loaded")
    ));
    assert!(matches!(
        r.tile(FeatureId::Updater).unwrap().data,
        TileData::Count { count: 3, .. }
    ));
    assert!(!r.tile(FeatureId::TrashBins).unwrap().runnable());
    // Ready = junk selection + mail selection; Trash is not part of Run.
    let junk = r.tile(FeatureId::SystemJunk).unwrap().plan().unwrap();
    assert_eq!(
        r.ready_bytes(&[]),
        junk.selected_size() + mail.selected_size()
    );
    assert_eq!(r.ready_bytes(&[FeatureId::MailAttachments]), junk.selected_size());
}

#[test]
fn run_moves_exactly_the_confirmed_files_and_undo_per_module() {
    let m = mac();
    let s = sources();
    let r = scan(&m, &s);
    let plans: Vec<Plan> = r.to_run(&[]).into_iter().cloned().collect();
    assert_eq!(plans.len(), 2);
    let listed: Vec<PathBuf> = plans
        .iter()
        .flat_map(|p| p.selected().map(|i| i.path.clone()))
        .collect();
    let all: Vec<PathBuf> = r
        .tiles
        .iter()
        .filter_map(|t| t.plan())
        .flat_map(|p| p.items.iter().map(|i| i.path.clone()))
        .collect();
    let confirmed: Vec<(Plan, String)> =
        plans.iter().map(|p| (p.clone(), p.digest())).collect();
    let runs = sweep::run(&m.env, &Settings::default(), &s, &confirmed, &|_| {});
    assert_eq!(runs.len(), 2);
    for p in &all {
        assert_eq!(
            p.exists(),
            !listed.contains(p),
            "{} moved state wrong",
            p.display()
        );
    }
    assert!(m.env.home_path(".Trash/old.dmg").exists(), "Trash untouched");
    // Undo only Mail.
    let mail_run = runs
        .iter()
        .find(|r| r.id == FeatureId::MailAttachments)
        .unwrap();
    let rep = mail_run.result.as_ref().unwrap();
    assert_eq!(rep.moved.len(), 2);
    let (n, errs) = undo_plan(&m.env, &rep.plan_id);
    assert_eq!((n, errs.len()), (2, 0));
    assert!(m.env.home_path("Library/Mail Downloads/b.zip").exists());
    assert!(
        !m.env
            .home_path("Library/Caches/com.example.Browser")
            .exists(),
        "System Junk stays cleaned"
    );
}

#[test]
fn skipped_module_is_not_run() {
    let m = mac();
    let r = scan(&m, &sources());
    let plans = r.to_run(&[FeatureId::SystemJunk]);
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].feature, Some(FeatureId::MailAttachments));
}

#[test]
fn changed_list_and_missing_module_are_refused_others_go_on() {
    let m = mac();
    let s = sources();
    let r = scan(&m, &s);
    let mut junk = r.tile(FeatureId::SystemJunk).unwrap().plan().unwrap().clone();
    let digest = junk.digest();
    junk.set_selected(0, false); // the list changed after confirming
    let mail = r.tile(FeatureId::MailAttachments).unwrap().plan().unwrap().clone();
    let mut orphan = Plan::new(FeatureId::Duplicates);
    orphan.push(m.env.home_path("Downloads/x"), 1, "c", "r", Risk::Low, true);
    let confirmed = vec![
        (junk.clone(), digest),
        (orphan.clone(), orphan.digest()),
        (mail.clone(), mail.digest()),
    ];
    let runs = sweep::run(&m.env, &Settings::default(), &s, &confirmed, &|_| {});
    assert!(runs[0].result.as_ref().unwrap_err().contains("changed"));
    assert!(runs[1].result.is_err());
    assert_eq!(runs[2].result.as_ref().unwrap().moved.len(), 2);
    assert!(junk.items.iter().all(|i| i.path.exists()), "junk refused");
}

#[test]
fn cancel_stops_between_modules() {
    let m = mac();
    let r = sweep::scan(
        &m.env,
        &Settings::default(),
        &sources(),
        &AtomicBool::new(true),
        &|_| {},
    );
    assert!(r.tiles.is_empty());
}

#[test]
fn describe_lines() {
    let m = mac();
    let r = scan(&m, &sources());
    let lines: Vec<String> = r.tiles.iter().map(sweep::describe).collect();
    assert!(lines[1].ends_with("(cleaned from its own page)"));
    assert_eq!(lines[4], "App Updates: 3 updates");
    assert!(lines[3].starts_with("Threat Check: could not scan"));
}
