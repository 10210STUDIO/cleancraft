//! Disk Map: what takes space in the home folder (or a folder in it). A storage bar, a block
//! map of the current folder's contents (area = size), the same items as a list sorted by size,
//! drill down by clicking, Back and breadcrumbs, checkboxes, and "Review and Remove", which
//! lists the exact items before they move to the Trash (with Undo).
//!
//! Measuring runs in the background with a Stop button. Each mapped folder's tree is kept, so
//! drilling down and coming back needs no new scan; after a removal or an undo only the
//! changed folders are measured again.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cleancraft_core::executor::{self, Method, Report};
use cleancraft_core::features::{self, space_lens};
use cleancraft_core::fsutil::human_size;
use cleancraft_core::space::{
    self, DiskUse, Node, Tree, lock_reason_deep, node_lock_reason, outermost,
};
use egui::{Align, Color32, CornerRadius, Layout, RichText, Sense, Stroke, Ui, vec2};

use crate::app::CleanApp;
use crate::icons::{self, Icon};
use crate::jobs::Job;
use crate::kit;
use crate::theme::{self, Weight, font, motion, size};

/// Blocks drawn at most; the rest are one "Other items" block.
pub const MAX_BLOCKS: usize = 40;

pub struct WorkOut {
    tree: Tree,
    result: Result<Report, String>,
    /// Undo: (put back, errors).
    undo: Option<(usize, Vec<String>)>,
}

#[derive(Default)]
pub enum Phase {
    #[default]
    Idle,
    Scanning(Job<Tree>),
    Map,
    Confirm,
    Working(Job<WorkOut>),
    Done(Report),
}

#[derive(Default)]
pub struct State {
    pub phase: Phase,
    /// Measured trees by mapped folder.
    pub trees: BTreeMap<PathBuf, Tree>,
    /// The mapped folder (key of `trees`).
    pub root: Option<PathBuf>,
    /// The folder shown.
    pub current: Option<PathBuf>,
    pub selected: BTreeSet<PathBuf>,
    pub cancel: Arc<AtomicBool>,
    pub error: Option<String>,
    pub folder_text: String,
    pub disk: Option<DiskUse>,
    /// The action log as the trees reflect it: moves made on other pages update the map.
    watch: super::history::JournalWatch,
}

impl State {
    pub fn tree(&self) -> Option<&Tree> {
        self.trees.get(self.root.as_ref()?)
    }

    pub fn current_node(&self) -> Option<&Node> {
        let t = self.tree()?;
        t.find(self.current.as_ref()?)
    }

    /// Selected items (outermost only) with their sizes in the map.
    pub fn selection(&self) -> Vec<(PathBuf, u64)> {
        let Some(t) = self.tree() else {
            return Vec::new();
        };
        let sel: Vec<PathBuf> = self.selected.iter().cloned().collect();
        outermost(&sel)
            .into_iter()
            .filter_map(|p| t.find(&p).map(|n| (p, n.size)))
            .collect()
    }

    pub fn selected_size(&self) -> u64 {
        self.selection().iter().map(|s| s.1).sum()
    }
}

fn start_map(app: &mut CleanApp, root: PathBuf, fresh: bool) {
    let st = &mut app.disk_map;
    st.error = None;
    st.selected.clear();
    if !fresh {
        // Already measured, or inside a measured folder: no new scan.
        if st.trees.contains_key(&root) {
            st.root = Some(root.clone());
            st.current = Some(root);
            st.phase = Phase::Map;
            return;
        }
        let from_parent = st
            .trees
            .values()
            .find(|t| t.find(&root).is_some_and(|n| n.is_dir))
            .and_then(|t| t.subtree(&root));
        if let Some(sub) = from_parent {
            st.trees.insert(root.clone(), sub);
            st.root = Some(root.clone());
            st.current = Some(root);
            st.phase = Phase::Map;
            return;
        }
    }
    st.cancel = Arc::new(AtomicBool::new(false));
    let cancel = st.cancel.clone();
    st.root = Some(root.clone());
    st.current = Some(root.clone());
    let job = Job::start(app.sync, move |status| {
        space::build(&root, &cancel, &|n, _| status(&format!("{n} files")))
    });
    app.disk_map.phase = Phase::Scanning(job);
}

