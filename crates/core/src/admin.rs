//! Maintenance without the password (Settings > Maintenance without a password, off by default).
//!
//! One password dialog writes `/etc/sudoers.d/cleancraft`, which lets this user run a short,
//! fixed list of harmless commands through `sudo -n`: Free Up RAM (`purge`), Flush DNS,
//! Reindex Spotlight and the XProtect rules update. [`run`] uses it for exactly those commands
//! and falls back to the password dialog for everything else, and whenever `sudo` refuses.
//!
//! Moving system files, turning off system services and deleting Time Machine snapshots keep
//! asking every time, on purpose: a rule for any command would let any program on the Mac
//! become administrator. Turning it off renames the file to `cleancraft.off`, which `sudo`
//! skips (a name with a dot); nothing is deleted.

use std::io;

use crate::env::Env;
use crate::sys::{ADMIN_TIMEOUT, CmdOut};

/// The rule file `sudo` reads.
pub const RULE_FILE: &str = "/etc/sudoers.d/cleancraft";
const DIR: &str = "/private/etc/sudoers.d";
const SUDO: &str = "/usr/bin/sudo";

/// The administrator shell commands CleanCraft runs that may skip the password, each with
/// the exact programs and arguments `sudo` runs for it (as the rule lists them).
const ALLOWED: &[(&str, &[&[&str]])] = &[
    ("purge", &[&["/usr/sbin/purge"]]),
    ("/usr/sbin/purge", &[&["/usr/sbin/purge"]]),
    (
        "dscacheutil -flushcache; killall -HUP mDNSResponder",
        &[
            &["/usr/bin/dscacheutil", "-flushcache"],
            &["/usr/bin/killall", "-HUP", "mDNSResponder"],
        ],
    ),
    ("mdutil -E /", &[&["/usr/bin/mdutil", "-E", "/"]]),
    (
        "/usr/bin/xprotect update",
        &[&["/usr/bin/xprotect", "update"]],
    ),
];

/// Is the rule in place?
pub fn is_on(env: &Env) -> bool {
    env.sys_path(RULE_FILE).is_file()
}

/// The rule's text for `user`: exact commands only. `purge ""` allows no arguments.
pub fn rule_text(user: &str) -> String {
    format!(
        "# CleanCraft: maintenance commands that run without the password. Turn this off in CleanCraft Settings.\n{user} ALL = (root) NOPASSWD: /usr/sbin/purge \"\", /usr/bin/dscacheutil -flushcache, /usr/bin/killall -HUP mDNSResponder, /usr/bin/mdutil -E /, /usr/bin/xprotect update\n"
    )
}

/// The one administrator command that writes the rule: to a temporary name `sudo` skips,
/// checked by `visudo`, then renamed into place. A rule that fails the check is never used.
pub fn set_up_script(user: &str) -> String {
    let lines = rule_text(user)
        .lines()
        .map(|l| format!("'{l}'"))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "umask 0337 && /bin/mkdir -p {DIR} && /usr/bin/printf '%s\\n' {lines} > {DIR}/cleancraft.new && /usr/sbin/chown root:wheel {DIR}/cleancraft.new && /usr/sbin/visudo -cf {DIR}/cleancraft.new && /bin/mv -f {DIR}/cleancraft.new {DIR}/cleancraft"
    )
}

/// The administrator command that turns it off, keeping the file under a name `sudo` skips.
pub fn turn_off_script() -> String {
    format!("/bin/mv -f {DIR}/cleancraft {DIR}/cleancraft.off")
}

