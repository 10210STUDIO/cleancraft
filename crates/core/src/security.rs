//! Security Check (a tool of the Protect section): five everyday macOS security settings,
//! read only. CleanCraft never changes any of these; each card's one action opens the System
//! Settings pane where the person flips the switch themselves.
//!
//! There is no score. The state is "Good" or "Needs attention": attention when at least one
//! of the five rules below applies, listed under "How this is decided". A setting that could
//! not be read neither helps nor hurts the state; it is listed under "Not read", never guessed.
//!
//! Mercenary spyware (such as Pegasus) is a different problem: targeted, often zero-click, and
//! built to leave little or no trace, so no consumer cleaner can reliably detect it. Security
//! Check looks only at the ordinary settings below, which is what actually helps most people.

use crate::env::Env;
use crate::route::AppRoute;

/// One documented rule: which setting, and the state it flags. Shown on the page under "How
/// this is decided".
pub struct Rule {
    pub id: &'static str,
    pub text: &'static str,
}

/// Every rule the page applies, in the order cards are made.
pub const RULES: &[Rule] = &[
    Rule {
        id: "filevault",
        text: "FileVault (disk encryption) is off.",
    },
    Rule {
        id: "firewall",
        text: "The firewall is off.",
    },
    Rule {
        id: "automatic_login",
        text: "Automatic login is turned on for a user.",
    },
    Rule {
        id: "guest_account",
        text: "The guest account is turned on.",
    },
    Rule {
        id: "screen_lock",
        text: "No password is asked for after sleep or the screen saver starts.",
    },
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    On,
    Off,
    /// Could not be read, never guessed.
    #[default]
    Unknown,
}

/// Everything the page shows, read once from `env.sys`. A rule's setting is [`Status::Unknown`]
/// only when it genuinely could not be read; it is never guessed from a missing preference when
/// the missing state has a documented, well-known meaning (automatic login, the guest account).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub now: i64,
    pub filevault: Status,
    pub firewall: Status,
    pub automatic_login: Status,
    pub guest_account: Status,
    pub screen_lock: Status,
    /// What could not be read, in plain words.
    pub problems: Vec<String>,
}

/// Read every fact (five short commands through `env.sys`, nothing else). Run it off the UI
/// thread; it is quick, but not free.
pub fn gather(env: &Env, now: i64) -> Facts {
    let mut f = Facts {
        now,
        ..Default::default()
    };

    // `fdesetup status` prints "FileVault is On." or "FileVault is Off.".
    match env.sys.run("/usr/bin/fdesetup", &["status"]) {
        Ok(o) if o.ok() && o.stdout.contains("FileVault is On") => f.filevault = Status::On,
        Ok(o) if o.ok() && o.stdout.contains("FileVault is Off") => f.filevault = Status::Off,
        _ => f
            .problems
            .push("FileVault's status could not be read".into()),
    }

    // globalstate: 0 off, 1 or 2 on (on for specific services, or on for essential services).
    match env.sys.run(
        "/usr/bin/defaults",
        &["read", "/Library/Preferences/com.apple.alf", "globalstate"],
    ) {
        Ok(o) if o.ok() && o.stdout.trim() == "0" => f.firewall = Status::Off,
        Ok(o) if o.ok() && ["1", "2"].contains(&o.stdout.trim()) => f.firewall = Status::On,
        _ => f
            .problems
            .push("the firewall's status could not be read".into()),
    }

    // autoLoginUser exists only once automatic login is turned on for a user; its absence is
    // the normal, safe state, not a failed read.
    match env.sys.run(
        "/usr/bin/defaults",
        &[
            "read",
            "/Library/Preferences/com.apple.loginwindow",
            "autoLoginUser",
        ],
    ) {
        Ok(o) if o.ok() && !o.stdout.trim().is_empty() => f.automatic_login = Status::On,
        Ok(_) => f.automatic_login = Status::Off,
        Err(_) => f
            .problems
            .push("automatic login's status could not be read".into()),
    }

    // GuestEnabled: 1 on, 0 off; a missing key means guest sharing was never turned on, same
    // as 0.
    match env.sys.run(
        "/usr/bin/defaults",
        &[
            "read",
            "/Library/Preferences/com.apple.loginwindow",
            "GuestEnabled",
        ],
    ) {
        Ok(o) if o.ok() && o.stdout.trim() == "1" => f.guest_account = Status::On,
        Ok(o) if o.ok() && o.stdout.trim() == "0" => f.guest_account = Status::Off,
        Ok(o) if o.ok() => f
            .problems
            .push("the guest account's status could not be read".into()),
        Ok(_) => f.guest_account = Status::Off,
        Err(_) => f
            .problems
            .push("the guest account's status could not be read".into()),
    }

    // askForPassword: 1 a password is asked for after sleep or the screen saver (good), 0 none
    // is. Read from the current-host domain, the one the lock screen pane actually uses.
    match env.sys.run(
        "/usr/bin/defaults",
        &[
            "-currentHost",
            "read",
            "com.apple.screensaver",
            "askForPassword",
        ],
    ) {
        Ok(o) if o.ok() && o.stdout.trim() == "1" => f.screen_lock = Status::On,
        Ok(o) if o.ok() && o.stdout.trim() == "0" => f.screen_lock = Status::Off,
        _ => f
            .problems
            .push("the screen lock setting could not be read".into()),
    }

    f
}

