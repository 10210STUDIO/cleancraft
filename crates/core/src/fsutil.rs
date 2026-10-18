//! Sizes and walking. One size formula everywhere, so the numbers on screen can be recalculated
//! in tests: bytes allocated on disk (`st_blocks * 512`) on Unix, file length elsewhere.
//! Symbolic links count as their own (tiny) size and are never followed.

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

/// Total size of a file or folder tree, without following links. Unreadable parts are
/// skipped and counted in the second value.
pub fn tree_size(p: &Path) -> (u64, u32) {
    let mut total = 0u64;
    let mut unreadable = 0u32;
    let Ok(md) = fs::symlink_metadata(p) else {
        return (0, 1);
    };
    total += entry_size(&md);
    if md.is_dir() {
        match fs::read_dir(p) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let (s, u) = tree_size(&e.path());
                    total += s;
                    unreadable += u;
                }
            }
            Err(_) => unreadable += 1,
        }
    }
    (total, unreadable)
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

/// Decimal units like Finder: 1 KB = 1000 bytes. "Zero KB" for 0.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KB", "MB", "GB", "TB", "PB"];
    if bytes == 0 {
        return "Zero KB".into();
    }
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    let mut v = bytes as f64 / 1000.0;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if v >= 100.0 || i == 0 {
        format!("{:.0} {}", v, UNITS[i])
    } else if v >= 10.0 {
        format!("{:.1} {}", v, UNITS[i])
    } else {
        format!("{:.2} {}", v, UNITS[i])
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
