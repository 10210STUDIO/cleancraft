//! Test helpers for CleanCraft's self-update ([`crate::self_update`]): an installed
//! CleanCraft.app on the fake Mac, GitHub's "latest release" answer, and scripted answers for
//! every command of an install (curl, hdiutil, codesign, ditto, xattr, launchctl) that also
//! write the files those tools would write.

use std::path::{Path, PathBuf};

use crate::self_update::{
    self, CODESIGN, CURL, DITTO, HDIUTIL, LATEST_URL, LSREGISTER, XATTR, check_args, download_args,
    text_args,
};
use crate::sys::{CmdOut, FakeSys};
use crate::testkit::FakeMac;

/// The download address of a version's disk image in [`release_json`].
pub fn dmg_url(version: &str) -> String {
    format!(
        "https://github.com/{}/releases/download/v{version}/CleanCraft-{version}-arm64.dmg",
        self_update::REPO
    )
}

pub fn sha_url(version: &str) -> String {
    format!("{}.sha256", dmg_url(version))
}

/// GitHub's answer for a latest release `version`, with or without the `.sha256` asset.
pub fn release_json(version: &str, with_sha: bool, notes: &str) -> String {
    let mut assets = vec![serde_json::json!({
        "name": format!("CleanCraft-{version}-arm64.dmg"),
        "browser_download_url": dmg_url(version),
        "size": DMG_BYTES.len(),
    })];
    if with_sha {
        assets.push(serde_json::json!({
            "name": format!("CleanCraft-{version}-arm64.dmg.sha256"),
            "browser_download_url": sha_url(version),
            "size": 100,
        }));
    }
    serde_json::json!({
        "tag_name": format!("v{version}"),
        "name": format!("CleanCraft v{version}"),
        "html_url": format!("https://github.com/{}/releases/tag/v{version}", self_update::REPO),
        "body": notes,
        "draft": false,
        "prerelease": false,
        "assets": assets,
    })
    .to_string()
}

/// The fake disk image's bytes.
pub const DMG_BYTES: &[u8] = b"fake CleanCraft disk image";

/// The `shasum -a 256` line of [`DMG_BYTES`].
pub fn good_sha_line(version: &str) -> String {
    use sha2::Digest;
    let h = hex::encode(sha2::Sha256::digest(DMG_BYTES));
    format!("{h}  CleanCraft-{version}-arm64.dmg\n")
}

/// An Info.plist for an app bundle.
pub fn info_plist(id: &str, version: &str) -> Vec<u8> {
    let mut d = plist::Dictionary::new();
    d.insert("CFBundleIdentifier".into(), id.into());
    d.insert("CFBundleShortVersionString".into(), version.into());
    d.insert("CFBundleVersion".into(), version.into());
    d.insert("CFBundleExecutable".into(), "CleanCraft".into());
    let mut out = Vec::new();
    let _ = plist::Value::Dictionary(d).to_writer_xml(&mut out);
    out
}

/// The "program arg arg" line FakeSys records.
pub fn line(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// CleanCraft `version` installed at /Applications/CleanCraft.app on the fake Mac.
pub fn install_app(m: &FakeMac, version: &str) -> PathBuf {
    let app = m.env.sys_path("/Applications/CleanCraft.app");
    write_bundle(&app, self_update::BUNDLE_ID, version);
    app
}

fn write_bundle(app: &Path, id: &str, version: &str) {
    let _ = std::fs::create_dir_all(app.join("Contents/MacOS"));
    let _ = std::fs::write(app.join("Contents/Info.plist"), info_plist(id, version));
    let _ = std::fs::write(
        app.join("Contents/MacOS/CleanCraft"),
        format!("CleanCraft {version}"),
    );
}

fn fake(m: &FakeMac) -> &FakeSys {
    m.env.fake_sys().expect("fake Mac")
}

/// GitHub answers the release check with `json`.
pub fn answer_check(m: &FakeMac, json: &str) {
    fake(m).answer(&line(CURL, &check_args()), json);
}

/// The release check fails like curl without a network (exit 6).
pub fn fail_check(m: &FakeMac) {
    fake(m).answer_full(
        &line(CURL, &check_args()),
        CmdOut {
            status: 6,
            stdout: String::new(),
            stderr: "curl: (6) Could not resolve host: api.github.com".into(),
        },
    );
}

/// How the scripted install should go.
pub struct Script<'a> {
    pub version: &'a str,
    /// The checksum file's text.
    pub sha_line: String,
    /// The bundle id and version of the app inside the disk image.
    pub dmg_id: &'a str,
    pub dmg_version: &'a str,
}

