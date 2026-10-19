//! Real app icons and small file previews, as PNG files in CleanCraft's own cache
//! (`~/Library/Caches/CleanCraft/icons`).
//!
//! An app's icon comes from its bundle: `CFBundleIconFile` names an `.icns` file in
//! `Contents/Resources`, converted with `sips -s format png -Z <px> <icns> --out <png>`. Apps
//! that only ship an asset catalog (`CFBundleIconName`, `Assets.car`) get a Quick Look
//! thumbnail of the bundle (`qlmanage -t -s <px> -o <dir> <app>`), which is the icon Finder
//! shows. When both fail, AppKit draws the icon (`osascript -l JavaScript`, NSWorkspace). Every
//! tool runs through `env.sys`, so tests answer with [`crate::sys::FakeSys`].
//!
//! Cache files are named by a hash of the path, its modification times and the size, so an
//! updated app gets a new icon by itself. When every method failed, an empty `<key>.none`
//! marker stops CleanCraft from running the tools again for that same version of the app.
//!
//! Everything here blocks: the UI calls it from its background queue (`images.rs`), with a
//! bounded number of workers, never from the frame.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

use crate::env::Env;

/// Pixel sizes the cache holds. A request is rounded up to one of these.
pub const SIZES: [u32; 2] = [64, 128];
/// Where the cache lives, under the home folder.
pub const CACHE_REL: &str = "Library/Caches/CleanCraft/icons";
/// Files larger than this get no preview (decoding them would take too long).
pub const PREVIEW_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// How long one icon tool may run.
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(20);
/// Changes when the way icons are made changes, so old cache files are not reused.
const VERSION: &str = "icons-v1";

pub const SIPS: &str = "/usr/bin/sips";
pub const QLMANAGE: &str = "/usr/bin/qlmanage";
pub const OSASCRIPT: &str = "/usr/bin/osascript";

/// `osascript -l JavaScript -e <this> <app> <png> <px>`: AppKit draws the icon Finder shows for
/// `app` into a `px` x `px` bitmap and writes it as PNG. Prints "ok" when the file was written.
pub const WORKSPACE_SCRIPT: &str = "function run(argv) {\n\
ObjC.import('AppKit');\n\
var s = parseInt(argv[2], 10);\n\
var img = $.NSWorkspace.sharedWorkspace.iconForFile(argv[0]);\n\
var rep = $.NSBitmapImageRep.alloc.initWithBitmapDataPlanesPixelsWidePixelsHighBitsPerSampleSamplesPerPixelHasAlphaIsPlanarColorSpaceNameBytesPerRowBitsPerPixel(null, s, s, 8, 4, true, false, $.NSDeviceRGBColorSpace, 0, 0);\n\
$.NSGraphicsContext.saveGraphicsState;\n\
$.NSGraphicsContext.setCurrentContext($.NSGraphicsContext.graphicsContextWithBitmapImageRep(rep));\n\
img.drawInRectFromRectOperationFraction($.NSMakeRect(0, 0, s, s), $.NSZeroRect, $.NSCompositingOperationSourceOver, 1.0);\n\
$.NSGraphicsContext.restoreGraphicsState;\n\
var png = rep.representationUsingTypeProperties($.NSBitmapImageFileTypePNG, $());\n\
return png.writeToFileAtomically(argv[1], true) ? 'ok' : 'failed';\n\
}";

/// One `qlmanage` at a time: it writes `<name>.png` into a shared folder.
static QUICK_LOOK: Mutex<()> = Mutex::new(());

/// The cache size for a request of `px` pixels.
pub fn bucket(px: f32) -> u32 {
    if px <= SIZES[0] as f32 {
        SIZES[0]
    } else {
        SIZES[1]
    }
}

pub fn cache_dir(env: &Env) -> PathBuf {
    env.home_path(CACHE_REL)
}

/// Where an app's icon comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconSource {
    /// An `.icns` file inside the bundle.
    Icns(PathBuf),
    /// Only an asset catalog (`CFBundleIconName` or `Contents/Resources/Assets.car`).
    AssetCatalog,
    /// Nothing named: macOS shows the generic app icon.
    Unknown,
}

/// How an icon or preview was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Already in the cache.
    Cache,
    Sips,
    QuickLook,
    /// AppKit through `osascript`.
    Workspace,
    /// Decoded and scaled by CleanCraft (common image formats).
    Decode,
}

