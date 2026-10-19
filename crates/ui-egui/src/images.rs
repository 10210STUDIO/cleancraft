//! Real app icons and file previews on screen.
//!
//! Pages call [`app_icon`] / [`paint_app_icon`] for apps and [`file_icon`] /
//! [`paint_file_icon`] for item rows. The first call for a path shows the placeholder (a
//! neutral rounded tile for apps, the drawn kind icon for files) and queues the work; a
//! bounded pool of background workers ([`WORKERS`]) runs `sips` / `qlmanage` through
//! `cleancraft_core::app_icons`, which caches PNGs in `~/Library/Caches/CleanCraft/icons`.
//! When a PNG is decoded it becomes an egui texture and fades in. The frame never waits for a
//! tool: only rows that are painted ask, and a long scroll drops requests that went stale
//! ([`QUEUE_MAX`]).
//!
//! The loader lives in the egui context ([`install`] / [`ensure`]); with none installed
//! (UI tests that do not ask for it) every call paints the fallback: the coloured initial
//! tile for apps, which also stays when no tool could make an icon.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::Duration;

use cleancraft_core::Env;
use cleancraft_core::app_icons::{self, Rgba};
use egui::{Color32, Painter, Rect, Response, Sense, TextureHandle, Ui, vec2};

use crate::file_icons::{self, FileKind};
use crate::kit;

/// Background workers: at most this many icon tools run at once.
pub const WORKERS: usize = 2;
/// Requests waiting beyond this many: the oldest are dropped (asked again when visible).
pub const QUEUE_MAX: usize = 96;
/// Fade-in time of a loaded image, seconds.
pub const FADE: f64 = 0.18;
/// Textures kept; past this the least recently painted are released.
pub const TEXTURE_CAP: usize = 768;

/// How the loader runs its work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Right away, inside the frame that asks (tests and snapshots).
    Sync,
    /// Queued until [`pump`] is called (tests of the loading state).
    Manual,
    /// On this many background threads (the app).
    Threads(usize),
}

/// What an icon slot showed the last time it was painted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Placeholder,
    Image,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    px: u32,
    preview: bool,
}

enum Entry {
    Pending,
    Ready {
        tex: TextureHandle,
        /// When it became ready (fade start); minus infinity when it never showed a
        /// placeholder.
        since: f64,
        used: u64,
    },
    Failed,
}

