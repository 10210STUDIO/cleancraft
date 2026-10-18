//! A fake Mac in a temporary folder, for tests, snapshots and demos. Dropping it deletes the
//! folder. Nothing here can reach real files: every helper writes under the temporary root.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::env::Env;

pub struct FakeMac {
    pub dir: tempfile::TempDir,
    pub env: Env,
}

impl FakeMac {
    /// An empty fake Mac with the usual home folders.
    pub fn new() -> FakeMac {
        let dir = tempfile::Builder::new()
            .prefix("cleancraft-fake-")
            .tempdir()
            .unwrap_or_else(|e| panic!("cannot create temp dir: {e}"));
        let env = Env::fake(dir.path());
        for d in [
            "Documents", "Desktop", "Downloads", "Pictures", "Movies", "Music", "Library/Caches",
            "Library/Logs", "Library/Preferences", "Library/Application Support", ".Trash",
        ] {
            let _ = fs::create_dir_all(env.home_path(d));
        }
        for d in ["/Applications", "/Library/Caches", "/Library/Logs", "/System/Library"] {
            let _ = fs::create_dir_all(env.sys_path(d));
        }
        FakeMac { dir, env }
    }

    /// Fake Mac with a little of everything (used by snapshots and UI tests).
    pub fn sample() -> FakeMac {
        let m = FakeMac::new();
        m.home_file("Library/Caches/com.example.Browser/Cache.db", 3_200_000);
        m.home_file("Library/Caches/com.example.Browser/img/1.bin", 900_000);
        m.home_file("Library/Caches/com.example.Editor/index", 1_400_000);
        m.home_file("Library/Caches/com.apple.bird/session", 50_000);
        m.home_file("Library/Logs/Example/app.log", 420_000);
        m.home_file("Library/Logs/DiagnosticReports/Example-2026.ips", 60_000);
        m.sys_file("/Library/Caches/com.example.updater/pkg", 2_000_000);
        m.sys_file("/Library/Logs/Install.log", 300_000);
        m.home_file("Library/Developer/Xcode/DerivedData/Demo-abc/Build/x.o", 5_000_000);
        m.home_file("Documents/report.pdf", 700_000);
        m.home_file("Desktop/notes.txt", 2_000);
        m
    }

    pub fn home(&self) -> PathBuf {
        self.env.home().to_path_buf()
    }

    /// A file of `size` bytes under the home folder. Returns its path.
    pub fn home_file(&self, rel: &str, size: usize) -> PathBuf {
        let p = self.env.home_path(rel);
        write_sized(&p, size);
        p
    }

    /// A file under an absolute macOS path (mapped into the fake root).
    pub fn sys_file(&self, abs: &str, size: usize) -> PathBuf {
        let p = self.env.sys_path(abs);
        write_sized(&p, size);
        p
    }

    pub fn write(&self, p: &Path, contents: &[u8]) {
        if let Some(d) = p.parent() {
            let _ = fs::create_dir_all(d);
        }
        let _ = fs::write(p, contents);
    }

    /// A minimal app bundle: `/Applications/<name>.app` with Info.plist and an executable.
    pub fn app(&self, name: &str, bundle_id: &str, version: &str) -> PathBuf {
        let app = self.env.sys_path(&format!("/Applications/{name}.app"));
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>{bundle_id}</string>
<key>CFBundleName</key><string>{name}</string>
<key>CFBundleExecutable</key><string>{name}</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
<key>CFBundleVersion</key><string>{version}</string>
</dict></plist>"#
        );
        self.write(&app.join("Contents/Info.plist"), plist.as_bytes());
        write_sized(&app.join(format!("Contents/MacOS/{name}")), 1_000_000);
        app
    }
}

impl Default for FakeMac {
    fn default() -> Self {
        Self::new()
    }
}

fn write_sized(p: &Path, size: usize) {
    if let Some(d) = p.parent() {
        let _ = fs::create_dir_all(d);
    }
    // Non-zero, varied bytes so files are really allocated and not all identical.
    let seed = p.to_string_lossy().len() as u8;
    let data: Vec<u8> = (0..size).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect();
    let _ = fs::write(p, data);
}

/// Set a file's modification and access time to `days` ago.
pub fn age(p: &Path, days: u64) {
    let t = SystemTime::now() - Duration::from_secs(days * 86_400);
    if let Ok(f) = fs::File::options().write(true).open(p) {
        let _ = f.set_times(fs::FileTimes::new().set_accessed(t).set_modified(t));
    }
}