// ---------------------------------------------------------------------------------------------
// Recommendations

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    /// The rule that made it ([`RULES`]).
    pub rule: &'static str,
    pub headline: String,
    pub detail: String,
    /// The one button: its label and the page it opens.
    pub action: String,
    pub route: AppRoute,
}

/// The cards for `f`: one per setting in its risky state, in [`RULES`] order. A setting that
/// could not be read, or is already in its safe state, gets no card.
pub fn recommendations(f: &Facts) -> Vec<Card> {
    let mut v = Vec::new();
    if f.filevault == Status::Off {
        v.push(Card {
            rule: "filevault",
            headline: "FileVault is off".into(),
            detail: "Without FileVault, anyone who gets hold of this Mac can read its disk without your password.".into(),
            action: "Open Privacy & Security".into(),
            route: AppRoute::SecurityPrivacySettings,
        });
    }
    if f.firewall == Status::Off {
        v.push(Card {
            rule: "firewall",
            headline: "The firewall is off".into(),
            detail: "The firewall blocks unwanted incoming connections from the network.".into(),
            action: "Open Privacy & Security".into(),
            route: AppRoute::SecurityPrivacySettings,
        });
    }
    if f.automatic_login == Status::On {
        v.push(Card {
            rule: "automatic_login",
            headline: "Automatic login is turned on".into(),
            detail: "Anyone who can start this Mac signs straight in, without a password.".into(),
            action: "Open Users & Groups".into(),
            route: AppRoute::UsersGroupsSettings,
        });
    }
    if f.guest_account == Status::On {
        v.push(Card {
            rule: "guest_account",
            headline: "The guest account is turned on".into(),
            detail: "Anyone can use this Mac without a password, signed in as a guest.".into(),
            action: "Open Users & Groups".into(),
            route: AppRoute::UsersGroupsSettings,
        });
    }
    if f.screen_lock == Status::Off {
        v.push(Card {
            rule: "screen_lock",
            headline: "No password is asked for after sleep".into(),
            detail: "Anyone who wakes this Mac from sleep or the screen saver gets straight in."
                .into(),
            action: "Open Lock Screen".into(),
            route: AppRoute::LockScreenSettings,
        });
    }
    v
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Good,
    NeedsAttention,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::Good => "Good",
            State::NeedsAttention => "Needs attention",
        }
    }
}

/// The page's state and why: the headlines of the cards that decided it.
pub fn summary(cards: &[Card]) -> (State, Vec<String>) {
    let why: Vec<String> = cards.iter().map(|c| c.headline.clone()).collect();
    if why.is_empty() {
        (State::Good, why)
    } else {
        (State::NeedsAttention, why)
    }
}

