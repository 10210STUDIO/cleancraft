//! Everything CleanCraft asks of the operating system beyond plain file access goes through
//! [`Sys`]: running system tools (`osascript`, `lsof`, `codesign`, `launchctl`...), the list of
//! open files and running apps, free disk space, opening URLs.
//!
//! [`RealSys`] runs the real tools. [`FakeSys`] answers from data the test set up and records
//! every command, so tests can check what would have run without running anything.

use std::collections::{BTreeMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CmdOut {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CmdOut {
    pub fn ok(&self) -> bool {
        self.status == 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningApp {
    pub pid: u32,
    pub name: String,
    pub bundle_id: Option<String>,
    pub path: Option<PathBuf>,
}

pub trait Sys: Send + Sync {
    /// Run a program and wait for it.
    fn run(&self, program: &str, args: &[&str]) -> io::Result<CmdOut>;
    /// Run a shell command as administrator. On macOS this is
    /// `osascript -e 'do shell script "..." with administrator privileges'`, which shows the
    /// system password dialog. The user can cancel it.
    fn run_admin(&self, shell_command: &str) -> io::Result<CmdOut>;
    /// Every file currently open by any process this user can see.
    fn open_files(&self) -> io::Result<HashSet<PathBuf>>;
    /// Apps currently running.
    fn running_apps(&self) -> Vec<RunningApp>;
    /// Free bytes on the volume holding `path`.
    fn free_space(&self, path: &Path) -> io::Result<u64>;
    /// Open a URL or file with the default handler (System Settings panes, Finder, web).
    fn open_url(&self, url: &str) -> io::Result<()>;
    /// True on macOS.
    fn is_macos(&self) -> bool;
}

/// The real operating system.
#[derive(Debug, Default)]
pub struct RealSys;

impl Sys for RealSys {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<CmdOut> {
        let out = Command::new(program).args(args).output()?;
        Ok(CmdOut {
            status: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    fn run_admin(&self, shell_command: &str) -> io::Result<CmdOut> {
        if !self.is_macos() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "administrator commands need macOS",
            ));
        }
        let escaped = shell_command.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("do shell script \"{escaped}\" with administrator privileges");
        self.run("/usr/bin/osascript", &["-e", &script])
    }

    fn open_files(&self) -> io::Result<HashSet<PathBuf>> {
        // -F n: one "n<path>" line per open file; -w: no warnings. lsof exits 1 when some
        // processes cannot be inspected, which is normal without root, so the status is ignored.
        let out = self
            .run("/usr/sbin/lsof", &["-F", "n", "-w"])
            .or_else(|_| self.run("lsof", &["-F", "n", "-w"]))?;
        Ok(parse_lsof(&out.stdout))
    }

    fn running_apps(&self) -> Vec<RunningApp> {
        let Ok(out) = self.run("/bin/ps", &["-axo", "pid=,comm="]) else {
            return Vec::new();
        };
        parse_ps(&out.stdout)
    }

    fn free_space(&self, path: &Path) -> io::Result<u64> {
        let p = path.to_string_lossy();
        let out = self.run("/bin/df", &["-Pk", &p])?;
        parse_df_free(&out.stdout)
            .ok_or_else(|| io::Error::other(format!("could not read free space: {}", out.stderr)))
    }

    fn open_url(&self, url: &str) -> io::Result<()> {
        let prog = if self.is_macos() {
            "/usr/bin/open"
        } else {
            "xdg-open"
        };
        let out = self.run(prog, &[url])?;
        if out.ok() {
            Ok(())
        } else {
            Err(io::Error::other(out.stderr))
        }
    }

    fn is_macos(&self) -> bool {
        cfg!(target_os = "macos")
    }
}

pub fn parse_lsof(stdout: &str) -> HashSet<PathBuf> {
    stdout
        .lines()
        .filter_map(|l| l.strip_prefix('n'))
        .filter(|p| p.starts_with('/'))
        .map(|p| PathBuf::from(p.split(" (").next().unwrap_or(p)))
        .collect()
}

/// `ps -axo pid=,comm=` lines: "  123 /Applications/Safari.app/Contents/MacOS/Safari".
pub fn parse_ps(stdout: &str) -> Vec<RunningApp> {
    stdout
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let (pid, comm) = l.split_once(char::is_whitespace)?;
            let pid: u32 = pid.parse().ok()?;
            let comm = comm.trim();
            let path = PathBuf::from(comm);
            let app_path = app_bundle_of(&path);
            let name = app_path
                .as_ref()
                .and_then(|p| p.file_stem())
                .or_else(|| path.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            Some(RunningApp {
                pid,
                name,
                bundle_id: None,
                path: app_path.or(Some(path)),
            })
        })
        .collect()
}

/// `/Applications/X.app/Contents/MacOS/X` -> `/Applications/X.app`.
pub fn app_bundle_of(path: &Path) -> Option<PathBuf> {
    let mut cur = path;
    while let Some(parent) = cur.parent() {
        if cur.extension().is_some_and(|e| e == "app") {
            return Some(cur.to_path_buf());
        }
        cur = parent;
    }
    None
}

/// `df -Pk` second line, fourth column is free 1024-byte blocks.
pub fn parse_df_free(stdout: &str) -> Option<u64> {
    let line = stdout.lines().nth(1)?;
    let avail: u64 = line.split_whitespace().nth(3)?.parse().ok()?;
    Some(avail * 1024)
}

/// A scripted operating system for tests. Nothing runs; every call is recorded.
#[derive(Debug, Default)]
pub struct FakeSys {
    pub state: Mutex<FakeState>,
}

#[derive(Debug, Default)]
pub struct FakeState {
    /// Every command asked for, as "program arg arg".
    pub calls: Vec<String>,
    /// Every administrator command asked for.
    pub admin_calls: Vec<String>,
    /// Answers by exact "program arg arg" or by program alone.
    pub answers: BTreeMap<String, CmdOut>,
    pub open_files: HashSet<PathBuf>,
    pub running: Vec<RunningApp>,
    pub free_space: Option<u64>,
    pub opened_urls: Vec<String>,
    /// When true, the administrator password dialog is "cancelled".
    pub admin_cancelled: bool,
    pub macos: bool,
}

impl FakeSys {
    pub fn new() -> FakeSys {
        FakeSys {
            state: Mutex::new(FakeState {
                macos: true,
                free_space: Some(100_000_000_000),
                ..Default::default()
            }),
        }
    }

    pub fn with<R>(&self, f: impl FnOnce(&mut FakeState) -> R) -> R {
        let mut g = match self.state.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        f(&mut g)
    }

    pub fn answer(&self, key: &str, stdout: &str) {
        self.with(|s| {
            s.answers.insert(
                key.to_string(),
                CmdOut {
                    status: 0,
                    stdout: stdout.to_string(),
                    stderr: String::new(),
                },
            )
        });
    }

    pub fn answer_full(&self, key: &str, out: CmdOut) {
        self.with(|s| s.answers.insert(key.to_string(), out));
    }

    pub fn calls(&self) -> Vec<String> {
        self.with(|s| s.calls.clone())
    }

    pub fn admin_calls(&self) -> Vec<String> {
        self.with(|s| s.admin_calls.clone())
    }

    pub fn set_open(&self, path: &Path) {
        self.with(|s| s.open_files.insert(path.to_path_buf()));
    }
}

impl Sys for FakeSys {
    fn run(&self, program: &str, args: &[&str]) -> io::Result<CmdOut> {
        let line = std::iter::once(program)
            .chain(args.iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        self.with(|s| {
            s.calls.push(line.clone());
            let short = Path::new(program)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let short_line = std::iter::once(short.as_str())
                .chain(args.iter().copied())
                .collect::<Vec<_>>()
                .join(" ");
            s.answers
                .get(&line)
                .or_else(|| s.answers.get(&short_line))
                .or_else(|| s.answers.get(program))
                .or_else(|| s.answers.get(&short))
                .cloned()
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("fake: no answer for `{line}`"),
                    )
                })
        })
    }

    fn run_admin(&self, shell_command: &str) -> io::Result<CmdOut> {
        self.with(|s| {
            s.admin_calls.push(shell_command.to_string());
            if s.admin_cancelled {
                Ok(CmdOut {
                    status: 1,
                    stdout: String::new(),
                    stderr: "User canceled. (-128)".into(),
                })
            } else {
                Ok(s.answers
                    .get(&format!("admin:{shell_command}"))
                    .cloned()
                    .unwrap_or_default())
            }
        })
    }

    fn open_files(&self) -> io::Result<HashSet<PathBuf>> {
        Ok(self.with(|s| s.open_files.clone()))
    }

    fn running_apps(&self) -> Vec<RunningApp> {
        self.with(|s| s.running.clone())
    }

    fn free_space(&self, _path: &Path) -> io::Result<u64> {
        self.with(|s| s.free_space)
            .ok_or_else(|| io::Error::other("fake: free space unknown"))
    }

    fn open_url(&self, url: &str) -> io::Result<()> {
        self.with(|s| s.opened_urls.push(url.to_string()));
        Ok(())
    }

    fn is_macos(&self) -> bool {
        self.with(|s| s.macos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsof_lines() {
        let s = parse_lsof("p12\nn/Users/a/x.db\nn/dev/null\nnlocalhost:443\nn/tmp/a (deleted)\n");
        assert!(s.contains(Path::new("/Users/a/x.db")));
        assert!(s.contains(Path::new("/tmp/a")));
        assert!(!s.iter().any(|p| p.to_string_lossy().contains("localhost")));
    }

    #[test]
    fn ps_lines() {
        let apps =
            parse_ps("  1 /sbin/launchd\n 512 /Applications/Safari.app/Contents/MacOS/Safari\n");
        assert_eq!(apps[1].name, "Safari");
        assert_eq!(
            apps[1].path.as_deref(),
            Some(Path::new("/Applications/Safari.app"))
        );
    }

    #[test]
    fn df_free() {
        let out = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 100 50 40 50% /\n";
        assert_eq!(parse_df_free(out), Some(40 * 1024));
    }
}
