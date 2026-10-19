//! Test helpers for app icons and previews ([`crate::app_icons`]): bundles with an `.icns` or
//! an asset catalog, PNG bytes, and scripted `sips` / `qlmanage` / `osascript` answers that
//! write their output file the way the real tools do.

use std::path::{Path, PathBuf};

use crate::app_icons::{self, OSASCRIPT, QLMANAGE, SIPS, WORKSPACE_SCRIPT};
use crate::sys::CmdOut;
use crate::testkit::FakeMac;

/// A `w` x `h` PNG filled with one colour.
pub fn png_bytes(w: u32, h: u32, rgba: [u8; 4]) -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(w, h, image::Rgba(rgba));
    let mut out = std::io::Cursor::new(Vec::new());
    let _ = image::DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png);
    out.into_inner()
}

/// How an app names its icon in Info.plist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconKind<'a> {
    /// `CFBundleIconFile` = this name; `Contents/Resources/<name>[.icns]` is created.
    Icns(&'a str),
    /// `CFBundleIconName` and `Contents/Resources/Assets.car`, no `.icns`.
    AssetCatalog,
    /// No icon keys at all.
    None,
}

pub trait FakeMacIcons {
    /// `/Applications/<name>.app` whose Info.plist names its icon as `kind` says.
    fn icon_app(&self, name: &str, bundle_id: &str, kind: IconKind<'_>) -> PathBuf;
    /// `sips` turns `input` into the cached icon PNG (`out`) at `px`.
    fn sips_writes(&self, input: &Path, out: &Path, px: u32);
    /// `sips` fails for `input`.
    fn sips_fails(&self, input: &Path, out: &Path, px: u32);
    /// `qlmanage -t` writes `<name>.png` for `file` into the Quick Look folder.
    fn quicklook_writes(&self, file: &Path, px: u32);
    /// `qlmanage -t` exits 0 for `file` but writes nothing (what it does for files it cannot
    /// preview).
    fn quicklook_silent(&self, file: &Path, px: u32);
    /// AppKit (osascript) draws `app`'s icon into `out`.
    fn workspace_writes(&self, app: &Path, out: &Path, px: u32);
}

pub fn sips_line(mac: &FakeMac, input: &Path, out: &Path, px: u32) -> String {
    format!(
        "{SIPS} -s format png -Z {px} {} --out {}",
        mac.env.display(input),
        mac.env.display(out)
    )
}

pub fn quicklook_line(mac: &FakeMac, file: &Path, px: u32) -> String {
    format!(
        "{QLMANAGE} -t -s {px} -o {} {}",
        mac.env.display(&quicklook_dir(mac)),
        mac.env.display(file)
    )
}

pub fn workspace_line(mac: &FakeMac, app: &Path, out: &Path, px: u32) -> String {
    format!(
        "{OSASCRIPT} -l JavaScript -e {WORKSPACE_SCRIPT} {} {} {px}",
        mac.env.display(app),
        mac.env.display(out)
    )
}

pub fn quicklook_dir(mac: &FakeMac) -> PathBuf {
    app_icons::cache_dir(&mac.env).join("ql")
}

impl FakeMacIcons for FakeMac {
    fn icon_app(&self, name: &str, bundle_id: &str, kind: IconKind<'_>) -> PathBuf {
        let app = self.app(name, bundle_id, "1.0");
        let info = app.join("Contents/Info.plist");
        let mut d = plist::Value::from_file(&info)
            .ok()
            .and_then(|v| v.into_dictionary())
            .unwrap_or_default();
        let res = app.join("Contents/Resources");
        match kind {
            IconKind::Icns(file) => {
                d.insert("CFBundleIconFile".into(), file.into());
                let on_disk = if file.ends_with(".icns") {
                    file.to_string()
                } else {
                    format!("{file}.icns")
                };
                self.write(&res.join(on_disk), b"icns fake");
            }
            IconKind::AssetCatalog => {
                d.insert("CFBundleIconName".into(), "AppIcon".into());
                self.write(&res.join("Assets.car"), b"car fake");
            }
            IconKind::None => {}
        }
        let _ = plist::Value::Dictionary(d).to_file_xml(&info);
        app
    }

    fn sips_writes(&self, input: &Path, out: &Path, px: u32) {
        if let Some(f) = self.env.fake_sys() {
            f.answer_creating(
                &sips_line(self, input, out, px),
                "",
                out,
                &png_bytes(px, px, [40, 120, 220, 255]),
            );
        }
    }

    fn sips_fails(&self, input: &Path, out: &Path, px: u32) {
        if let Some(f) = self.env.fake_sys() {
            f.answer_full(
                &sips_line(self, input, out, px),
                CmdOut {
                    status: 1,
                    stdout: String::new(),
                    stderr: "Error: Unable to render destination image\n".into(),
                },
            );
        }
    }

    fn quicklook_writes(&self, file: &Path, px: u32) {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let made = quicklook_dir(self).join(format!("{name}.png"));
        if let Some(f) = self.env.fake_sys() {
            f.answer_creating(
                &quicklook_line(self, file, px),
                &format!("Testing Quick Look thumbnails with files:\n\t{}\n* {}: Done producing thumbnails\n", self.env.display(file), self.env.display(file)),
                &made,
                &png_bytes(px, px, [230, 120, 40, 255]),
            );
        }
    }

    fn quicklook_silent(&self, file: &Path, px: u32) {
        if let Some(f) = self.env.fake_sys() {
            f.answer(
                &quicklook_line(self, file, px),
                "Testing Quick Look thumbnails with files:\n* no thumbnail created\n",
            );
        }
    }

    fn workspace_writes(&self, app: &Path, out: &Path, px: u32) {
        if let Some(f) = self.env.fake_sys() {
            f.answer_creating(
                &workspace_line(self, app, out, px),
                "ok\n",
                out,
                &png_bytes(px, px, [120, 200, 80, 255]),
            );
        }
    }
}
