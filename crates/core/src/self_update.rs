//! CleanCraft updates itself from the GitHub Releases of the public repository.
//!
//! - Check: `GET https://api.github.com/repos/cristianizzo/cleancraft/releases/latest` through
//!   `curl` (so tests answer it), the tag compared with the running version, the Apple silicon
//!   disk image and its `.sha256` picked from the assets, the release notes kept as text.
//! - When: the menu bar agent asks at most once every 24 hours; the app asks at launch with
//!   the same 24 hour limit (shared through `update-state.json`), so it also works with the
//!   agent off. "Check automatically" (on by default) turns both off.
//! - Install: the disk image goes to `~/Library/Caches/CleanCraft/updates/`, its SHA-256 must
//!   match the `.sha256` asset (refused when it differs or is missing), it is mounted read
//!   only, the app inside must have our bundle id, the release's version and a valid
//!   signature. A copy is made next to the installed app (same volume), checked again, then
//!   the installed app moves to the Trash (never deleted; History's Undo puts it back) and
//!   the copy takes its place. The menu bar agent is restarted when it runs; the app
//!   relaunches through a small detached helper that waits for it to quit.
//! - Never while a cleanup runs, never when the app's folder cannot be changed (the person
//!   is told what to do), never a version older than the running one ("You are ahead").

use std::cmp::Ordering;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::env::Env;
use crate::executor::{self, ActionKind, ActionRecord};
use crate::plan::FeatureId;
use crate::progress::Progress;
use crate::sys::shell_quote;
use crate::updater::{compare_versions, curl_error};

/// The repository the releases come from.
pub const REPO: &str = "cristianizzo/cleancraft";
pub const LATEST_URL: &str = "https://api.github.com/repos/cristianizzo/cleancraft/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/cristianizzo/cleancraft/releases";
/// The running version.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// The only architecture released (Apple silicon).
pub const ARCH: &str = "arm64";
/// Automatic checks happen at most this often (seconds).
pub const CHECK_EVERY_SECS: i64 = 24 * 3600;
/// The bundle id the new app must have.
pub const BUNDLE_ID: &str = crate::notify::BUNDLE_ID;

pub const CURL: &str = "/usr/bin/curl";
pub const HDIUTIL: &str = "/usr/bin/hdiutil";
pub const CODESIGN: &str = "/usr/bin/codesign";
pub const DITTO: &str = "/usr/bin/ditto";
pub const XATTR: &str = "/usr/bin/xattr";
pub const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

/// Longest release notes kept, in characters.
const NOTES_MAX: usize = 4000;
/// Time limits: the release list, the checksum file, the disk image.
const CHECK_TIMEOUT_SECS: &str = "20";
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// The four steps of an install, in order, as the progress bar names them.
pub const STEPS: [&str; 4] = ["Download", "Verify", "Install", "Relaunch"];

/// The words of the History line.
pub const HISTORY_TEXT: &str = "Updated CleanCraft";

// ---------------------------------------------------------------------------------------------
// The release.

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Release {
    /// The tag ("v0.1.3").
    pub tag: String,
    /// The version in the tag ("0.1.3").
    pub version: String,
    /// Release notes as plain text, trimmed.
    pub notes: String,
    /// The release's page on GitHub.
    pub page_url: String,
    /// The Apple silicon disk image.
    pub dmg_name: String,
    pub dmg_url: String,
    pub dmg_size: Option<u64>,
    /// Its `.sha256` file, when the release has one (install refuses without it).
    pub sha_url: Option<String>,
}

/// What a check found, compared with the running version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum Found {
    /// A newer release can be installed.
    Available { release: Release },
    /// The running version is the latest.
    UpToDate { latest: String },
    /// The running version is newer than the latest release (a build from source).
    Ahead { latest: String },
}

impl Found {
    pub fn latest(&self) -> &str {
        match self {
            Found::Available { release } => &release.version,
            Found::UpToDate { latest } | Found::Ahead { latest } => latest,
        }
    }

    /// One sentence for the person.
    pub fn message(&self, current: &str) -> String {
        match self {
            Found::Available { release } => format!(
                "CleanCraft {} is available. You have {current}.",
                release.version
            ),
            Found::UpToDate { .. } => format!("CleanCraft {current} is the latest version."),
            Found::Ahead { latest } => format!(
                "You are ahead of the latest release: this is CleanCraft {current}, the latest release is {latest}. Nothing to install."
            ),
        }
    }
}

