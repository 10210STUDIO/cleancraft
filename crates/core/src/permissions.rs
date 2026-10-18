//! Full Disk Access. macOS has no API to ask whether an app has it; the documented approach is
//! to try reading a protected location. CleanCraft never needs it to start: without it, scans
//! skip the protected places and say so.

use std::io::ErrorKind;
use std::path::PathBuf;

use crate::env::{Env, Fault};

/// The System Settings pane where the user grants Full Disk Access.
pub const FDA_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Granted,
    Missing,
    /// No protected place exists to test (fresh account): treat as granted for scans.
    Unknown,
}

/// Places only Full Disk Access can read.
pub fn probes(env: &Env) -> Vec<PathBuf> {
    vec![
        env.home_path("Library/Application Support/com.apple.TCC/TCC.db"),
        env.home_path("Library/Safari/Bookmarks.plist"),
        env.home_path("Library/Mail"),
    ]
}

pub fn full_disk_access(env: &Env) -> Access {
    let mut saw_any = false;
    for p in probes(env) {
        if matches!(env.fault_for(&p), Some(Fault::PermissionDenied)) {
            return Access::Missing;
        }
        let r = if p.is_dir() {
            std::fs::read_dir(&p).map(|_| ())
        } else {
            std::fs::File::open(&p).map(|_| ())
        };
        match r {
            Ok(()) => return Access::Granted,
            Err(e) if e.kind() == ErrorKind::PermissionDenied => return Access::Missing,
            Err(e) if e.kind() == ErrorKind::NotFound => {}
            Err(_) => saw_any = true,
        }
    }
    if saw_any {
        Access::Missing
    } else {
        Access::Unknown
    }
}

pub fn open_fda_settings(env: &Env) -> std::io::Result<()> {
    env.sys.open_url(FDA_SETTINGS_URL)
}
