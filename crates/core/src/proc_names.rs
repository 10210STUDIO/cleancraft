//! Plain words for process names in Live Monitor's top lists: what common macOS processes do
//! ("WindowServer" draws the screen), which app a helper belongs to ("Brave Browser Helper
//! (Renderer)" is a tab or page in Brave Browser), and the processes grouped by app. Our own
//! wording. Processes that are part of macOS are marked so the UI never offers to quit them.

use std::collections::BTreeMap;

use crate::monitor::AppUse;

/// What a process is, in a few words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Explained {
    pub what: String,
    /// Part of macOS: never offered for quitting.
    pub macos: bool,
    /// The app it belongs to, for an app's helper.
    pub app: Option<String>,
}

/// (names, explanation). All part of macOS.
const MACOS: &[(&[&str], &str)] = &[
    (
        &["WindowServer"],
        "Part of macOS: draws everything on your screen. It works harder when apps animate or redraw a lot.",
    ),
    (
        &["kernel_task"],
        "Part of macOS: the core of the system. It also holds the CPU on purpose to cool the Mac down.",
    ),
    (
        &[
            "mds",
            "mds_stores",
            "mdworker",
            "mdworker_shared",
            "mdsync",
            "corespotlightd",
            "Spotlight",
        ],
        "Part of macOS: Spotlight indexing your files so search can find them.",
    ),
    (
        &["backupd", "backupd-helper"],
        "Part of macOS: Time Machine making a backup.",
    ),
    (
        &["photoanalysisd", "photolibraryd", "mediaanalysisd"],
        "Part of macOS: Photos looking through your library for faces, objects and text.",
    ),
    (
        &["cloudd", "bird", "fileproviderd"],
        "Part of macOS: iCloud keeping your files in sync.",
    ),
    (
        &["softwareupdated", "softwareupdate_notify_agent"],
        "Part of macOS: checking for or downloading macOS updates.",
    ),
    (
        &["coreaudiod"],
        "Part of macOS: plays and records all sound.",
    ),
    (
        &["launchd"],
        "Part of macOS: starts every other process and restarts them when needed.",
    ),
    (&["logd"], "Part of macOS: writes the system log."),
    (
        &["syspolicyd"],
        "Part of macOS: Gatekeeper checking apps before they open.",
    ),
    (
        &["trustd"],
        "Part of macOS: checks security certificates for websites and apps.",
    ),
    (
        &["nsurlsessiond"],
        "Part of macOS: downloads in the background for apps and the system.",
    ),
    (
        &["sharingd"],
        "Part of macOS: AirDrop, Handoff and sharing between your devices.",
    ),
    (&["bluetoothd"], "Part of macOS: Bluetooth connections."),
    (&["airportd", "wifid"], "Part of macOS: Wi-Fi."),
    (
        &["hidd"],
        "Part of macOS: keyboard, mouse and trackpad input.",
    ),
    (
        &["powerd"],
        "Part of macOS: sleep, wake and power settings.",
    ),
    (&["loginwindow"], "Part of macOS: your login session."),
    (
        &["Dock"],
        "Part of macOS: the Dock, Mission Control and Launchpad.",
    ),
    (
        &["Finder"],
        "Part of macOS: Finder, your files and the desktop.",
    ),
    (
        &["SystemUIServer", "ControlCenter"],
        "Part of macOS: the menu bar items and Control Center.",
    ),
    (
        &["com.apple.WebKit.WebContent", "Safari Web Content"],
        "Part of macOS: a web page shown by Safari or another app.",
    ),
    (
        &["com.apple.WebKit.GPU", "com.apple.WebKit.Networking"],
        "Part of macOS: graphics or network work for web pages.",
    ),
    (
        &[
            "distnoted",
            "cfprefsd",
            "UserEventAgent",
            "runningboardd",
            "opendirectoryd",
            "configd",
            "notifyd",
        ],
        "Part of macOS: a system service other programs rely on.",
    ),
];

/// "<App> Helper (Renderer)" -> ("<App>", "Renderer"); "<App> Helper" -> ("<App>", "").
pub fn split_helper(name: &str) -> Option<(String, String)> {
    let (base, kind) = match name.rsplit_once(" (") {
        Some((b, k)) if k.ends_with(')') => (b, k.trim_end_matches(')')),
        _ => (name, ""),
    };
    let app = base.strip_suffix(" Helper")?.trim();
    (!app.is_empty()).then(|| (app.to_string(), kind.to_string()))
}

