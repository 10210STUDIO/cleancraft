//! Fake Macs for the cleanup modules (System Junk, Mail Downloads, Trash Bins, Privacy
//! Traces): realistic folder trees plus scripted answers for the system tools these modules
//! ask (`defaults`, `uname`, `lipo`, `plutil`, `sqlite3`, `networksetup`). Everything lives in
//! the FakeMac's temporary folder; nothing here reaches real files or runs real tools.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::env::Env;
use crate::features::{mail, privacy, trash};
use crate::sys::CmdOut;
use crate::testkit::{FakeMac, age};

/// The command line FakeSys records for `program args...`.
pub fn cmd(program: &str, args: &[&str]) -> String {
    std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn answer(env: &Env, program: &str, args: &[&str], stdout: &str) {
    if let Some(f) = env.fake_sys() {
        f.answer(&cmd(program, args), stdout);
    }
}

pub fn answer_fail(env: &Env, program: &str, args: &[&str], stderr: &str) {
    if let Some(f) = env.fake_sys() {
        f.answer_full(
            &cmd(program, args),
            CmdOut {
                status: 1,
                stdout: String::new(),
                stderr: stderr.into(),
            },
        );
    }
}

/// Mark an app as running in the fake OS.
pub fn set_running(env: &Env, name: &str, app_path: &str) {
    if let Some(f) = env.fake_sys() {
        f.with(|s| {
            s.running.push(crate::sys::RunningApp {
                pid: 4242 + s.running.len() as u32,
                name: name.into(),
                bundle_id: None,
                path: Some(PathBuf::from(app_path)),
            })
        });
    }
}

pub fn clear_running(env: &Env) {
    if let Some(f) = env.fake_sys() {
        f.with(|s| s.running.clear());
    }
}

/// Make a folder impossible to list, the way a folder protected by macOS privacy settings is
/// (tests may run as root, which ignores file modes): the folder is moved aside and a plain file
/// takes its name, so listing it fails. Nothing is deleted.
pub fn make_unlistable(p: &Path) {
    let aside = p.with_extension("moved-aside");
    let _ = std::fs::rename(p, &aside);
    let _ = std::fs::write(p, b"not a folder");
}

/// Set a folder's modification time to `days` ago.
pub fn age_dir(p: &Path, days: u64) {
    let t = SystemTime::now() - Duration::from_secs(days * 86_400);
    if let Ok(f) = std::fs::File::open(p) {
        let _ = f.set_times(std::fs::FileTimes::new().set_accessed(t).set_modified(t));
    }
}

/// Write a property list (XML) to `p`.
pub fn write_plist(p: &Path, v: &plist::Value) {
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = v.to_file_xml(p);
}

fn dict(entries: Vec<(&str, plist::Value)>) -> plist::Value {
    let mut d = plist::Dictionary::new();
    for (k, v) in entries {
        d.insert(k.to_string(), v);
    }
    plist::Value::Dictionary(d)
}

fn s(v: &str) -> plist::Value {
    plist::Value::String(v.into())
}

fn days_ago(days: u64) -> plist::Value {
    plist::Value::Date((SystemTime::now() - Duration::from_secs(days * 86_400)).into())
}

/// An app bundle with `.lproj` folders.
pub fn app_with_languages(m: &FakeMac, name: &str, langs: &[&str], each: usize) -> PathBuf {
    let app = m.app(name, &format!("com.example.{name}"), "1.0");
    for l in langs {
        m.sys_file(
            &format!("/Applications/{name}.app/Contents/Resources/{l}.lproj/Localizable.strings"),
            each,
        );
    }
    app
}

pub fn exe_of(m: &FakeMac, name: &str) -> PathBuf {
    m.env
        .sys_path(&format!("/Applications/{name}.app/Contents/MacOS/{name}"))
}

/// A fake Mac with something in every System Junk category (J1 to J20) and the tool answers
/// the scan needs: system languages Italian and English, an Apple silicon processor, Atlas a
/// universal app (x86_64 slice 600,000 bytes), Solo arm64 only.
pub fn junk_mac() -> FakeMac {
    let m = FakeMac::new();
    let env = m.env.clone();
    // J1 user caches (+ kept iCloud cache) and a sandboxed app's cache.
    m.home_file("Library/Caches/com.example.Browser/Cache.db", 3_200_000);
    m.home_file("Library/Caches/com.example.Browser/img/1.bin", 900_000);
    m.home_file("Library/Caches/com.example.Editor/index", 1_400_000);
    m.home_file("Library/Caches/com.apple.bird/session", 50_000);
    m.home_file(
        "Library/Containers/com.example.Notes/Data/Library/Caches/com.example.Notes/cache.db",
        700_000,
    );
    m.home_file(
        "Library/Containers/com.example.Notes/Data/Library/Caches/CloudKit/x",
        90_000,
    );
    // J15 Sparkle update inside an app cache, and a macOS update download.
    m.home_file(
        "Library/Caches/com.example.Updater/org.sparkle-project.Sparkle/Installation/Update.zip",
        2_500_000,
    );
    m.home_file("Library/Caches/com.example.Updater/fsCachedData/x", 100_000);
    m.sys_file("/Library/Updates/012-34567/update.pkg", 1_000_000);
    // J17 developer caches.
    m.home_file(
        "Library/Caches/Homebrew/downloads/abc--wget.bottle.tar.gz",
        1_800_000,
    );
    m.home_file(".npm/_cacache/content-v2/sha512/aa", 600_000);
    m.home_file(".npm/_logs/debug.log", 10_000);
    m.home_file(".gradle/caches/modules-2/x.jar", 900_000);
    m.home_file(".cargo/registry/cache/index.crates.io/serde.crate", 300_000);
    // J18 AI tool clutter.
    m.home_file(
        "Library/Application Support/Claude/Cache/Cache_Data/data_1",
        1_100_000,
    );
    m.home_file("Library/Application Support/Claude/config.json", 1_000);
    m.home_file("Library/Application Support/Cursor/GPUCache/data", 400_000);
    m.home_file("Library/Logs/Claude/main.log", 250_000);
    // J2 to J5.
    m.sys_file("/Library/Caches/com.example.updater/pkg", 2_000_000);
    m.home_file("Library/Logs/Example/app.log", 420_000);
    m.sys_file("/Library/Logs/Install.log", 300_000);
    m.sys_file("/private/var/log/system.log", 500_000);
    m.home_file(
        "Library/Logs/DiagnosticReports/Example-2026-05-01-101010.ips",
        60_000,
    );
    m.home_file(
        "Library/Logs/DiagnosticReports/Example-2026-05-02-101010.ips",
        70_000,
    );
    m.home_file(
        "Library/Logs/DiagnosticReports/Other_2026-05-01.crash",
        30_000,
    );
    // J6, J7 apps.
    app_with_languages(&m, "Atlas", &["en", "Base", "it", "de", "fr"], 40_000);
    m.app("Solo", "com.example.Solo", "2.0");
    answer(
        &env,
        "/usr/bin/defaults",
        &["read", "-g", "AppleLanguages"],
        "(\n    \"it-IT\",\n    \"en-US\"\n)\n",
    );
    answer(&env, "/usr/bin/uname", &["-m"], "arm64\n");
    let atlas = exe_of(&m, "Atlas").to_string_lossy().into_owned();
    let solo = exe_of(&m, "Solo").to_string_lossy().into_owned();
    answer(&env, "/usr/bin/lipo", &["-archs", &atlas], "x86_64 arm64\n");
    answer(
        &env,
        "/usr/bin/lipo",
        &["-detailed_info", &atlas],
        &format!(
            "Fat header in: {atlas}\nfat_magic 0xcafebabe\nnfat_arch 2\narchitecture x86_64\n    cputype CPU_TYPE_X86_64\n    offset 16384\n    size 600000\n    align 2^14 (16384)\narchitecture arm64\n    cputype CPU_TYPE_ARM64\n    offset 622592\n    size 400000\n    align 2^14 (16384)\n"
        ),
    );
    answer(&env, "/usr/bin/lipo", &["-archs", &solo], "arm64\n");
    // J8 launch agents: one broken, one fine.
    write_plist(
        &env.home_path("Library/LaunchAgents/com.example.gone.plist"),
        &dict(vec![
            ("Label", s("com.example.gone")),
            ("Program", s("/Applications/Gone.app/Contents/MacOS/gone")),
        ]),
    );
    write_plist(
        &env.home_path("Library/LaunchAgents/com.example.atlas.plist"),
        &dict(vec![
            ("Label", s("com.example.atlas")),
            (
                "ProgramArguments",
                plist::Value::Array(vec![s("/Applications/Atlas.app/Contents/MacOS/Atlas")]),
            ),
        ]),
    );
    // J9 preferences: broken (plutil agrees), fine, broken but plutil unavailable.
    let broken = env.home_path("Library/Preferences/com.example.broken.plist");
    m.write(&broken, b"<?xml version=\"1.0\"?><plist><dict><key>a</key>");
    answer_fail(
        &env,
        "/usr/bin/plutil",
        &["-lint", &broken.to_string_lossy()],
        &format!("{}: Unexpected character", broken.display()),
    );
    write_plist(
        &env.home_path("Library/Preferences/com.example.good.plist"),
        &dict(vec![("Key", s("value"))]),
    );
    let unchecked = env.home_path("Library/Preferences/com.example.unchecked.plist");
    m.write(
        &unchecked,
        b"<?xml version=\"1.0\"?><plist><array><string>x",
    );
    // J10 document versions.
    m.sys_file("/.DocumentRevisions-V100/PerUID/501/ab/file", 800_000);
    // J11 backups: two of one iPhone, one of an iPad.
    for (dir, name, id, days, size) in [
        ("AAA", "Tester's iPhone", "DEV1", 120, 3_000_000),
        ("BBB", "Tester's iPhone", "DEV1", 3, 2_000_000),
        ("CCC", "Tester's iPad", "DEV2", 40, 1_000_000),
    ] {
        let b = env.home_path(&format!(
            "Library/Application Support/MobileSync/Backup/{dir}"
        ));
        write_plist(
            &b.join("Info.plist"),
            &dict(vec![
                ("Device Name", s(name)),
                ("Target Identifier", s(id)),
                ("Last Backup Date", days_ago(days)),
            ]),
        );
        m.home_file(
            &format!("Library/Application Support/MobileSync/Backup/{dir}/Manifest.db"),
            size,
        );
    }
    // J12.
    m.home_file(
        "Library/iTunes/iPhone Software Updates/iPhone_17.0_Restore.ipsw",
        4_000_000,
    );
    // J13 Xcode.
    m.home_file(
        "Library/Developer/Xcode/DerivedData/Demo-abc/Build/x.o",
        5_000_000,
    );
    m.home_file(
        "Library/Developer/Xcode/DerivedData/ModuleCache.noindex/m.pcm",
        1_000_000,
    );
    m.home_file(
        "Library/Developer/Xcode/Archives/2026-05-01/Demo 5-1-26.xcarchive/Info.plist",
        1_200_000,
    );
    m.home_file(
        "Library/Developer/Xcode/iOS DeviceSupport/17.0 (21A329)/Symbols/x",
        2_200_000,
    );
    m.home_file("Library/Developer/CoreSimulator/Caches/dyld/x", 800_000);
    // J14 unfinished downloads (one still open), J16 disk images.
    m.home_file("Downloads/movie.mp4.crdownload", 1_500_000);
    m.home_file("Downloads/big.zip.download/big.zip", 700_000);
    let active = m.home_file("Downloads/active.part", 300_000);
    env.fake_sys().unwrap().set_open(&active);
    let old = m.home_file("Downloads/Old Installer.dmg", 2_600_000);
    age(&old, 200);
    let new = m.home_file("Downloads/New.dmg", 1_000_000);
    age(&new, 5);
    let nested = m.home_file("Downloads/sub/Nested.dmg", 900_000);
    age(&nested, 120);
    // J19 local models.
    m.home_file(
        ".ollama/models/manifests/registry.ollama.ai/library/llama3/8b",
        1_000,
    );
    m.home_file(".ollama/models/blobs/sha256-aaa", 3_000_000);
    m.home_file(".ollama/id_ed25519", 400);
    m.home_file(
        ".lmstudio/models/lmstudio-community/Qwen-7B-GGUF/q.gguf",
        2_400_000,
    );
    // J20.
    m.sys_file("/Users/Deleted Users/olduser.dmg", 1_300_000);
    // Personal files that must never appear.
    m.home_file("Documents/report.pdf", 700_000);
    m.home_file("Desktop/notes.txt", 2_000);
    m
}

pub const MAIL_V: &str = "Library/Mail/V10";
pub const UUID_PLIST_ACCOUNT: &str = "5E1F0000-0000-4000-8000-000000000001";
pub const UUID_DB_ACCOUNT: &str = "7B7B0000-0000-4000-8000-000000000002";
pub const UUID_UNKNOWN_ACCOUNT: &str = "9A9A0000-0000-4000-8000-000000000003";

/// An attachment file inside an account's mailbox.
pub fn mail_attachment(
    m: &FakeMac,
    account: &str,
    mailbox: &str,
    msg: &str,
    file: &str,
    size: usize,
) -> PathBuf {
    m.home_file(
        &format!("{MAIL_V}/{account}/{mailbox}.mbox/0F0F/Data/1/Attachments/{msg}/1/{file}"),
        size,
    )
}

/// A fake Mac with Mail data: two opened attachments (one edited after opening), an IMAP
/// account with an untouched and an edited attachment, a POP account and an On My Mac
/// mailbox (both must be left out), an account known from Accounts.plist (IMAP), one known
/// from the system account store (Exchange), one of unknown type, and a deleted-messages
/// mailbox.
pub fn mail_mac() -> FakeMac {
    let m = FakeMac::new();
    let env = m.env.clone();
    let md = mail::MAIL_DOWNLOADS;
    let inv = m.home_file(&format!("{md}/A1B2/Invoice.pdf"), 300_000);
    age(&inv, 10);
    let con = m.home_file(&format!("{md}/C3D4/Contract.docx"), 200_000);
    age(&con, 10);
    age_dir(&env.home_path(&format!("{md}/C3D4")), 30);

    let photo = mail_attachment(
        &m,
        "IMAP-me@example.com",
        "INBOX",
        "12345",
        "photo.jpg",
        1_200_000,
    );
    age(&photo, 20);
    let edited = mail_attachment(
        &m,
        "IMAP-me@example.com",
        "INBOX",
        "12346",
        "edited.key",
        800_000,
    );
    age(&edited, 10);
    if let Some(d) = edited.parent() {
        age_dir(d, 40);
    }
    m.home_file(
        &format!("{MAIL_V}/IMAP-me@example.com/INBOX.mbox/0F0F/Data/1/Messages/12345.emlx"),
        5_000,
    );
    mail_attachment(
        &m,
        "POP-old@pop.example",
        "INBOX",
        "9",
        "only-copy.pdf",
        600_000,
    );
    mail_attachment(&m, "Mailboxes", "Archive", "7", "local.pdf", 400_000);
    mail_attachment(
        &m,
        UUID_PLIST_ACCOUNT,
        "Sent Messages",
        "55",
        "deck.pdf",
        900_000,
    );
    mail_attachment(&m, UUID_DB_ACCOUNT, "INBOX", "77", "budget.xlsx", 350_000);
    mail_attachment(
        &m,
        UUID_UNKNOWN_ACCOUNT,
        "INBOX",
        "66",
        "mystery.zip",
        500_000,
    );
    write_plist(
        &env.home_path(&format!("{MAIL_V}/MailData/Accounts.plist")),
        &dict(vec![(
            "MailAccounts",
            plist::Value::Array(vec![dict(vec![
                ("uniqueId", s(UUID_PLIST_ACCOUNT)),
                ("AccountType", s("IMAPAccount")),
                ("AccountName", s("Work")),
            ])]),
        )]),
    );
    let db = m.home_file(mail::ACCOUNTS_DB, 4_096);
    answer(
        &env,
        "/usr/bin/sqlite3",
        &[
            "-separator",
            "|",
            &db.to_string_lossy(),
            mail::ACCOUNTS_QUERY,
        ],
        &format!("{UUID_DB_ACCOUNT}|com.apple.account.Exchange\nFFFF|com.apple.account.CalDAV\n"),
    );
    m.home_file(
        &format!("{MAIL_V}/IMAP-me@example.com/Deleted Messages.mbox/0A/Data/Messages/1.emlx"),
        20_000,
    );
    m
}

/// A fake Mac with things in the startup disk's Trash, an external disk's Trash, an empty
/// external Trash, and Mail's deleted messages.
pub fn trash_mac() -> FakeMac {
    let m = FakeMac::new();
    let env = m.env.clone();
    m.home_file(".Trash/old.zip", 1_000_000);
    m.home_file(".Trash/Folder/a.txt", 300_000);
    let uid = trash::uid(&env);
    m.sys_file(&format!("/Volumes/USB/.Trashes/{uid}/movie.mov"), 2_000_000);
    let _ = std::fs::create_dir_all(env.sys_path(&format!("/Volumes/Empty/.Trashes/{uid}")));
    m.home_file(
        &format!("{MAIL_V}/IMAP-me@example.com/Deleted Messages.mbox/0A/Data/Messages/1.emlx"),
        20_000,
    );
    m.home_file(
        &format!("{MAIL_V}/IMAP-me@example.com/Deleted Messages.mbox/0A/Data/Messages/2.emlx"),
        30_000,
    );
    m
}

/// The sqlite3 command line Privacy uses to read a TCC database.
pub fn tcc_args(db: &Path) -> Vec<String> {
    vec![
        "-readonly".into(),
        "-separator".into(),
        "|".into(),
        db.to_string_lossy().into_owned(),
        privacy::TCC_QUERY.into(),
    ]
}

pub fn answer_tcc(env: &Env, db: &Path, rows: &str) {
    let a = tcc_args(db);
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    answer(env, privacy::SQLITE3, &a, rows);
}

pub fn answer_tcc_denied(env: &Env, db: &Path) {
    let a = tcc_args(db);
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    answer_fail(
        env,
        privacy::SQLITE3,
        &a,
        "Error: unable to open database \"TCC.db\": authorization denied",
    );
}

pub fn answer_wifi(env: &Env, networks: &[&str]) {
    answer(
        env,
        privacy::NETWORKSETUP,
        &["-listallhardwareports"],
        "\nHardware Port: Ethernet\nDevice: en1\nEthernet Address: 00:00:00:00:00:01\n\nHardware Port: Wi-Fi\nDevice: en0\nEthernet Address: 00:00:00:00:00:02\n",
    );
    let mut out = "Preferred networks on en0:\n".to_string();
    for n in networks {
        out.push('\t');
        out.push_str(n);
        out.push('\n');
    }
    answer(
        env,
        privacy::NETWORKSETUP,
        &["-listpreferredwirelessnetworks", "en0"],
        &out,
    );
}

/// A fake Mac with browser data for Safari, Chrome (two profiles), Brave and Firefox, recent
/// items lists (and the Finder favorites list, which must not be offered), two known Wi-Fi
/// networks and app permissions in the user and system TCC databases.
pub fn privacy_mac() -> FakeMac {
    let m = FakeMac::new();
    let env = m.env.clone();
    // Safari.
    m.home_file("Library/Safari/History.db", 900_000);
    m.home_file("Library/Safari/LastSession.plist", 40_000);
    m.home_file("Library/Safari/Downloads.plist", 10_000);
    m.home_file("Library/Safari/Form Values", 20_000);
    m.home_file("Library/Safari/Bookmarks.plist", 50_000);
    m.home_file(
        "Library/Containers/com.apple.Safari/Data/Library/Cookies/Cookies.binarycookies",
        60_000,
    );
    m.home_file("Library/Caches/com.apple.Safari/Cache.db", 1_000_000);
    // Chrome.
    let chrome = "Library/Application Support/Google/Chrome";
    for (f, size) in [
        ("Default/Preferences", 5_000),
        ("Default/History", 2_000_000),
        ("Default/Cookies", 300_000),
        ("Default/Web Data", 150_000),
        ("Default/Login Data", 80_000),
        ("Default/Current Session", 30_000),
        ("Default/Bookmarks", 20_000),
        ("Profile 1/Preferences", 5_000),
        ("Profile 1/History", 500_000),
        ("System Profile/History", 10_000),
    ] {
        m.home_file(&format!("{chrome}/{f}"), size);
    }
    m.home_file("Library/Caches/Google/Chrome/Default/Cache/data", 1_200_000);
    // Brave.
    let brave = "Library/Application Support/BraveSoftware/Brave-Browser";
    m.home_file(&format!("{brave}/Default/Preferences"), 5_000);
    m.home_file(&format!("{brave}/Default/History"), 400_000);
    // Firefox.
    let ff = "Library/Application Support/Firefox/Profiles/abcd.default-release";
    for (f, size) in [
        ("places.sqlite", 1_500_000),
        ("cookies.sqlite", 200_000),
        ("formhistory.sqlite", 60_000),
        ("sessionstore.jsonlz4", 40_000),
        ("logins.json", 10_000),
        ("prefs.js", 5_000),
    ] {
        m.home_file(&format!("{ff}/{f}"), size);
    }
    m.home_file(
        "Library/Caches/Firefox/Profiles/abcd.default-release/cache2/x",
        700_000,
    );
    // Recent items.
    let sfl = privacy::SHARED_FILE_LIST;
    m.home_file(
        &format!("{sfl}/com.apple.LSSharedFileList.RecentDocuments.sfl3"),
        12_000,
    );
    m.home_file(
        &format!("{sfl}/com.apple.LSSharedFileList.RecentApplications.sfl3"),
        8_000,
    );
    m.home_file(
        &format!("{sfl}/com.apple.LSSharedFileList.RecentServers.sfl2"),
        4_000,
    );
    m.home_file(
        &format!("{sfl}/com.apple.LSSharedFileList.FavoriteItems.sfl3"),
        6_000,
    );
    m.home_file(
        &format!("{sfl}/{}/com.apple.textedit.sfl3", privacy::RECENT_DOCS_DIR),
        5_000,
    );
    // Wi-Fi.
    answer_wifi(&env, &["Home Net", "Cafe Guest"]);
    // Permissions.
    let user_db = m.home_file(privacy::TCC_USER, 8_192);
    let sys_db = m.sys_file(privacy::TCC_SYSTEM, 8_192);
    answer_tcc(
        &env,
        &user_db,
        "kTCCServiceCamera|us.zoom.xos|2\nkTCCServiceMicrophone|us.zoom.xos|2\nkTCCServiceAddressBook|com.example.Old|0\n",
    );
    answer_tcc(
        &env,
        &sys_db,
        "kTCCServiceScreenCapture|com.example.Recorder|2\n",
    );
    m.home_file("Documents/report.pdf", 700_000);
    m
}
