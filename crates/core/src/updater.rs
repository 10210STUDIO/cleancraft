//! App Updates (A8-A10). CleanCraft only looks and hands off; it never downloads or installs
//! software itself.
//!
//! - Sparkle apps: the appcast at `SUFeedURL` (Info.plist) is fetched with `curl`, the newest
//!   item is compared with the installed `CFBundleVersion` / `CFBundleShortVersionString`, and
//!   its release notes are shown. Update opens the app (its own updater does the work) or the
//!   download page.
//! - App Store apps: the public lookup API gives the store version; Update opens the App Store
//!   updates page.
//! - macOS: `softwareupdate --list`; the button opens Software Update in System Settings.
//!
//! Network failures are reported per app and never stop the check of the others.

use std::cmp::Ordering;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::apps::{AppInfo, Store};
use crate::env::Env;

pub const APP_STORE_UPDATES_URL: &str = "macappstore://showUpdatesPage";
pub const SOFTWARE_UPDATE_URL: &str =
    "x-apple.systempreferences:com.apple.Software-Update-Settings.extension";
/// Longest release notes kept, in characters.
const NOTES_MAX: usize = 4000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Sparkle,
    AppStore,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            // Plain words (REVIEW-U U15).
            Source::Sparkle => "From the developer",
            Source::AppStore => "App Store",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpdateInfo {
    pub app: PathBuf,
    pub name: String,
    pub bundle_id: String,
    pub source: Source,
    pub installed: String,
    /// Newest version found, when the check worked.
    pub latest: Option<String>,
    pub available: bool,
    /// Release notes as plain text.
    pub notes: String,
    /// Sparkle: the newest item's download address.
    pub download_url: Option<String>,
    /// Why the check failed for this app.
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct MacUpdate {
    pub label: String,
    pub title: String,
    pub version: String,
    pub size_bytes: Option<u64>,
    pub recommended: bool,
    pub restart: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpdatesReport {
    pub apps: Vec<UpdateInfo>,
    pub macos: Vec<MacUpdate>,
    pub macos_error: Option<String>,
    /// Apps with no update channel CleanCraft can read.
    pub not_checked: usize,
}

impl UpdatesReport {
    pub fn available(&self) -> impl Iterator<Item = &UpdateInfo> {
        self.apps.iter().filter(|u| u.available)
    }
}

// ---------- versions ----------

#[derive(Debug, PartialEq, Eq)]
enum Tok {
    Num(u64),
    Word(String),
}

fn tokens(v: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut digits = false;
    let flush = |cur: &mut String, digits: bool, out: &mut Vec<Tok>| {
        if cur.is_empty() {
            return;
        }
        if digits {
            out.push(Tok::Num(cur.parse().unwrap_or(u64::MAX)));
        } else {
            out.push(Tok::Word(cur.to_lowercase()));
        }
        cur.clear();
    };
    for c in v.trim().chars() {
        if c.is_ascii_digit() {
            if !digits {
                flush(&mut cur, digits, &mut out);
                digits = true;
            }
            cur.push(c);
        } else if c.is_alphabetic() {
            if digits {
                flush(&mut cur, digits, &mut out);
                digits = false;
            }
            cur.push(c);
        } else {
            flush(&mut cur, digits, &mut out);
        }
    }
    flush(&mut cur, digits, &mut out);
    // "v3.1" is "3.1".
    if matches!(out.first(), Some(Tok::Word(w)) if w == "v") {
        out.remove(0);
    }
    out
}

/// Compare two version strings by segments: numbers as numbers ("1.10" > "1.9"), missing
/// zero segments equal ("1.2" = "1.2.0"), a letter segment after the common part marks a
/// pre-release ("1.0b2" < "1.0"), and a number beats a letter at the same place.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let (ta, tb) = (tokens(a), tokens(b));
    let n = ta.len().max(tb.len());
    for i in 0..n {
        let o = match (ta.get(i), tb.get(i)) {
            (Some(Tok::Num(x)), Some(Tok::Num(y))) => x.cmp(y),
            (Some(Tok::Word(x)), Some(Tok::Word(y))) => x.cmp(y),
            (Some(Tok::Num(_)), Some(Tok::Word(_))) => Ordering::Greater,
            (Some(Tok::Word(_)), Some(Tok::Num(_))) => Ordering::Less,
            (Some(Tok::Num(x)), None) => {
                if *x == 0 {
                    Ordering::Equal
                } else {
                    Ordering::Greater
                }
            }
            (None, Some(Tok::Num(y))) => {
                if *y == 0 {
                    Ordering::Equal
                } else {
                    Ordering::Less
                }
            }
            (Some(Tok::Word(_)), None) => Ordering::Less,
            (None, Some(Tok::Word(_))) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    Ordering::Equal
}

// ---------- appcast ----------

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AppcastItem {
    pub title: String,
    /// `sparkle:version` (build number).
    pub version: String,
    /// `sparkle:shortVersionString`.
    pub short_version: String,
    pub description: String,
    pub notes_link: Option<String>,
    pub download_url: Option<String>,
    /// Beta and other channels are skipped.
    pub channel: Option<String>,
}

impl AppcastItem {
    fn key(&self) -> &str {
        if self.version.is_empty() {
            &self.short_version
        } else {
            &self.version
        }
    }

    pub fn shown_version(&self) -> &str {
        if self.short_version.is_empty() {
            &self.version
        } else {
            &self.short_version
        }
    }
}

/// Decode the five XML entities and numeric references.
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let after = &rest[i..];
        let Some(end) = after.find(';').filter(|e| *e <= 10) else {
            out.push('&');
            rest = &after[1..];
            continue;
        };
        let ent = &after[1..end];
        let rep = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => ent
                .strip_prefix("#x")
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match rep {
            Some(c) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = &after[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Start of the opening tag `<name` followed by `>`, `/` or white space.
fn find_open(s: &str, name: &str) -> Option<usize> {
    let pat = format!("<{name}");
    let mut from = 0;
    while let Some(i) = s[from..].find(&pat) {
        let at = from + i;
        let next = s[at + pat.len()..].chars().next();
        if matches!(next, Some('>' | '/' | ' ' | '\t' | '\n' | '\r')) {
            return Some(at);
        }
        from = at + pat.len();
    }
    None
}

/// Text of the first `<name>...</name>` element, CDATA kept as is, entities decoded.
fn element_text(s: &str, name: &str) -> Option<String> {
    let start = find_open(s, name)?;
    let gt = start + s[start..].find('>')?;
    if s[..gt].ends_with('/') {
        return Some(String::new());
    }
    let close = format!("</{name}>");
    let end = gt + 1 + s[gt + 1..].find(&close)?;
    let inner = s[gt + 1..end].trim();
    Some(cdata_or_text(inner))
}

fn cdata_or_text(inner: &str) -> String {
    if inner.contains("<![CDATA[") {
        let mut out = String::new();
        let mut rest = inner;
        while let Some(i) = rest.find("<![CDATA[") {
            out.push_str(&decode_entities(&rest[..i]));
            let body = &rest[i + 9..];
            match body.find("]]>") {
                Some(e) => {
                    out.push_str(&body[..e]);
                    rest = &body[e + 3..];
                }
                None => {
                    out.push_str(body);
                    rest = "";
                }
            }
        }
        out.push_str(&decode_entities(rest));
        out
    } else {
        decode_entities(inner)
    }
}

/// Attributes of the first `<name ...>` tag.
fn attributes(s: &str, name: &str) -> Vec<(String, String)> {
    let Some(start) = find_open(s, name) else {
        return Vec::new();
    };
    let Some(len) = s[start..].find('>') else {
        return Vec::new();
    };
    let tag = &s[start + name.len() + 1..start + len];
    let mut out = Vec::new();
    let mut rest = tag;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            break;
        };
        let Some(end) = after[1..].find(q) else {
            break;
        };
        out.push((key, decode_entities(&after[1..1 + end])));
        rest = &after[end + 2..];
    }
    out
}

/// The `<item>`s of a Sparkle appcast.
pub fn parse_appcast(xml: &str) -> Vec<AppcastItem> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = find_open(rest, "item") {
        let body_start = start + 5;
        let Some(end) = rest[body_start..].find("</item>") else {
            break;
        };
        let body = &rest[body_start..body_start + end];
        rest = &rest[body_start + end + 7..];
        let attrs = attributes(body, "enclosure");
        let attr = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let el = |k: &str| element_text(body, k).filter(|t| !t.is_empty());
        out.push(AppcastItem {
            title: el("title").unwrap_or_default(),
            version: el("sparkle:version")
                .or_else(|| attr("sparkle:version"))
                .unwrap_or_default(),
            short_version: el("sparkle:shortVersionString")
                .or_else(|| attr("sparkle:shortVersionString"))
                .unwrap_or_default(),
            description: el("description").unwrap_or_default(),
            notes_link: el("sparkle:releaseNotesLink"),
            download_url: attr("url").or_else(|| el("link")),
            channel: el("sparkle:channel"),
        });
    }
    out
}

/// The newest default-channel item.
pub fn newest(items: &[AppcastItem]) -> Option<&AppcastItem> {
    items
        .iter()
        .filter(|i| i.channel.is_none() && !i.key().is_empty())
        .max_by(|a, b| compare_versions(a.key(), b.key()))
}

/// Is `item` newer than the installed app? Like Sparkle: the build numbers decide when both
/// are known, and a lower marketing version is never offered.
pub fn is_newer(item: &AppcastItem, build: &str, short: &str) -> bool {
    let short_ok = item.short_version.is_empty()
        || short.is_empty()
        || compare_versions(&item.short_version, short) != Ordering::Less;
    if !item.version.is_empty() && !build.is_empty() {
        return short_ok && compare_versions(&item.version, build) == Ordering::Greater;
    }
    !item.short_version.is_empty()
        && !short.is_empty()
        && compare_versions(&item.short_version, short) == Ordering::Greater
}

/// Plain text from HTML release notes: tags removed, list items as "- ", paragraphs kept.
pub fn strip_html(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    let mut skip_until: Option<&str> = None;
    while !rest.is_empty() {
        if let Some(end_tag) = skip_until {
            match rest.to_ascii_lowercase().find(end_tag) {
                Some(i) => {
                    rest = &rest[i..];
                    skip_until = None;
                }
                None => break,
            }
        }
        match rest.find('<') {
            Some(i) => {
                out.push_str(&rest[..i]);
                let after = &rest[i..];
                let Some(gt) = after.find('>') else {
                    out.push_str(after);
                    break;
                };
                let tag = after[1..gt].trim().to_lowercase();
                let name: String = tag
                    .trim_start_matches('/')
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric())
                    .collect();
                let closing = tag.starts_with('/');
                match name.as_str() {
                    "br" => out.push('\n'),
                    "li" if !closing => out.push_str("\n- "),
                    "p" | "div" | "ul" | "ol" | "h1" | "h2" | "h3" | "h4" | "tr" => out.push('\n'),
                    "script" | "style" if !closing => {
                        skip_until = Some(if name == "script" {
                            "</script"
                        } else {
                            "</style"
                        })
                    }
                    _ => {}
                }
                rest = &after[gt + 1..];
            }
            None => {
                out.push_str(rest);
                break;
            }
        }
    }
    let text = decode_entities(&out);
    let mut lines: Vec<String> = Vec::new();
    for l in text.lines() {
        let l = l.split_whitespace().collect::<Vec<_>>().join(" ");
        if l.is_empty() || l == "-" {
            if lines.last().is_some_and(|p| !p.is_empty()) {
                lines.push(String::new());
            }
        } else {
            lines.push(l);
        }
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let mut s = lines.join("\n");
    if s.chars().count() > NOTES_MAX {
        s = s.chars().take(NOTES_MAX).collect::<String>() + "...";
    }
    s
}

// ---------- App Store ----------

pub fn lookup_url(bundle_id: &str) -> String {
    format!("https://itunes.apple.com/lookup?bundleId={bundle_id}&entity=macSoftware")
}

/// (version, release notes) from the lookup API answer.
pub fn parse_lookup(json: &str) -> Result<(String, String), String> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|_| "the App Store gave an answer CleanCraft could not read".to_string())?;
    let r = v
        .get("results")
        .and_then(|r| r.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| "not found in the App Store".to_string())?;
    let ver = r
        .get("version")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "the App Store did not give a version".to_string())?;
    let notes = r
        .get("releaseNotes")
        .and_then(|x| x.as_str())
        .unwrap_or_default();
    Ok((ver.to_string(), notes.trim().to_string()))
}