/// A version taken from a tag: digits, letters, dots and dashes only (it ends up in file
/// names), at most 40 characters, and starting with a digit.
pub fn version_of_tag(tag: &str) -> Option<String> {
    let v = tag.trim();
    let v = v
        .strip_prefix('v')
        .or_else(|| v.strip_prefix('V'))
        .unwrap_or(v);
    let ok = !v.is_empty()
        && v.len() <= 40
        && v.starts_with(|c: char| c.is_ascii_digit())
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
    ok.then(|| v.to_string())
}

/// Release notes as shown: line ends made plain, trimmed, at most [`NOTES_MAX`] characters.
pub fn clean_notes(body: &str) -> String {
    let t = body.replace("\r\n", "\n").replace('\r', "\n");
    let t = t.trim();
    if t.chars().count() <= NOTES_MAX {
        return t.to_string();
    }
    let cut: String = t.chars().take(NOTES_MAX).collect();
    format!("{}...", cut.trim_end())
}

/// Read the GitHub "latest release" answer and pick the disk image for [`ARCH`].
pub fn parse_release(json: &str) -> Result<Release, String> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|_| "the update server sent something CleanCraft cannot read".to_string())?;
    let tag = v["tag_name"]
        .as_str()
        .ok_or("the latest release has no version tag")?
        .to_string();
    let version = version_of_tag(&tag)
        .ok_or_else(|| format!("the latest release tag {tag} is not a version"))?;
    let assets: Vec<(String, String, Option<u64>)> = v["assets"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some((
                        x["name"].as_str()?.to_string(),
                        x["browser_download_url"].as_str()?.to_string(),
                        x["size"].as_u64(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    let exact = format!("CleanCraft-{version}-{ARCH}.dmg");
    let dmg = assets
        .iter()
        .find(|(n, _, _)| *n == exact)
        .or_else(|| {
            assets
                .iter()
                .find(|(n, _, _)| n.ends_with(".dmg") && n.to_lowercase().contains(ARCH))
        })
        .ok_or_else(|| format!("release {version} has no Apple silicon download"))?;
    let sha_name = format!("{}.sha256", dmg.0);
    let sha_url = assets
        .iter()
        .find(|(n, _, _)| *n == sha_name)
        .map(|(_, u, _)| u.clone());
    for u in std::iter::once(&dmg.1).chain(sha_url.as_ref()) {
        if !u.starts_with("https://") {
            return Err("the release's download address is not secure".into());
        }
    }
    Ok(Release {
        tag,
        version,
        notes: clean_notes(v["body"].as_str().unwrap_or_default()),
        page_url: v["html_url"]
            .as_str()
            .filter(|u| u.starts_with("https://"))
            .unwrap_or(RELEASES_PAGE)
            .to_string(),
        dmg_name: dmg.0.clone(),
        dmg_url: dmg.1.clone(),
        dmg_size: dmg.2,
        sha_url,
    })
}

/// Compare a release with the running version.
pub fn compare(release: Release, current: &str) -> Found {
    match compare_versions(&release.version, current) {
        Ordering::Greater => Found::Available { release },
        Ordering::Equal => Found::UpToDate {
            latest: release.version,
        },
        Ordering::Less => Found::Ahead {
            latest: release.version,
        },
    }
}

fn user_agent() -> String {
    format!("User-Agent: CleanCraft/{CURRENT}")
}

/// The `curl` arguments of the release check (tests answer exactly these).
pub fn check_args() -> Vec<String> {
    vec![
        "-fsSL".into(),
        "--max-time".into(),
        CHECK_TIMEOUT_SECS.into(),
        "-H".into(),
        "Accept: application/vnd.github+json".into(),
        "-H".into(),
        user_agent(),
        LATEST_URL.into(),
    ]
}

/// The `curl` arguments that fetch a small text file (the checksum).
pub fn text_args(url: &str) -> Vec<String> {
    vec![
        "-fsSL".into(),
        "--max-time".into(),
        CHECK_TIMEOUT_SECS.into(),
        "-H".into(),
        user_agent(),
        url.into(),
    ]
}

/// The `curl` arguments that download the disk image to `to`.
pub fn download_args(url: &str, to: &Path) -> Vec<String> {
    vec![
        "-fsSL".into(),
        "--max-time".into(),
        DOWNLOAD_TIMEOUT.as_secs().to_string(),
        "-H".into(),
        user_agent(),
        "-o".into(),
        to.to_string_lossy().into_owned(),
        url.into(),
    ]
}

fn curl(env: &Env, args: &[String], limit: Duration) -> Result<String, String> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match env.sys.run_timeout(CURL, &refs, limit) {
        Ok(o) if o.ok() => Ok(o.stdout),
        Ok(o) => Err(curl_error(o.status, &o.stderr)),
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            Err("the update server did not answer in time".into())
        }
        Err(e) => Err(format!("could not run curl: {e}")),
    }
}

/// Ask GitHub for the latest release and compare it with `current`.
pub fn check_against(env: &Env, current: &str) -> Result<Found, String> {
    let body = curl(env, &check_args(), Duration::from_secs(30))?;
    Ok(compare(parse_release(&body)?, current))
}

/// [`check_against`] the running version.
pub fn check(env: &Env) -> Result<Found, String> {
    check_against(env, CURRENT)
}

/// A failed check in calm words.
pub fn check_failed_text(why: &str) -> String {
    format!("Could not check for updates ({why}). CleanCraft will try again later.")
}

// ---------------------------------------------------------------------------------------------
// What the last check found, shared by the app, the agent, the CLI and the MCP server.

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct UpdateState {
    /// When the last check ran (UTC seconds), worked or not.
    pub last_check: Option<i64>,
    /// What it found, when it worked.
    pub found: Option<Found>,
    /// The version that was running at that check.
    pub checked_from: String,
    /// Why the last check failed.
    pub error: Option<String>,
    /// The version the person was last notified about (one notification per version).
    pub notified: Option<String>,
}

impl UpdateState {
    pub fn path(env: &Env) -> PathBuf {
        env.data_dir().join("update-state.json")
    }

    pub fn load(env: &Env) -> UpdateState {
        std::fs::read(Self::path(env))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, env: &Env) -> std::io::Result<()> {
        std::fs::create_dir_all(env.data_dir())?;
        let tmp = Self::path(env).with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?,
        )?;
        std::fs::rename(tmp, Self::path(env))
    }

    /// Is an automatic check due at `now` (never checked, 24 hours passed, or the clock went
    /// back)?
    pub fn due(&self, now: i64) -> bool {
        match self.last_check {
            None => true,
            Some(t) => now < t || now - t >= CHECK_EVERY_SECS,
        }
    }

    /// The release newer than `current` the last check found.
    pub fn available(&self, current: &str) -> Option<&Release> {
        match &self.found {
            Some(Found::Available { release })
                if compare_versions(&release.version, current) == Ordering::Greater =>
            {
                Some(release)
            }
            _ => None,
        }
    }

    /// Note a check's result at `now`.
    pub fn record(&mut self, res: &Result<Found, String>, now: i64, current: &str) {
        self.last_check = Some(now);
        self.checked_from = current.to_string();
        match res {
            Ok(f) => {
                self.found = Some(f.clone());
                self.error = None;
            }
            Err(e) => self.error = Some(e.clone()),
        }
    }
}

