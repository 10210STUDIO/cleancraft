//! Notifications from the menu bar agent, and "open the app at this page" requests.
//!
//! Two senders (U7-2):
//! - The native one, installed by the menu bar agent on macOS ([`set_native_sender`]): the
//!   notification is posted as CleanCraft (its bundle id and icon) and a click opens the app
//!   on the notification's route. Never used on a fake Mac.
//! - `osascript -e 'display notification ...'` (through `env.sys`, so tests record it), the
//!   fallback when the native one is missing or fails. macOS shows such a notification as
//!   coming from Script Editor, and clicking it opens Script Editor (or nothing), never
//!   CleanCraft.
//!
//! Either way every notification that leads somewhere also leaves a "Review" item in the menu
//! bar menu, and its text ends with [`MENU_HINT`] (until the native sender's click is
//! verified on a Mac, it is said with both).
//!
//! Opening the app on a page: [`request_open`] writes `open-request.json` in CleanCraft's data
//! folder, then asks Launch Services to open the app by its bundle id (`open -b`). A running
//! app is brought to the front (one instance only: Launch Services never starts a second
//! copy for `open -b`); a closed app starts. Either way the app reads the request
//! ([`OpenInbox`]) and switches to that page. A request older than [`REQUEST_MAX_AGE`] is
//! ignored, so an old file never moves the app later.

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::env::Env;

pub const OSASCRIPT: &str = "/usr/bin/osascript";
pub const OPEN: &str = "/usr/bin/open";
/// The app's bundle id (scripts/install-app.sh writes it in Info.plist).
pub const BUNDLE_ID: &str = "com.cristianizzo.cleancraft";
/// The URL scheme the app registers: `cleancraft://open/quick_sweep`.
pub const URL_SCHEME: &str = "cleancraft";
/// Requests older than this are ignored (seconds).
pub const REQUEST_MAX_AGE: i64 = 120;
/// The longest route accepted.
pub const ROUTE_MAX: usize = 1024;

/// One notification: a title line, an optional subtitle, a body, and where the app should
/// open when the person follows it (a route such as `scan:quick_sweep`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notification {
    pub title: String,
    pub subtitle: Option<String>,
    pub body: String,
    pub route: Option<String>,
}

/// The longest title, subtitle or body passed on (characters).
pub const TEXT_MAX: usize = 200;

/// The last sentence of every notification that leads somewhere.
pub const MENU_HINT: &str = "Open the CleanCraft menu bar item to review.";

/// The body as shown: for a notification with a route, cut so [`MENU_HINT`] always fits, then
/// the hint (once).
pub fn shown_body(n: &Notification) -> String {
    let hint = crate::i18n::tr(MENU_HINT);
    let body = n.body.trim();
    if n.route.is_none() || body.ends_with(&*hint) {
        return clean_text(body);
    }
    let room = TEXT_MAX - hint.chars().count() - 1;
    let cut = clean_text(body);
    let cut: String = cut.chars().take(room).collect();
    if cut.is_empty() {
        hint.into_owned()
    } else {
        format!("{} {hint}", cut.trim_end())
    }
}

/// `n` in the interface language. The menu hint is left out of the body: [`shown_body`] adds
/// it back, translated.
fn localized(n: &Notification) -> Notification {
    let tr = |s: &str| crate::i18n::tr(s).into_owned();
    let body = n.body.trim();
    let body = match (&n.route, body.strip_suffix(MENU_HINT)) {
        (Some(_), Some(cut)) => cut.trim_end(),
        _ => body,
    };
    Notification {
        title: tr(&n.title),
        subtitle: n.subtitle.as_deref().map(tr),
        body: tr(body),
        route: n.route.clone(),
    }
}

/// A sender that posts a notification as CleanCraft itself and opens the app on its route
/// when it is clicked (the menu bar agent installs it on macOS). Errors fall back to
/// `osascript`.
pub type NativeSender = Box<dyn Fn(&Notification) -> Result<(), String> + Send + Sync>;

static NATIVE: std::sync::OnceLock<NativeSender> = std::sync::OnceLock::new();

/// Install the native sender (once per process; later calls are ignored).
pub fn set_native_sender(s: NativeSender) {
    let _ = NATIVE.set(s);
}

