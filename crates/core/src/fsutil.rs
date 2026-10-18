//! Sizes and walking. One size formula everywhere, so the numbers on screen can be recalculated
//! in tests: bytes allocated on disk (`st_blocks * 512`) on Unix, file length elsewhere.
//! Symbolic links count as their own (tiny) size and are never followed.

use std::collections::HashSet;
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Size of one directory entry without following links.
pub fn entry_size(md: &Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        md.blocks() * 512
    }
    #[cfg(not(unix))]
    {
        md.len()
    }
}

/// A file's identity for hard links: (device, inode) when it has more than one link.
pub fn link_id(md: &Metadata) -> Option<(u64, u64)> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        (!md.is_dir() && md.nlink() > 1).then(|| (md.dev(), md.ino()))
    }
    #[cfg(not(unix))]
    {
        let _ = md;
        None
    }
}

/// Total size of a file or folder tree, without following links. A file with several hard
/// links inside the tree counts once (its data is on disk once), like the Disk Map.
/// Unreadable parts are skipped and counted in the second value.
pub fn tree_size(p: &Path) -> (u64, u32) {
    tree_size_once(p, &mut HashSet::new())
}

/// [`tree_size`] sharing `seen` with other trees, so a hard-linked file counts once across a
/// whole list (a plan, an executor report).
pub fn tree_size_once(p: &Path, seen: &mut HashSet<(u64, u64)>) -> (u64, u32) {
    let mut total = 0u64;
    let mut unreadable = 0u32;
    let Ok(md) = fs::symlink_metadata(p) else {
        return (0, 1);
    };
    if link_id(&md).is_none_or(|id| seen.insert(id)) {
        total += entry_size(&md);
    }
    if md.is_dir() {
        match fs::read_dir(p) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let (s, u) = tree_size_once(&e.path(), seen);
                    total += s;
                    unreadable += u;
                }
            }
            Err(_) => unreadable += 1,
        }
    }
    (total, unreadable)
}

/// [`tree_size`] of `p`, and the files in it that have other hard links, with their own size.
pub fn tree_size_with_links(p: &Path) -> (u64, Vec<((u64, u64), u64)>) {
    fn walk(p: &Path, seen: &mut HashSet<(u64, u64)>, links: &mut Vec<((u64, u64), u64)>) -> u64 {
        let Ok(md) = fs::symlink_metadata(p) else {
            return 0;
        };
        let mut total = 0;
        match link_id(&md) {
            Some(id) => {
                if seen.insert(id) {
                    total += entry_size(&md);
                    links.push((id, entry_size(&md)));
                }
            }
            None => total += entry_size(&md),
        }
        if md.is_dir()
            && let Ok(rd) = fs::read_dir(p)
        {
            for e in rd.flatten() {
                total += walk(&e.path(), seen, links);
            }
        }
        total
    }
    let mut links = Vec::new();
    let total = walk(p, &mut HashSet::new(), &mut links);
    (total, links)
}

/// Number of files (non-directories) in a tree.
pub fn tree_file_count(p: &Path) -> u64 {
    let Ok(md) = fs::symlink_metadata(p) else {
        return 0;
    };
    if !md.is_dir() {
        return 1;
    }
    fs::read_dir(p)
        .map(|rd| rd.flatten().map(|e| tree_file_count(&e.path())).sum())
        .unwrap_or(0)
}