/// Finish background work. Returns true while something runs.
pub fn poll(app: &mut CleanApp) -> bool {
    let st = &mut app.disk_map;
    let mut notice: Option<(String, bool)> = None;
    // Tell the other pages (plan pages, Quick Sweep) what moved or came back.
    let mut moved: Option<Vec<PathBuf>> = None;
    let mut came_back = false;
    let busy = match &mut st.phase {
        Phase::Scanning(job) => {
            if let Some(tree) = job.poll() {
                if tree.root.is_none() {
                    st.error = Some("That folder could not be read.".into());
                    st.phase = Phase::Idle;
                } else if let Some(r) = st.root.clone() {
                    st.trees.insert(r, tree);
                    st.phase = Phase::Map;
                }
                st.watch.prime(&app.env);
                false
            } else if job.is_dead() {
                st.error = Some("Measuring stopped unexpectedly.".into());
                st.phase = Phase::Idle;
                false
            } else {
                true
            }
        }
        Phase::Working(job) => {
            if let Some(out) = job.poll() {
                if let Some(r) = st.root.clone() {
                    st.trees.insert(r, out.tree);
                }
                // Its own move or undo is in this tree already; other mapped folders catch up.
                if let Some(ch) = st.watch.changes(&app.env) {
                    let root = st.root.clone();
                    for (k, t) in st.trees.iter_mut() {
                        if Some(k) != root.as_ref() {
                            t.refresh_after(&app.env, &ch);
                        }
                    }
                }
                st.disk = None;
                // The shown folder may be gone now.
                while let Some(c) = st.current.clone() {
                    if st.tree().and_then(|t| t.find(&c)).is_some() {
                        break;
                    }
                    st.current = c.parent().map(Path::to_path_buf);
                }
                match (out.result, out.undo) {
                    (_, Some((n, errs))) => {
                        came_back = true;
                        notice = Some(if errs.is_empty() {
                            (
                                format!("{} put back.", cleancraft_core::fsutil::items(n)),
                                false,
                            )
                        } else {
                            (
                                format!(
                                    "{} put back, {} could not be: {}",
                                    cleancraft_core::fsutil::items(n),
                                    errs.len(),
                                    errs[0]
                                ),
                                true,
                            )
                        });
                        st.phase = Phase::Map;
                    }
                    (Ok(rep), None) => {
                        moved = Some(CleanApp::moved_paths(&rep));
                        st.selected.clear();
                        st.phase = Phase::Done(rep);
                    }
                    (Err(e), None) => {
                        st.error = Some(e);
                        st.phase = Phase::Map;
                    }
                }
                false
            } else if job.is_dead() {
                st.error = Some("The removal stopped unexpectedly.".into());
                st.phase = Phase::Map;
                false
            } else {
                true
            }
        }
        _ => false,
    };
    if let Some(m) = moved {
        app.files_moved(&m);
    }
    if came_back {
        app.files_back(&[]);
    }
    if let Some((m, err)) = notice {
        if err {
            app.warn(m);
        } else {
            app.tell(m);
        }
    }
    busy
}

/// Moves and undos made elsewhere (another page, History, an agent) since the map was
/// measured: measure just those places again, so moved blocks leave the map and the numbers
/// match a new scan.
fn catch_up(app: &mut CleanApp) {
    let st = &mut app.disk_map;
    if !matches!(st.phase, Phase::Idle | Phase::Map | Phase::Done(_)) || st.trees.is_empty() {
        return;
    }
    let Some(ch) = st.watch.changes(&app.env) else {
        return;
    };
    if super::history::moved_paths(&ch).is_empty() {
        return;
    }
    for t in st.trees.values_mut() {
        t.refresh_after(&app.env, &ch);
    }
    st.selected.retain(|p| std::fs::symlink_metadata(p).is_ok());
    st.disk = None;
    while let Some(c) = st.current.clone() {
        if st.tree().and_then(|t| t.find(&c)).is_some() {
            break;
        }
        st.current = c.parent().map(Path::to_path_buf);
    }
}

fn start_remove(app: &mut CleanApp) {
    let Some(root) = app.disk_map.root.clone() else {
        return;
    };
    let Some(mut tree) = app.disk_map.trees.remove(&root) else {
        return;
    };
    let env = app.env.clone();
    let settings = app.settings.clone();
    let plan = space_lens::plan_for(&env, &{
        let st = &app.disk_map;
        let sel: Vec<PathBuf> = st.selected.iter().cloned().collect();
        outermost(&sel)
            .into_iter()
            .filter_map(|p| tree.find(&p).map(|n| (p, n.size)))
            .collect::<Vec<_>>()
    });
    let job = Job::start(app.sync, move |status| {
        let digest = plan.digest();
        let r = features::run_plan(&env, &settings, &plan, &digest, &mut |n, total| {
            status(&format!("{n} of {total}"))
        });
        let result = match r {
            Ok(rep) => {
                tree.refresh_after(&env, &rep.moved);
                Ok(rep)
            }
            Err(e) => Err(format!("{e:#}")),
        };
        WorkOut {
            tree,
            result,
            undo: None,
        }
    });
    app.disk_map.phase = Phase::Working(job);
}