/// One notification text as an argument: control characters (line breaks) become spaces,
/// cut at [`TEXT_MAX`] characters.
pub fn clean_text(s: &str) -> String {
    s.chars()
        .take(TEXT_MAX)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// The `osascript` arguments that show `n`. The texts are passed as `argv` items of a fixed
/// script, never written into the script, so a file or app name with quotes cannot change
/// what runs (safety6 R2/R7).
pub fn osascript_args(n: &Notification) -> Vec<String> {
    let line = match n.subtitle {
        Some(_) => {
            "display notification (item 2 of argv) with title (item 1 of argv) subtitle (item 3 of argv)"
        }
        None => "display notification (item 2 of argv) with title (item 1 of argv)",
    };
    let mut v: Vec<String> = vec![
        "-e".into(),
        "on run argv".into(),
        "-e".into(),
        line.into(),
        "-e".into(),
        "end run".into(),
        // getopt stops here: a text that starts with "-e" stays a text (safety6 SAF6-6).
        "--".into(),
        clean_text(&n.title),
        shown_body(n),
    ];
    if let Some(sub) = &n.subtitle {
        v.push(clean_text(sub));
    }
    v
}

/// Show `n`: with the native sender when one is installed (never on a fake Mac), else, or
/// when it fails, through `osascript`. No shell is involved and the texts are arguments.
pub fn send(env: &Env, n: &Notification) -> io::Result<()> {
    let n = &localized(n);
    if !env.is_fake()
        && let Some(native) = NATIVE.get()
    {
        match native(n) {
            Ok(()) => return Ok(()),
            Err(e) => eprintln!("CleanCraft: native notification failed ({e}); using osascript"),
        }
    }
    let args = osascript_args(n);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = env.sys.run(OSASCRIPT, &refs)?;
    if out.ok() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "the notification was not shown: {}",
            out.stderr.trim()
        )))
    }
}

// ---------------------------------------------------------------------------------------------
// Routes and open requests.

/// A route is plain text of at most [`ROUTE_MAX`] characters without control characters.
/// Empty means "just open the app".
pub fn valid_route(r: &str) -> bool {
    r.len() <= ROUTE_MAX && !r.chars().any(char::is_control)
}

/// The route in a `cleancraft://` URL: `cleancraft://open/quick_sweep`,
/// `cleancraft://quick_sweep` and `cleancraft:quick_sweep` all give `quick_sweep`; `%XX`
/// escapes are decoded (`uninstall:/Applications/My%20App.app`). `None` for another scheme or
/// a route that is not valid.
pub fn route_from_url(url: &str) -> Option<String> {
    let rest = url.trim();
    let (scheme, rest) = rest.split_once(':')?;
    if !scheme.eq_ignore_ascii_case(URL_SCHEME) {
        return None;
    }
    let rest = rest.trim_start_matches('/');
    let rest = rest.strip_prefix("open").map_or(rest, |r| {
        if r.is_empty() || r.starts_with('/') || r.starts_with('?') {
            r.trim_start_matches('/')
        } else {
            rest
        }
    });
    let rest = rest.strip_prefix("?page=").unwrap_or(rest);
    let route = percent_decode(rest.trim_end_matches('/'))?;
    valid_route(&route).then_some(route)
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// What `open-request.json` holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRequest {
    pub route: String,
    /// Seconds since the epoch.
    pub time: i64,
    /// Different for every request, so the app handles each one once.
    pub seq: u64,
}

pub fn request_path(env: &Env) -> PathBuf {
    env.data_dir().join("open-request.json")
}

/// Write the request for `route` (empty: no page change) and open the app with Launch
/// Services (`open -b com.cristianizzo.cleancraft`), which brings a running app to the front
/// or starts it. Returns the request written.
pub fn request_open(env: &Env, route: &str) -> Result<OpenRequest, String> {
    if !valid_route(route) {
        return Err("That page name is not valid.".into());
    }
    let now = chrono::Utc::now();
    let req = OpenRequest {
        route: route.to_string(),
        time: now.timestamp(),
        seq: now.timestamp_nanos_opt().unwrap_or(0) as u64,
    };
    write_request(env, &req).map_err(|e| format!("Could not leave the page for the app: {e}"))?;
    let out = env
        .sys
        .run(OPEN, &["-b", BUNDLE_ID])
        .map_err(|e| format!("CleanCraft could not be opened: {e}"))?;
    if !out.ok() {
        return Err(format!(
            "CleanCraft could not be opened: {}",
            out.stderr.trim()
        ));
    }
    Ok(req)
}

fn write_request(env: &Env, req: &OpenRequest) -> io::Result<()> {
    std::fs::create_dir_all(env.data_dir())?;
    let path = request_path(env);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(req).map_err(io::Error::other)?)?;
    std::fs::rename(tmp, path)
}

/// The app's side: each new, recent request once.
#[derive(Clone, Debug, Default)]
pub struct OpenInbox {
    last_seq: Option<u64>,
}

