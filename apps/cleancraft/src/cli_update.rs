//! `cleancraft update`: CleanCraft's own updates from the command line, so a person can also
//! wire their own schedule (a LaunchAgent, cron).
//!
//! - `cleancraft update` or `--check`: ask GitHub now and print the result with the notes.
//! - `cleancraft update --install`: install the newer version (same checks as the app:
//!   checksum, bundle id, signature, writable folder; the old version goes to the Trash with
//!   Undo in History). Refused while the CleanCraft app is open, since it may be cleaning.

use std::io::Write;

use clap::Args;
use cleancraft_core::Env;
use cleancraft_core::progress::Progress;
use cleancraft_core::self_update::{self, Found, Target};

#[derive(Args, Debug, Default)]
pub struct UpdateArgs {
    /// Look for a newer CleanCraft and print it (the default).
    #[arg(long)]
    pub check: bool,
    /// Install the newer CleanCraft when there is one.
    #[arg(long)]
    pub install: bool,
}

pub fn run(env: &Env, a: &UpdateArgs, current: &str, out: &mut dyn Write) -> anyhow::Result<()> {
    let res = self_update::check_against(env, current);
    let mut st = self_update::UpdateState::load(env);
    st.record(&res, chrono::Utc::now().timestamp(), current);
    let _ = st.save(env);
    let found = res.map_err(|e| anyhow::anyhow!(self_update::check_failed_text(&e)))?;
    writeln!(out, "{}", found.message(current))?;
    let Found::Available { release } = found else {
        return Ok(());
    };
    if !release.notes.is_empty() {
        writeln!(
            out,
            "\nWhat is new in {}:\n{}\n",
            release.version, release.notes
        )?;
    }
    if !a.install {
        writeln!(out, "Install it with: cleancraft update --install")?;
        return Ok(());
    }
    let Some(app) = self_update::installed_app(env) else {
        anyhow::bail!(
            "This copy of CleanCraft does not run from an app bundle (a build from source). Download the release from {}.",
            self_update::RELEASES_PAGE
        );
    };
    if self_update::app_running(env, &app) {
        anyhow::bail!(
            "CleanCraft is open and may be cleaning. Quit it first, or install from Settings > Updates in the app."
        );
    }
    let target = Target {
        app,
        current: current.to_string(),
        busy: None,
    };
    let meter = Progress::new();
    let done = self_update::install(env, &release, &target, &meter).map_err(anyhow::Error::msg)?;
    writeln!(out, "{}", done.message())?;
    if let Some(n) = &done.note {
        writeln!(out, "{n}")?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use cleancraft_core::testkit::FakeMac;
    use cleancraft_core::testkit_update::*;

    #[test]
    fn check_prints_the_result_and_install_swaps_the_app() {
        let m = FakeMac::new();
        install_app(&m, "0.1.2");
        answer_check(&m, &release_json("0.1.3", true, "Faster."));
        let mut out = Vec::new();
        run(&m.env, &UpdateArgs::default(), "0.1.2", &mut out).expect("check");
        let text = String::from_utf8(out).unwrap();
        assert!(
            text.contains("CleanCraft 0.1.3 is available. You have 0.1.2."),
            "{text}"
        );
        assert!(text.contains("Faster."));
        assert!(text.contains("cleancraft update --install"));
        assert_eq!(installed_version(&m).as_deref(), Some("0.1.2"));

        answer_install(&m, &Script::good("0.1.3"));
        let mut out = Vec::new();
        let a = UpdateArgs {
            install: true,
            ..Default::default()
        };
        run(&m.env, &a, "0.1.2", &mut out).expect("install");
        assert_eq!(installed_version(&m).as_deref(), Some("0.1.3"));
        assert!(
            String::from_utf8(out)
                .unwrap()
                .contains("CleanCraft 0.1.3 is installed.")
        );
    }

    #[test]
    fn install_is_refused_while_the_app_is_open_and_ahead_does_nothing() {
        let m = FakeMac::new();
        let app = install_app(&m, "0.1.2");
        answer_check(&m, &release_json("0.1.3", true, ""));
        answer_install(&m, &Script::good("0.1.3"));
        m.env.fake_sys().unwrap().with(|s| {
            s.running.push(cleancraft_core::sys::RunningApp {
                pid: 99_999,
                name: "CleanCraft".into(),
                bundle_id: None,
                path: Some(self_update::main_exe(&app)),
            })
        });
        let a = UpdateArgs {
            install: true,
            ..Default::default()
        };
        let e = run(&m.env, &a, "0.1.2", &mut Vec::new()).expect_err("open");
        assert!(e.to_string().contains("Quit it first"), "{e}");
        assert_eq!(installed_version(&m).as_deref(), Some("0.1.2"));

        let mut out = Vec::new();
        run(&m.env, &a, "0.2.0", &mut out).expect("ahead");
        assert!(
            String::from_utf8(out)
                .unwrap()
                .starts_with("You are ahead of the latest release")
        );
    }

    #[test]
    fn no_network_is_a_calm_error() {
        let m = FakeMac::new();
        fail_check(&m);
        let e = run(&m.env, &UpdateArgs::default(), "0.1.2", &mut Vec::new()).expect_err("x");
        assert!(
            e.to_string().starts_with("Could not check for updates"),
            "{e}"
        );
    }
}
