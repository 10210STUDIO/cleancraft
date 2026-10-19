//! Coordinator fixes after the final check: group-writable system folders (admins can clean
//! /Library/Caches without the password), startup agents only "broken" when the program is
//! really not found, renamed feature ids with the old ones still accepted.

use std::os::unix::fs::{PermissionsExt, chown};

use cleancraft_core::features::system_junk::program_gone;
use cleancraft_core::testkit::FakeMac;
use cleancraft_core::{Fault, FeatureId};

#[test]
fn group_writable_folder_is_movable_for_group_members_only() {
    let m = FakeMac::new();
    let dir = m.env.sys_path("/Library/Caches");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o1775)).unwrap();
    if chown(&dir, Some(54_321), Some(4_321)).is_err() {
        eprintln!("cannot chown here (not root); skipping");
        return;
    }
    // Not a member: locked.
    m.env.fake_sys().unwrap().answer("id -G", "20 12 61");
    assert!(!m.env.can_modify(&dir));
    // Member of the folder's group (macOS admin group): movable.
    m.env.fake_sys().unwrap().answer("id -G", "20 12 61 4321");
    assert!(m.env.can_modify(&dir));
    // Not group-writable: locked even for members.
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o1755)).unwrap();
    assert!(!m.env.can_modify(&dir));
}

#[test]
fn startup_program_is_gone_only_when_not_found() {
    let m = FakeMac::new();
    let present = m.sys_file("/Applications/Tool.app/Contents/MacOS/helper", 10);
    assert!(!program_gone(
        &m.env,
        "/Applications/Tool.app/Contents/MacOS/helper"
    ));
    assert!(program_gone(
        &m.env,
        "/Applications/Gone.app/Contents/MacOS/helper"
    ));
    // A privacy refusal is not "gone".
    let protected = "/Users/tester/Documents/tools/agent";
    m.env
        .inject_fault(&m.env.sys_path(protected), Fault::PermissionDenied);
    assert!(!program_gone(&m.env, protected));
    assert!(present.exists());
}

#[test]
fn feature_ids_use_our_names_and_accept_the_old_ones() {
    assert_eq!(FeatureId::SmartCare.as_str(), "quick_sweep");
    assert_eq!(FeatureId::SpaceLens.as_str(), "disk_map");
    assert_eq!(FeatureId::parse("smart_care"), Some(FeatureId::SmartCare));
    assert_eq!(FeatureId::parse("space-lens"), Some(FeatureId::SpaceLens));
    assert_eq!(FeatureId::parse("Disk Map"), Some(FeatureId::SpaceLens));
    let old: Vec<FeatureId> = serde_json::from_str(r#"["smart_care","space_lens"]"#).unwrap();
    assert_eq!(old, vec![FeatureId::SmartCare, FeatureId::SpaceLens]);
    assert_eq!(
        serde_json::to_string(&FeatureId::SpaceLens).unwrap(),
        r#""disk_map""#
    );
}
