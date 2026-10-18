//! Domain QA review (reviewer "domain"), by clicks: Disk Map must never remove a Photos
//! library or the contents of Documents. Defects DOM-1 and DOM-2 (docs/agents/reports/domain.md).
//! Run: `cargo test -p cleancraft-ui-egui --test review_domain_disk_map -- --ignored`.

mod common;

use cleancraft_core::testkit::FakeMac;
use common::*;

fn mac() -> FakeMac {
    let mac = FakeMac::new();
    mac.env.fake_sys().unwrap().answer(
        "df",
        "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3s1 488245288 300000000 188245288 62% /\n",
    );
    mac
}

#[test]
fn dom2_ticking_pictures_moves_the_photos_library() {
    let mac = mac();
    let library = mac.home_file(
        "Pictures/Photos Library.photoslibrary/database/Photos.sqlite",
        2_000_000,
    );
    mac.home_file("Pictures/holiday.jpg", 300_000);
    let mut h = harness(&mac);
    click(&mut h, "Disk Map");
    click(&mut h, "Map Home Folder");
    click(&mut h, "Select Pictures");
    if has(&h, "Review and Remove") && !h.state().disk_map.selected.is_empty() {
        click(&mut h, "Review and Remove");
        click(&mut h, "Move to Trash");
    }
    assert!(
        library.exists(),
        "the Photos library was moved to the Trash with Pictures (FEATURES S5, D3)"
    );
}

#[test]
fn dom1_typed_lowercase_documents_can_be_emptied() {
    let mac = mac();
    // Stands for ~/Documents: macOS volumes are case-insensitive by default.
    let secret = mac.home_file("documents/tax-return-2025.pdf", 400_000);
    let mut h = harness(&mac);
    click(&mut h, "Disk Map");
    type_into(&mut h, "Folder to map", "~/documents");
    click(&mut h, "Map Folder");
    if has(&h, "Select tax-return-2025.pdf") {
        click(&mut h, "Select tax-return-2025.pdf");
        if !h.state().disk_map.selected.is_empty() {
            click(&mut h, "Review and Remove");
            click(&mut h, "Move to Trash");
        }
    }
    assert!(
        secret.exists(),
        "a file in ~/documents was moved to the Trash (FEATURES S5: never the contents of Documents)"
    );
}

#[test]
fn documents_and_library_rows_stay_locked() {
    let mac = mac();
    mac.home_file("Documents/report.pdf", 700_000);
    mac.home_file("Library/Caches/x/y", 100_000);
    let mut h = harness(&mac);
    click(&mut h, "Disk Map");
    click(&mut h, "Map Home Folder");
    click(&mut h, "Select Documents");
    click(&mut h, "Select Library");
    assert!(h.state().disk_map.selected.is_empty());
}