fn start_undo(app: &mut CleanApp, rep: Report) {
    let Some(root) = app.disk_map.root.clone() else {
        return;
    };
    let Some(mut tree) = app.disk_map.trees.remove(&root) else {
        return;
    };
    let env = app.env.clone();
    let job = Job::start(app.sync, move |_| {
        let (n, errs) = executor::undo_plan(&env, &rep.plan_id);
        tree.refresh_after(&env, &rep.moved);
        WorkOut {
            tree,
            result: Ok(Report::default()),
            undo: Some((n, errs.into_iter().map(|e| e.1.to_string()).collect())),
        }
    });
    app.disk_map.phase = Phase::Working(job);
}

const SUBTITLE: &str =
    "See what takes space in your home folder. Tick what you do not need and move it to the Trash.";

pub fn ui(app: &mut CleanApp, ui: &mut Ui, f: cleancraft_core::FeatureId) {
    let t = app.t;
    catch_up(app);
    let (icon, _) = kit::feature_look(&t, f);
    let phase = match &app.disk_map.phase {
        Phase::Idle => 0,
        Phase::Scanning(_) => 1,
        Phase::Map => 2,
        Phase::Confirm => 3,
        Phase::Working(_) => 4,
        Phase::Done(_) => 5,
    };
    if phase != 0 {
        kit::header(ui, &t, icon, f.title(), SUBTITLE);
    }
    if phase != 0 && phase != 5 {
        storage_bar(app, ui);
        ui.add_space(theme::space::S);
    }
    if let Some(e) = app.disk_map.error.clone() {
        kit::strip(ui, Icon::Warning, t.danger, &e);
        ui.add_space(theme::space::S);
    }
    match phase {
        0 => idle(app, ui),
        1 => {
            let (status, secs) = match &app.disk_map.phase {
                Phase::Scanning(j) => (j.status(), j.elapsed_secs()),
                _ => (String::new(), 0),
            };
            let what = app
                .disk_map
                .root
                .as_ref()
                .map(|r| app.env.display(r))
                .unwrap_or_default();
            if kit::progress(
                ui,
                &t,
                f,
                &format!("Measuring {what}..."),
                &status,
                secs,
                true,
            ) {
                app.act();
                app.disk_map.cancel.store(true, Ordering::Relaxed);
            }
        }
        2 => map(app, ui),
        3 => {
            map(app, ui);
            confirm(app, ui);
        }
        4 => {
            let (status, secs) = match &app.disk_map.phase {
                Phase::Working(j) => (j.status(), j.elapsed_secs()),
                _ => (String::new(), 0),
            };
            kit::progress(
                ui,
                &t,
                f,
                "Working on the selected items...",
                &status,
                secs,
                false,
            );
        }
        _ => done(app, ui),
    }
}

fn storage_bar(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    if app.disk_map.disk.is_none() {
        app.disk_map.disk = Some(space::disk_use(&app.env, app.env.home()));
    }
    let d = app.disk_map.disk.unwrap_or_default();
    let sel = app.disk_map.selected_size();
    kit::card(&t)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let text = match (d.used, d.total, d.free) {
                (Some(u), Some(tot), Some(fr)) => format!(
                    "Startup disk: {} used of {}, {} free",
                    human_size(u),
                    human_size(tot),
                    human_size(fr)
                ),
                (_, _, Some(fr)) => format!("Startup disk: {} free", human_size(fr)),
                _ => "Startup disk: free space unknown".to_string(),
            };
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(text)
                        .font(font(size::BODY, Weight::SemiBold))
                        .color(t.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!("Selected: {}", human_size(sel)))
                            .font(font(size::CALLOUT, Weight::Medium))
                            .color(t.warning),
                    );
                });
            });
            if let (Some(u), Some(tot)) = (d.used, d.total)
                && tot > 0
            {
                let w = ui.available_width();
                let (r, _) = ui.allocate_exact_size(vec2(w, 8.0), Sense::hover());
                let p = ui.painter();
                let used_f = (u as f32 / tot as f32).clamp(0.0, 1.0);
                kit::paint_bar(p, r, &t, used_f, t.sec_a);
                let used_x = r.left() + r.width() * used_f;
                let mut s = r;
                s.min.x = used_x - r.width() * (sel as f32 / tot as f32).clamp(0.0, 1.0);
                s.max.x = used_x;
                if s.width() > 0.5 {
                    p.rect_filled(s, CornerRadius::same(4), t.warning);
                }
            }
        });
}