/// The work queue shared with the workers.
struct Queue {
    env: Env,
    jobs: Mutex<VecDeque<Key>>,
    wake: Condvar,
    done: Mutex<Vec<(Key, Option<Rgba>)>>,
    ctx: egui::Context,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Queue {
    fn work(env: &Env, k: &Key) -> Option<Rgba> {
        let made = if k.preview {
            app_icons::preview_png(env, &k.path, k.px)
        } else {
            app_icons::app_icon_png(env, &k.path, k.px)
        };
        made.ok().and_then(|(png, _)| app_icons::load_png(&png))
    }

    fn run_one(&self) -> bool {
        let Some(k) = lock(&self.jobs).pop_back() else {
            return false;
        };
        let img = Queue::work(&self.env, &k);
        lock(&self.done).push((k, img));
        true
    }
}

fn worker(q: Weak<Queue>) {
    loop {
        let Some(q) = q.upgrade() else {
            return;
        };
        let job = {
            let mut g = lock(&q.jobs);
            if g.is_empty() {
                g = q
                    .wake
                    .wait_timeout(g, Duration::from_millis(500))
                    .map(|(g, _)| g)
                    .unwrap_or_else(|p| p.into_inner().0);
            }
            // Newest first: what is on screen now.
            g.pop_back()
        };
        if let Some(k) = job {
            let img = Queue::work(&q.env, &k);
            lock(&q.done).push((k, img));
            q.ctx.request_repaint();
        }
    }
}

struct Images {
    mode: Mode,
    queue: Arc<Queue>,
    entries: HashMap<Key, Entry>,
    shown: HashMap<PathBuf, Shown>,
}

type Shared = Arc<Mutex<Images>>;

fn store_id() -> egui::Id {
    egui::Id::new("cleancraft-images")
}

fn shared(ctx: &egui::Context) -> Option<Shared> {
    ctx.data(|d| d.get_temp::<Shared>(store_id()))
}

/// Start the loader for `env` (replacing any earlier one).
pub fn install(ctx: &egui::Context, env: &Env, mode: Mode) {
    let queue = Arc::new(Queue {
        env: env.clone(),
        jobs: Mutex::new(VecDeque::new()),
        wake: Condvar::new(),
        done: Mutex::new(Vec::new()),
        ctx: ctx.clone(),
    });
    if let Mode::Threads(n) = mode {
        for i in 0..n.max(1) {
            let w = Arc::downgrade(&queue);
            let _ = std::thread::Builder::new()
                .name(format!("cleancraft-icons-{i}"))
                .spawn(move || worker(w));
        }
    }
    let images: Shared = Arc::new(Mutex::new(Images {
        mode,
        queue,
        entries: HashMap::new(),
        shown: HashMap::new(),
    }));
    ctx.data_mut(|d| d.insert_temp(store_id(), images));
}

/// Called every frame by the app: starts the background loader once on a real Mac. Tests
/// (`sync`) and fake Macs get none unless they [`install`] one.
pub fn ensure(ctx: &egui::Context, env: &Env, sync: bool) {
    if sync || env.is_fake() || shared(ctx).is_some() {
        return;
    }
    install(ctx, env, Mode::Threads(WORKERS));
}

/// [`Mode::Manual`]: do every queued request now. Returns how many ran.
pub fn pump(ctx: &egui::Context) -> usize {
    let Some(s) = shared(ctx) else {
        return 0;
    };
    let q = lock(&s).queue.clone();
    let mut n = 0;
    while q.run_one() {
        n += 1;
    }
    ctx.request_repaint();
    n
}

/// What the slot for `path` showed when last painted (tests).
pub fn shown(ctx: &egui::Context, path: &Path) -> Option<Shown> {
    shared(ctx).and_then(|s| lock(&s).shown.get(path).copied())
}

/// Requests waiting for a worker.
pub fn queued(ctx: &egui::Context) -> usize {
    shared(ctx).map_or(0, |s| {
        let q = lock(&s).queue.clone();
        lock(&q.jobs).len()
    })
}

/// Textures held now.
pub fn textures(ctx: &egui::Context) -> usize {
    shared(ctx).map_or(0, |s| {
        lock(&s)
            .entries
            .values()
            .filter(|e| matches!(e, Entry::Ready { .. }))
            .count()
    })
}

fn texture(ctx: &egui::Context, k: &Key, img: &Rgba) -> TextureHandle {
    let ci = egui::ColorImage::from_rgba_unmultiplied(
        [img.width as usize, img.height as usize],
        &img.pixels,
    );
    let opts = egui::TextureOptions {
        mipmap_mode: Some(egui::TextureFilter::Linear),
        ..egui::TextureOptions::LINEAR
    };
    ctx.load_texture(
        format!("cc-icon:{}:{}:{}", k.preview, k.px, k.path.display()),
        ci,
        opts,
    )
}

impl Images {
    /// Turn finished work into textures.
    fn collect(&mut self, ctx: &egui::Context, now: f64) {
        let done = std::mem::take(&mut *lock(&self.queue.done));
        let frame = ctx.cumulative_frame_nr();
        for (k, img) in done {
            if !self.entries.contains_key(&k) {
                continue; // dropped while it ran
            }
            let e = match img {
                Some(img) => Entry::Ready {
                    tex: texture(ctx, &k, &img),
                    since: now,
                    used: frame,
                },
                None => Entry::Failed,
            };
            self.entries.insert(k, e);
        }
        self.trim();
    }

