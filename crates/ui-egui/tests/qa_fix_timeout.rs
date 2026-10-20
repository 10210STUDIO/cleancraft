//! FIX-3 by clicks: a system command that hangs is stopped at its time limit and the user
//! reads why. Tune-up tasks get their own, longer limit (20 minutes); the result says the
//! command did not finish and was stopped, the run is logged as failed, the next task is not
//! blocked.

mod common;

use cleancraft_core::executor;
use cleancraft_core::tasks::TASK_TIMEOUT;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{ActionKind, FeatureId};
use common::*;

#[test]
fn a_hung_task_command_is_stopped_and_explained() {
    let m = FakeMac::new();
    let f = m.env.fake_sys().unwrap();
    f.answer("id -u", "501\n");
    f.answer("sw_vers -productVersion", "14.6.1\n");
    f.time_out("/usr/bin/tmutil thinlocalsnapshots / 999999999999 4");
    let mut h = harness(&m);
    // The tool sits in the "Speed" section of the sidebar.
    click(&mut h, "Speed");
    click(&mut h, "Tune-up Tasks");
    click(&mut h, "Free up purgeable space");
    click(&mut h, "Run Selected");
    click(&mut h, "Run");
    assert!(has(
        &h,
        "Failed: /usr/bin/tmutil did not finish within 20 minutes and was stopped"
    ));
    assert!(has_text(&h, "0 done, 1 failed, 0 cancelled."));
    // The task's own limit was used, not the 60 second default.
    let limits = f.with(|s| s.limits.clone());
    assert!(
        limits.iter().any(
            |(l, t)| l == "/usr/bin/tmutil thinlocalsnapshots / 999999999999 4"
                && *t == TASK_TIMEOUT
        ),
        "{limits:?}"
    );
    // Logged as a failed command.
    let hist = executor::history(&m.env);
    assert_eq!(hist.len(), 1);
    assert_eq!(hist[0].feature, Some(FeatureId::Maintenance));
    assert!(matches!(
        &hist[0].kind,
        ActionKind::Command { ok: false, output, .. } if output.contains("did not finish")
    ));
}