fn idle(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let mut folder_go = false;
    let go = kit::hero(
        ui,
        &t,
        cleancraft_core::FeatureId::SpaceLens,
        kit::HeroText {
            title: cleancraft_core::FeatureId::SpaceLens.title(),
            desc: SUBTITLE,
            covers: &[],
            action: "Map Home Folder",
            note: "Measuring only looks. Nothing moves until you review the list and confirm.",
        },
        |ui| {
            ui.scope(|ui| {
                ui.set_max_width(520.0);
                storage_bar(app, ui);
            });
            ui.add_space(theme::space::M);
            kit::centered_row(ui, egui::Id::new("disk-map-folder-row"), |ui| {
                let r = kit::text_field(ui, &t, &mut app.disk_map.folder_text, "~/Projects", 300.0);
                r.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Folder to map")
                });
                if r.changed() {
                    app.act();
                    app.disk_map.error = None;
                }
                let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    app.act();
                    app.disk_map.folder_text.clear();
                    app.disk_map.error = None;
                }
                let can = !app.disk_map.folder_text.trim().is_empty();
                let clicked = ui
                    .add_enabled_ui(can, |ui| {
                        kit::pill(
                            ui,
                            &t,
                            "Map Folder",
                            kit::Kind::Secondary,
                            Some(Icon::Folder),
                        )
                    })
                    .inner
                    .clicked();
                if (clicked || enter) && can {
                    folder_go = true;
                }
            });
        },
    );
    if folder_go {
        app.act();
        match space::parse_map_folder(&app.env, &app.disk_map.folder_text) {
            Ok(p) => start_map(app, p, false),
            Err(e) => app.disk_map.error = Some(e),
        }
    } else if go {
        app.act();
        let home = app.env.home().to_path_buf();
        start_map(app, home, false);
    }
}

fn node_label(app: &CleanApp, n: &Node) -> String {
    if n.path == app.env.home() {
        "Home folder".into()
    } else {
        n.name.clone()
    }
}

fn open_folder(app: &mut CleanApp, p: PathBuf) {
    app.act();
    app.disk_map.current = Some(p);
}

fn toggle(app: &mut CleanApp, p: &Path, on: bool) {
    app.act();
    if let Some(why) = lock_reason_deep(&app.env, p) {
        app.warn(format!("{} cannot be selected: {why}.", app.env.display(p)));
        return;
    }
    if on {
        app.disk_map.selected.insert(p.to_path_buf());
    } else {
        app.disk_map.selected.remove(p);
    }
}