/// Value text for each setting, for the facts table. "Unknown" when it could not be read.
pub fn fact_rows(f: &Facts) -> Vec<(&'static str, &'static str)> {
    let label = |s: Status, on: &'static str, off: &'static str| match s {
        Status::On => on,
        Status::Off => off,
        Status::Unknown => "Unknown",
    };
    vec![
        ("FileVault", label(f.filevault, "On", "Off")),
        ("Firewall", label(f.firewall, "On", "Off")),
        ("Automatic login", label(f.automatic_login, "On", "Off")),
        ("Guest account", label(f.guest_account, "On", "Off")),
        (
            "Password after sleep",
            label(f.screen_lock, "Asked for", "Not asked for"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sys::{CmdOut, FakeSys};
    use crate::testkit::FakeMac;

    const NOW: i64 = 1_790_000_000;

    fn fake(mac: &FakeMac) -> &FakeSys {
        mac.env.fake_sys().expect("fake env")
    }

    /// All five settings in their safe state.
    fn set_safe(mac: &FakeMac) {
        let f = fake(mac);
        f.answer("fdesetup status", "FileVault is On.\n");
        f.answer(
            "defaults read /Library/Preferences/com.apple.alf globalstate",
            "1\n",
        );
        f.answer_full(
            "defaults read /Library/Preferences/com.apple.loginwindow autoLoginUser",
            CmdOut {
                status: 1,
                stdout: String::new(),
                stderr: "The domain/default pair of (/Library/Preferences/com.apple.loginwindow, autoLoginUser) does not exist".into(),
            },
        );
        f.answer(
            "defaults read /Library/Preferences/com.apple.loginwindow GuestEnabled",
            "0\n",
        );
        f.answer(
            "defaults -currentHost read com.apple.screensaver askForPassword",
            "1\n",
        );
    }

    #[test]
    fn a_safe_mac_is_good_with_no_cards() {
        let mac = FakeMac::new();
        set_safe(&mac);
        let f = gather(&mac.env, NOW);
        assert_eq!(f.filevault, Status::On);
        assert_eq!(f.firewall, Status::On);
        assert_eq!(f.automatic_login, Status::Off);
        assert_eq!(f.guest_account, Status::Off);
        assert_eq!(f.screen_lock, Status::On);
        assert!(f.problems.is_empty(), "{:?}", f.problems);
        let cards = recommendations(&f);
        assert!(cards.is_empty(), "{cards:#?}");
        let (state, why) = summary(&cards);
        assert_eq!(state, State::Good);
        assert!(why.is_empty());
    }

    #[test]
    fn every_risky_setting_gets_its_own_card() {
        let mac = FakeMac::new();
        let f = fake(&mac);
        f.answer("fdesetup status", "FileVault is Off.\n");
        f.answer(
            "defaults read /Library/Preferences/com.apple.alf globalstate",
            "0\n",
        );
        f.answer(
            "defaults read /Library/Preferences/com.apple.loginwindow autoLoginUser",
            "cristiano\n",
        );
        f.answer(
            "defaults read /Library/Preferences/com.apple.loginwindow GuestEnabled",
            "1\n",
        );
        f.answer(
            "defaults -currentHost read com.apple.screensaver askForPassword",
            "0\n",
        );
        let facts = gather(&mac.env, NOW);
        assert_eq!(facts.filevault, Status::Off);
        assert_eq!(facts.firewall, Status::Off);
        assert_eq!(facts.automatic_login, Status::On);
        assert_eq!(facts.guest_account, Status::On);
        assert_eq!(facts.screen_lock, Status::Off);
        let cards = recommendations(&facts);
        let rules: Vec<&str> = cards.iter().map(|c| c.rule).collect();
        assert_eq!(
            rules,
            vec![
                "filevault",
                "firewall",
                "automatic_login",
                "guest_account",
                "screen_lock"
            ]
        );
        let (state, why) = summary(&cards);
        assert_eq!(state, State::NeedsAttention);
        assert_eq!(why.len(), 5);
    }

    #[test]
    fn every_card_has_a_documented_rule_and_one_action() {
        let mac = FakeMac::new();
        let f = fake(&mac);
        f.answer("fdesetup status", "FileVault is Off.\n");
        f.answer(
            "defaults read /Library/Preferences/com.apple.alf globalstate",
            "0\n",
        );
        f.answer(
            "defaults read /Library/Preferences/com.apple.loginwindow autoLoginUser",
            "cristiano\n",
        );
        f.answer(
            "defaults read /Library/Preferences/com.apple.loginwindow GuestEnabled",
            "1\n",
        );
        f.answer(
            "defaults -currentHost read com.apple.screensaver askForPassword",
            "0\n",
        );
        let facts = gather(&mac.env, NOW);
        for c in recommendations(&facts) {
            assert!(
                RULES.iter().any(|r| r.id == c.rule),
                "undocumented rule {}",
                c.rule
            );
            assert!(!c.action.is_empty());
        }
    }

    #[test]
    fn a_command_that_cannot_run_at_all_is_unknown_not_guessed() {
        let mac = FakeMac::new();
        // No answers scripted at all: every command fails to run ("no answer for...", the
        // same shape as the real binary being missing). Nothing here is guessed as safe.
        let f = gather(&mac.env, NOW);
        assert_eq!(f.filevault, Status::Unknown);
        assert_eq!(f.firewall, Status::Unknown);
        assert_eq!(f.automatic_login, Status::Unknown);
        assert_eq!(f.guest_account, Status::Unknown);
        assert_eq!(f.screen_lock, Status::Unknown);
        assert!(recommendations(&f).is_empty());
        assert_eq!(f.problems.len(), 5, "{:?}", f.problems);
    }

    #[test]
    fn a_missing_autologin_or_guest_key_reads_as_off_not_unknown() {
        let mac = FakeMac::new();
        let f = fake(&mac);
        f.answer("fdesetup status", "FileVault is On.\n");
        f.answer(
            "defaults read /Library/Preferences/com.apple.alf globalstate",
            "1\n",
        );
        // Both keys fail to read (status 1, "does not exist"): the documented, normal shape
        // of a user who never turned either setting on, not a failed read.
        f.answer_full(
            "defaults read /Library/Preferences/com.apple.loginwindow autoLoginUser",
            CmdOut {
                status: 1,
                stdout: String::new(),
                stderr: "does not exist".into(),
            },
        );
        f.answer_full(
            "defaults read /Library/Preferences/com.apple.loginwindow GuestEnabled",
            CmdOut {
                status: 1,
                stdout: String::new(),
                stderr: "does not exist".into(),
            },
        );
        f.answer(
            "defaults -currentHost read com.apple.screensaver askForPassword",
            "1\n",
        );
        let facts = gather(&mac.env, NOW);
        assert_eq!(facts.automatic_login, Status::Off);
        assert_eq!(facts.guest_account, Status::Off);
        assert!(facts.problems.is_empty(), "{:?}", facts.problems);
    }

    #[test]
    fn an_unreadable_firewall_is_not_counted_as_a_problem_setting() {
        let mac = FakeMac::new();
        let f = fake(&mac);
        f.answer("fdesetup status", "FileVault is On.\n");
        f.answer_full(
            "defaults read /Library/Preferences/com.apple.alf globalstate",
            CmdOut {
                status: 1,
                stdout: String::new(),
                stderr: "boom".into(),
            },
        );
        f.answer_full(
            "defaults read /Library/Preferences/com.apple.loginwindow autoLoginUser",
            CmdOut {
                status: 1,
                stdout: String::new(),
                stderr: "does not exist".into(),
            },
        );
        f.answer(
            "defaults read /Library/Preferences/com.apple.loginwindow GuestEnabled",
            "0\n",
        );
        f.answer(
            "defaults -currentHost read com.apple.screensaver askForPassword",
            "1\n",
        );
        let facts = gather(&mac.env, NOW);
        assert_eq!(facts.firewall, Status::Unknown);
        assert!(
            facts.problems.iter().any(|p| p.contains("firewall")),
            "{:?}",
            facts.problems
        );
        assert!(recommendations(&facts).is_empty());
    }
}