/// What `name` is, when CleanCraft knows.
pub fn explain(name: &str) -> Option<Explained> {
    let n = name.trim();
    if let Some((_, what)) = MACOS.iter().find(|(names, _)| names.contains(&n)) {
        return Some(Explained {
            what: (*what).to_string(),
            macos: true,
            app: None,
        });
    }
    let (app, kind) = split_helper(n)?;
    let what = match kind.as_str() {
        "Renderer" => format!("A tab or page in {app}."),
        "GPU" => format!("Graphics work for {app}."),
        "Plugin" => format!("A plug-in running inside {app}."),
        "" => format!("A helper process of {app}."),
        other => format!("A helper process of {app} ({other})."),
    };
    Some(Explained {
        what,
        macos: false,
        app: Some(app),
    })
}

/// What a process of the list is: [`explain`] for a known name; otherwise, from its path, the
/// app whose bundle holds it ("Helper of Docker."), CleanCraft itself, a reverse-DNS name
/// ("com.docker.backend": "Background process, by its name from Docker."), or "Background
/// process." for any other path. `None` only for an unknown bare name (no path to go by).
pub fn explain_proc(p: &AppUse) -> Option<Explained> {
    if let Some(e) = explain(&p.name) {
        return Some(e);
    }
    let plain = |what: String, app: Option<String>| {
        Some(Explained {
            what,
            macos: false,
            app,
        })
    };
    if p.name.eq_ignore_ascii_case("cleancraft") {
        return plain("CleanCraft (this app).".into(), None);
    }
    if let Some(path) = &p.path {
        if let Some(app) = app_of_path(path) {
            if app == p.name {
                return plain(format!("The {app} app."), None);
            }
            return plain(format!("Helper of {app}."), Some(app));
        }
        if path.starts_with("/System/") || path.starts_with("/usr/") || path.starts_with("/sbin/") {
            return Some(Explained {
                what: "Part of macOS: a background process.".into(),
                macos: true,
                app: None,
            });
        }
        return plain("Background process.".into(), None);
    }
    let parts: Vec<&str> = p.name.split('.').collect();
    if parts.len() >= 3
        && ["com", "org", "net", "io", "app", "dev"].contains(&parts[0])
        && !parts[1].is_empty()
        && parts[1] != "apple"
    {
        let mut v = parts[1].chars();
        let vendor = v
            .next()
            .map(|c| c.to_uppercase().collect::<String>() + v.as_str())
            .unwrap_or_default();
        return plain(
            format!("Background process, by its name from {vendor}."),
            None,
        );
    }
    None
}

/// The app whose bundle holds an executable: "/Applications/Docker.app/Contents/MacOS/x"
/// gives "Docker" (the outermost app bundle).
pub fn app_of_path(path: &str) -> Option<String> {
    let i = path.find(".app/")?;
    let start = path[..i].rfind('/').map(|s| s + 1).unwrap_or(0);
    let name = &path[start..i];
    (!name.is_empty()).then(|| name.to_string())
}

/// Whether `name` is part of macOS by CleanCraft's list.
pub fn is_macos(name: &str) -> bool {
    explain(name).is_some_and(|e| e.macos)
}

/// The processes of one app (or one process on its own).
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// The app's name ("Brave Browser") or the process name.
    pub name: String,
    pub processes: usize,
    /// Percent of one core, summed.
    pub cpu: f64,
    pub memory: u64,
    pub macos: bool,
    /// What it is, for a known macOS process or a single helper.
    pub what: Option<String>,
}

/// Group processes by app: helpers join the app they belong to ("Brave Browser: 3
/// processes").
pub fn group(procs: &[AppUse]) -> Vec<Group> {
    let mut by: BTreeMap<String, Group> = BTreeMap::new();
    for p in procs {
        let ex = explain_proc(p);
        let key = ex
            .as_ref()
            .and_then(|e| e.app.clone())
            .unwrap_or_else(|| p.name.clone());
        let g = by.entry(key.clone()).or_insert_with(|| Group {
            name: key,
            processes: 0,
            cpu: 0.0,
            memory: 0,
            macos: false,
            what: None,
        });
        g.processes += 1;
        g.cpu += p.cpu;
        g.memory += p.memory;
        match ex {
            Some(e) if e.macos => {
                g.macos = true;
                g.what = Some(e.what);
            }
            Some(e) if g.what.is_none() => g.what = Some(e.what),
            _ => {}
        }
    }
    let mut v: Vec<Group> = by.into_values().collect();
    for g in &mut v {
        if !g.macos && g.processes > 1 {
            g.what = Some(format!("{} and its helper processes.", g.name));
        }
    }
    v
}