fn map(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let Some(node) = app.disk_map.current_node().cloned() else {
        app.disk_map.phase = Phase::Idle;
        return;
    };
    let root = app.disk_map.root.clone().unwrap_or_default();
    // Navigation row.
    ui.horizontal(|ui| {
        let can_back = node.path != root;
        if ui
            .add_enabled_ui(can_back, |ui| {
                kit::pill(ui, &t, "Back", kit::Kind::Secondary, Some(Icon::Back))
            })
            .inner
            .clicked()
        {
            open_folder(app, node.path.parent().unwrap_or(&root).to_path_buf());
        }
        // Breadcrumbs from the mapped folder down to the current one.
        let mut crumbs = vec![root.clone()];
        if let Ok(rel) = node.path.strip_prefix(&root) {
            let mut cur = root.clone();
            for c in rel.components() {
                cur = cur.join(c);
                crumbs.push(cur.clone());
            }
        }
        for (i, c) in crumbs.iter().enumerate() {
            if i > 0 {
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 14.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Chevron, t.text_faint);
            }
            let label = if *c == app.env.home() {
                "Home folder".to_string()
            } else {
                c.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            };
            let kind = if *c == node.path {
                kit::Kind::Secondary
            } else {
                kit::Kind::Ghost
            };
            let r = kit::pill(ui, &t, &label, kind, None);
            r.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Go to {label}"))
            });
            if r.clicked() {
                open_folder(app, c.clone());
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if kit::secondary(ui, &t, "Map Again") {
                app.act();
                start_map(app, root.clone(), true);
            }
            if kit::secondary(ui, &t, "Other Folder") {
                app.act();
                app.disk_map.phase = Phase::Idle;
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(human_size(node.size))
                .font(font(size::LARGE_TITLE, Weight::Bold))
                .color(icons::lerp(t.sec_a, Color32::WHITE, 0.15)),
        );
        ui.label(
            RichText::new(format!(
                "in {} ({} files)",
                node_label(app, &node),
                node.files
            ))
            .color(t.text_dim),
        );
    });
    if let Some(tree) = app.disk_map.tree() {
        if tree.unreadable > 0 {
            kit::dim(
                ui,
                &t,
                &format!(
                    "{} folders could not be read; their size is not counted. Full Disk Access may help.",
                    tree.unreadable
                ),
            );
        }
        if tree.other_volumes > 0 {
            kit::dim(
                ui,
                &t,
                &format!(
                    "{} folders on other disks are shown empty (not measured).",
                    tree.other_volumes
                ),
            );
        }
        if tree.stopped {
            kit::dim(ui, &t, "Measuring was stopped; sizes are partial.");
        }
    }
    // Bottom bar.
    let n = app.disk_map.selection().len();
    let size = app.disk_map.selected_size();
    kit::bottom_bar(ui, &t, egui::Id::new("disk-map-bottom"), |ui| {
        ui.label(
            RichText::new(format!("{n} selected, {}", human_size(size)))
                .font(font(size::BODY, Weight::SemiBold))
                .color(t.text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if n > 0 && kit::primary(ui, &t, "Review and Remove") {
                app.act();
                app.disk_map.phase = Phase::Confirm;
            }
            if n > 0 && kit::secondary(ui, &t, "Clear Selection") {
                app.act();
                app.disk_map.selected.clear();
            }
            if n == 0 {
                kit::dim(
                    ui,
                    &t,
                    "Click a block to open it. Tick items in the list to remove them.",
                );
            }
        });
    });
    ui.add_space(6.0);
    let avail = ui.available_size();
    let h = (avail.y - 8.0).max(120.0);
    let body = egui::Rect::from_min_size(ui.cursor().min, vec2(avail.x, h));
    let map_w = (avail.x * 0.52).max(200.0);
    let left = egui::Rect::from_min_size(body.min, vec2(map_w, h));
    let right = egui::Rect::from_min_max(egui::pos2(left.right() + 12.0, body.top()), body.max);
    ui.allocate_rect(body, Sense::hover());
    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        ui.set_clip_rect(left.intersect(ui.clip_rect()));
        treemap(app, ui, &node, left);
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
        ui.set_clip_rect(right.expand(24.0).intersect(ui.clip_rect()));
        list(app, ui, &node, h);
    });
}

/// Block colours: the Files section's warm family first, then cool contrasts; plain files
/// are slate.
fn block_color(i: usize, is_dir: bool) -> Color32 {
    const PALETTE: [Color32; 8] = [
        Color32::from_rgb(0xF5, 0xA6, 0x3B),
        Color32::from_rgb(0x4E, 0x9C, 0xF0),
        Color32::from_rgb(0xEE, 0x72, 0x55),
        Color32::from_rgb(0x3F, 0xC1, 0xA0),
        Color32::from_rgb(0x8E, 0x7D, 0xF0),
        Color32::from_rgb(0xE8, 0x6A, 0x9A),
        Color32::from_rgb(0xC9, 0xB8, 0x3E),
        Color32::from_rgb(0x4C, 0xB8, 0xD8),
    ];
    if is_dir {
        PALETTE[i % PALETTE.len()]
    } else {
        Color32::from_rgb(0x5C, 0x67, 0x78)
    }
}