// ---------- macOS ----------

/// Parse `softwareupdate --list`.
pub fn parse_softwareupdate(out: &str) -> Vec<MacUpdate> {
    let mut v: Vec<MacUpdate> = Vec::new();
    for line in out.lines() {
        let t = line.trim();
        if let Some(label) = t.strip_prefix("* Label:") {
            v.push(MacUpdate {
                label: label.trim().to_string(),
                title: label.trim().to_string(),
                version: String::new(),
                size_bytes: None,
                recommended: false,
                restart: false,
            });
        } else if t.starts_with("Title:")
            && let Some(cur) = v.last_mut()
        {
            for field in t.split(',') {
                let Some((k, val)) = field.split_once(':') else {
                    continue;
                };
                let val = val.trim();
                match k.trim() {
                    "Title" => cur.title = val.to_string(),
                    "Version" => cur.version = val.to_string(),
                    "Size" => {
                        cur.size_bytes = val
                            .trim_end_matches("KiB")
                            .trim_end_matches('K')
                            .trim()
                            .parse::<u64>()
                            .ok()
                            .map(|k| k * 1024)
                    }
                    "Recommended" => cur.recommended = val.eq_ignore_ascii_case("yes"),
                    "Action" => cur.restart = val.eq_ignore_ascii_case("restart"),
                    _ => {}
                }
            }
        }
    }
    v
}

