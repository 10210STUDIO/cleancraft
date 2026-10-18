//! Fake Mac set-ups for Tune-up Tasks, Startup Items and Threat Check tests.
//!
//! The command outputs below are SAMPLES written from knowledge of macOS, not recorded on a
//! real Mac. Each must be checked by hand once (see docs/agents/reports/perf.md, "Hand checks").

use std::path::PathBuf;
use std::sync::Arc;

use crate::env::Env;
use crate::startup::{LOGIN_ITEMS_SCRIPT, PS_ARGS, command_key, osascript_args};
use crate::sys::{CmdOut, FakeSys, RunningApp, Sys};
use crate::testkit::FakeMac;

pub const UID: u32 = 501;

/// Sample: `codesign -dv --verbose=2` of a Developer ID program (stderr).
pub fn codesign_developer(program: &str, who: &str, team: &str) -> String {
    format!(
        "Executable={program}\nIdentifier=com.example\nFormat=Mach-O universal (x86_64 arm64)\nCodeDirectory v=20500 size=1234 flags=0x10000(runtime) hashes=28+7 location=embedded\nSignature size=9000\nAuthority=Developer ID Application: {who} ({team})\nAuthority=Developer ID Certification Authority\nAuthority=Apple Root CA\nTimestamp=12 Oct 2026 at 10:00:00\nInfo.plist=not bound\nTeamIdentifier={team}\nRuntime Version=14.0.0\nSealed Resources=none\nInternal requirements count=1 size=180\n"
    )
}

/// Sample: `codesign -dv --verbose=2` of an Apple program (stderr).
pub fn codesign_apple(program: &str) -> String {
    format!(
        "Executable={program}\nIdentifier=com.apple.sample\nFormat=Mach-O universal (x86_64 arm64e)\nCodeDirectory v=20400 size=800 flags=0x0(none) hashes=20+7 location=embedded\nPlatform identifier=15\nSignature size=4442\nAuthority=Software Signing\nAuthority=Apple Code Signing Certification Authority\nAuthority=Apple Root CA\nSigned Time=1 Oct 2026 at 08:00:00\nInfo.plist=not bound\nTeamIdentifier=not set\nSealed Resources=none\nInternal requirements count=1 size=64\n"
    )
}

/// Sample: ad hoc signed (stderr).
pub fn codesign_adhoc(program: &str) -> String {
    format!(
        "Executable={program}\nIdentifier=upd-55554944a1b2\nFormat=Mach-O thin (arm64)\nCodeDirectory v=20400 size=500 flags=0x20002(adhoc,linker-signed) hashes=12+0 location=embedded\nSignature=adhoc\nInfo.plist=not bound\nTeamIdentifier=not set\nSealed Resources=none\nInternal requirements=none\n"
    )
}

/// Sample: not signed (stderr, exit 1).
pub fn codesign_unsigned(program: &str) -> CmdOut {
    CmdOut {
        status: 1,
        stdout: String::new(),
        stderr: format!("{program}: code object is not signed at all\n"),
    }
}

/// Sample: `launchctl print-disabled gui/501` (macOS 13 and later wording).
pub const PRINT_DISABLED_GUI: &str = "disabled services = {\n\t\"com.example.global\" => disabled\n\t\"com.example.helper\" => enabled\n\t\"com.apple.ScriptMenuApp\" => disabled\n}\n\nlogin item associations = {\n\t\"com.example.helper\" => com.example.app\n}\n";

/// Sample: `launchctl print-disabled system`.
pub const PRINT_DISABLED_SYSTEM: &str =
    "disabled services = {\n\t\"com.apple.ftpd\" => disabled\n\t\"com.apple.mrt\" => disabled\n}\n";

/// Sample: `ps -axo pid=,uid=,%cpu=,%mem=,comm=`.
pub const PS_SAMPLE: &str = "    1     0   0.1  0.1 /sbin/launchd\n  312   501   2.0  0.5 /System/Library/CoreServices/Dock.app/Contents/MacOS/Dock\n  640   501  92.5  6.1 /Applications/Example.app/Contents/MacOS/Example\n  701   501  12.0 14.2 /Applications/Big Editor.app/Contents/MacOS/Big Editor\n  702   501  45.0  1.0 /Users/tester/bin/render\n  705   501   0.3  0.2 /Applications/Quiet.app/Contents/MacOS/Quiet\n  800   502  99.0  3.0 /Applications/Other.app/Contents/MacOS/Other\n  900   501  60.0  2.0 /Applications/CleanCraft.app/Contents/MacOS/cleancraft\n";