fn treemap(app: &mut CleanApp, ui: &mut Ui, node: &Node, area: egui::Rect) {
    let t = app.t;
    let kids: Vec<&Node> = node.children.iter().filter(|c| c.size > 0).collect();
    let shown = kids.len().min(MAX_BLOCKS);
    let mut values: Vec<f64> = kids[..shown].iter().map(|c| c.size as f64).collect();
    let rest: u64 = kids[shown..].iter().map(|c| c.size).sum();
    if rest > 0 {
        values.push(rest as f64);
    }
    if values.is_empty() {
        ui.label(RichText::new("This folder is empty.").color(t.text_dim));
        return;
    }
    // Keep "biggest first" for the layout.
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|a, b| values[*b].total_cmp(&values[*a]));
    let sorted: Vec<f64> = order.iter().map(|i| values[*i]).collect();
    let rects = space::squarify(
        &sorted,
        [area.left(), area.top(), area.width(), area.height()],
    );
    let mut clicked: Option<(PathBuf, bool)> = None;
    for (k, r) in order.iter().zip(rects) {
        let rect = egui::Rect::from_min_size(egui::pos2(r[0], r[1]), vec2(r[2], r[3])).shrink(1.5);
        if rect.width() < 1.0 || rect.height() < 1.0 {
            continue;
        }
        let (label, color, target) = if *k < shown {
            let c = kids[*k];
            (
                c.name.clone(),
                block_color(*k, c.is_dir),
                Some((c.path.clone(), c.is_dir)),
            )
        } else {
            (
                format!("Other items ({})", kids.len() - shown),
                Color32::from_rgb(0x44, 0x48, 0x50),
                None,
            )
        };
        let selected = target
            .as_ref()
            .is_some_and(|(p, _)| app.disk_map.selected.contains(p));
        let id = ui.id().with(("block", &label));
        let resp = ui.interact(rect, id, Sense::click());
        let name = format!("Block {label}");
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
        let p = ui.painter();
        let round = (rect.width().min(rect.height()) * 0.18).clamp(2.0, 8.0);
        let feel = kit::feel_ring(ui, &t, &resp, rect, round);
        let hov = (feel.hov - 0.4 * feel.down).max(0.0);
        let top = icons::lerp(color, Color32::WHITE, 0.10 + 0.12 * hov);
        let bottom = icons::lerp(color, Color32::BLACK, 0.28 - 0.1 * hov);
        icons::gradient_tile(p, rect, round, top, bottom);
        p.rect_stroke(
            rect,
            round,
            Stroke::new(1.0, Color32::from_white_alpha((28.0 + 40.0 * hov) as u8)),
            egui::StrokeKind::Inside,
        );
        if selected {
            p.rect_stroke(
                rect,
                round,
                Stroke::new(3.0, t.warning),
                egui::StrokeKind::Inside,
            );
            let cr =
                egui::Rect::from_min_size(rect.right_top() + vec2(-20.0, 4.0), vec2(16.0, 16.0));
            if rect.width() > 30.0 && rect.height() > 30.0 {
                p.circle_filled(cr.center(), 8.0, t.warning);
                icons::paint_with(p, cr.shrink(3.0), Icon::Check, Color32::BLACK, 1.8);
            }
        }
        if rect.width() > 56.0 && rect.height() > 30.0 {
            let size = if let Some((path, _)) = &target {
                app.disk_map
                    .tree()
                    .and_then(|tr| tr.find(path))
                    .map(|n| n.size)
                    .unwrap_or(0)
            } else {
                rest
            };
            // Ink or white, whichever reads better on the block (WCAG), one line for the
            // name cut at its end (never broken mid-word) and one for the size.
            let ink = theme::on_color(icons::lerp(top, bottom, 0.35));
            let nf = font(size::FOOTNOTE, Weight::SemiBold);
            let sf = font(size::FOOTNOTE, Weight::Regular);
            let name_s = kit::truncate_end(ui.ctx(), &label, &nf, rect.width() - 16.0);
            let pos = rect.left_top() + vec2(8.0, 7.0);
            let clip = p.with_clip_rect(rect.shrink(2.0));
            if ink == Color32::WHITE {
                clip.text(
                    pos + vec2(0.0, 1.0),
                    egui::Align2::LEFT_TOP,
                    &name_s,
                    nf.clone(),
                    Color32::from_black_alpha(90),
                );
            }
            clip.text(pos, egui::Align2::LEFT_TOP, &name_s, nf, ink);
            clip.text(
                pos + vec2(0.0, 16.0),
                egui::Align2::LEFT_TOP,
                human_size(size),
                sf,
                ink.gamma_multiply(0.8),
            );
        }
        let resp = match &target {
            Some((path, true)) => resp.on_hover_text(format!("Open {}", app.env.display(path))),
            Some((path, false)) => resp.on_hover_text(app.env.display(path)),
            None => resp.on_hover_text("Smaller items, listed on the right"),
        };
        if resp.clicked() {
            match &target {
                Some(t) => clicked = Some(t.clone()),
                None => {
                    app.act();
                    app.tell("The smaller items are in the list on the right.");
                }
            }
        }
    }
    if let Some((path, is_dir)) = clicked {
        if is_dir {
            open_folder(app, path);
        } else {
            let on = !app.disk_map.selected.contains(&path);
            toggle(app, &path, on);
        }
    }
}

/// Height of a line in the list beside the map.
const LIST_ROW_H: f32 = 52.0;