    fn trim(&mut self) {
        let mut ready: Vec<(u64, Key)> = self
            .entries
            .iter()
            .filter_map(|(k, e)| match e {
                Entry::Ready { used, .. } => Some((*used, k.clone())),
                _ => None,
            })
            .collect();
        if ready.len() <= TEXTURE_CAP {
            return;
        }
        ready.sort_by_key(|(u, _)| *u);
        for (_, k) in ready.into_iter().take(TEXTURE_CAP / 4) {
            self.entries.remove(&k);
        }
    }

    /// Ask for `k` (first time it is painted).
    fn request(&mut self, ctx: &egui::Context, k: Key) {
        match self.mode {
            Mode::Sync => {
                let e = match Queue::work(&self.queue.env, &k) {
                    Some(img) => Entry::Ready {
                        tex: texture(ctx, &k, &img),
                        since: f64::NEG_INFINITY,
                        used: ctx.cumulative_frame_nr(),
                    },
                    None => Entry::Failed,
                };
                self.entries.insert(k, e);
            }
            Mode::Manual | Mode::Threads(_) => {
                self.entries.insert(k.clone(), Entry::Pending);
                let mut jobs = lock(&self.queue.jobs);
                jobs.push_back(k);
                while jobs.len() > QUEUE_MAX {
                    if let Some(old) = jobs.pop_front() {
                        self.entries.remove(&old);
                    }
                }
                drop(jobs);
                self.queue.wake.notify_one();
            }
        }
    }
}

/// Ease out (cubic) for the fade.
fn ease(x: f64) -> f32 {
    let x = x.clamp(0.0, 1.0);
    (1.0 - (1.0 - x).powi(3)) as f32
}

/// What a placeholder stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wait {
    /// The image is on its way.
    Loading,
    /// No image will come (no loader, or every tool failed).
    Never,
}

/// Paint `k` in `rect`, or the placeholder. Returns what was shown.
fn paint(
    ui: &Ui,
    rect: Rect,
    k: Key,
    corner: f32,
    placeholder: &dyn Fn(&Painter, Rect, Wait),
) -> Shown {
    let ctx = ui.ctx();
    let Some(s) = shared(ctx) else {
        placeholder(ui.painter(), rect, Wait::Never);
        return Shown::Placeholder;
    };
    let now = ctx.input(|i| i.time);
    let mut g = lock(&s);
    g.collect(ctx, now);
    if !g.entries.contains_key(&k) {
        g.request(ctx, k.clone());
    }
    let frame = ctx.cumulative_frame_nr();
    let path = k.path.clone();
    let shown = match g.entries.get_mut(&k) {
        Some(Entry::Ready { tex, since, used }) => {
            *used = frame;
            let a = ease((now - *since) / FADE);
            if a < 1.0 {
                let mut p = ui.painter().clone();
                p.multiply_opacity(1.0 - a);
                placeholder(&p, rect, Wait::Loading);
                ctx.request_repaint();
            }
            let size = tex.size_vec2();
            let scale = (rect.width() / size.x).min(rect.height() / size.y);
            let grow = 0.94 + 0.06 * a;
            let r = Rect::from_center_size(rect.center(), size * scale * grow);
            egui::Image::from_texture(egui::load::SizedTexture::from_handle(tex))
                .corner_radius(corner)
                .tint(Color32::WHITE.gamma_multiply(a))
                .paint_at(ui, r);
            Shown::Image
        }
        Some(Entry::Failed) => {
            placeholder(ui.painter(), rect, Wait::Never);
            Shown::Placeholder
        }
        Some(Entry::Pending) | None => {
            placeholder(ui.painter(), rect, Wait::Loading);
            Shown::Placeholder
        }
    };
    g.shown.insert(path, shown);
    shown
}

fn px(ui: &Ui, size_pt: f32) -> u32 {
    app_icons::bucket(size_pt * ui.ctx().pixels_per_point())
}