/// Sample: `tmutil listlocalsnapshots /`.
pub const TMUTIL_LIST: &str = "Snapshots for disk /:\ncom.apple.TimeMachine.2026-10-17-101530.local\ncom.apple.TimeMachine.2026-10-18-091200.local\n";

/// Sample: `spctl --assess --type execute` rejecting an app (stderr, exit 3).
pub fn spctl_rejected(app: &str) -> CmdOut {
    CmdOut {
        status: 3,
        stdout: String::new(),
        stderr: format!("{app}: rejected\nsource=no usable signature\n"),
    }
}

/// Sample: login items from CleanCraft's System Events script (name TAB path per line).
pub const LOGIN_ITEMS_SAMPLE: &str =
    "Example\t/Applications/Example.app\nDropbox\t/Applications/Dropbox.app\n";

/// The FakeSys key of the login items script.
pub fn login_items_key() -> String {
    command_key("/usr/bin/osascript", &osascript_args(LOGIN_ITEMS_SCRIPT))
}

pub fn ps_key() -> String {
    let a: Vec<String> = PS_ARGS.iter().map(|s| s.to_string()).collect();
    command_key("/bin/ps", &a)
}

/// A launchd property list.
pub fn launch_plist(label: &str, program: &str, run_at_load: bool) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>Label</key><string>{label}</string>
<key>ProgramArguments</key><array><string>{program}</string><string>--background</string></array>
<key>RunAtLoad</key><{run_at_load}/>
</dict></plist>"#
    )
}

fn fake(mac: &FakeMac) -> &FakeSys {
    mac.env.fake_sys().expect("fake env")
}

/// Write a launch item (property list + program file) and answer its codesign call.
/// `folder` is a macOS path like "~/Library/LaunchAgents". Returns the plist path.
pub fn add_launch_item(
    mac: &FakeMac,
    folder: &str,
    label: &str,
    program: &str,
    signature: Option<CmdOut>,
) -> PathBuf {
    let dir = match folder.strip_prefix("~/") {
        Some(rel) => mac.env.home_path(rel),
        None => mac.env.sys_path(folder),
    };
    let plist = dir.join(format!("{label}.plist"));
    mac.write(&plist, launch_plist(label, program, true).as_bytes());
    if let Some(sig) = signature {
        mac.write(&mac.env.sys_path(program), b"\xcf\xfa\xed\xfe program");
        fake(mac).answer_full(&format!("codesign -dv --verbose=2 {program}"), sig);
    }
    plist
}

fn stderr_ok(s: String) -> CmdOut {
    CmdOut {
        status: 0,
        stdout: String::new(),
        stderr: s,
    }
}

pub const HELPER_PROG: &str = "/Applications/Example.app/Contents/Library/LoginItems/ExampleHelper.app/Contents/MacOS/ExampleHelper";
pub const GENIEO_PROG: &str = "/Users/tester/Library/Application Support/Genieo/engine";
pub const UPDATER_PROG: &str = "/Users/Shared/.cache/upd";
pub const GLOBAL_PROG: &str = "/Library/Application Support/Example/agent";
pub const DAEMON_PROG: &str = "/Library/PrivilegedHelperTools/com.example.daemon";
pub const APPLE_PROG: &str = "/usr/libexec/sampled";