// ---------- fetching ----------

/// Plain words for a failed `curl -fsSL`.
pub fn curl_error(status: i32, stderr: &str) -> String {
    match status {
        6 => "the update server could not be found (no internet?)".into(),
        7 => "could not connect to the update server".into(),
        22 => "the update server answered with an error".into(),
        28 => "the update server did not answer in time".into(),
        35 | 51 | 60 => "the update server's secure connection failed".into(),
        _ => {
            let e = stderr.trim();
            if e.is_empty() {
                format!("download failed (code {status})")
            } else {
                e.to_string()
            }
        }
    }
}

/// GET `url` through `curl` (so tests can answer it).
pub fn fetch(env: &Env, url: &str) -> Result<String, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("the update address is not a web address".into());
    }
    match env
        .sys
        .run("/usr/bin/curl", &["-fsSL", "--max-time", "15", url])
    {
        Ok(o) if o.ok() => Ok(o.stdout),
        Ok(o) => Err(curl_error(o.status, &o.stderr)),
        Err(e) => Err(format!("could not run curl: {e}")),
    }
}

fn base_info(app: &AppInfo, source: Source) -> UpdateInfo {
    UpdateInfo {
        app: app.path.clone(),
        name: app.name.clone(),
        bundle_id: app.bundle_id.clone(),
        source,
        installed: if app.version.is_empty() {
            app.build.clone()
        } else {
            app.version.clone()
        },
        latest: None,
        available: false,
        notes: String::new(),
        download_url: None,
        error: None,
    }
}

