//! Test helpers for quitting apps before they are removed ([`crate::app_quit`]): processes with
//! owners, helper apps inside a bundle, launchd domains, the window server's app list, and
//! commands that stop processes, all on a [`FakeMac`].

use std::path::{Path, PathBuf};

use crate::app_quit::{LSAPPINFO, PS, PS_ARGS};
use crate::startup::LAUNCHCTL;
use crate::sys::{FakeEffect, RunningApp, app_bundle_of};
use crate::testkit::FakeMac;

/// The FakeSys key of `ps -axo pid=,uid=,comm=`.
pub fn ps_key() -> String {
    std::iter::once(PS)
        .chain(PS_ARGS.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}

pub trait FakeMacQuit {
    /// `id -u` answers `uid`.
    fn user_id(&self, uid: u32);
    /// A process runs `program` (env path) with this pid and owner: it shows in the running
    /// apps (like `ps -axo pid=,comm=`) and in `ps -axo pid=,uid=,comm=`.
    fn run_program(&self, program: &Path, pid: u32, uid: u32) -> RunningApp;
    /// An app inside `app` at `rel` ("Contents/Library/LoginItems/X Menu.app") with its own
    /// bundle id. Returns its executable (env path).
    fn helper_app(&self, app: &Path, rel: &str, bundle_id: &str) -> PathBuf;
    /// `launchctl print <domain>` lists these services: (pid, label), pid 0 when loaded but
    /// not running.
    fn launchd(&self, domain: &str, services: &[(u32, &str)]);
    /// `lsappinfo list` knows these apps: (pid, name, bundle id, type).
    fn window_server(&self, apps: &[(u32, &str, &str, &str)]);
    /// When `key` runs, these pids stop.
    fn stops(&self, key: &str, pids: &[u32]);
}

/// The text `launchctl print <domain>` prints for `services`.
pub fn print_text(domain: &str, services: &[(u32, &str)]) -> String {
    let mut s = format!("{domain} = {{\n\ttype = login\n\tservices = {{\n");
    for (pid, label) in services {
        let pid = if *pid == 0 {
            "-".to_string()
        } else {
            pid.to_string()
        };
        s.push_str(&format!("\t\t{pid:>8}      -  {label}\n"));
    }
    s.push_str("\t}\n}\n");
    s
}

impl FakeMacQuit for FakeMac {
    fn user_id(&self, uid: u32) {
        if let Some(f) = self.env.fake_sys() {
            f.answer("/usr/bin/id -u", &format!("{uid}\n"));
        }
    }

    fn run_program(&self, program: &Path, pid: u32, uid: u32) -> RunningApp {
        let shown = PathBuf::from(self.env.display(program));
        let app = app_bundle_of(&shown);
        let r = RunningApp {
            pid,
            name: app
                .as_ref()
                .and_then(|a| a.file_stem())
                .or_else(|| shown.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            bundle_id: None,
            path: app.or(Some(shown.clone())),
        };
        if let Some(f) = self.env.fake_sys() {
            let key = ps_key();
            f.with(|s| {
                s.running.push(r.clone());
                let mut text = s
                    .answers
                    .get(&key)
                    .map(|o| o.stdout.clone())
                    .unwrap_or_default();
                text.push_str(&format!("{pid:>6} {uid:>5} {}\n", shown.display()));
                s.answers.insert(
                    key,
                    crate::sys::CmdOut {
                        status: 0,
                        stdout: text,
                        stderr: String::new(),
                    },
                );
            });
        }
        r
    }

    fn helper_app(&self, app: &Path, rel: &str, bundle_id: &str) -> PathBuf {
        let h = app.join(rel);
        let name = h
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut d = plist::Dictionary::new();
        d.insert("CFBundleIdentifier".into(), bundle_id.into());
        d.insert("CFBundleExecutable".into(), name.clone().into());
        let _ = std::fs::create_dir_all(h.join("Contents/MacOS"));
        let _ = plist::Value::Dictionary(d).to_file_xml(h.join("Contents/Info.plist"));
        let exe = h.join("Contents/MacOS").join(&name);
        self.write(&exe, &[7u8; 4096]);
        exe
    }

    fn launchd(&self, domain: &str, services: &[(u32, &str)]) {
        if let Some(f) = self.env.fake_sys() {
            f.answer(
                &format!("{LAUNCHCTL} print {domain}"),
                &print_text(domain, services),
            );
        }
    }

    fn window_server(&self, apps: &[(u32, &str, &str, &str)]) {
        let mut s = String::new();
        for (i, (pid, name, id, kind)) in apps.iter().enumerate() {
            s.push_str(&format!(
                "{}) \"{name}\" ASN:0x0-0x{:x}:\n    bundleID=\"{id}\"\n    pid = {pid} type=\"{kind}\" flavor=3\n",
                i + 1,
                0x1000 + i
            ));
        }
        if let Some(f) = self.env.fake_sys() {
            f.answer(&format!("{LSAPPINFO} list"), &s);
        }
    }

    fn stops(&self, key: &str, pids: &[u32]) {
        if let Some(f) = self.env.fake_sys() {
            f.on_run(
                key,
                FakeEffect {
                    stop: pids.to_vec(),
                    ..Default::default()
                },
            );
        }
    }
}
