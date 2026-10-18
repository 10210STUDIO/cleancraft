//! Similar Images: pictures that look alike (bursts, edits, resized or recompressed copies) in
//! `~/Pictures` (never inside Photos or other libraries, which are protected) and `~/Downloads`.
//!
//! Each picture longer than [`MIN_LEN`] bytes is decoded (JPEG, PNG, GIF, BMP, TIFF, WebP with
//! the `image` crate; HEIC and HEIF are first converted by macOS with
//! `sips -s format png -Z 256 <file> --out <cache>`, cached under
//! `<data>/Cache/Similar/`), reduced to 9 x 8 grey pixels, and turned into a 64-bit
//! difference hash: one bit per pair of neighbouring pixels, set when the left one is darker.
//! Two pictures are similar when their hashes differ in at most [`MAX_DISTANCE`] bits (of
//! 64); sets are built transitively (a looks like b, b like c: one set).
//!
//! In each set the suggested keeper is the picture with the most pixels, then the biggest file;
//! the others are selected. The keep-one-copy rule of [`super::duplicates`] applies.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use image::{DynamicImage, imageops::FilterType};

use crate::env::Env;
use crate::features::{Feature, ScanCtx};
use crate::plan::{FeatureId, Plan, Risk};

use super::duplicates::{KeepRule, apply_keep_rule, check_keeps};
use super::large_old::walk_area;

pub struct SimilarImages;

/// Only pictures longer than this (100 KB).
pub const MIN_LEN: u64 = 100_000;
/// Hashes at most this many bits apart are "similar". 10 of 64 tolerates resizing,
/// recompression and small edits while keeping different shots of a scene apart.
pub const MAX_DISTANCE: u32 = 10;

pub const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "bmp", "tif", "tiff", "webp", "heic", "heif",
];