impl OpenInbox {
    /// The route of a request not handled yet and at most [`REQUEST_MAX_AGE`] seconds old at
    /// `now` (seconds since the epoch). A file that cannot be read is ignored.
    pub fn poll(&mut self, env: &Env, now: i64) -> Option<String> {
        let bytes = std::fs::read(request_path(env)).ok()?;
        let req: OpenRequest = serde_json::from_slice(&bytes).ok()?;
        if self.last_seq == Some(req.seq) {
            return None;
        }
        self.last_seq = Some(req.seq);
        let age = now - req.time;
        if !(-5..=REQUEST_MAX_AGE).contains(&age) || !valid_route(&req.route) {
            return None;
        }
        Some(req.route)
    }
}

// ---------------------------------------------------------------------------------------------
// One menu bar agent at a time.

pub fn agent_pid_path(env: &Env) -> PathBuf {
    env.data_dir().join("agent.pid")
}

/// Claim the agent's place: refused when the process named in `agent.pid` is still a running
/// CleanCraft (checked with `ps -p <pid> -o comm=`). Otherwise `pid` is written there.
pub fn claim_agent(env: &Env, pid: u32) -> Result<(), String> {
    if let Ok(old) = std::fs::read_to_string(agent_pid_path(env))
        && let Ok(old) = old.trim().parse::<u32>()
        && old != pid
        && is_cleancraft(env, old)
    {
        return Err(format!(
            "The CleanCraft menu bar agent is already running (process {old})."
        ));
    }
    std::fs::create_dir_all(env.data_dir()).map_err(|e| e.to_string())?;
    std::fs::write(agent_pid_path(env), pid.to_string()).map_err(|e| e.to_string())
}

fn is_cleancraft(env: &Env, pid: u32) -> bool {
    let pid = pid.to_string();
    match env.sys.run("/bin/ps", &["-p", &pid, "-o", "comm="]) {
        Ok(out) if out.ok() => out.stdout.to_lowercase().contains("cleancraft"),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_texts_are_arguments_never_script() {
        let n = Notification {
            title: r#"x" & do shell script "touch /tmp/p" & ""#.into(),
            subtitle: None,
            body: "two\nlines".into(),
            route: None,
        };
        let a = osascript_args(&n);
        assert_eq!(
            a,
            [
                "-e",
                "on run argv",
                "-e",
                "display notification (item 2 of argv) with title (item 1 of argv)",
                "-e",
                "end run",
                "--",
                r#"x" & do shell script "touch /tmp/p" & ""#,
                "two lines",
            ]
        );
        assert_eq!(clean_text(&"y".repeat(500)).chars().count(), TEXT_MAX);
    }

    #[test]
    fn every_routed_body_ends_with_the_menu_bar_hint() {
        let mut n = Notification {
            title: "Viewer was moved to the Trash".into(),
            subtitle: None,
            body: "4 items it left behind use 1 MB.".into(),
            route: Some("uninstall:/Users/a/.Trash/Viewer.app".into()),
        };
        assert_eq!(
            shown_body(&n),
            format!("4 items it left behind use 1 MB. {MENU_HINT}")
        );
        // Once only, and it survives a long body.
        n.body = format!("x {MENU_HINT}");
        assert_eq!(shown_body(&n), format!("x {MENU_HINT}"));
        n.body = "z".repeat(600);
        let b = shown_body(&n);
        assert!(
            b.ends_with(MENU_HINT) && b.chars().count() <= TEXT_MAX,
            "{b}"
        );
        assert_eq!(osascript_args(&n)[8], b);
        // Nothing to open: no hint.
        n.route = None;
        n.body = "Done.".into();
        assert_eq!(shown_body(&n), "Done.");
    }

    #[test]
    fn urls_give_routes() {
        for (url, want) in [
            ("cleancraft://open/quick_sweep", Some("quick_sweep")),
            ("cleancraft://quick-sweep", Some("quick-sweep")),
            ("cleancraft:settings", Some("settings")),
            (
                "CLEANCRAFT://open/scan:quick_sweep/",
                Some("scan:quick_sweep"),
            ),
            ("cleancraft://open?page=history", Some("history")),
            (
                "cleancraft://open/uninstall:/Applications/My%20App.app",
                Some("uninstall:/Applications/My App.app"),
            ),
            ("cleancraft://open", Some("")),
            ("cleancraft://opensomething", Some("opensomething")),
            ("https://example.com", None),
            ("cleancraft://bad%zz", None),
            ("cleancraft://line%0Abreak", None),
        ] {
            assert_eq!(route_from_url(url).as_deref(), want, "{url}");
        }
    }
}