/// Check now, note the result and return it.
pub fn check_and_record(env: &Env, now: i64) -> Result<Found, String> {
    let res = check(env);
    let mut st = UpdateState::load(env);
    st.record(&res, now, CURRENT);
    let _ = st.save(env);
    res
}

/// The last check time as words ("Never", "2026-10-28 10:42").
pub fn last_check_text(last: Option<i64>) -> String {
    match last.and_then(|t| chrono::DateTime::from_timestamp(t, 0)) {
        None => "Never".into(),
        Some(d) => d
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// Where CleanCraft is installed.

/// The outermost app bundle around `exe` (`/Applications/CleanCraft.app` for both the app's
/// executable and the menu bar helper inside it), `None` for a build outside a bundle.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .filter(|a| a.extension().is_some_and(|e| e == "app"))
        .last()
        .map(Path::to_path_buf)
}

/// The installed CleanCraft.app this process runs from. On a fake Mac (tests):
/// `/Applications/CleanCraft.app` inside it.
pub fn installed_app(env: &Env) -> Option<PathBuf> {
    if env.is_fake() {
        let p = env.sys_path("/Applications/CleanCraft.app");
        return p.is_dir().then_some(p);
    }
    let exe = std::env::current_exe().ok()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    bundle_of(&exe)
}

/// The main executable of an app bundle.
pub fn main_exe(app: &Path) -> PathBuf {
    app.join("Contents/MacOS/CleanCraft")
}

/// Is the app at `app` running (its main executable, not the menu bar helper)?
pub fn app_running(env: &Env, app: &Path) -> bool {
    let exe = main_exe(app);
    let me = std::process::id();
    env.sys
        .running_apps()
        .iter()
        .any(|r| r.pid != me && r.path.as_deref() == Some(exe.as_path()))
}

/// Why CleanCraft cannot replace the app at `app` by itself, with what to do instead.
pub fn not_writable_reason(env: &Env, app: &Path) -> Option<String> {
    let shown = env.display(app);
    if shown.contains("/AppTranslocation/") {
        return Some(
            "macOS runs this copy of CleanCraft from a temporary place. Move CleanCraft to your Applications folder in Finder, open it from there, then install the update.".into(),
        );
    }
    let parent = app.parent()?;
    if env.display(parent).starts_with("/Volumes/") && !env.can_modify(parent) {
        return Some(format!(
            "CleanCraft runs from a disk image or a read-only disk ({}). Drag it to your Applications folder, open it from there, then install the update.",
            env.display(parent)
        ));
    }
    if !env.can_modify(parent) || !env.can_modify(app) {
        return Some(format!(
            "CleanCraft cannot replace itself in {}: your account cannot change that folder. Download CleanCraft from {RELEASES_PAGE} and drag it to Applications, or ask an administrator to update it.",
            env.display(parent)
        ));
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Install.

/// Where downloads and the mount point go.
pub fn cache_dir(env: &Env) -> PathBuf {
    env.home_path("Library/Caches/CleanCraft/updates")
}

/// The downloaded disk image (one file, replaced by each download).
pub fn dmg_path(env: &Env) -> PathBuf {
    cache_dir(env).join("CleanCraft-update.dmg")
}

/// Where the disk image is mounted.
pub fn mount_dir(env: &Env) -> PathBuf {
    cache_dir(env).join("mount")
}

/// The staged copy next to the installed app (same folder, so the swap is a rename).
pub fn staging_path(app: &Path, version: &str) -> PathBuf {
    let dir = app.parent().unwrap_or(Path::new("/"));
    executor::free_name(dir, &format!(".CleanCraft-{version}-update.app"), true)
}

/// What the install needs to know about the app around it.
#[derive(Clone, Debug)]
pub struct Target {
    /// The installed app bundle to replace.
    pub app: PathBuf,
    /// The running version.
    pub current: String,
    /// Why it must not install now (a cleanup is running), when that is so.
    pub busy: Option<String>,
}

/// What an install did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub from: String,
    pub to: String,
    /// The installed app (now the new version).
    pub app: PathBuf,
    /// Where the old version went (the Trash).
    pub old_in_trash: PathBuf,
    /// The History line (with Undo).
    pub record: String,
    /// The menu bar agent was restarted.
    pub agent_restarted: bool,
    /// Something that did not work but did not stop the update.
    pub note: Option<String>,
}

impl Installed {
    pub fn message(&self) -> String {
        format!(
            "CleanCraft {} is installed. The previous version ({}) is in the Trash; History can put it back.",
            self.to, self.from
        )
    }
}

/// The SHA-256 written in a `shasum -a 256` file: the first word, 64 hex digits.
pub fn parse_sha(text: &str) -> Option<String> {
    let w = text.split_whitespace().next()?.to_lowercase();
    (w.len() == 64 && w.chars().all(|c| c.is_ascii_hexdigit())).then_some(w)
}

/// SHA-256 of a file, in lowercase hex.
pub fn sha256_file(p: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// The bundle id and short version in an app's Info.plist.
pub fn bundle_info(app: &Path) -> Option<(String, String)> {
    let v = plist::Value::from_file(app.join("Contents/Info.plist")).ok()?;
    let d = v.as_dictionary()?;
    let id = d.get("CFBundleIdentifier")?.as_string()?.to_string();
    let ver = d
        .get("CFBundleShortVersionString")
        .and_then(|x| x.as_string())
        .unwrap_or_default()
        .to_string();
    Some((id, ver))
}

/// The app must be CleanCraft, at the release's version, with a valid signature.
pub fn verify_bundle(env: &Env, app: &Path, version: &str) -> Result<(), String> {
    let Some((id, ver)) = bundle_info(app) else {
        return Err("the disk image does not hold a CleanCraft app".into());
    };
    if id != BUNDLE_ID {
        return Err(format!(
            "the app in the disk image is not CleanCraft (its id is {id})"
        ));
    }
    if compare_versions(&ver, version) != Ordering::Equal {
        return Err(format!(
            "the app in the disk image is version {ver}, the release says {version}"
        ));
    }
    let p = app.to_string_lossy();
    match env
        .sys
        .run(CODESIGN, &["--verify", "--deep", "--strict", &p])
    {
        Ok(o) if o.ok() => Ok(()),
        Ok(o) => Err(format!(
            "the app's signature is not valid: {}",
            crate::tasks::first_lines(&o.stderr, 1)
        )),
        Err(e) => Err(format!("could not check the app's signature: {e}")),
    }
}

fn mkdir(p: &Path) -> Result<(), String> {
    std::fs::create_dir_all(p).map_err(|e| format!("could not create {}: {e}", p.display()))
}

/// Download, verify, mount, check and swap. `meter` gets the first three of [`STEPS`] (the
/// fourth, Relaunch, is the caller's). On any error nothing of the installed app changed.
pub fn install(
    env: &Env,
    release: &Release,
    target: &Target,
    meter: &Progress,
) -> Result<Installed, String> {
    if let Some(b) = &target.busy {
        return Err(format!(
            "{b} Wait for it to finish, then install the update."
        ));
    }
    if compare_versions(&release.version, &target.current) != Ordering::Greater {
        return Err(format!(
            "CleanCraft {} is not newer than {}, so there is nothing to install.",
            release.version, target.current
        ));
    }
    if let Some(why) = not_writable_reason(env, &target.app) {
        return Err(why);
    }
    let Some(sha_url) = &release.sha_url else {
        return Err(format!(
            "Release {} has no checksum file, so CleanCraft will not install it.",
            release.version
        ));
    };
    crate::tasks::journal_writable(env)
        .map_err(|e| format!("Could not write the action log, nothing changed: {e}"))?;

    // 1. Download.
    let dl = meter.span(0, STEPS.len());
    dl.set_step(&format!("Downloading CleanCraft {}", release.version));
    let dir = cache_dir(env);
    mkdir(&dir)?;
    let expected = curl(env, &text_args(sha_url), Duration::from_secs(30))
        .map_err(|e| format!("Could not download the checksum: {e}."))
        .and_then(|t| {
            parse_sha(&t).ok_or_else(|| {
                "The checksum file of the release cannot be read, so nothing was installed."
                    .to_string()
            })
        })?;
    let part = dir.join("CleanCraft-update.dmg.part");
    download(env, release, &part, &dl).map_err(|e| format!("The download failed: {e}."))?;
    let dmg = dmg_path(env);
    std::fs::rename(&part, &dmg).map_err(|e| format!("Could not keep the download: {e}"))?;
    dl.complete();

    // 2. Verify.
    let vf = meter.span(1, STEPS.len());
    vf.set_step("Checking the download");
    let got = sha256_file(&dmg).map_err(|e| format!("Could not read the download: {e}"))?;
    if got != expected {
        return Err(
            "The download does not match its checksum, so it was not installed. Nothing changed."
                .into(),
        );
    }
    vf.set_fraction(0.3);
    let mnt = mount_dir(env);
    mkdir(&mnt)?;
    let mnt_s = mnt.to_string_lossy().into_owned();
    let dmg_s = dmg.to_string_lossy().into_owned();
    match env.sys.run(
        HDIUTIL,
        &[
            "attach",
            "-nobrowse",
            "-readonly",
            "-noautoopen",
            "-mountpoint",
            &mnt_s,
            &dmg_s,
        ],
    ) {
        Ok(o) if o.ok() => {}
        Ok(o) => {
            return Err(format!(
                "Could not open the disk image: {}",
                crate::tasks::first_lines(&o.stderr, 1)
            ));
        }
        Err(e) => return Err(format!("Could not open the disk image: {e}")),
    }
    // From here the image is detached whatever happens.
    let staged = stage(env, release, target, &mnt, &vf);
    let detach = env.sys.run(HDIUTIL, &["detach", &mnt_s, "-quiet"]);
    let mut note = match detach {
        Ok(o) if o.ok() => None,
        _ => Some(format!(
            "The update disk image is still open at {}; eject it in Finder.",
            env.display(&mnt)
        )),
    };
    let staging = staged?;
    vf.complete();

    // 3. Install: the old app to the Trash, the copy in its place.
    let ins = meter.span(2, STEPS.len());
    ins.set_step(&format!("Installing CleanCraft {}", release.version));
    let app = &target.app;
    let trash = executor::trash_dir_for(env, app);
    if let Err(e) = mkdir(&trash) {
        put_aside(env, &staging);
        return Err(e);
    }
    let old = executor::free_name(&trash, &format!("CleanCraft {}.app", target.current), true);
    if let Err(e) = std::fs::rename(app, &old) {
        put_aside(env, &staging);
        return Err(format!(
            "Could not move the current CleanCraft to the Trash ({e}). Nothing changed."
        ));
    }
    if let Err(e) = crate::fsutil::rename_no_replace(&staging, app) {
        // Put the old one back where it was.
        let back = crate::fsutil::rename_no_replace(&old, app);
        put_aside(env, &staging);
        return Err(match back {
            Ok(()) => format!("Could not put the new version in place ({e}). Nothing changed."),
            Err(b) => format!(
                "Could not put the new version in place ({e}), and the previous one could not go back ({b}). It is in the Trash at {}: drag it back to {}.",
                env.display(&old),
                env.display(app.parent().unwrap_or(Path::new("/")))
            ),
        });
    }
    ins.set_fraction(0.6);
    // Launch Services learns the new version at once (failure is harmless).
    let _ = env.sys.run(LSREGISTER, &["-f", &app.to_string_lossy()]);
    let agent_on = crate::login_agent::is_on(env);
    let record = journal(env, target, release, app, &old, agent_on)
        .map_err(|e| format!("CleanCraft {} is installed, but {e}", release.version))?;
    let mut agent_restarted = false;
    if agent_on {
        let target = format!(
            "gui/{}/{}",
            crate::features::trash::uid(env),
            crate::login_agent::LABEL
        );
        match env
            .sys
            .run(crate::login_agent::LAUNCHCTL, &["kickstart", "-k", &target])
        {
            Ok(o) if o.ok() => agent_restarted = true,
            _ => {
                note.get_or_insert_with(|| {
                    "The menu bar agent runs the new version from your next login.".into()
                });
            }
        }
    }
    ins.complete();
    Ok(Installed {
        from: target.current.clone(),
        to: release.version.clone(),
        app: app.clone(),
        old_in_trash: old,
        record,
        agent_restarted,
        note,
    })
}

/// curl writes the disk image; the bar follows the file's size when the release gave it.
fn download(env: &Env, release: &Release, part: &Path, meter: &Progress) -> Result<(), String> {
    let args = download_args(&release.dmg_url, part);
    let done = std::sync::atomic::AtomicBool::new(false);
    std::thread::scope(|s| {
        if let Some(total) = release.dmg_size.filter(|t| *t > 0) {
            let done = &done;
            s.spawn(move || {
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    if let Ok(md) = std::fs::metadata(part) {
                        meter.set(md.len(), total);
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
            });
        }
        let r = curl(env, &args, DOWNLOAD_TIMEOUT + Duration::from_secs(30));
        done.store(true, std::sync::atomic::Ordering::Relaxed);
        r
    })?;
    if !part.is_file() {
        return Err("nothing was downloaded".into());
    }
    Ok(())
}

/// Check the app in the mounted image, copy it next to the installed one and check the copy.
fn stage(
    env: &Env,
    release: &Release,
    target: &Target,
    mnt: &Path,
    meter: &Progress,
) -> Result<PathBuf, String> {
    let src = mnt.join("CleanCraft.app");
    verify_bundle(env, &src, &release.version)
        .map_err(|e| format!("The update was refused: {e}. Nothing changed."))?;
    meter.set_fraction(0.6);
    let staging = staging_path(&target.app, &release.version);
    let (a, b) = (
        src.to_string_lossy().into_owned(),
        staging.to_string_lossy().into_owned(),
    );
    match env
        .sys
        .run_timeout(DITTO, &[&a, &b], Duration::from_secs(300))
    {
        Ok(o) if o.ok() => {}
        Ok(o) => {
            put_aside(env, &staging);
            return Err(format!(
                "Could not copy the new version: {}. Nothing changed.",
                crate::tasks::first_lines(&o.stderr, 1)
            ));
        }
        Err(e) => {
            put_aside(env, &staging);
            return Err(format!(
                "Could not copy the new version: {e}. Nothing changed."
            ));
        }
    }
    if let Err(e) = verify_bundle(env, &staging, &release.version) {
        put_aside(env, &staging);
        return Err(format!("The copied app was refused: {e}. Nothing changed."));
    }
    // Only the copy loses the download mark; failure is fine (curl sets none).
    let _ = env.sys.run(XATTR, &["-dr", "com.apple.quarantine", &b]);
    Ok(staging)
}

/// A staged copy that will not be used goes to the Trash (never deleted).
fn put_aside(env: &Env, staging: &Path) {
    if std::fs::symlink_metadata(staging).is_err() {
        return;
    }
    let trash = executor::trash_dir_for(env, staging);
    let _ = std::fs::create_dir_all(&trash);
    let name = staging
        .file_name()
        .map(|n| n.to_string_lossy().trim_start_matches('.').to_string())
        .unwrap_or_else(|| "CleanCraft update.app".into());
    let to = executor::free_name(&trash, &name, true);
    let _ = std::fs::rename(staging, to);
}

/// The undo command: the new version goes to the Trash and the old one comes back, each only
/// when its place is free; then the menu bar agent restarts on the old version.
pub fn undo_command(app: &Path, old: &Path, new_in_trash: &Path, agent: Option<&str>) -> String {
    let q = |p: &Path| shell_quote(&p.to_string_lossy());
    let trash = new_in_trash.parent().unwrap_or(Path::new("/"));
    let mut cmd = format!(
        "/bin/mkdir -p {t} && [ ! -e {n} ] && /bin/mv -n -- {a} {n} && [ ! -e {a} ] && /bin/mv -n -- {o} {a}",
        t = q(trash),
        n = q(new_in_trash),
        a = q(app),
        o = q(old),
    );
    if let Some(target) = agent {
        cmd.push_str(&format!(
            " && {{ {} kickstart -k {} >/dev/null 2>&1 || true; }}",
            crate::login_agent::LAUNCHCTL,
            shell_quote(target)
        ));
    }
    cmd
}

fn journal(
    env: &Env,
    target: &Target,
    release: &Release,
    app: &Path,
    old: &Path,
    agent_on: bool,
) -> Result<String, String> {
    let trash = old.parent().unwrap_or(Path::new("/"));
    let new_in_trash =
        executor::free_name(trash, &format!("CleanCraft {}.app", release.version), true);
    let agent = agent_on.then(|| {
        format!(
            "gui/{}/{}",
            crate::features::trash::uid(env),
            crate::login_agent::LABEL
        )
    });
    let now = chrono::Utc::now();
    let plan_id = format!("self-update-{}", now.timestamp_nanos_opt().unwrap_or(0));
    let rec = ActionRecord {
        id: format!("{plan_id}#0"),
        plan_id,
        feature: Some(FeatureId::Updater),
        time: now.timestamp(),
        size: 0,
        kind: ActionKind::Toggled {
            what: format!(
                "{HISTORY_TEXT} from {} to {} (the previous version is in the Trash: {})",
                target.current,
                release.version,
                env.display(old)
            ),
            undo_command: Some(undo_command(app, old, &new_in_trash, agent.as_deref())),
        },
        undone: None,
    };
    executor::append_journal(env, &rec)
        .map_err(|e| format!("the action log could not be written: {e}"))?;
    Ok(rec.id)
}

/// The detached helper that opens the app again once process `pid` has quit (at most 20
/// seconds of waiting): `/bin/sh -c <script> sh <pid> <app>`, the values passed as arguments.
pub const RELAUNCH_SCRIPT: &str = "i=0; while kill -0 \"$1\" 2>/dev/null && [ $i -lt 100 ]; do sleep 0.2; i=$((i+1)); done; /usr/bin/open \"$2\"";

/// Start the relaunch helper for `app`, waiting for process `pid`.
pub fn relaunch(env: &Env, app: &Path, pid: u32) -> std::io::Result<()> {
    let pid = pid.to_string();
    let app = app.to_string_lossy();
    env.sys
        .spawn("/bin/sh", &["-c", RELAUNCH_SCRIPT, "sh", &pid, &app])
}

// ---------------------------------------------------------------------------------------------
// The automatic check (menu bar agent, app launch, `cleancraft update --check`).

/// What an automatic check did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Auto {
    /// "Check automatically" is off.
    Off,
    /// Checked less than 24 hours ago.
    NotDue,
    /// The check failed (calm words, no notification).
    Failed(String),
    /// Nothing newer (or ahead).
    Current(Found),
    /// A newer version: the person was told (or already had been for this version).
    Told { release: Release, notified: bool },
    /// Installed by itself ("Install automatically" on).
    Installed(Installed),
    /// "Install automatically" is on but it could not install; the person was told why.
    InstallFailed { release: Release, why: String },
}

/// Where the automatic check runs and what it may do there.
pub struct AutoCtx<'a> {
    pub now: i64,
    /// Send macOS notifications (the agent; the app shows its banner instead).
    pub notify: bool,
    /// The installed app, and whether installing by itself is possible here now (the agent:
    /// only while the app is closed; never during a cleanup). `None`: never install here.
    pub install: Option<Target>,
    pub current: &'a str,
}

/// The automatic check: at most once every 24 hours, only with "Check automatically" on.
/// A newer version is installed when "Install automatically" is on and `ctx.install` allows
/// it, otherwise the person is notified once per version.
pub fn auto_check(
    env: &Env,
    settings: &crate::settings::Settings,
    ctx: &AutoCtx,
    check: &dyn Fn(&Env) -> Result<Found, String>,
) -> Auto {
    if !settings.update_check_auto {
        return Auto::Off;
    }
    let mut st = UpdateState::load(env);
    if !st.due(ctx.now) {
        return Auto::NotDue;
    }
    let res = check(env);
    st.record(&res, ctx.now, ctx.current);
    let _ = st.save(env);
    let found = match res {
        Err(e) => return Auto::Failed(check_failed_text(&e)),
        Ok(f) => f,
    };
    let Found::Available { release } = found else {
        return Auto::Current(found);
    };
    if settings.update_install_auto
        && let Some(target) = &ctx.install
    {
        return match install(env, &release, target, &Progress::new()) {
            Ok(done) => {
                if ctx.notify {
                    let _ = crate::notify::send(
                        env,
                        &crate::notify::Notification {
                            title: "CleanCraft was updated".into(),
                            subtitle: None,
                            body: format!(
                                "Version {} is installed. The previous version is in the Trash.",
                                done.to
                            ),
                            route: None,
                        },
                    );
                }
                Auto::Installed(done)
            }
            Err(why) => {
                tell(env, &mut st, &release, ctx, Some(&why));
                Auto::InstallFailed { release, why }
            }
        };
    }
    let notified = tell(env, &mut st, &release, ctx, None);
    Auto::Told { release, notified }
}

/// Notify once per version (unless notifications are not wanted here or are paused).
fn tell(
    env: &Env,
    st: &mut UpdateState,
    release: &Release,
    ctx: &AutoCtx,
    why: Option<&str>,
) -> bool {
    if !ctx.notify || st.notified.as_deref() == Some(release.version.as_str()) {
        return false;
    }
    let settings = crate::settings::Settings::load(env);
    if settings.notifications_paused {
        return false;
    }
    let body = match why {
        None => format!(
            "Version {} is ready. Open CleanCraft to read what is new and install it.",
            release.version
        ),
        Some(w) => format!(
            "Version {} could not install by itself: {w}",
            release.version
        ),
    };
    let ok = crate::notify::send(
        env,
        &crate::notify::Notification {
            title: "A CleanCraft update is available".into(),
            subtitle: None,
            body,
            route: Some("settings".into()),
        },
    )
    .is_ok();
    if ok {
        st.notified = Some(release.version.clone());
        let _ = st.save(env);
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_become_safe_versions() {
        assert_eq!(version_of_tag("v0.1.3").as_deref(), Some("0.1.3"));
        assert_eq!(
            version_of_tag("1.2.0-beta.1").as_deref(),
            Some("1.2.0-beta.1")
        );
        assert_eq!(version_of_tag("v../../x"), None);
        assert_eq!(version_of_tag("latest"), None);
        assert_eq!(version_of_tag("v1 2"), None);
    }

    #[test]
    fn bundle_of_takes_the_outer_app() {
        let p = Path::new(
            "/Applications/CleanCraft.app/Contents/Library/LoginItems/CleanCraftAgent.app/Contents/MacOS/CleanCraftAgent",
        );
        assert_eq!(
            bundle_of(p).as_deref(),
            Some(Path::new("/Applications/CleanCraft.app"))
        );
        assert_eq!(
            bundle_of(Path::new("/Users/a/target/release/cleancraft")),
            None
        );
    }

    #[test]
    fn sha_files_are_read_strictly() {
        let h = "a".repeat(64);
        assert_eq!(
            parse_sha(&format!("{h}  CleanCraft-0.1.3-arm64.dmg\n")),
            Some(h)
        );
        assert_eq!(parse_sha("abc  x.dmg"), None);
        assert_eq!(parse_sha(""), None);
    }

    #[test]
    fn notes_are_trimmed_and_capped() {
        assert_eq!(clean_notes("\r\n  Fixes.\r\nMore.\n\n"), "Fixes.\nMore.");
        let long = "x".repeat(NOTES_MAX + 50);
        assert!(clean_notes(&long).ends_with("..."));
    }

    #[test]
    fn throttle_is_24_hours() {
        let mut st = UpdateState::default();
        assert!(st.due(1000));
        st.last_check = Some(1000);
        assert!(!st.due(1000 + CHECK_EVERY_SECS - 1));
        assert!(st.due(1000 + CHECK_EVERY_SECS));
        assert!(st.due(10), "a clock that went back checks again");
    }
}
