//! Final checker's tests: safety and text rules verified independently of the builders' tests.
//! Everything runs on a FakeMac (temporary folder); the source scans only read files of this
//! repository.

use std::fs;
use std::path::{Path, PathBuf};

use cleancraft_core::features::{self, allow_roots};
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::testkit_cleanup::junk_mac;
use cleancraft_core::{FeatureId, Risk, Settings, SkipReason};

/// A cache folder that is a symbolic link to Documents: nothing inside it may be proposed or
/// moved, even from a forged plan.
#[test]
fn check_symlinked_allow_root_is_never_followed() {
    let m = FakeMac::new();
    let report = m.home_file("Documents/report.pdf", 50_000);
    let caches = m.env.home_path("Library/Caches");
    // Put the real folder aside (nothing deleted) and link Caches to Documents.
    fs::rename(&caches, m.env.home_path("Library/Caches.aside")).unwrap();
    std::os::unix::fs::symlink(m.env.home_path("Documents"), &caches).unwrap();

    let plan = features::scan(&m.env, &Settings::default(), FeatureId::SystemJunk).unwrap();
    assert!(
        !plan.items.iter().any(|i| i.path.starts_with(&caches)),
        "items proposed through the link: {:?}",
        plan.items.iter().map(|i| &i.path).collect::<Vec<_>>()
    );

    let mut forged = plan.clone();
    forged.push(
        caches.join("report.pdf"),
        50_000,
        "user_caches",
        "forged",
        Risk::Low,
        true,
    );
    let allow = allow_roots(&m.env, FeatureId::SystemJunk);
    let rep = cleancraft_core::executor::execute(
        &m.env,
        &forged,
        &allow,
        &forged.digest(),
        cleancraft_core::Method::Trash,
        &mut |_, _| {},
    )
    .unwrap();
    assert!(report.exists(), "Documents/report.pdf moved through a link");
    assert!(
        rep.skipped
            .iter()
            .any(|(_, why)| matches!(why, SkipReason::Refused(_))),
        "{:?}",
        rep.skipped
    );
}

fn running_as_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    // A new file belongs to the user who made it.
    tempfile::tempfile()
        .and_then(|f| f.metadata())
        .is_ok_and(|md| md.uid() == 0)
}

/// What a real Mac looks like for a user who is not root. `/private/var/log` and
/// `/Library/Updates` are root:wheel 755: every item there is locked (J4, J15). `/Library/Logs`
/// and `/Library/Caches` are root:admin and group writable: an administrator account (member of
/// the folder's group, from `id -G`) can move entries there without the password; a standard
/// account cannot. The builders' UI tests run as root, where everything is changeable.
#[test]
fn check_system_folders_owned_by_root_lock_j4_on_a_real_mac() {
    if !running_as_root() {
        // Only root can give the fake folders another owner; on a Mac the folders already are.
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let set = |m: &FakeMac, abs: &str, gid: u32, mode: u32| {
        let p = m.env.sys_path(abs);
        std::os::unix::fs::chown(&p, Some(4242), Some(gid)).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(mode)).unwrap();
    };
    for admin in [true, false] {
        let m = junk_mac();
        set(&m, "/private/var/log", 4243, 0o755);
        set(&m, "/Library/Updates", 4243, 0o755);
        set(&m, "/Library/Logs", 80, 0o775);
        set(&m, "/Library/Caches", 80, 0o1775);
        m.env
            .fake_sys()
            .unwrap()
            .answer("id -G", if admin { "20 12 61 80" } else { "20 12 61" });
        let plan = features::scan(&m.env, &Settings::default(), FeatureId::SystemJunk).unwrap();
        let var_log = m.env.sys_path("/private/var/log");
        let updates = m.env.sys_path("/Library/Updates");
        for root_only in [&var_log, &updates] {
            let items: Vec<_> = plan
                .items
                .iter()
                .filter(|i| i.path.starts_with(root_only))
                .collect();
            assert!(
                items
                    .iter()
                    .all(|i| !i.selected && (i.locked.is_some() || i.needs_admin)),
                "root-only {} items selectable (admin={admin}): {items:?}",
                root_only.display()
            );
        }
        let lib = [
            m.env.sys_path("/Library/Logs"),
            m.env.sys_path("/Library/Caches"),
        ];
        let group_items: Vec<_> = plan
            .items
            .iter()
            .filter(|i| lib.iter().any(|l| i.path.starts_with(l)))
            .collect();
        assert!(!group_items.is_empty());
        if admin {
            assert!(
                group_items
                    .iter()
                    .all(|i| i.locked.is_none() && !i.needs_admin),
                "admin cannot clean group-writable folders: {group_items:?}"
            );
        } else {
            assert!(
                group_items
                    .iter()
                    .all(|i| !i.selected && (i.locked.is_some() || i.needs_admin)),
                "standard user can clean root:admin folders: {group_items:?}"
            );
        }
    }
}

/// Every `.rs` file under `crates/*/src` and `apps/*/src`, with the text before its
/// `#[cfg(test)]` module (test modules sit at the end of each file).
fn app_sources() -> Vec<(PathBuf, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for top in ["crates", "apps"] {
        for e in fs::read_dir(root.join(top)).unwrap().flatten() {
            stack.push(e.path().join("src"));
        }
    }
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                // Test helpers that only build fake Macs.
                if name.starts_with("testkit") {
                    continue;
                }
                let text = fs::read_to_string(&p).unwrap();
                let app = text.split("#[cfg(test)]").next().unwrap().to_string();
                out.push((p, app));
            }
        }
    }
    assert!(out.len() > 30, "sources not found: {}", out.len());
    out
}

/// Code lines only (no comments).
fn code_lines(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| !l.trim_start().starts_with("//"))
}

#[test]
fn check_app_code_never_deletes_permanently() {
    let banned = [
        "remove_file",
        "remove_dir",
        "unlink(",
        "\"rm\"",
        "\"/bin/rm\"",
        "rm -",
        "srm ",
    ];
    let mut bad = Vec::new();
    for (p, text) in app_sources() {
        for (n, l) in code_lines(&text) {
            for b in banned {
                if l.contains(b) {
                    bad.push(format!("{}:{}: {}", p.display(), n + 1, l.trim()));
                }
            }
        }
    }
    assert!(
        bad.is_empty(),
        "permanent delete in app code:\n{}",
        bad.join("\n")
    );
}

/// The string literals of one line (naive: enough for this code base, which has no
/// multi-line raw strings with quotes inside in UI code).
fn literals(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut inside = false;
    let mut esc = false;
    for c in line.chars() {
        if inside {
            if esc {
                esc = false;
                cur.push(c);
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                out.push(std::mem::take(&mut cur));
                inside = false;
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            inside = true;
        }
    }
    out
}

#[test]
fn check_texts_follow_the_rules_and_name_no_competitor() {
    let mut bad = Vec::new();
    for (p, text) in app_sources() {
        for (n, l) in code_lines(&text) {
            if l.contains('\u{2014}') {
                bad.push(format!("{}:{}: em dash", p.display(), n + 1));
            }
            for s in literals(l) {
                let low = s.to_lowercase();
                if low
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|w| w == concat!("n", "or"))
                {
                    bad.push(format!("{}:{}: the banned conjunction in {s:?}", p.display(), n + 1));
                }
                for name in [
                    "the reference app",
                    "example",
                    "smart scan",
                    "space lens",
                    "smart care",
                ] {
                    if low.contains(name) {
                        bad.push(format!("{}:{}: {name} in {s:?}", p.display(), n + 1));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
