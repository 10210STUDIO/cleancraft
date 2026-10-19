//! Drawn file-type icons for item rows: folders, archives, images, videos, documents, disk
//! images, logs, caches, property lists and more. Vector shapes only (no image files), so they
//! stay sharp at any size and cost nothing to load; thousands of rows are fine because only
//! the visible rows paint.
//!
//! `FileKind::of(path)` picks the kind from the name (and, for folders, one `stat`).
//! `paint(painter, rect, kind)` draws it in `rect` (square; any size from 14 points).
//! Real previews of images and videos come from `images::file_icon`, which falls back to these.

use std::path::Path;

use egui::epaint::{PathShape, PathStroke};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileKind {
    Folder,
    Document,
    Archive,
    Image,
    Video,
    Audio,
    DiskImage,
    Package,
    Log,
    Cache,
    Plist,
    App,
    Other,
}

impl FileKind {
    pub const ALL: [FileKind; 13] = [
        FileKind::Folder,
        FileKind::Document,
        FileKind::Archive,
        FileKind::Image,
        FileKind::Video,
        FileKind::Audio,
        FileKind::DiskImage,
        FileKind::Package,
        FileKind::Log,
        FileKind::Cache,
        FileKind::Plist,
        FileKind::App,
        FileKind::Other,
    ];

    /// The kind for a name alone (no disk access): by extension.
    pub fn of_name(path: &Path) -> Option<FileKind> {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        Some(match ext.as_str() {
            "app" => FileKind::App,
            "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "zst" | "lz" | "cpio"
            | "xip" => FileKind::Archive,
            "dmg" | "iso" | "img" | "sparseimage" | "sparsebundle" | "toast" | "cdr" => {
                FileKind::DiskImage
            }
            "pkg" | "mpkg" | "ipa" => FileKind::Package,
            "png" | "jpg" | "jpeg" | "heic" | "heif" | "gif" | "tif" | "tiff" | "webp" | "bmp"
            | "svg" | "dng" | "cr2" | "cr3" | "nef" | "arw" | "raf" | "orf" | "psd" | "icns" => {
                FileKind::Image
            }
            "mov" | "mp4" | "m4v" | "avi" | "mkv" | "webm" | "mpg" | "mpeg" | "3gp" => {
                FileKind::Video
            }
            "mp3" | "m4a" | "aac" | "wav" | "aif" | "aiff" | "flac" | "ogg" | "caf" | "m4r" => {
                FileKind::Audio
            }
            "pdf" | "doc" | "docx" | "pages" | "txt" | "rtf" | "md" | "key" | "numbers" | "xls"
            | "xlsx" | "ppt" | "pptx" | "csv" | "odt" | "epub" | "html" | "json" | "xml" => {
                FileKind::Document
            }
            "log" | "crash" | "ips" | "diag" | "spin" | "hang" | "panic" => FileKind::Log,
            "plist" => FileKind::Plist,
            "cache" | "db-wal" | "db-shm" => FileKind::Cache,
            _ => return None,
        })
    }