/// A fake Mac with login items, agents and daemons of every kind, running apps and
/// Gatekeeper answers. All command answers are samples (see the module comment).
pub fn startup_mac() -> FakeMac {
    let mac = FakeMac::new();
    let f = fake(&mac);
    f.answer("id -u", "501\n");
    f.answer("sw_vers -productVersion", "14.6.1\n");
    f.answer("launchctl print-disabled gui/501", PRINT_DISABLED_GUI);
    f.answer("launchctl print-disabled system", PRINT_DISABLED_SYSTEM);
    f.answer(&login_items_key(), LOGIN_ITEMS_SAMPLE);
    f.answer(&ps_key(), PS_SAMPLE);
    // Gatekeeper accepts everything unless told otherwise.
    f.answer_full(
        "spctl",
        stderr_ok("accepted\nsource=Notarized Developer ID\n".into()),
    );
    // launchctl changes succeed.
    f.answer("launchctl", "");
    add_launch_item(
        &mac,
        "~/Library/LaunchAgents",
        "com.example.helper",
        HELPER_PROG,
        Some(stderr_ok(codesign_developer(
            HELPER_PROG,
            "Example Corp",
            "ABCDE12345",
        ))),
    );
    add_launch_item(
        &mac,
        "~/Library/LaunchAgents",
        "com.genieo.engine",
        GENIEO_PROG,
        Some(codesign_unsigned(GENIEO_PROG)),
    );
    add_launch_item(
        &mac,
        "~/Library/LaunchAgents",
        "com.example.updater",
        UPDATER_PROG,
        Some(stderr_ok(codesign_adhoc(UPDATER_PROG))),
    );
    add_launch_item(
        &mac,
        "~/Library/LaunchAgents",
        "com.example.old",
        "/Applications/Gone.app/Contents/MacOS/Gone",
        None,
    );
    add_launch_item(
        &mac,
        "/Library/LaunchAgents",
        "com.example.global",
        GLOBAL_PROG,
        Some(stderr_ok(codesign_developer(
            GLOBAL_PROG,
            "Example Corp",
            "ABCDE12345",
        ))),
    );
    add_launch_item(
        &mac,
        "/Library/LaunchAgents",
        "com.apple.sampled",
        APPLE_PROG,
        Some(stderr_ok(codesign_apple(APPLE_PROG))),
    );
    add_launch_item(
        &mac,
        "/Library/LaunchDaemons",
        "com.example.daemon",
        DAEMON_PROG,
        Some(stderr_ok(codesign_developer(
            DAEMON_PROG,
            "Example Corp",
            "ABCDE12345",
        ))),
    );
    mac.app("Example", "com.example.app", "1.0");
    mac.app("Big Editor", "com.example.bigeditor", "2.0");
    mac.app("Quiet", "com.example.quiet", "1.0");
    mac.app("MacKeeper", "com.mackeeper.MacKeeper", "6.0");
    let unsigned = mac.app("Unsigned Tool", "org.example.tool", "0.1");
    f.answer_full(
        &format!(
            "spctl --assess --type execute {}",
            mac.env.display(&unsigned)
        ),
        spctl_rejected(&mac.env.display(&unsigned)),
    );
    // On a real Mac these folders belong to root.
    lock_system_folders(&mac);
    mac
}

/// Make `/Library/LaunchAgents` and `/Library/LaunchDaemons` behave as root-owned folders.
pub fn lock_system_folders(mac: &FakeMac) {
    for d in ["/Library/LaunchAgents", "/Library/LaunchDaemons"] {
        mac.env
            .inject_fault(&mac.env.sys_path(d), crate::env::Fault::PermissionDenied);
    }
}

/// Mail open or closed.
pub fn set_mail_running(mac: &FakeMac, on: bool) {
    fake(mac).with(|s| {
        s.running.retain(|a| a.name != "Mail");
        if on {
            s.running.push(RunningApp {
                pid: 444,
                name: "Mail".into(),
                bundle_id: None,
                path: Some("/System/Applications/Mail.app".into()),
            });
        }
    });
}

/// A Sys that reports `by` more free bytes once a command containing `trigger` has run.
/// Everything else goes to the fake Mac's FakeSys (which still records every call).
pub struct FreeSpaceRises {
    inner: Env,
    trigger: String,
    by: u64,
}

impl Sys for FreeSpaceRises {
    fn run(&self, program: &str, args: &[&str]) -> std::io::Result<CmdOut> {
        self.inner.sys.run(program, args)
    }
    fn run_admin(&self, shell_command: &str) -> std::io::Result<CmdOut> {
        self.inner.sys.run_admin(shell_command)
    }
    fn open_files(&self) -> std::io::Result<std::collections::HashSet<PathBuf>> {
        self.inner.sys.open_files()
    }
    fn running_apps(&self) -> Vec<RunningApp> {
        self.inner.sys.running_apps()
    }
    fn free_space(&self, path: &std::path::Path) -> std::io::Result<u64> {
        let base = self.inner.sys.free_space(path)?;
        let fs = self.inner.fake_sys().expect("fake env");
        let hit = fs
            .calls()
            .iter()
            .chain(fs.admin_calls().iter())
            .any(|c| c.contains(&self.trigger));
        Ok(if hit { base + self.by } else { base })
    }
    fn open_url(&self, url: &str) -> std::io::Result<()> {
        self.inner.sys.open_url(url)
    }
    fn is_macos(&self) -> bool {
        self.inner.sys.is_macos()
    }
}

/// Free space on the fake Mac grows by `by` bytes after a command containing `trigger`.
pub fn free_space_rises_after(mac: &mut FakeMac, trigger: &str, by: u64) {
    let inner = mac.env.clone();
    mac.env.sys = Arc::new(FreeSpaceRises {
        inner,
        trigger: trigger.into(),
        by,
    });
}

/// Mail's index file of `size` bytes in `~/Library/Mail/V10/MailData`.
pub fn mail_index(mac: &FakeMac, size: usize) -> PathBuf {
    mac.home_file("Library/Mail/V10/MailData/Envelope Index", size)
}
