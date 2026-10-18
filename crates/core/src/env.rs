//! Where CleanCraft looks. Every path a feature scans or moves is built from an [`Env`]:
//! `env.home()` for the user's folder and `env.sys_path("/Library/Caches")` for system
//! locations. In tests the whole tree lives in a temporary folder ([`Env::fake`]), so no test
//! can reach real files: system paths map into it and the executor refuses anything outside.

use std::collections::BTreeMap;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::sys::{FakeSys, RealSys, Sys};

/// Injected failures for tests: the next operation on a path fails with this error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    PermissionDenied,
    InUse,
    DiskFull,
}

#[derive(Clone)]
pub struct Env {
    home: PathBuf,
    /// "/" for real use; the temporary folder in tests.
    root: PathBuf,
    data_dir: PathBuf,
    pub sys: Arc<dyn Sys>,
    fake: Option<Arc<FakeSys>>,
    faults: Arc<Mutex<BTreeMap<PathBuf, Fault>>>,
}

impl std::fmt::Debug for Env {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Env")
            .field("home", &self.home)
            .field("root", &self.root)
            .field("fake", &self.fake.is_some())
            .finish()
    }
}

/// Error from [`Env::real`] inside test or `cargo run` processes.
pub const FORBID_VAR: &str = "CLEANCRAFT_FORBID_REAL";

impl Env {
    /// The real Mac. Refuses to start inside cargo-launched processes (tests, examples), which
    /// have `CLEANCRAFT_FORBID_REAL=1` from `.cargo/config.toml`.
    pub fn real() -> io::Result<Env> {
        if std::env::var_os(FORBID_VAR).is_some_and(|v| v == "1") {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "CleanCraft refuses to use the real home folder in a test or cargo-run process",
            ));
        }
        let home = dirs::home_dir().ok_or_else(|| io::Error::other("no home folder"))?;
        let data_dir = home.join("Library/Application Support/CleanCraft");
        Ok(Env {
            home,
            root: PathBuf::from("/"),
            data_dir,
            sys: Arc::new(RealSys),
            fake: None,
            faults: Arc::default(),
        })
    }

    /// A fake Mac inside `dir`: home is `dir/Users/tester`, "/Library" is `dir/Library`.
    pub fn fake(dir: &Path) -> Env {
        let root = dir.to_path_buf();
        let home = root.join("Users/tester");
        let data_dir = home.join("Library/Application Support/CleanCraft");
        let fake = Arc::new(FakeSys::new());
        Env {
            home,
            root,
            data_dir,
            sys: fake.clone(),
            fake: Some(fake),
            faults: Arc::default(),
        }
    }

    pub fn is_fake(&self) -> bool {
        self.fake.is_some()
    }

    /// The scripted OS of a fake env.
    pub fn fake_sys(&self) -> Option<&FakeSys> {
        self.fake.as_deref()
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `~/rel`.
    pub fn home_path(&self, rel: &str) -> PathBuf {
        self.home.join(rel.trim_start_matches("~/").trim_start_matches('/'))
    }

    /// An absolute macOS path, mapped into the fake root in tests.
    pub fn sys_path(&self, abs: &str) -> PathBuf {
        self.root.join(abs.trim_start_matches('/'))
    }

    /// CleanCraft's own folder: journal, quarantine, settings.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn quarantine_dir(&self) -> PathBuf {
        self.data_dir.join("Quarantine")
    }

    pub fn user_trash(&self) -> PathBuf {
        self.home.join(".Trash")
    }

    /// The path as the user knows it (fake root stripped), for display and logs.
    pub fn display(&self, p: &Path) -> String {
        if self.root == Path::new("/") {
            return p.to_string_lossy().into_owned();
        }
        match p.strip_prefix(&self.root) {
            Ok(rel) => format!("/{}", rel.to_string_lossy()),
            Err(_) => p.to_string_lossy().into_owned(),
        }
    }

    /// True when `p` is inside this env's root. Always true for the real Mac; in tests this
    /// is the fence that keeps every operation inside the temporary folder.
    pub fn contains(&self, p: &Path) -> bool {
        if !p.is_absolute() || has_parent_component(p) {
            return false;
        }
        p.starts_with(&self.root)
    }

    pub fn inject_fault(&self, p: &Path, f: Fault) {
        if let Ok(mut g) = self.faults.lock() {
            g.insert(p.to_path_buf(), f);
        }
    }

    pub fn clear_faults(&self) {
        if let Ok(mut g) = self.faults.lock() {
            g.clear();
        }
    }

    /// Can CleanCraft, running as this user without administrator rights, move things out of
    /// `dir`? On Unix: the folder is owned by this user and writable by its owner, or writable
    /// by everyone. An injected `PermissionDenied` fault answers no (tests).
    pub fn can_modify(&self, dir: &Path) -> bool {
        if matches!(self.fault_for(dir), Some(Fault::PermissionDenied)) {
            return false;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let Ok(md) = std::fs::metadata(dir) else { return false };
            let me = std::fs::metadata(&self.home).map(|m| m.uid()).unwrap_or(u32::MAX);
            let mode = md.mode();
            (md.uid() == me && mode & 0o200 != 0) || mode & 0o002 != 0
        }
        #[cfg(not(unix))]
        {
            std::fs::metadata(dir).map(|m| !m.permissions().readonly()).unwrap_or(false)
        }
    }

    /// A fault injected for `p` or one of its ancestors.
    pub fn fault_for(&self, p: &Path) -> Option<Fault> {
        let g = self.faults.lock().ok()?;
        g.iter()
            .find(|(k, _)| p.starts_with(k))
            .map(|(_, f)| *f)
    }
}

pub fn has_parent_component(p: &Path) -> bool {
    p.components().any(|c| matches!(c, Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_is_refused_under_cargo() {
        // cargo test sets CLEANCRAFT_FORBID_REAL=1 through .cargo/config.toml
        assert_eq!(std::env::var(FORBID_VAR).as_deref(), Ok("1"));
        assert!(Env::real().is_err());
    }

    #[test]
    fn fake_maps_paths() {
        let d = tempfile::tempdir().unwrap();
        let e = Env::fake(d.path());
        assert_eq!(e.sys_path("/Library/Caches"), d.path().join("Library/Caches"));
        assert_eq!(e.home_path("~/Library"), d.path().join("Users/tester/Library"));
        assert_eq!(e.display(&e.sys_path("/Library")), "/Library");
        assert!(e.contains(&e.home_path("x")));
        assert!(!e.contains(Path::new("/etc/passwd")));
        assert!(!e.contains(&e.home_path("../../../../etc")));
    }
}