/// Read `CFBundleIconFile` / `CFBundleIconName` from the app's Info.plist.
pub fn icon_source(app: &Path) -> IconSource {
    let res = app.join("Contents/Resources");
    let dict = plist::Value::from_file(app.join("Contents/Info.plist"))
        .ok()
        .and_then(|v| v.into_dictionary());
    let key = |k: &str| {
        dict.as_ref()
            .and_then(|d| d.get(k))
            .and_then(|v| v.as_string())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    if let Some(name) = key("CFBundleIconFile") {
        // "AppIcon" and "AppIcon.icns" both name Resources/AppIcon.icns.
        let mut tries = vec![res.join(&name)];
        if !name.to_ascii_lowercase().ends_with(".icns") {
            tries.push(res.join(format!("{name}.icns")));
        }
        if let Some(p) = tries.into_iter().find(|p| p.is_file()) {
            return IconSource::Icns(p);
        }
    }
    if key("CFBundleIconName").is_some() || res.join("Assets.car").is_file() {
        return IconSource::AssetCatalog;
    }
    let fallback = res.join("AppIcon.icns");
    if fallback.is_file() {
        return IconSource::Icns(fallback);
    }
    IconSource::Unknown
}

/// "modified-nanoseconds:length" of a path, or `None` when it cannot be read.
fn stamp(p: &Path) -> Option<String> {
    let md = std::fs::metadata(p).ok()?;
    let ns = md
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(format!("{ns}:{}", md.len()))
}

fn hash(parts: &str) -> String {
    blake3::hash(parts.as_bytes()).to_hex()[..32].to_string()
}

/// The cache name of an app's icon: path, bundle and Info.plist modification times, size.
/// `None` when the app is gone.
pub fn app_key(env: &Env, app: &Path, px: u32) -> Option<String> {
    let bundle = stamp(app)?;
    let info = stamp(&app.join("Contents/Info.plist")).unwrap_or_default();
    Some(hash(&format!(
        "{VERSION}|app|{}|{bundle}|{info}|{px}",
        env.display(app)
    )))
}

/// The cache name of a file's preview.
pub fn preview_key(env: &Env, file: &Path, px: u32) -> Option<String> {
    let st = stamp(file)?;
    Some(hash(&format!(
        "{VERSION}|preview|{}|{st}|{px}",
        env.display(file)
    )))
}

/// The PNG an app's icon is (or will be) cached in.
pub fn app_icon_path(env: &Env, app: &Path, px: u32) -> Option<PathBuf> {
    app_key(env, app, px).map(|k| cache_dir(env).join(format!("{k}.png")))
}

/// The PNG a file's preview is (or will be) cached in.
pub fn preview_path(env: &Env, file: &Path, px: u32) -> Option<PathBuf> {
    preview_key(env, file, px).map(|k| cache_dir(env).join(format!("p-{k}.png")))
}

/// The marker written when no method could make the image at `png`.
pub fn failure_marker(png: &Path) -> PathBuf {
    png.with_extension("none")
}

/// True when `p` starts with the PNG signature.
pub fn is_png(p: &Path) -> bool {
    let mut sig = [0u8; 8];
    std::fs::File::open(p)
        .and_then(|mut f| f.read_exact(&mut sig))
        .is_ok()
        && sig == *b"\x89PNG\r\n\x1a\n"
}

/// A decoded image, straight RGBA rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// Decode a cached PNG for the UI. Large images are scaled down to at most 256 pixels.
pub fn load_png(p: &Path) -> Option<Rgba> {
    let img = image::ImageReader::open(p)
        .ok()?
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?;
    let img = if img.width() > 256 || img.height() > 256 {
        img.thumbnail(256, 256)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Some(Rgba {
        width: rgba.width(),
        height: rgba.height(),
        pixels: rgba.into_raw(),
    })
}

/// Why a method did not work, and whether the tool ran at all (a missing tool is not a
/// reason to give up on the app for good).
struct Miss {
    ran: bool,
    why: String,
}

fn run_tool(env: &Env, program: &str, args: &[&str]) -> Result<String, Miss> {
    match env.sys.run_timeout(program, args, TOOL_TIMEOUT) {
        Ok(out) if out.ok() => Ok(out.stdout),
        Ok(out) => Err(Miss {
            ran: true,
            why: format!(
                "{program} failed: {}",
                first_line(&out.stderr).unwrap_or("no message")
            ),
        }),
        // Not started, or stopped after its time limit: worth trying again another time.
        Err(e) => Err(Miss {
            ran: false,
            why: format!("{program}: {e}"),
        }),
    }
}

fn first_line(s: &str) -> Option<&str> {
    s.lines().map(str::trim).find(|l| !l.is_empty())
}

fn check_png(png: &Path, tool: &str) -> Result<(), Miss> {
    if is_png(png) {
        Ok(())
    } else {
        Err(Miss {
            ran: true,
            why: format!("{tool} wrote no PNG"),
        })
    }
}

/// `sips -s format png -Z <px> <input> --out <png>`.
fn sips(env: &Env, input: &Path, px: u32, png: &Path) -> Result<(), Miss> {
    let (input, out, size) = (env.display(input), env.display(png), px.to_string());
    run_tool(
        env,
        SIPS,
        &["-s", "format", "png", "-Z", &size, &input, "--out", &out],
    )?;
    check_png(png, "sips")
}

/// `qlmanage -t -s <px> -o <dir> <file>` writes `<dir>/<file name>.png`; it is renamed to
/// `png`. `qlmanage` can exit 0 without writing anything, so the file decides.
fn quick_look(env: &Env, file: &Path, px: u32, png: &Path) -> Result<(), Miss> {
    let _one = QUICK_LOOK.lock().unwrap_or_else(|p| p.into_inner());
    let dir = cache_dir(env).join("ql");
    std::fs::create_dir_all(&dir).map_err(|e| Miss {
        ran: false,
        why: format!("could not make the preview folder: {e}"),
    })?;
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let made = dir.join(format!("{name}.png"));
    // An earlier run's output must not pass for this one's.
    if made.exists() {
        let _ = std::fs::rename(&made, dir.join("previous.png"));
    }
    let (shown_dir, shown_file, size) = (env.display(&dir), env.display(file), px.to_string());
    run_tool(
        env,
        QLMANAGE,
        &["-t", "-s", &size, "-o", &shown_dir, &shown_file],
    )?;
    check_png(&made, "qlmanage")?;
    std::fs::rename(&made, png).map_err(|e| Miss {
        ran: true,
        why: format!("could not keep the Quick Look image: {e}"),
    })
}

/// AppKit draws the icon (`osascript -l JavaScript`, [`WORKSPACE_SCRIPT`]).
fn workspace(env: &Env, app: &Path, px: u32, png: &Path) -> Result<(), Miss> {
    let (shown_app, out, size) = (env.display(app), env.display(png), px.to_string());
    let said = run_tool(
        env,
        OSASCRIPT,
        &[
            "-l",
            "JavaScript",
            "-e",
            WORKSPACE_SCRIPT,
            &shown_app,
            &out,
            &size,
        ],
    )?;
    if said.trim() != "ok" {
        return Err(Miss {
            ran: true,
            why: format!("AppKit could not draw the icon: {}", said.trim()),
        });
    }
    check_png(png, "osascript")
}

/// Run `methods` in order until one makes `png`. When all ran and failed, the failure marker
/// is written so the next launch does not try again for this version of the file.
fn make(
    png: &Path,
    methods: &[(Method, &dyn Fn() -> Result<(), Miss>)],
) -> Result<(PathBuf, Method), String> {
    let mut why = Vec::new();
    let mut ran = false;
    for (m, f) in methods {
        match f() {
            Ok(()) => return Ok((png.to_path_buf(), *m)),
            Err(miss) => {
                ran |= miss.ran;
                why.push(miss.why);
            }
        }
    }
    if ran {
        let _ = std::fs::write(failure_marker(png), b"");
    }
    Err(if why.is_empty() {
        "no way to make an image for this file".into()
    } else {
        why.join("; ")
    })
}

/// Check the cache for `png`: `Some(Ok)` on a hit, `Some(Err)` when an earlier try failed for
/// this same version, `None` to make it now (the cache folder then exists).
fn cached(png: &Path) -> Option<Result<(PathBuf, Method), String>> {
    if is_png(png) {
        return Some(Ok((png.to_path_buf(), Method::Cache)));
    }
    if failure_marker(png).exists() {
        return Some(Err("no image could be made for this version earlier".into()));
    }
    if let Some(dir) = png.parent()
        && let Err(e) = std::fs::create_dir_all(dir)
    {
        return Some(Err(format!("could not make the icon cache: {e}")));
    }
    None
}

/// The PNG of `app`'s icon at `px` pixels (64 or 128), made on a cache miss. Blocks.
pub fn app_icon_png(env: &Env, app: &Path, px: u32) -> Result<(PathBuf, Method), String> {
    let px = bucket(px as f32);
    let png = app_icon_path(env, app, px).ok_or("the app could not be read")?;
    if let Some(r) = cached(&png) {
        return r;
    }
    let source = icon_source(app);
    let by_sips = || match &source {
        IconSource::Icns(icns) => sips(env, icns, px, &png),
        _ => Err(Miss {
            ran: false,
            why: "no .icns file".into(),
        }),
    };
    let by_ql = || quick_look(env, app, px, &png);
    let by_ws = || workspace(env, app, px, &png);
    make(
        &png,
        &[
            (Method::Sips, &by_sips),
            (Method::QuickLook, &by_ql),
            (Method::Workspace, &by_ws),
        ],
    )
}

/// How a file's preview would be made, or `None` when it gets a drawn icon only.
pub fn preview_method(file: &Path) -> Option<Method> {
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" | "webp" => Some(Method::Decode),
        "heic" | "heif" | "dng" | "cr2" | "cr3" | "nef" | "arw" | "raf" | "orf" => {
            Some(Method::Sips)
        }
        "mov" | "mp4" | "m4v" | "pdf" => Some(Method::QuickLook),
        _ => None,
    }
}

/// Decode and scale with the `image` crate; written next to `png` and renamed in place.
fn decode(file: &Path, px: u32, png: &Path) -> Result<(), Miss> {
    let miss = |why: String| Miss { ran: true, why };
    let img = image::ImageReader::open(file)
        .map_err(|e| miss(format!("could not open the image: {e}")))?
        .with_guessed_format()
        .map_err(|e| miss(format!("could not read the image: {e}")))?
        .decode()
        .map_err(|e| miss(format!("could not decode the image: {e}")))?;
    let tmp = png.with_extension("part");
    img.thumbnail(px, px)
        .save_with_format(&tmp, image::ImageFormat::Png)
        .map_err(|e| miss(format!("could not write the preview: {e}")))?;
    std::fs::rename(&tmp, png).map_err(|e| miss(format!("could not keep the preview: {e}")))
}

/// A small preview of a file (images, photos, videos, PDFs), made on a cache miss. Blocks.
pub fn preview_png(env: &Env, file: &Path, px: u32) -> Result<(PathBuf, Method), String> {
    let px = bucket(px as f32);
    let how = preview_method(file).ok_or("no preview for this kind of file")?;
    let md = std::fs::metadata(file).map_err(|e| format!("could not read the file: {e}"))?;
    if !md.is_file() {
        return Err("not a file".into());
    }
    if md.len() > PREVIEW_MAX_BYTES {
        return Err("too large for a quick preview".into());
    }
    let png = preview_path(env, file, px).ok_or("the file could not be read")?;
    if let Some(r) = cached(&png) {
        return r;
    }
    let by_decode = || decode(file, px, &png);
    let by_sips = || sips(env, file, px, &png);
    let by_ql = || quick_look(env, file, px, &png);
    match how {
        Method::Decode => make(&png, &[(Method::Decode, &by_decode)]),
        Method::Sips => make(
            &png,
            &[(Method::Sips, &by_sips), (Method::QuickLook, &by_ql)],
        ),
        _ => make(&png, &[(Method::QuickLook, &by_ql)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_round_up() {
        assert_eq!(bucket(16.0), 64);
        assert_eq!(bucket(64.0), 64);
        assert_eq!(bucket(76.0), 128);
        assert_eq!(bucket(512.0), 128);
    }

    #[test]
    fn previews_by_extension() {
        assert_eq!(preview_method(Path::new("a.JPG")), Some(Method::Decode));
        assert_eq!(preview_method(Path::new("a.heic")), Some(Method::Sips));
        assert_eq!(preview_method(Path::new("a.mov")), Some(Method::QuickLook));
        assert_eq!(preview_method(Path::new("a.log")), None);
        assert_eq!(preview_method(Path::new("folder")), None);
    }
}