fn list(app: &mut CleanApp, ui: &mut Ui, node: &Node, h: f32) {
    let t = app.t;
    kit::card(&t)
        .inner_margin(egui::Margin::symmetric(6, 8))
        .show(ui, |ui| {
            ui.set_min_size(vec2(ui.available_width(), h - 18.0));
            let kids = &node.children;
            if kids.is_empty() {
                ui.label(RichText::new("Nothing here.").color(t.text_dim));
                return;
            }
            let total = node.size.max(1);
            egui::ScrollArea::vertical()
                .id_salt("disk-map-list")
                .max_height(h - 18.0)
                .auto_shrink([false, false])
                .show_rows(ui, LIST_ROW_H, kids.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (k, c) in kids[range.clone()].iter().enumerate() {
                        let idx = range.start + k;
                        let locked = node_lock_reason(&app.env, c);
                        let w = ui.available_width();
                        let (r, row) = ui.allocate_exact_size(vec2(w, LIST_ROW_H), Sense::hover());
                        let hov = kit::anim(
                            ui.ctx(),
                            row.id.with("hov"),
                            row.contains_pointer(),
                            motion::HOVER,
                        );
                        if hov > 0.0 {
                            ui.painter().rect_filled(
                                r.shrink(2.0),
                                9.0,
                                Color32::from_white_alpha((9.0 * hov) as u8),
                            );
                        }
                        let was = app.disk_map.selected.contains(&c.path);
                        let mut on = was;
                        let label = format!("Select {}", c.name);
                        let cb_rect = egui::Rect::from_center_size(
                            egui::pos2(r.left() + 18.0, r.center().y),
                            vec2(22.0, 22.0),
                        );
                        let cb = kit::place(ui, cb_rect, |ui| {
                            if locked.is_some() {
                                // Still a (disabled) checkbox for VoiceOver and tests; a
                                // padlock on screen, like the module result lists.
                                ui.multiply_opacity(0.0);
                            }
                            ui.add_enabled_ui(locked.is_none(), |ui| {
                                kit::check(ui, &t, &mut on, &label)
                            })
                            .inner
                        });
                        if locked.is_some() {
                            icons::paint(
                                ui.painter(),
                                egui::Rect::from_center_size(cb_rect.center(), vec2(18.0, 18.0)),
                                Icon::Lock,
                                t.text_faint,
                            );
                        }
                        if cb.changed() {
                            toggle(app, &c.path, on);
                        }
                        // Colour dot matching the block.
                        let dot = egui::pos2(r.left() + 42.0, r.center().y);
                        let color = if idx < MAX_BLOCKS {
                            block_color(idx, c.is_dir)
                        } else {
                            Color32::from_rgb(0x44, 0x48, 0x50)
                        };
                        // The block's colour as a short bar, then the item's own kind icon
                        // (folder, archive, picture...), like every other file list.
                        ui.painter().rect_filled(
                            egui::Rect::from_center_size(dot - vec2(6.0, 0.0), vec2(3.0, 22.0)),
                            1.5,
                            color,
                        );
                        let ir =
                            egui::Rect::from_center_size(dot + vec2(8.0, 0.0), vec2(26.0, 26.0));
                        crate::images::paint_file_icon(
                            ui,
                            ir,
                            &c.path,
                            crate::file_icons::FileKind::of(&c.path),
                        );
                        let size_text = human_size(c.size);
                        let sf = font(size::BODY, Weight::SemiBold);
                        let size_w = kit::galley(ui, &size_text, sf.clone(), t.text).size().x;
                        kit::text_at(
                            ui,
                            egui::Rect::from_min_max(
                                egui::pos2(r.right() - size_w - 12.0, r.top() + 4.0),
                                egui::pos2(r.right() - 8.0, r.top() + 28.0),
                            ),
                            &size_text,
                            &size_text,
                            sf,
                            t.text,
                            Align::Max,
                        );
                        let x0 = r.left() + 70.0;
                        let name_w = (r.right() - size_w - 20.0 - x0).max(40.0);
                        let nf = font(size::BODY, Weight::Medium);
                        let name_s = kit::truncate_middle(ui.ctx(), &c.name, &nf, name_w);
                        let name_rect = egui::Rect::from_min_size(
                            egui::pos2(x0, r.top() + 6.0),
                            vec2(name_w, 22.0),
                        );
                        if c.is_dir {
                            let resp = ui.interact(name_rect, row.id.with("open"), Sense::click());
                            kit::feel_ring(ui, &t, &resp, name_rect, 4.0);
                            // A folder name opens it: a link, for VoiceOver and tests.
                            resp.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::Link, true, &c.name)
                            });
                            let col = if resp.hovered() {
                                icons::lerp(t.sec_a, Color32::WHITE, 0.3)
                            } else {
                                t.text
                            };
                            ui.painter().text(
                                name_rect.left_center(),
                                egui::Align2::LEFT_CENTER,
                                &name_s,
                                nf,
                                col,
                            );
                            let resp = resp.on_hover_text("Open this folder");
                            if resp.clicked() {
                                open_folder(app, c.path.clone());
                            }
                        } else {
                            kit::text_at(ui, name_rect, &name_s, &c.name, nf, t.text, Align::Min);
                        }
                        // Share bar, or the reason it is locked.
                        let line = egui::Rect::from_min_max(
                            egui::pos2(x0, r.top() + 31.0),
                            egui::pos2(r.right() - 12.0, r.top() + 46.0),
                        );
                        if let Some(why) = &locked {
                            let text = format!("locked: {why}");
                            let lf = font(size::FOOTNOTE, Weight::Regular);
                            // The whole reason when it fits; else its first clause (never a
                            // sentence cut mid-way with an open parenthesis); the tooltip
                            // has it all.
                            // The padlock says "locked"; the line says why, once.
                            let cap = |s: &str| {
                                let mut c = s.chars();
                                c.next()
                                    .map(|f| f.to_uppercase().chain(c).collect::<String>())
                                    .unwrap_or_default()
                            };
                            let short = match why.split_once(" (") {
                                Some((head, _)) => cap(head),
                                None => cap(why),
                            };
                            let full_shown = cap(why);
                            let fits = |s: &str| {
                                kit::galley(ui, s, lf.clone(), t.text_faint).size().x
                                    <= line.width()
                            };
                            let s = if fits(&full_shown) {
                                full_shown
                            } else if fits(&short) {
                                short
                            } else {
                                kit::truncate_end(ui.ctx(), &short, &lf, line.width())
                            };
                            kit::text_at(ui, line, &s, &text, lf, t.text_dim, Align::Min)
                                .on_hover_text(format!("Locked: {why}"));
                        } else {
                            let bar = egui::Rect::from_min_size(
                                egui::pos2(line.left(), line.center().y - 2.0),
                                vec2(line.width(), 4.0),
                            );
                            kit::paint_bar(
                                ui.painter(),
                                bar,
                                &t,
                                c.size as f32 / total as f32,
                                color,
                            );
                        }
                    }
                });
        });
}