    /// The kind of an item on disk: by extension, then folders (inside `Caches` or `Logs`
    /// folders: caches and logs), then where the file lives.
    pub fn of(path: &Path) -> FileKind {
        if let Some(k) = FileKind::of_name(path) {
            return k;
        }
        let in_dir = |name: &str| {
            path.components()
                .any(|c| c.as_os_str().eq_ignore_ascii_case(name))
        };
        let cache = in_dir("Caches") || in_dir("Cache") || in_dir("CachedData");
        let log = in_dir("Logs") || in_dir("DiagnosticReports");
        if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
            return if cache {
                FileKind::Cache
            } else if log {
                FileKind::Log
            } else {
                FileKind::Folder
            };
        }
        if cache {
            FileKind::Cache
        } else if log {
            FileKind::Log
        } else {
            FileKind::Other
        }
    }

    /// The accent colour of the kind.
    pub fn color(self) -> Color32 {
        match self {
            FileKind::Folder => Color32::from_rgb(0x5C, 0xA8, 0xF5),
            FileKind::Document => Color32::from_rgb(0x4F, 0x86, 0xF0),
            FileKind::Archive => Color32::from_rgb(0xE0, 0xA1, 0x3A),
            FileKind::Image => Color32::from_rgb(0x3D, 0xB8, 0x7A),
            FileKind::Video => Color32::from_rgb(0x9A, 0x6B, 0xF2),
            FileKind::Audio => Color32::from_rgb(0xEC, 0x5F, 0x8E),
            FileKind::DiskImage => Color32::from_rgb(0xA7, 0xB0, 0xBF),
            FileKind::Package => Color32::from_rgb(0xC2, 0x8A, 0x52),
            FileKind::Log => Color32::from_rgb(0x3F, 0xB5, 0xB0),
            FileKind::Cache => Color32::from_rgb(0x8B, 0x95, 0xA8),
            FileKind::Plist => Color32::from_rgb(0xF0, 0x7E, 0x4A),
            FileKind::App => Color32::from_rgb(0x6E, 0x7D, 0xF7),
            FileKind::Other => Color32::from_rgb(0x9A, 0xA3, 0xB5),
        }
    }

    /// A short word for accessibility and tests.
    pub fn label(self) -> &'static str {
        match self {
            FileKind::Folder => "Folder",
            FileKind::Document => "Document",
            FileKind::Archive => "Archive",
            FileKind::Image => "Image",
            FileKind::Video => "Video",
            FileKind::Audio => "Audio",
            FileKind::DiskImage => "Disk image",
            FileKind::Package => "Installer package",
            FileKind::Log => "Log",
            FileKind::Cache => "Cache",
            FileKind::Plist => "Settings file",
            FileKind::App => "App",
            FileKind::Other => "File",
        }
    }
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    crate::icons::lerp(a, b, t)
}

/// Paint `kind` in `rect` (centred square inside it).
pub fn paint(p: &Painter, rect: Rect, kind: FileKind) {
    let s = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), vec2(s, s));
    // Unit coordinates (0..1) to points.
    let at = |x: f32, y: f32| pos2(r.left() + x * s, r.top() + y * s);
    let c = kind.color();
    match kind {
        FileKind::Folder => folder(p, r, c),
        FileKind::DiskImage => disk(p, r, at, c),
        FileKind::Package => package(p, r, at, c),
        FileKind::App => app(p, r, c),
        _ => {
            page(p, r, at, c);
            glyph(p, r, at, kind, c);
        }
    }
}

fn folder(p: &Painter, r: Rect, c: Color32) {
    let s = r.width();
    let back = mix(c, Color32::BLACK, 0.22);
    let tab = Rect::from_min_max(
        pos2(r.left() + 0.08 * s, r.top() + 0.18 * s),
        pos2(r.left() + 0.46 * s, r.top() + 0.34 * s),
    );
    p.rect_filled(tab, 0.06 * s, back);
    let body_back = Rect::from_min_max(
        pos2(r.left() + 0.08 * s, r.top() + 0.26 * s),
        pos2(r.right() - 0.08 * s, r.bottom() - 0.16 * s),
    );
    p.rect_filled(body_back, 0.08 * s, back);
    let front = Rect::from_min_max(
        pos2(r.left() + 0.08 * s, r.top() + 0.36 * s),
        pos2(r.right() - 0.08 * s, r.bottom() - 0.16 * s),
    );
    crate::icons::gradient_tile(
        p,
        front,
        0.08 * s,
        mix(c, Color32::WHITE, 0.18),
        mix(c, Color32::BLACK, 0.05),
    );
    p.hline(
        front.x_range().shrink(0.06 * s),
        front.top() + 0.06 * s,
        Stroke::new((0.025 * s).max(0.6), Color32::from_white_alpha(70)),
    );
}

/// A sheet of paper with a folded corner.
fn page(p: &Painter, r: Rect, at: impl Fn(f32, f32) -> Pos2, c: Color32) {
    let s = r.width();
    let paper = Color32::from_rgb(0xF1, 0xF3, 0xF8);
    let edge = mix(c, Color32::WHITE, 0.35);
    let outline = vec![
        at(0.20, 0.06),
        at(0.62, 0.06),
        at(0.82, 0.26),
        at(0.82, 0.94),
        at(0.20, 0.94),
    ];
    // Soft shadow.
    p.add(Shape::convex_polygon(
        outline.iter().map(|q| *q + vec2(0.0, 0.03 * s)).collect(),
        Color32::from_black_alpha(50),
        Stroke::NONE,
    ));
    p.add(Shape::convex_polygon(
        outline,
        paper,
        Stroke::new((0.02 * s).max(0.5), edge),
    ));
    // Folded corner.
    p.add(Shape::convex_polygon(
        vec![at(0.62, 0.06), at(0.82, 0.26), at(0.62, 0.26)],
        mix(paper, c, 0.35),
        Stroke::NONE,
    ));
}

