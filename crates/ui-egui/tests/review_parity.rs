//! Defects found by the parity lead that are not fixed yet. Each test is ignored and fails
//! today; whoever fixes the defect removes the `#[ignore]`. Ids match docs/agents/reports/parity.md
//! and the board.

mod common;

use cleancraft_core::testkit::FakeMac;
use common::*;

/// PAR-3: Settings offers "Show the live monitor in the menu bar", but no menu bar item exists
/// (FEATURES R6 is PARTIAL) and nothing reads `Settings::menu_bar`. The checkbox saves a value
/// that changes nothing: a dead control. Either show the status item, or leave the option out
/// until there is one (rule 6: no controls for things that cannot work).
#[test]
#[ignore = "PAR-3: menu bar setting has no effect"]
fn menu_bar_option_only_when_there_is_a_menu_bar_item() {
    let mac = FakeMac::new();
    let mut h = harness(&mac);
    click(&mut h, "Settings");
    assert!(
        !has(&h, "Show the live monitor in the menu bar"),
        "the option is shown but no menu bar item exists"
    );
}