impl<'a> Script<'a> {
    /// Everything right for `version`.
    pub fn good(version: &'a str) -> Script<'a> {
        Script {
            version,
            sha_line: good_sha_line(version),
            dmg_id: self_update::BUNDLE_ID,
            dmg_version: version,
        }
    }
}

/// Answer every command of an install of `s.version`: the checksum, the download (writes the
/// disk image), the mount (writes the app inside it), codesign, ditto (writes the copy next
/// to the installed app), xattr, lsregister, detach, launchctl.
pub fn answer_install(m: &FakeMac, s: &Script) {
    let f = fake(m);
    let env = &m.env;
    f.answer(&line(CURL, &text_args(&sha_url(s.version))), &s.sha_line);
    let part = self_update::cache_dir(env).join("CleanCraft-update.dmg.part");
    f.answer_creating(
        &line(CURL, &download_args(&dmg_url(s.version), &part)),
        "",
        &part,
        DMG_BYTES,
    );
    let mnt = self_update::mount_dir(env);
    let attach = line(
        HDIUTIL,
        &[
            "attach".into(),
            "-nobrowse".into(),
            "-readonly".into(),
            "-noautoopen".into(),
            "-mountpoint".into(),
            mnt.to_string_lossy().into_owned(),
            self_update::dmg_path(env).to_string_lossy().into_owned(),
        ],
    );
    let src = mnt.join("CleanCraft.app/Contents");
    f.answer_creating(
        &attach,
        "/dev/disk9s1\tApple_HFS\t/mount",
        &src.join("Info.plist"),
        &info_plist(s.dmg_id, s.dmg_version),
    );
    f.answer_creating(&attach, "", &src.join("MacOS/CleanCraft"), b"new");
    f.answer(
        &line(
            HDIUTIL,
            &[
                "detach".into(),
                mnt.to_string_lossy().into_owned(),
                "-quiet".into(),
            ],
        ),
        "",
    );
    f.answer(CODESIGN, "");
    let app = env.sys_path("/Applications/CleanCraft.app");
    let staging = self_update::staging_path(&app, s.version);
    let ditto = line(
        DITTO,
        &[
            mnt.join("CleanCraft.app").to_string_lossy().into_owned(),
            staging.to_string_lossy().into_owned(),
        ],
    );
    f.answer_creating(
        &ditto,
        "",
        &staging.join("Contents/Info.plist"),
        &info_plist(s.dmg_id, s.dmg_version),
    );
    f.answer_creating(
        &ditto,
        "",
        &staging.join("Contents/MacOS/CleanCraft"),
        format!("CleanCraft {}", s.dmg_version).as_bytes(),
    );
    f.answer(XATTR, "");
    f.answer(LSREGISTER, "");
    f.answer("launchctl", "");
    f.answer("osascript", "");
}

/// The version of the app now at /Applications/CleanCraft.app.
pub fn installed_version(m: &FakeMac) -> Option<String> {
    self_update::bundle_info(&m.env.sys_path("/Applications/CleanCraft.app")).map(|(_, v)| v)
}

/// The release check's address (for asserting what was asked).
pub fn check_line() -> String {
    let _ = LATEST_URL;
    line(CURL, &check_args())
}