/// Groups by CPU, biggest first, `n` of them.
pub fn top_groups_by_cpu(procs: &[AppUse], n: usize) -> Vec<Group> {
    let mut v = group(procs);
    v.sort_by(|a, b| b.cpu.total_cmp(&a.cpu).then_with(|| a.name.cmp(&b.name)));
    v.truncate(n);
    v
}

/// Groups by memory, biggest first, `n` of them.
pub fn top_groups_by_memory(procs: &[AppUse], n: usize) -> Vec<Group> {
    let mut v = group(procs);
    v.sort_by(|a, b| b.memory.cmp(&a.memory).then_with(|| a.name.cmp(&b.name)));
    v.truncate(n);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str, cpu: f64, mem: u64) -> AppUse {
        AppUse {
            pid: 1,
            name: name.into(),
            cpu,
            memory: mem,
            path: None,
        }
    }

    #[test]
    fn unknown_processes_get_words() {
        let mut d = p("com.docker.backend", 38.0, 1);
        assert_eq!(
            explain_proc(&d).unwrap().what,
            "Background process, by its name from Docker."
        );
        d.path = Some("/Applications/Docker.app/Contents/MacOS/com.docker.backend".into());
        let e = explain_proc(&d).unwrap();
        assert_eq!(
            (e.what.as_str(), e.app.as_deref()),
            ("Helper of Docker.", Some("Docker"))
        );
        assert_eq!(
            explain_proc(&p("CleanCraft", 1.0, 1)).unwrap().what,
            "CleanCraft (this app)."
        );
        assert_eq!(explain_proc(&p("thing", 1.0, 1)), None);
        let mut other = p("thing", 1.0, 1);
        other.path = Some("/opt/tool/bin/thing".into());
        assert_eq!(explain_proc(&other).unwrap().what, "Background process.");
        let mut sys = p("cfprefsd", 1.0, 1);
        sys.path = Some("/usr/sbin/cfprefsd".into());
        assert!(explain_proc(&sys).unwrap().macos);
    }

    #[test]
    fn known_macos_processes() {
        for n in [
            "WindowServer",
            "kernel_task",
            "mds_stores",
            "mdworker_shared",
            "backupd",
            "photoanalysisd",
            "cloudd",
            "bird",
            "softwareupdated",
            "coreaudiod",
            "launchd",
        ] {
            let e = explain(n).unwrap_or_else(|| panic!("{n}"));
            assert!(e.macos, "{n}");
            assert!(e.what.starts_with("Part of macOS"), "{n}");
            assert!(!e.what.contains('\u{2014}'));
        }
        assert!(
            explain("WindowServer")
                .unwrap()
                .what
                .contains("draws everything on your screen")
        );
        assert!(explain("mds").unwrap().what.contains("Spotlight"));
    }

    #[test]
    fn helpers_name_their_app() {
        let e = explain("Brave Browser Helper (Renderer)").unwrap();
        assert_eq!(e.what, "A tab or page in Brave Browser.");
        assert_eq!(e.app.as_deref(), Some("Brave Browser"));
        assert!(!e.macos);
        assert_eq!(
            explain("Figma Helper").unwrap().what,
            "A helper process of Figma."
        );
        assert_eq!(
            explain("Google Chrome Helper (GPU)").unwrap().what,
            "Graphics work for Google Chrome."
        );
        assert_eq!(explain("Xcode"), None);
        assert_eq!(explain(" Helper"), None);
    }

    #[test]
    fn grouping_sums_an_app_and_its_helpers() {
        let procs = vec![
            p("Brave Browser", 5.0, 100),
            p("Brave Browser Helper (Renderer)", 71.0, 300),
            p("Brave Browser Helper (GPU)", 9.0, 50),
            p("WindowServer", 47.0, 200),
            p("Figma Helper (Renderer)", 28.0, 10),
        ];
        let g = top_groups_by_cpu(&procs, 3);
        assert_eq!(g[0].name, "Brave Browser");
        assert_eq!(g[0].processes, 3);
        assert!((g[0].cpu - 85.0).abs() < 1e-9);
        assert_eq!(g[0].memory, 450);
        assert!(!g[0].macos);
        assert_eq!(g[1].name, "WindowServer");
        assert!(g[1].macos);
        assert_eq!(g[2].name, "Figma");
        assert_eq!(top_groups_by_memory(&procs, 1)[0].name, "Brave Browser");
    }
}