fn confirm(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    let items = app.disk_map.selection();
    let total: u64 = items.iter().map(|i| i.1).sum();
    let dest = match app.settings.method {
        Method::Trash => "the Trash",
        Method::Quarantine | Method::AdminQuarantine => "the CleanCraft quarantine",
    };
    let mut go = false;
    let mut close = false;
    let width = (ui.ctx().content_rect().width() - 120.0).clamp(420.0, 640.0);
    let max_list = (ui.ctx().content_rect().height() - 330.0).clamp(120.0, 340.0);
    let modal = kit::modal(egui::Id::new("disk-map-confirm"), &t).show(ui.ctx(), |ui| {
        ui.set_width(width);
        kit::dialog_head(
            ui,
            &t,
            Icon::Trash,
            t.sec_a,
            &format!(
                "Move {} ({}) to {dest}?",
                cleancraft_core::fsutil::items(items.len()),
                human_size(total)
            ),
            "These exact items will be moved. You can put them back with Undo or from History.",
        );
        ui.add_space(theme::space::M);
        kit::well(&t).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .max_height(max_list)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for (p, s) in &items {
                        kit::confirm_line(ui, &t, p, &app.env.display(p), "", &human_size(*s));
                    }
                });
        });
        ui.add_space(theme::space::L);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = match app.settings.method {
                Method::Trash => "Move to Trash",
                Method::Quarantine | Method::AdminQuarantine => "Move to Quarantine",
            };
            if kit::danger(ui, &t, label) {
                go = true;
            }
            if kit::secondary(ui, &t, "Cancel") {
                close = true;
            }
        });
    });
    if go {
        app.act();
        start_remove(app);
    } else if close || modal.should_close() {
        app.act();
        app.disk_map.phase = Phase::Map;
    }
}

fn done(app: &mut CleanApp, ui: &mut Ui) {
    let Phase::Done(rep) = &app.disk_map.phase else {
        return;
    };
    let rep = rep.clone();
    let summary = format!(
        "{} moved. {} skipped.",
        cleancraft_core::fsutil::items(rep.moved.len()),
        rep.skipped.len()
    );
    super::plan_page::done_view(
        app,
        ui,
        cleancraft_core::FeatureId::SpaceLens,
        &rep,
        &summary,
        |app, ui| {
            let t = app.t;
            if !rep.moved.is_empty()
                && kit::pill(ui, &t, "Undo", kit::Kind::Secondary, Some(Icon::Undo)).clicked()
            {
                app.act();
                start_undo(app, rep.clone());
            }
            if kit::pill(
                ui,
                &t,
                "Show History",
                kit::Kind::Secondary,
                Some(Icon::History),
            )
            .clicked()
            {
                app.act();
                app.navigate("history");
            }
            if kit::primary(ui, &t, "Done") {
                app.act();
                app.disk_map.phase = Phase::Map;
            }
        },
    );
}