/// The name an app's placeholder tile shows.
fn app_name(app_path: &Path) -> String {
    app_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The tile shown while an app icon loads: a quiet rounded square in the shape of a macOS
/// icon, no letter and no colour, so a list of loading rows reads as calm (REVIEW-A 44).
pub fn loading_tile(p: &Painter, r: Rect) {
    // macOS app icons: a rounded square inset ~10% with corners ~22.5% of its side.
    let tile = r.shrink(r.width() * 0.08);
    let round = tile.width() * 0.225;
    p.rect(
        tile,
        round,
        LOADING_FILL,
        egui::Stroke::new(1.0, LOADING_EDGE),
        egui::StrokeKind::Inside,
    );
}

/// Fill and edge of [`loading_tile`]: grey, so no app looks like another while it loads.
pub const LOADING_FILL: Color32 = Color32::from_rgba_premultiplied(16, 16, 16, 16);
pub const LOADING_EDGE: Color32 = Color32::from_rgba_premultiplied(24, 24, 24, 24);

/// Paint an app's real icon in `rect`: the neutral [`loading_tile`] while it loads, the coloured
/// initial tile when no icon can be made (or no loader runs). `app_path` is the bundle path
/// in the env (as `AppInfo::path`).
pub fn paint_app_icon(ui: &Ui, rect: Rect, app_path: &Path) -> Shown {
    let name = app_name(app_path);
    let k = Key {
        path: app_path.to_path_buf(),
        px: px(ui, rect.width().max(rect.height())),
        preview: false,
    };
    paint(ui, rect, k, 0.0, &|p, r, wait| match wait {
        Wait::Loading => loading_tile(p, r),
        Wait::Never => kit::initial_tile(p, r, &name, kit::name_color(&name)),
    })
}

/// An app icon of `size_pt` points, allocated in the layout.
pub fn app_icon(ui: &mut Ui, app_path: &Path, size_pt: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size_pt, size_pt), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_app_icon(ui, rect, app_path);
    }
    resp
}

/// Paint a file's icon in `rect`: a small preview for images and videos once it is ready,
/// else the drawn icon of `kind`.
pub fn paint_file_icon(ui: &Ui, rect: Rect, path: &Path, kind: FileKind) -> Shown {
    if kind == FileKind::App {
        return paint_app_icon(ui, rect, path);
    }
    let drawn = |p: &Painter, r: Rect, _: Wait| file_icons::paint(p, r, kind);
    if app_icons::preview_method(path).is_none() || shared(ui.ctx()).is_none() {
        drawn(ui.painter(), rect, Wait::Never);
        return Shown::Placeholder;
    }
    let k = Key {
        path: path.to_path_buf(),
        px: px(ui, rect.width().max(rect.height())),
        preview: true,
    };
    paint(ui, rect, k, (rect.width() * 0.16).min(6.0), &drawn)
}

/// A file icon of `size_pt` points, allocated in the layout.
pub fn file_icon(ui: &mut Ui, path: &Path, kind: FileKind, size_pt: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size_pt, size_pt), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint_file_icon(ui, rect, path, kind);
    }
    resp
}

/// The head of a dialog about one app: its real icon, a headline and a line under it (the
/// same layout as `kit::dialog_head`, with the app's icon in place of the badge).
pub fn dialog_head_app(
    ui: &mut Ui,
    t: &crate::theme::Tokens,
    app_path: &Path,
    headline: &str,
    sub: &str,
) {
    use crate::theme::{Weight, font};
    use egui::RichText;
    ui.horizontal(|ui| {
        app_icon(ui, app_path, 48.0);
        ui.add_space(6.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.add(
                egui::Label::new(
                    RichText::new(headline)
                        .font(font(18.0, Weight::Bold))
                        .color(t.text),
                )
                .wrap(),
            );
            if !sub.is_empty() {
                ui.add(
                    egui::Label::new(
                        RichText::new(sub)
                            .font(font(12.5, Weight::Regular))
                            .color(t.text_dim),
                    )
                    .wrap(),
                );
            }
        });
    });
}