/// Check one app. None when it has no update channel CleanCraft reads (or is part of macOS).
pub fn check_app(env: &Env, app: &AppInfo) -> Option<UpdateInfo> {
    if app.path.starts_with(env.sys_path("/System")) {
        return None;
    }
    if app.store == Store::AppStore {
        let mut u = base_info(app, Source::AppStore);
        match fetch(env, &lookup_url(&app.bundle_id)).and_then(|j| parse_lookup(&j)) {
            Ok((ver, notes)) => {
                u.available = compare_versions(&ver, &u.installed) == Ordering::Greater;
                u.latest = Some(ver);
                u.notes = notes;
            }
            Err(e) => u.error = Some(e),
        }
        return Some(u);
    }
    if app.apple {
        return None;
    }
    let feed = app.feed_url.as_ref()?;
    let mut u = base_info(app, Source::Sparkle);
    let xml = match fetch(env, feed) {
        Ok(x) => x,
        Err(e) => {
            u.error = Some(e);
            return Some(u);
        }
    };
    let items = parse_appcast(&xml);
    let Some(item) = newest(&items) else {
        u.error = Some("the update feed lists no versions".into());
        return Some(u);
    };
    u.latest = Some(item.shown_version().to_string());
    u.available = is_newer(item, &app.build, &app.version);
    // Same version, newer build (uTorrent Web 1.6.0 build 6413 then 6575): show the builds.
    if u.available
        && u.latest.as_deref() == Some(u.installed.as_str())
        && !app.build.is_empty()
        && !item.version.is_empty()
    {
        u.installed = format!("{} ({})", u.installed, app.build);
        u.latest = Some(format!("{} ({})", item.shown_version(), item.version));
    }
    u.download_url = item.download_url.clone();
    u.notes = if !item.description.trim().is_empty() {
        strip_html(&item.description)
    } else if let Some(link) = &item.notes_link {
        match fetch(env, link) {
            Ok(html) => strip_html(&html),
            Err(_) => format!("Release notes: {link}"),
        }
    } else {
        String::new()
    };
    Some(u)
}