/// Immediate children, sorted by name. Missing or unreadable folders give an empty list.
pub fn children(p: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(p)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

/// Every file under `p` (not following links), with metadata. `skip` prunes folders.
pub fn walk_files(p: &Path, skip: &dyn Fn(&Path) -> bool, out: &mut Vec<(PathBuf, Metadata)>) {
    let Ok(md) = fs::symlink_metadata(p) else {
        return;
    };
    if skip(p) {
        return;
    }
    if md.is_dir() {
        for c in children(p) {
            walk_files(&c, skip, out);
        }
    } else if md.is_file() {
        out.push((p.to_path_buf(), md));
    }
}

pub fn modified_days_ago(md: &Metadata, now: SystemTime) -> Option<u64> {
    let m = md.modified().ok()?;
    now.duration_since(m).ok().map(|d| d.as_secs() / 86_400)
}

pub fn accessed_days_ago(md: &Metadata, now: SystemTime) -> Option<u64> {
    let m = md.accessed().ok()?;
    now.duration_since(m).ok().map(|d| d.as_secs() / 86_400)
}

/// How many calendar days back `t` is from `now` on the user's local calendar (seconds since
/// the epoch): 0 for today, 1 for yesterday, even when less than 24 hours passed. Used for
/// "today" / "yesterday" / "N days ago" texts; thresholds such as "not opened for 365 days"
/// keep counting full days.
pub fn calendar_days_ago(t: i64, now: i64) -> i64 {
    use chrono::{Local, TimeZone};
    let day = |s: i64| Local.timestamp_opt(s, 0).earliest().map(|d| d.date_naive());
    match (day(t), day(now)) {
        (Some(a), Some(b)) => (b - a).num_days().max(0),
        _ => ((now - t) / 86_400).max(0),
    }
}

/// "1 item", "2 items": a count with its noun in the right number.
pub fn count(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// Decimal units like Finder: 1 KB = 1000 bytes. "Zero KB" for 0. Three significant digits;
/// a value that rounds up to the next precision or unit is shown there ("1.00 MB", not
/// "1000 KB").
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if bytes == 0 {
        return "Zero KB".into();
    }
    if bytes == 1 {
        return "1 byte".into();
    }
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut v = bytes as f64 / 1000.0;
    let mut i = 0;
    loop {
        while v >= 1000.0 && i < UNITS.len() - 1 {
            v /= 1000.0;
            i += 1;
        }
        // Kilobytes are whole numbers; otherwise 3 significant digits.
        let mut dec: i32 = if i == 0 || v >= 100.0 {
            0
        } else if v >= 10.0 {
            1
        } else {
            2
        };
        let round = |v: f64, dec: i32| (v * 10f64.powi(dec)).round() / 10f64.powi(dec);
        let mut r = round(v, dec);
        if r >= 1000.0 && i < UNITS.len() - 1 {
            v = r / 1000.0;
            i += 1;
            continue;
        }
        while dec > 0 && r >= 10f64.powi(3 - dec) {
            dec -= 1;
            r = round(v, dec);
        }
        return format!("{:.*} {}", dec as usize, r, UNITS[i]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human() {
        assert_eq!(human_size(0), "Zero KB");
        assert_eq!(human_size(512), "512 bytes");
        assert_eq!(human_size(4096), "4 KB");
        assert_eq!(human_size(1_500_000), "1.50 MB");
        assert_eq!(human_size(25_300_000_000), "25.3 GB");
        assert_eq!(human_size(250_000_000_000), "250 GB");
        assert_eq!(human_size(1), "1 byte");
        assert_eq!(human_size(999_500), "1.00 MB");
        assert_eq!(human_size(9_996_000), "10.0 MB");
        assert_eq!(human_size(99_960_000), "100 MB");
        assert_eq!(human_size(9_994_000), "9.99 MB");
        assert_eq!(count(1, "item", "items"), "1 item");
        let now = chrono::Local::now().timestamp();
        assert_eq!(calendar_days_ago(now, now), 0);
        // One minute before local midnight is yesterday, whatever the hour now.
        use chrono::TimeZone;
        let midnight = chrono::Local
            .from_local_datetime(
                &chrono::Local::now()
                    .date_naive()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
            )
            .earliest()
            .unwrap()
            .timestamp();
        assert_eq!(calendar_days_ago(midnight - 60, now), 1);
        assert_eq!(calendar_days_ago(midnight + 60, now.max(midnight + 60)), 0);
        assert_eq!(count(0, "item", "items"), "0 items");
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_count_once() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        fs::create_dir_all(&a).unwrap();
        fs::write(a.join("x"), vec![1u8; 40_000]).unwrap();
        fs::hard_link(a.join("x"), a.join("y")).unwrap();
        let one = entry_size(&fs::symlink_metadata(a.join("x")).unwrap());
        let dir = entry_size(&fs::symlink_metadata(&a).unwrap());
        assert_eq!(tree_size(&a).0, dir + one);
        // Across trees with a shared set: the second link adds nothing.
        let mut seen = HashSet::new();
        assert_eq!(tree_size_once(&a.join("x"), &mut seen).0, one);
        assert_eq!(tree_size_once(&a.join("y"), &mut seen).0, 0);
    }

    #[test]
    fn sizes_add_up() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        fs::create_dir_all(a.join("b")).unwrap();
        fs::write(a.join("x"), vec![1u8; 10_000]).unwrap();
        fs::write(a.join("b/y"), vec![1u8; 5_000]).unwrap();
        let (s, u) = tree_size(&a);
        assert_eq!(u, 0);
        let expect: u64 = [a.clone(), a.join("b"), a.join("x"), a.join("b/y")]
            .iter()
            .map(|p| entry_size(&fs::symlink_metadata(p).unwrap()))
            .sum();
        assert_eq!(s, expect);
        assert_eq!(tree_file_count(&a), 2);
    }
}