fn ext(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn is_heic(p: &Path) -> bool {
    matches!(ext(p).as_str(), "heic" | "heif")
}

/// 64-bit difference hash.
pub fn dhash(img: &DynamicImage) -> u64 {
    let g = image::imageops::resize(&img.to_luma8(), 9, 8, FilterType::Triangle);
    let mut h = 0u64;
    for y in 0..8 {
        for x in 0..8 {
            let l = g.get_pixel(x, y)[0];
            let r = g.get_pixel(x + 1, y)[0];
            h = (h << 1) | u64::from(l < r);
        }
    }
    h
}

pub fn distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Where the PNG made from a HEIC picture is cached.
pub fn heic_thumb_path(env: &Env, p: &Path) -> PathBuf {
    let h = blake3::hash(p.to_string_lossy().as_bytes()).to_hex();
    env.data_dir()
        .join("Cache/Similar")
        .join(format!("{}.png", &h[..24]))
}

fn heic_thumb(env: &Env, p: &Path) -> Option<PathBuf> {
    let out = heic_thumb_path(env, p);
    let src_m = std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let out_m = std::fs::metadata(&out).and_then(|m| m.modified()).ok();
    if let (Some(s), Some(o)) = (src_m, out_m)
        && o >= s
    {
        return Some(out);
    }
    std::fs::create_dir_all(out.parent()?).ok()?;
    let src = p.to_string_lossy();
    let dst = out.to_string_lossy();
    let r = env
        .sys
        .run(
            "/usr/bin/sips",
            &["-s", "format", "png", "-Z", "256", &src, "--out", &dst],
        )
        .ok()?;
    if r.ok() && out.is_file() {
        Some(out)
    } else {
        None
    }
}

fn decode(env: &Env, p: &Path) -> Option<DynamicImage> {
    let file = if is_heic(p) {
        heic_thumb(env, p)?
    } else {
        p.to_path_buf()
    };
    image::ImageReader::open(&file)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()
}

/// `sips -g pixelWidth -g pixelHeight <file>` output -> (width, height).
pub fn parse_sips_size(out: &str) -> Option<(u32, u32)> {
    let mut w = None;
    let mut h = None;
    for l in out.lines() {
        let l = l.trim();
        if let Some(v) = l.strip_prefix("pixelWidth:") {
            w = v.trim().parse().ok();
        } else if let Some(v) = l.strip_prefix("pixelHeight:") {
            h = v.trim().parse().ok();
        }
    }
    Some((w?, h?))
}

/// Width and height of a picture without decoding it (HEIC through `sips`).
pub fn dimensions(env: &Env, p: &Path) -> Option<(u32, u32)> {
    if is_heic(p) {
        let path = p.to_string_lossy();
        let out = env
            .sys
            .run(
                "/usr/bin/sips",
                &["-g", "pixelWidth", "-g", "pixelHeight", &path],
            )
            .ok()?;
        return if out.ok() {
            parse_sips_size(&out.stdout)
        } else {
            None
        };
    }
    image::ImageReader::open(p)
        .ok()?
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

/// Pixels of a picture, 0 when unknown.
pub fn pixel_count(env: &Env, p: &Path) -> u64 {
    dimensions(env, p)
        .map(|(w, h)| u64::from(w) * u64::from(h))
        .unwrap_or(0)
}

/// Sets of indexes into `hashes` whose hashes are within [`MAX_DISTANCE`] (transitively).
pub fn cluster(hashes: &[u64]) -> Vec<Vec<usize>> {
    let n = hashes.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for i in 0..n {
        for j in i + 1..n {
            if distance(hashes[i], hashes[j]) <= MAX_DISTANCE {
                let a = find(&mut parent, i);
                let b = find(&mut parent, j);
                if a != b {
                    parent[b.max(a)] = a.min(b);
                }
            }
        }
    }
    let mut sets: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..n {
        let r = find(&mut parent, i);
        sets.entry(r).or_default().push(i);
    }
    sets.into_values().filter(|s| s.len() >= 2).collect()
}

impl Feature for SimilarImages {
    fn id(&self) -> FeatureId {
        FeatureId::SimilarPhotos
    }

    fn allow_roots(&self, env: &Env) -> Vec<PathBuf> {
        vec![env.home_path("Pictures"), env.home_path("Downloads")]
    }

    fn scan(&self, ctx: &ScanCtx<'_>) -> anyhow::Result<Plan> {
        let env = ctx.env;
        let allow = self.allow_roots(env);
        let mut plan = Plan::new(FeatureId::SimilarPhotos);
        plan.add_category(
            "similar",
            "Look-alike pictures",
            "Sets of pictures that look the same or almost the same. One stays in each set.",
            Risk::High,
        );
        let mut pics = Vec::new();
        let mut stopped = false;
        for root in &allow {
            if !root.is_dir() {
                continue;
            }
            let w = walk_area(ctx, root, &allow, MIN_LEN + 1, false);
            stopped |= w.stopped;
            for f in w.found {
                if IMAGE_EXTS.contains(&ext(&f.path).as_str()) {
                    pics.push(f);
                }
            }
        }
        let mut hashes = Vec::new();
        let mut kept = Vec::new();
        let mut unreadable = 0usize;
        for (n, f) in pics.iter().enumerate() {
            if ctx.cancelled() {
                stopped = true;
                break;
            }
            if n % 16 == 0 {
                (ctx.progress)(&env.display(&f.path));
            }
            match decode(env, &f.path) {
                Some(img) => {
                    hashes.push(dhash(&img));
                    kept.push(f);
                }
                None => unreadable += 1,
            }
        }
        for (n, set) in cluster(&hashes).into_iter().enumerate() {
            let key = format!("sim-{:04}", n + 1);
            let others = set.len() - 1;
            for i in set {
                let f = kept[i];
                let dims = dimensions(env, &f.path)
                    .map(|(w, h)| format!(", {w} x {h}"))
                    .unwrap_or_default();
                let reason = if others == 1 {
                    format!("Looks like 1 other picture{dims}")
                } else {
                    format!("Looks like {others} other pictures{dims}")
                };
                let id = plan.push(f.path.clone(), f.size, "similar", reason, Risk::High, false);
                plan.items[id].group = Some(key.clone());
                plan.items[id].modified = f
                    .md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64);
            }
        }
        apply_keep_rule(env, &mut plan, KeepRule::Smart);
        if unreadable > 0 {
            plan.notes.push(format!(
                "{unreadable} pictures could not be read and were left out."
            ));
        }
        if stopped {
            plan.notes.push("Scan stopped before the end.".into());
        }
        Ok(plan)
    }

    fn prepare(&self, _env: &Env, plan: &Plan) -> anyhow::Result<()> {
        check_keeps(plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sips_size() {
        let out = "/Users/a/IMG_1.heic\n  pixelWidth: 4032\n  pixelHeight: 3024\n";
        assert_eq!(parse_sips_size(out), Some((4032, 3024)));
        assert_eq!(parse_sips_size("nothing"), None);
    }

    #[test]
    fn clusters_are_transitive() {
        let a = 0u64;
        let b = 0b1111_1111u64; // 8 from a
        let c = 0xFFFFu64; // 8 from b, 16 from a
        let d = u64::MAX;
        let sets = cluster(&[a, b, c, d]);
        assert_eq!(sets, vec![vec![0, 1, 2]]);
    }
}