fn glyph(p: &Painter, r: Rect, at: impl Fn(f32, f32) -> Pos2, kind: FileKind, c: Color32) {
    let s = r.width();
    let w = (0.055 * s).max(1.0);
    let line = |y: f32, x0: f32, x1: f32, col: Color32| {
        p.line_segment([at(x0, y), at(x1, y)], Stroke::new(w, col));
    };
    let frame = Rect::from_min_max(at(0.28, 0.40), at(0.74, 0.80));
    match kind {
        FileKind::Document => {
            for (i, x1) in [0.72, 0.72, 0.60, 0.70, 0.52].iter().enumerate() {
                line(0.38 + i as f32 * 0.11, 0.30, *x1, c);
            }
        }
        FileKind::Log => {
            for (i, x1) in [0.70, 0.62, 0.72, 0.56, 0.66].iter().enumerate() {
                let y = 0.38 + i as f32 * 0.11;
                p.circle_filled(at(0.32, y), w * 0.75, c);
                line(y, 0.38, *x1, mix(c, Color32::GRAY, 0.35));
            }
        }
        FileKind::Archive => {
            // A zipper down the middle.
            let x = 0.51;
            for i in 0..6 {
                let y = 0.10 + i as f32 * 0.09;
                let (x0, x1) = if i % 2 == 0 {
                    (x - 0.06, x)
                } else {
                    (x, x + 0.06)
                };
                p.rect_filled(Rect::from_min_max(at(x0, y), at(x1, y + 0.06)), 0.0, c);
            }
            p.rect_filled(
                Rect::from_min_max(at(x - 0.08, 0.64), at(x + 0.08, 0.84)),
                0.03 * s,
                mix(c, Color32::BLACK, 0.15),
            );
        }
        FileKind::Image => {
            p.rect_filled(frame, 0.06 * s, mix(c, Color32::WHITE, 0.55));
            p.add(Shape::convex_polygon(
                vec![at(0.28, 0.80), at(0.44, 0.56), at(0.58, 0.80)],
                c,
                Stroke::NONE,
            ));
            p.add(Shape::convex_polygon(
                vec![at(0.46, 0.80), at(0.60, 0.62), at(0.74, 0.80)],
                mix(c, Color32::BLACK, 0.2),
                Stroke::NONE,
            ));
            p.circle_filled(
                at(0.64, 0.50),
                0.05 * s,
                Color32::from_rgb(0xF6, 0xC3, 0x4A),
            );
        }
        FileKind::Video => {
            p.rect_filled(frame, 0.06 * s, c);
            p.add(Shape::convex_polygon(
                vec![at(0.45, 0.50), at(0.62, 0.60), at(0.45, 0.70)],
                Color32::WHITE,
                Stroke::NONE,
            ));
        }
        FileKind::Audio => {
            p.circle_filled(at(0.42, 0.74), 0.075 * s, c);
            p.line_segment([at(0.49, 0.74), at(0.49, 0.42)], Stroke::new(w, c));
            p.add(PathShape::line(
                vec![at(0.49, 0.42), at(0.64, 0.48), at(0.64, 0.56)],
                PathStroke::new(w, c),
            ));
        }
        FileKind::Plist => {
            let size = 0.36 * s;
            if size >= 5.0 {
                p.text(
                    at(0.51, 0.60),
                    Align2::CENTER_CENTER,
                    "{ }",
                    FontId::monospace(size),
                    c,
                );
            } else {
                line(0.6, 0.32, 0.70, c);
            }
        }
        FileKind::Cache => {
            // A small stack of discs.
            for i in 0..3 {
                let y = 0.70 - i as f32 * 0.13;
                p.rect_filled(
                    Rect::from_center_size(at(0.51, y), vec2(0.40 * s, 0.09 * s)),
                    0.045 * s,
                    mix(c, Color32::BLACK, i as f32 * 0.12),
                );
            }
        }
        _ => {}
    }
}