/// Quick Sweep's app updates tile: how many apps have a newer version, by the same checks as
/// the App Updates page (macOS updates are not counted here). When no app could be checked
/// at all, the error says why instead of a zero.
pub fn available_count(env: &Env, _settings: &crate::settings::Settings) -> anyhow::Result<usize> {
    let apps = crate::apps::inventory(env, false, &|_| {});
    let checked: Vec<UpdateInfo> = apps.iter().filter_map(|a| check_app(env, a)).collect();
    if !checked.is_empty() && checked.iter().all(|u| u.error.is_some()) {
        let why = checked[0].error.clone().unwrap_or_default();
        anyhow::bail!("no app could be checked: {why}");
    }
    Ok(checked.iter().filter(|u| u.available).count())
}

/// Check every app and macOS.
pub fn check_all(env: &Env, apps: &[AppInfo], progress: &dyn Fn(&str)) -> UpdatesReport {
    check_all_metered(env, apps, progress, &crate::progress::Progress::new())
}

/// [`check_all`] reporting into `meter`: apps checked of the apps listed, then macOS (one
/// more step).
pub fn check_all_metered(
    env: &Env,
    apps: &[AppInfo],
    progress: &dyn Fn(&str),
    meter: &crate::progress::Progress,
) -> UpdatesReport {
    let mut r = UpdatesReport::default();
    let steps = apps.len() as u64 + 1;
    for (k, a) in apps.iter().enumerate() {
        meter.set(k as u64, steps);
        progress(&a.name);
        match check_app(env, a) {
            Some(u) => r.apps.push(u),
            None => {
                if !a.apple {
                    r.not_checked += 1
                }
            }
        }
    }
    r.apps.sort_by(|a, b| {
        b.available
            .cmp(&a.available)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    meter.set(steps - 1, steps);
    progress("macOS");
    match env.sys.run("/usr/sbin/softwareupdate", &["--list"]) {
        Ok(o) if o.ok() => r.macos = parse_softwareupdate(&o.stdout),
        Ok(o) => {
            r.macos_error = Some(format!(
                "Software Update could not check: {}",
                o.stderr.trim()
            ))
        }
        Err(e) => r.macos_error = Some(format!("Software Update could not check: {e}")),
    }
    r
}

/// Hand an update to the right place: the App Store updates page, or the app itself (its
/// own updater). Returns what the user should do next.
pub fn start_update(env: &Env, u: &UpdateInfo) -> std::io::Result<String> {
    match u.source {
        Source::AppStore => {
            env.sys.open_url(APP_STORE_UPDATES_URL)?;
            Ok("The App Store opened on its Updates page.".into())
        }
        Source::Sparkle => {
            let o = env.sys.run("/usr/bin/open", &["-b", &u.bundle_id])?;
            if !o.ok() {
                return Err(std::io::Error::other(o.stderr.trim().to_string()));
            }
            Ok(format!(
                "{} is open. Choose Check for Updates in its app menu to install {}.",
                u.name,
                u.latest.clone().unwrap_or_default()
            ))
        }
    }
}

/// Open the newest version's download in the browser.
pub fn open_download(env: &Env, u: &UpdateInfo) -> std::io::Result<()> {
    match &u.download_url {
        Some(url) => env.sys.open_url(url),
        None => Err(std::io::Error::other("no download address")),
    }
}

pub fn open_software_update(env: &Env) -> std::io::Result<()> {
    env.sys.open_url(SOFTWARE_UPDATE_URL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering::*;

    #[test]
    fn version_table() {
        let table = [
            ("1.10", "1.9", Greater),
            ("1.2", "1.2.0", Equal),
            ("1.2.0.1", "1.2", Greater),
            ("2.0", "10.0", Less),
            ("1.0b2", "1.0", Less),
            ("1.0", "1.0rc1", Greater),
            ("1.0b2", "1.0b10", Less),
            ("1.0.1", "1.0b1", Greater),
            ("1.0 (45)", "1.0 (44)", Greater),
            ("2024.3", "2024.10", Less),
            ("v3.1", "3.1", Equal),
            ("", "", Equal),
        ];
        for (a, b, want) in table {
            assert_eq!(compare_versions(a, b), want, "{a} vs {b}");
            assert_eq!(compare_versions(b, a), want.reverse(), "{b} vs {a}");
        }
    }

    const CAST: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
<channel><title>Example</title>
<item><title>Version 2.1</title>
<sparkle:version>210</sparkle:version><sparkle:shortVersionString>2.1</sparkle:shortVersionString>
<description><![CDATA[<h2>New</h2><ul><li>Faster &amp; smaller</li><li>Dark mode</li></ul>]]></description>
<enclosure url="https://example.com/Example-2.1.zip" length="100" type="application/octet-stream" sparkle:edSignature="x"/>
</item>
<item><title>Version 2.2 beta</title><sparkle:channel>beta</sparkle:channel>
<enclosure url="https://example.com/b.zip" sparkle:version="220" sparkle:shortVersionString="2.2b1"/></item>
<item><title>Version 2.0</title>
<enclosure url='https://example.com/Example-2.0.zip' sparkle:version="200" sparkle:shortVersionString="2.0"/>
<sparkle:releaseNotesLink>https://example.com/notes/2.0.html</sparkle:releaseNotesLink>
</item></channel></rss>"#;

    #[test]
    fn appcast() {
        let items = parse_appcast(CAST);
        assert_eq!(items.len(), 3);
        assert_eq!(items[2].version, "200");
        assert_eq!(items[2].short_version, "2.0");
        assert_eq!(
            items[2].download_url.as_deref(),
            Some("https://example.com/Example-2.0.zip")
        );
        let n = newest(&items).unwrap();
        assert_eq!(n.short_version, "2.1", "beta channel skipped");
        assert!(is_newer(n, "200", "2.0"));
        assert!(!is_newer(n, "210", "2.1"));
        assert_eq!(
            strip_html(&n.description),
            "New\n\n- Faster & smaller\n- Dark mode"
        );
    }

    #[test]
    fn html_notes() {
        let s = strip_html(
            "<html><head><style>p{color:red}</style></head><body><p>Fixes</p><p>A&nbsp;bug&#33;<br>More</p></body></html>",
        );
        assert_eq!(s, "Fixes\n\nA bug!\nMore");
    }

    #[test]
    fn lookup() {
        let j = r#"{"resultCount":1,"results":[{"version":"3.4","releaseNotes":"Bug fixes"}]}"#;
        assert_eq!(parse_lookup(j), Ok(("3.4".into(), "Bug fixes".into())));
        assert!(parse_lookup(r#"{"resultCount":0,"results":[]}"#).is_err());
        assert!(parse_lookup("<html>").is_err());
    }

    #[test]
    fn softwareupdate_list() {
        let out = "Software Update Tool\n\nFinding available software\nSoftware Update found the following new or updated software:\n* Label: macOS Sonoma 14.5-23F79\n\tTitle: macOS Sonoma 14.5, Version: 14.5, Size: 6766793KiB, Recommended: YES, Action: restart, \n* Label: Command Line Tools for Xcode-15.3\n\tTitle: Command Line Tools for Xcode, Version: 15.3, Size: 707200KiB, Recommended: YES, \n";
        let v = parse_softwareupdate(out);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].title, "macOS Sonoma 14.5");
        assert_eq!(v[0].version, "14.5");
        assert_eq!(v[0].size_bytes, Some(6_766_793 * 1024));
        assert!(v[0].restart && v[0].recommended);
        assert!(!v[1].restart);
        assert!(parse_softwareupdate("No new software available.\n").is_empty());
    }
}