/// A short user name `sudo` reads as one word.
fn valid_user(u: &str) -> bool {
    !u.is_empty()
        && u != "ALL"
        && !u.starts_with('-')
        && u.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Was it turned off with Ask Every Time? Then it is never set up again on its own.
pub fn was_turned_off(env: &Env) -> bool {
    env.sys_path(&format!("{RULE_FILE}.off")).is_file()
}

fn user(env: &Env) -> Result<String, String> {
    let out = env
        .sys
        .run("/usr/bin/id", &["-un"])
        .map_err(|e| format!("Could not read your user name: {e}"))?;
    let user = out.stdout.trim();
    if !out.ok() || !valid_user(user) {
        return Err(format!("Could not use the user name \"{user}\"."));
    }
    Ok(user.to_string())
}

/// Ask for the password once and write the rule. `Ok(false)`: the dialog was cancelled.
pub fn set_up(env: &Env) -> Result<bool, String> {
    admin_result(env.sys.run_admin(&set_up_script(&user(env)?)))
}

/// Ask for the password once and take the rule away.
pub fn turn_off(env: &Env) -> Result<bool, String> {
    admin_result(env.sys.run_admin(&turn_off_script()))
}

fn admin_result(r: io::Result<CmdOut>) -> Result<bool, String> {
    let out = r.map_err(|e| e.to_string())?;
    if crate::sys::admin_cancelled(&out) {
        return Ok(false);
    }
    if !out.ok() {
        let why = out.stderr.trim();
        return Err(if why.is_empty() {
            format!("The command ended with status {}.", out.status)
        } else {
            why.to_string()
        });
    }
    Ok(true)
}

/// Run `shell` as administrator: through `sudo -n` without a dialog when the rule is on and
/// the command is one it allows, otherwise (or when `sudo` refuses) with the password dialog.
pub fn run(env: &Env, shell: &str) -> io::Result<CmdOut> {
    let steps = ALLOWED.iter().find(|(s, _)| *s == shell).map(|(_, a)| *a);
    let Some(steps) = steps else {
        return env.sys.run_admin(shell);
    };
    if !is_on(env) {
        // The first time one of them asks is the last: the same dialog writes the rule too,
        // unless the person turned it off. In a subshell, so its umask stays there; a failed setup
        // never stops the command.
        return match user(env) {
            Ok(u) if !was_turned_off(env) => env.sys.run_admin(&format!(
                "( {} ) >/dev/null 2>&1 ; {shell}",
                set_up_script(&u)
            )),
            _ => env.sys.run_admin(shell),
        };
    }
    let mut all = CmdOut::default();
    for argv in steps {
        let args: Vec<&str> = std::iter::once("-n").chain(argv.iter().copied()).collect();
        let out = env.sys.run_timeout(SUDO, &args, ADMIN_TIMEOUT)?;
        // sudo's own refusal (the rule changed or is gone): ask as before.
        if !out.ok() && out.stderr.starts_with("sudo:") {
            return env.sys.run_admin(shell);
        }
        all.stdout.push_str(&out.stdout);
        all.stderr.push_str(&out.stderr);
        if !out.ok() && all.ok() {
            all.status = out.status;
        }
    }
    Ok(all)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::testkit::FakeMac;

    #[test]
    fn the_rule_lists_exact_commands_and_odd_user_names_are_refused() {
        let r = rule_text("mario");
        assert!(r.contains("mario ALL = (root) NOPASSWD: /usr/sbin/purge \"\", "));
        assert!(!r.contains("/bin/mv") && !r.contains("ALL\n") && !r.contains("'"));
        let s = set_up_script("mario");
        assert!(s.contains("visudo -cf /private/etc/sudoers.d/cleancraft.new && /bin/mv -f"));
        for u in ["", "ALL", "-x", "a b", "a'b", "a,b", "ALL:"] {
            assert!(!valid_user(u), "{u}");
        }
        assert!(valid_user("mario.de-luca_2"));
    }

    #[test]
    fn allowed_commands_skip_the_dialog_only_while_the_rule_is_on() {
        let m = FakeMac::new();
        let fake = m.env.fake_sys().unwrap();
        fake.answer("id -un", "mario\n");
        fake.answer("/usr/bin/sudo -n /usr/sbin/purge", "");
        // Never set up: the first dialog is the last, it also writes the rule.
        run(&m.env, "/usr/sbin/purge").unwrap();
        let first = &fake.admin_calls()[0];
        assert!(first.starts_with(&format!("( {} ) ", set_up_script("mario"))));
        assert!(first.ends_with(" ; /usr/sbin/purge"), "{first}");
        // Other commands are never joined to it.
        run(&m.env, "tmutil thinlocalsnapshots / 1 4").unwrap();
        assert_eq!(fake.admin_calls()[1], "tmutil thinlocalsnapshots / 1 4");
        // Turned off with Ask Every Time: never set up again on its own.
        m.write(&m.env.sys_path("/etc/sudoers.d/cleancraft.off"), b"");
        run(&m.env, "/usr/sbin/purge").unwrap();
        assert_eq!(fake.admin_calls()[2], "/usr/sbin/purge");
        let calls = fake.admin_calls().len();
        assert!(set_up(&m.env).unwrap());
        assert_eq!(fake.admin_calls()[calls], set_up_script("mario"));
        // What the root script writes.
        m.write(&m.env.sys_path(RULE_FILE), rule_text("mario").as_bytes());
        assert!(is_on(&m.env));
        assert!(run(&m.env, "purge").unwrap().ok());
        assert!(run(&m.env, "/usr/sbin/purge").unwrap().ok());
        assert_eq!(fake.admin_calls().len(), 4, "no dialog");
        // Any other command still asks.
        run(&m.env, "mdutil -E / ; reboot").unwrap();
        assert_eq!(fake.admin_calls().len(), 5);
        // sudo refuses (the rule was edited): the dialog again.
        fake.answer_full(
            "/usr/bin/sudo -n /usr/bin/mdutil -E /",
            CmdOut {
                status: 1,
                stdout: String::new(),
                stderr: "sudo: a password is required\n".into(),
            },
        );
        run(&m.env, "mdutil -E /").unwrap();
        assert_eq!(fake.admin_calls().last().unwrap(), "mdutil -E /");
        // Both DNS steps, without a dialog.
        fake.answer("/usr/bin/sudo -n /usr/bin/dscacheutil -flushcache", "");
        fake.answer("/usr/bin/sudo -n /usr/bin/killall -HUP mDNSResponder", "");
        let n = fake.admin_calls().len();
        assert!(
            run(
                &m.env,
                "dscacheutil -flushcache; killall -HUP mDNSResponder"
            )
            .unwrap()
            .ok()
        );
        assert_eq!(fake.admin_calls().len(), n);
        assert!(turn_off(&m.env).unwrap());
        assert_eq!(fake.admin_calls().last().unwrap(), &turn_off_script());
    }

    #[test]
    fn a_cancelled_dialog_is_not_an_error() {
        let m = FakeMac::new();
        let fake = m.env.fake_sys().unwrap();
        fake.answer("id -un", "mario\n");
        fake.with(|s| s.admin_cancelled = true);
        assert_eq!(set_up(&m.env), Ok(false));
        assert_eq!(turn_off(&m.env), Ok(false));
    }
}