fn disk(p: &Painter, r: Rect, at: impl Fn(f32, f32) -> Pos2, c: Color32) {
    let s = r.width();
    let body = Rect::from_min_max(at(0.10, 0.28), at(0.90, 0.76));
    p.rect_filled(
        body.translate(vec2(0.0, 0.03 * s)),
        0.10 * s,
        Color32::from_black_alpha(50),
    );
    crate::icons::gradient_tile(
        p,
        body,
        0.10 * s,
        mix(c, Color32::WHITE, 0.35),
        mix(c, Color32::BLACK, 0.25),
    );
    p.hline(
        body.x_range().shrink(0.10 * s),
        body.center().y + 0.08 * s,
        Stroke::new((0.04 * s).max(0.8), mix(c, Color32::BLACK, 0.5)),
    );
    p.circle_filled(
        at(0.78, 0.40),
        0.035 * s,
        Color32::from_rgb(0x58, 0xD6, 0x8D),
    );
}

fn package(p: &Painter, r: Rect, at: impl Fn(f32, f32) -> Pos2, c: Color32) {
    let s = r.width();
    let top = mix(c, Color32::WHITE, 0.25);
    let left = c;
    let right = mix(c, Color32::BLACK, 0.25);
    let (n, e, so, w, mid, bl, br, bot) = (
        at(0.50, 0.10),
        at(0.88, 0.28),
        at(0.50, 0.46),
        at(0.12, 0.28),
        at(0.50, 0.46),
        at(0.12, 0.74),
        at(0.88, 0.74),
        at(0.50, 0.92),
    );
    p.add(Shape::convex_polygon(vec![n, e, so, w], top, Stroke::NONE));
    p.add(Shape::convex_polygon(
        vec![w, mid, bot, bl],
        left,
        Stroke::NONE,
    ));
    p.add(Shape::convex_polygon(
        vec![mid, e, br, bot],
        right,
        Stroke::NONE,
    ));
    p.line_segment(
        [at(0.31, 0.19), at(0.69, 0.37)],
        Stroke::new((0.05 * s).max(0.8), Color32::from_white_alpha(150)),
    );
}

fn app(p: &Painter, r: Rect, c: Color32) {
    let s = r.width();
    let tile = r.shrink(0.08 * s);
    crate::icons::gradient_tile(
        p,
        tile,
        0.22 * s,
        mix(c, Color32::WHITE, 0.12),
        mix(c, Color32::BLACK, 0.3),
    );
    let g = tile.shrink(0.24 * s);
    let h = g.width() * 0.42;
    for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        let q = Rect::from_min_size(
            g.min + vec2(dx * (g.width() - h), dy * (g.height() - h)),
            vec2(h, h),
        );
        p.rect_filled(q, h * 0.28, Color32::from_white_alpha(220));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_by_name() {
        let k = |n: &str| FileKind::of_name(Path::new(n));
        assert_eq!(k("a.ZIP"), Some(FileKind::Archive));
        assert_eq!(k("Install.dmg"), Some(FileKind::DiskImage));
        assert_eq!(k("x.pkg"), Some(FileKind::Package));
        assert_eq!(k("IMG_1.HEIC"), Some(FileKind::Image));
        assert_eq!(k("clip.mov"), Some(FileKind::Video));
        assert_eq!(k("song.m4a"), Some(FileKind::Audio));
        assert_eq!(k("Report.pdf"), Some(FileKind::Document));
        assert_eq!(k("system.log"), Some(FileKind::Log));
        assert_eq!(k("com.x.plist"), Some(FileKind::Plist));
        assert_eq!(k("Safari.app"), Some(FileKind::App));
        assert_eq!(k("noext"), None);
    }

    #[test]
    fn kinds_by_place() {
        let mac = cleancraft_core::testkit::FakeMac::new();
        let d = mac.home();
        let cache = d.join("Library/Caches/com.x");
        let logs = d.join("Library/Logs/x");
        let plain = d.join("Stuff");
        for p in [&cache, &logs, &plain] {
            std::fs::create_dir_all(p).unwrap();
        }
        assert_eq!(FileKind::of(&cache), FileKind::Cache);
        assert_eq!(FileKind::of(&logs), FileKind::Log);
        assert_eq!(FileKind::of(&plain), FileKind::Folder);
        assert_eq!(
            FileKind::of(&d.join("Library/Caches/blob")),
            FileKind::Cache
        );
        assert_eq!(FileKind::of(&d.join("Stuff/readme")), FileKind::Other);
        assert_eq!(
            FileKind::of(&d.join("Library/Caches/x.png")),
            FileKind::Image
        );
    }
}
