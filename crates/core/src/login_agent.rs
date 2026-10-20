//! "Show CleanCraft in the menu bar": a per-user LaunchAgent that starts `cleancraft agent`
//! at login.
//!
//! On: write `~/Library/LaunchAgents/com.cristianizzo.cleancraft.agent.plist` and load it now
//! with `launchctl bootstrap gui/<uid> <plist>`. Off: `launchctl bootout gui/<uid>/<label>` and
//! move the plist to the Trash (never deleted). Both are journaled as `Toggled` with an undo
//! command, so History can take them back. The switch shows whether the plist is there.

use std::path::{Path, PathBuf};

use crate::env::Env;
use crate::executor::{self, ActionKind, ActionRecord};
use crate::plan::FeatureId;
use crate::sys::shell_quote;

pub const LABEL: &str = "com.cristianizzo.cleancraft.agent";
pub const LAUNCHCTL: &str = "/bin/launchctl";
/// The helper app inside CleanCraft.app that runs the agent (scripts/install-app.sh builds it:
/// its own bundle id and `LSUIElement`, so it has no Dock icon and `open -b` of the main app
/// never lands on it).
pub const HELPER_REL: &str =
    "Contents/Library/LoginItems/CleanCraftAgent.app/Contents/MacOS/CleanCraftAgent";
/// "What" of the journal lines.
pub const ON_TEXT: &str = "Turned on CleanCraft in the menu bar";
pub const OFF_TEXT: &str = "Turned off CleanCraft in the menu bar";

pub fn plist_path(env: &Env) -> PathBuf {
    env.home_path(&format!("Library/LaunchAgents/{LABEL}.plist"))
}

/// Is the menu bar agent set to start at login (the plist is there)?
pub fn is_on(env: &Env) -> bool {
    plist_path(env).is_file()
}

/// The program the LaunchAgent starts, from the app's own executable
/// (`.../CleanCraft.app/Contents/MacOS/CleanCraft`): the helper app's copy when it is
/// installed, otherwise the executable itself (a build that is not in an app bundle).
pub fn agent_program(exe: &Path) -> PathBuf {
    let bundle = exe
        .ancestors()
        .find(|a| a.extension().is_some_and(|e| e == "app"));
    match bundle {
        Some(b) if b.join(HELPER_REL).is_file() => b.join(HELPER_REL),
        _ => exe.to_path_buf(),
    }
}

/// The LaunchAgent: `program agent` at login, in the GUI session only, not restarted when it
/// quits (KeepAlive false), output in ~/Library/Logs/CleanCraft/agent.log.
pub fn plist_xml(program: &Path, log: &Path) -> String {
    let mut d = plist::Dictionary::new();
    d.insert("Label".into(), LABEL.into());
    d.insert(
        "ProgramArguments".into(),
        plist::Value::Array(vec![
            program.to_string_lossy().into_owned().into(),
            "agent".into(),
        ]),
    );
    d.insert("RunAtLoad".into(), true.into());
    d.insert("KeepAlive".into(), false.into());
    d.insert("LimitLoadToSessionType".into(), "Aqua".into());
    d.insert("ProcessType".into(), "Interactive".into());
    let log = log.to_string_lossy().into_owned();
    d.insert("StandardOutPath".into(), log.clone().into());
    d.insert("StandardErrorPath".into(), log.into());
    let mut out = Vec::new();
    match plist::Value::Dictionary(d).to_writer_xml(&mut out) {
        Ok(()) => String::from_utf8_lossy(&out).into_owned(),
        Err(_) => String::new(),
    }
}

pub fn log_path(env: &Env) -> PathBuf {
    env.home_path("Library/Logs/CleanCraft/agent.log")
}

