//! FIX-2 by clicks: "Reveal" shows only on plan rows that are files or folders on disk.
//! Privacy's Wi-Fi networks and app permissions are labels, so their rows have no Reveal;
//! file rows do, and Reveal asks Finder to show exactly that path.

mod common;

use cleancraft_core::FeatureId;
use cleancraft_core::fsutil::human_size;
use cleancraft_core::sys::CmdOut;
use cleancraft_core::testkit_cleanup::privacy_mac;
use common::*;
use egui_kittest::kittest::Queryable;

fn reveals(h: &H) -> usize {
    h.query_all_by_label("Reveal").count()
}

#[test]
fn reveal_only_for_paths_that_exist() {
    let mac = privacy_mac();
    let fs = mac.env.fake_sys().unwrap();
    fs.answer("open", "");
    let mut h = harness(&mac);
    // The tool sits in the "Protect" section of the sidebar.
    click(&mut h, "Protect");
    click(&mut h, "Privacy Traces");
    click(&mut h, "Scan");
    let plan = h.state().plans[&FeatureId::Privacy]
        .plan
        .clone()
        .expect("plan");
    let on_disk = |cat: &str| {
        plan.items
            .iter()
            .filter(|i| i.category == cat)
            .filter(|i| i.path.exists())
            .count()
    };

    click(&mut h, "Wi-Fi networks  0 KB");
    assert!(has(&h, "Home Net"), "the networks are listed");
    assert_eq!(on_disk("wifi"), 0);
    assert_eq!(reveals(&h), 0, "Wi-Fi rows are not files");

    click(&mut h, "App permissions  0 KB");
    assert!(has_text(&h, "us.zoom.xos"));
    assert_eq!(reveals(&h), 0, "permission rows are not files");

    // A category of files: every row has one Reveal, and it shows that file.
    let cat = plan
        .categories
        .iter()
        .filter(|c| {
            let n = plan.category_count(&c.id);
            n > 0 && on_disk(&c.id) == n
        })
        .min_by_key(|c| plan.category_count(&c.id))
        .expect("a category of files");
    click(
        &mut h,
        &format!("{}  {}", cat.title, human_size(plan.category_size(&cat.id))),
    );
    let files: Vec<_> = plan.items.iter().filter(|i| i.category == cat.id).collect();
    assert_eq!(reveals(&h), files.len());
    click_nth(&mut h, "Reveal", 0);
    let call = fs
        .calls()
        .into_iter()
        .find(|c| c.starts_with("/usr/bin/open -R "))
        .expect("Finder asked");
    let shown = call.trim_start_matches("/usr/bin/open -R ");
    assert!(
        files.iter().any(|i| i.path.to_string_lossy() == shown),
        "{call}"
    );

    // A failing `open` is said, not ignored.
    fs.answer_full(
        "open",
        CmdOut {
            status: 1,
            stdout: String::new(),
            stderr: "The file does not exist.".into(),
        },
    );
    click_nth(&mut h, "Reveal", 0);
    assert!(has_text(
        &h,
        "Could not open Finder: The file does not exist."
    ));
}
