//! Test helpers for apps (A1-A11): app bundles of every kind, leftovers, Spotlight dates,
//! running apps and scripted web answers, all on a [`FakeMac`].

use std::path::{Path, PathBuf};

use crate::sys::{CmdOut, RunningApp};
use crate::testkit::FakeMac;

/// An app bundle to create.
#[derive(Clone, Debug)]
pub struct AppSpec<'a> {
    pub name: &'a str,
    pub bundle_id: &'a str,
    pub version: &'a str,
    pub build: &'a str,
    /// Folder: "/Applications", "/Applications/Setapp", "~/Applications", "/System/Applications".
    pub dir: &'a str,
    /// App Store receipt.
    pub mas: bool,
    pub feed: Option<&'a str>,
    /// Size of the main executable.
    pub size: usize,
    /// Write Info.plist in the binary format.
    pub binary_plist: bool,
}

impl<'a> AppSpec<'a> {
    pub fn new(name: &'a str, bundle_id: &'a str, version: &'a str) -> AppSpec<'a> {
        AppSpec {
            name,
            bundle_id,
            version,
            build: version,
            dir: "/Applications",
            mas: false,
            feed: None,
            size: 1_000_000,
            binary_plist: false,
        }
    }
}

pub trait FakeMacApps {
    /// Create an app bundle from a spec. Returns its path.
    fn app_spec(&self, spec: &AppSpec<'_>) -> PathBuf;
    /// Make Spotlight say the app was last opened `days` ago.
    fn last_opened(&self, app: &Path, days: i64);
    /// Answer `curl -fsSL --max-time 15 <url>` with `body`.
    fn web(&self, url: &str, body: &str);
    /// Make `curl` for `url` fail with this exit code.
    fn web_fails(&self, url: &str, code: i32);
    /// Pretend the app is running.
    fn run_app(&self, app: &Path);
    /// Answer Spotlight's search for other copies of apps: nothing found.
    fn spotlight_finds_nothing(&self);
}

impl FakeMacApps for FakeMac {
    fn app_spec(&self, s: &AppSpec<'_>) -> PathBuf {
        let dir = match s.dir.strip_prefix("~/") {
            Some(rel) => self.env.home_path(rel),
            None => self.env.sys_path(s.dir),
        };
        let app = dir.join(format!("{}.app", s.name));
        let mut d = plist::Dictionary::new();
        d.insert("CFBundleIdentifier".into(), s.bundle_id.into());
        d.insert("CFBundleName".into(), s.name.into());
        d.insert("CFBundleExecutable".into(), s.name.into());
        d.insert("CFBundleShortVersionString".into(), s.version.into());
        d.insert("CFBundleVersion".into(), s.build.into());
        if let Some(f) = s.feed {
            d.insert("SUFeedURL".into(), f.into());
        }
        let _ = std::fs::create_dir_all(app.join("Contents"));
        let v = plist::Value::Dictionary(d);
        let p = app.join("Contents/Info.plist");
        let _ = if s.binary_plist {
            v.to_file_binary(&p)
        } else {
            v.to_file_xml(&p)
        };
        let exe = app.join(format!("Contents/MacOS/{}", s.name));
        self.write(&exe, &vec![7u8; s.size]);
        if s.mas {
            self.write(&app.join("Contents/_MASReceipt/receipt"), b"receipt");
        }
        app
    }

    fn last_opened(&self, app: &Path, days: i64) {
        let t = chrono::Utc::now() - chrono::Duration::days(days);
        let line = format!(
            "/usr/bin/mdls -name kMDItemLastUsedDate -raw {}",
            self.env.display(app)
        );
        if let Some(f) = self.env.fake_sys() {
            f.answer(&line, &t.format("%Y-%m-%d %H:%M:%S +0000").to_string());
        }
    }

    fn web(&self, url: &str, body: &str) {
        if let Some(f) = self.env.fake_sys() {
            f.answer(&format!("/usr/bin/curl -fsSL --max-time 15 {url}"), body);
        }
    }

    fn web_fails(&self, url: &str, code: i32) {
        if let Some(f) = self.env.fake_sys() {
            f.answer_full(
                &format!("/usr/bin/curl -fsSL --max-time 15 {url}"),
                CmdOut {
                    status: code,
                    stdout: String::new(),
                    stderr: format!("curl: ({code}) failed"),
                },
            );
        }
    }

    fn run_app(&self, app: &Path) {
        let shown = PathBuf::from(self.env.display(app));
        let name = app
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some(f) = self.env.fake_sys() {
            f.with(|s| {
                s.running.push(RunningApp {
                    pid: 4242,
                    name: name.clone(),
                    bundle_id: None,
                    path: Some(shown.clone()),
                })
            });
        }
    }

    fn spotlight_finds_nothing(&self) {
        if let Some(f) = self.env.fake_sys() {
            f.answer("/usr/bin/mdfind", "");
        }
    }
}

/// A Sparkle appcast with one item per (build, short version, notes html).
pub fn appcast(items: &[(&str, &str, &str)]) -> String {
    let mut s = String::from(
        r#"<?xml version="1.0" encoding="utf-8"?><rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle"><channel>"#,
    );
    for (build, short, notes) in items {
        s.push_str(&format!(
            r#"<item><title>Version {short}</title><description><![CDATA[{notes}]]></description><enclosure url="https://example.com/dl/{short}.zip" sparkle:version="{build}" sparkle:shortVersionString="{short}" length="1" type="application/octet-stream"/></item>"#
        ));
    }
    s.push_str("</channel></rss>");
    s
}