/// The real path of `p` for commands: the same as `p` on a Mac; in tests the fake path.
fn arg(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// What a switch change did: the journal line, and a note when it only half worked (saved,
/// but launchctl did not start it now).
#[derive(Clone, Debug)]
pub struct Change {
    pub record: ActionRecord,
    pub note: Option<String>,
}

fn domain(env: &Env) -> String {
    format!("gui/{}", crate::features::trash::uid(env))
}

/// Turn on: write the plist (when it is not there yet) and load it now.
pub fn turn_on(env: &Env, program: &Path) -> Result<Change, String> {
    crate::tasks::journal_writable(env)
        .map_err(|e| format!("Could not write the action log, nothing changed: {e}"))?;
    let plist = plist_path(env);
    if !plist.is_file() {
        let dir = plist.parent().map(Path::to_path_buf).unwrap_or_default();
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("Could not create {}: {e}", env.display(&dir)))?;
        if let Some(logs) = log_path(env).parent() {
            let _ = std::fs::create_dir_all(logs);
        }
        let tmp = plist.with_extension("plist.tmp");
        std::fs::write(&tmp, plist_xml(program, &log_path(env)))
            .and_then(|_| std::fs::rename(&tmp, &plist))
            .map_err(|e| format!("Could not save the login item: {e}"))?;
    }
    let dom = domain(env);
    let note = match env.sys.run(LAUNCHCTL, &["bootstrap", &dom, &arg(&plist)]) {
        Ok(out) if out.ok() => None,
        Ok(out) => Some(format!(
            "It starts at your next login; launchctl did not start it now: {}",
            crate::tasks::first_lines(&out.stderr, 1)
        )),
        Err(e) => Some(format!(
            "It starts at your next login; launchctl did not start it now: {e}"
        )),
    };
    // Undo: stop it and move the plist to the Trash (never deleted).
    let trash = executor::free_name(&env.user_trash(), &format!("{LABEL}.plist"), false);
    let undo = format!(
        "{LAUNCHCTL} bootout {}; /bin/mkdir -p {} && /bin/mv -n -- {} {}",
        shell_quote(&format!("{dom}/{LABEL}")),
        shell_quote(&arg(&env.user_trash())),
        shell_quote(&arg(&plist)),
        shell_quote(&arg(&trash))
    );
    let record = log(env, format!("{ON_TEXT} ({})", env.display(&plist)), undo)?;
    Ok(Change { record, note })
}

/// Turn off: stop the agent and move the plist to the Trash.
pub fn turn_off(env: &Env) -> Result<Change, String> {
    crate::tasks::journal_writable(env)
        .map_err(|e| format!("Could not write the action log, nothing changed: {e}"))?;
    let plist = plist_path(env);
    if !plist.is_file() {
        return Err("CleanCraft was not set to start in the menu bar.".into());
    }
    let dom = domain(env);
    // Not running is fine: it is off either way once the plist is gone.
    let _ = env
        .sys
        .run(LAUNCHCTL, &["bootout", &format!("{dom}/{LABEL}")]);
    let trash_dir = env.user_trash();
    std::fs::create_dir_all(&trash_dir).map_err(|e| format!("Could not open the Trash: {e}"))?;
    let stored = executor::free_name(&trash_dir, &format!("{LABEL}.plist"), false);
    std::fs::rename(&plist, &stored)
        .map_err(|e| format!("Could not move the login item to the Trash: {e}"))?;
    let undo = format!(
        "/bin/mv -n -- {} {} && {LAUNCHCTL} bootstrap {} {}",
        shell_quote(&arg(&stored)),
        shell_quote(&arg(&plist)),
        shell_quote(&dom),
        shell_quote(&arg(&plist))
    );
    let record = log(
        env,
        format!("{OFF_TEXT} ({} moved to the Trash)", env.display(&plist)),
        undo,
    )?;
    Ok(Change { record, note: None })
}

fn log(env: &Env, what: String, undo: String) -> Result<ActionRecord, String> {
    let now = chrono::Utc::now();
    let plan_id = format!("menubar-{}", now.timestamp_nanos_opt().unwrap_or(0));
    let rec = ActionRecord {
        id: format!("{plan_id}#0"),
        plan_id,
        feature: Some(FeatureId::Monitor),
        time: now.timestamp(),
        size: 0,
        kind: ActionKind::Toggled {
            what,
            undo_command: Some(undo),
        },
        undone: None,
    };
    executor::append_journal(env, &rec).map_err(|e| {
        format!("The change was made, but the action log could not be written: {e}")
    })?;
    Ok(rec)
}
