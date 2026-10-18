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
use cleancraft_core::space::{self, DiskUse, Node, Tree, lock_reason, outermost};
use egui::{Align, Color32, CornerRadius, Layout, RichText, Sense, Stroke, Ui, vec2};

use crate::app::CleanApp;
use crate::jobs::Job;
use crate::kit;
use crate::theme::{Weight, font};

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
        let from_parent = st.trees.values().find_map(|t| t.find(&root).cloned());
        if let Some(node) = from_parent
            && node.is_dir
        {
            st.trees.insert(
                root.clone(),
                Tree {
                    root: Some(node),
                    ..Default::default()
                },
            );
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
                        notice = Some(if errs.is_empty() {
                            (format!("{n} items put back."), false)
                        } else {
                            (
                                format!(
                                    "{n} items put back, {} could not be: {}",
                                    errs.len(),
                                    errs[0]
                                ),
                                true,
                            )
                        });
                        st.phase = Phase::Map;
                    }
                    (Ok(rep), None) => {
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
    if let Some((m, err)) = notice {
        if err {
            app.warn(m);
        } else {
            app.tell(m);
        }
    }
    busy
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

pub fn ui(app: &mut CleanApp, ui: &mut Ui, f: cleancraft_core::FeatureId) {
    let t = app.t;
    let (glyph, color) = kit::feature_look(&t, f);
    ui.horizontal(|ui| {
        kit::badge(ui, color, glyph, 44.0);
        ui.vertical(|ui| {
            kit::title(ui, &t, f.title());
            kit::subtitle(
                ui,
                &t,
                "See what takes space in your home folder. Tick what you do not need and move it to the Trash.",
            );
        });
    });
    ui.add_space(8.0);
    storage_bar(app, ui);
    ui.add_space(8.0);
    if let Some(e) = app.disk_map.error.clone() {
        ui.label(RichText::new(e).color(t.danger));
    }
    let phase = match &app.disk_map.phase {
        Phase::Idle => 0,
        Phase::Scanning(_) => 1,
        Phase::Map => 2,
        Phase::Confirm => 3,
        Phase::Working(_) => 4,
        Phase::Done(_) => 5,
    };
    match phase {
        0 => idle(app, ui),
        1 => {
            let status = match &app.disk_map.phase {
                Phase::Scanning(j) => j.status(),
                _ => String::new(),
            };
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!(
                    "Measuring {}... {status}",
                    app.disk_map
                        .root
                        .as_ref()
                        .map(|r| app.env.display(r))
                        .unwrap_or_default()
                ));
                if kit::secondary(ui, &t, "Stop") {
                    app.act();
                    app.disk_map.cancel.store(true, Ordering::Relaxed);
                }
            });
        }
        2 => map(app, ui),
        3 => {
            map(app, ui);
            confirm(app, ui);
        }
        4 => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Working on the selected items...");
            });
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
    kit::card(&t).show(ui, |ui| {
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
            ui.label(RichText::new(text).font(font(13.0, Weight::SemiBold)));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("Selected: {}", human_size(sel))).color(t.warning));
            });
        });
        if let (Some(u), Some(tot)) = (d.used, d.total)
            && tot > 0
        {
            let w = ui.available_width();
            let (r, _) = ui.allocate_exact_size(vec2(w, 10.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(r, CornerRadius::same(5), t.well);
            let mut used = r;
            used.set_width(r.width() * (u as f32 / tot as f32).clamp(0.0, 1.0));
            p.rect_filled(used, CornerRadius::same(5), t.accent);
            let mut s = r;
            s.min.x = used.max.x - r.width() * (sel as f32 / tot as f32).clamp(0.0, 1.0);
            s.max.x = used.max.x;
            if s.width() > 0.5 {
                p.rect_filled(s, CornerRadius::same(5), t.warning);
            }
        }
    });
}

fn idle(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    ui.add_space(30.0);
    ui.vertical_centered(|ui| {
        kit::dim(
            ui,
            &t,
            "Measuring only looks. Nothing moves until you review the list and confirm.",
        );
        ui.add_space(8.0);
        if kit::primary(ui, &t, "Map Home Folder") {
            app.act();
            let home = app.env.home().to_path_buf();
            start_map(app, home, false);
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.add_space((ui.available_width() - 470.0).max(0.0) / 2.0);
            let r = ui.add(
                egui::TextEdit::singleline(&mut app.disk_map.folder_text)
                    .hint_text("~/Projects")
                    .desired_width(320.0),
            );
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
                .add_enabled(can, egui::Button::new("Map Folder"))
                .clicked();
            if (clicked || enter) && can {
                app.act();
                match space::parse_map_folder(&app.env, &app.disk_map.folder_text) {
                    Ok(p) => start_map(app, p, false),
                    Err(e) => app.disk_map.error = Some(e),
                }
            }
        });
    });
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
    if let Some(why) = lock_reason(&app.env, p) {
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
            .add_enabled(can_back, egui::Button::new("Back"))
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
                ui.label(RichText::new("›").color(t.text_faint));
            }
            let label = if *c == app.env.home() {
                "Home folder".to_string()
            } else {
                c.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            };
            let r = ui.selectable_label(*c == node.path, label.clone());
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
                .font(font(26.0, Weight::Bold))
                .color(t.accent),
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
    egui::Panel::bottom(egui::Id::new("disk-map-bottom"))
        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(0, 8)))
        .show(ui, |ui| {
            let n = app.disk_map.selection().len();
            let size = app.disk_map.selected_size();
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{n} selected, {}", human_size(size)))
                        .font(font(14.0, Weight::SemiBold)),
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
                });
            });
        });
    ui.add_space(6.0);
    let avail = ui.available_size();
    let h = (avail.y - 8.0).max(120.0);
    let body = egui::Rect::from_min_size(ui.cursor().min, vec2(avail.x, h));
    let map_w = (avail.x * 0.55).max(200.0);
    let left = egui::Rect::from_min_size(body.min, vec2(map_w, h));
    let right = egui::Rect::from_min_max(egui::pos2(left.right() + 12.0, body.top()), body.max);
    ui.allocate_rect(body, Sense::hover());
    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
        ui.set_clip_rect(left.intersect(ui.clip_rect()));
        treemap(app, ui, &node, left);
    });
    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
        ui.set_clip_rect(right.intersect(ui.clip_rect()));
        list(app, ui, &node, h);
    });
}

fn block_color(i: usize, is_dir: bool) -> Color32 {
    const PALETTE: [Color32; 6] = [
        Color32::from_rgb(0x3E, 0x8E, 0xD0),
        Color32::from_rgb(0x2E, 0xA8, 0x8F),
        Color32::from_rgb(0x8A, 0x6F, 0xD8),
        Color32::from_rgb(0xD0, 0x82, 0x3E),
        Color32::from_rgb(0xC0, 0x5A, 0x8C),
        Color32::from_rgb(0x5C, 0xA0, 0x4C),
    ];
    if is_dir {
        PALETTE[i % PALETTE.len()]
    } else {
        Color32::from_rgb(0x5A, 0x63, 0x70)
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
        let rect = egui::Rect::from_min_size(egui::pos2(r[0], r[1]), vec2(r[2], r[3])).shrink(1.0);
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
        let fill = if resp.hovered() {
            color.gamma_multiply(1.25)
        } else {
            color
        };
        p.rect_filled(rect, CornerRadius::same(4), fill);
        if selected {
            p.rect_stroke(
                rect,
                CornerRadius::same(4),
                Stroke::new(3.0, t.warning),
                egui::StrokeKind::Inside,
            );
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
            let galley = p.layout(
                format!("{label}\n{}", human_size(size)),
                font(11.5, Weight::SemiBold),
                Color32::WHITE,
                rect.width() - 8.0,
            );
            let pos = rect.left_top() + vec2(4.0, 4.0);
            p.with_clip_rect(rect).galley(pos, galley, Color32::WHITE);
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

fn list(app: &mut CleanApp, ui: &mut Ui, node: &Node, h: f32) {
    let t = app.t;
    kit::card(&t).show(ui, |ui| {
        ui.set_min_size(vec2(ui.available_width(), h - 30.0));
        let kids = &node.children;
        if kids.is_empty() {
            ui.label(RichText::new("Nothing here.").color(t.text_dim));
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("disk-map-list")
            .max_height(h - 30.0)
            .show_rows(ui, 34.0, kids.len(), |ui, range| {
                for c in &kids[range] {
                    let locked = lock_reason(&app.env, &c.path);
                    ui.horizontal(|ui| {
                        ui.set_height(30.0);
                        let was = app.disk_map.selected.contains(&c.path);
                        let mut on = was;
                        let cb =
                            ui.add_enabled(locked.is_none(), egui::Checkbox::without_text(&mut on));
                        let label = format!("Select {}", c.name);
                        cb.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::Checkbox,
                                locked.is_none(),
                                was,
                                &label,
                            )
                        });
                        if cb.changed() {
                            toggle(app, &c.path, on);
                        }
                        if c.is_dir {
                            let link = ui
                                .link(RichText::new(&c.name).font(font(13.0, Weight::Medium)))
                                .on_hover_text("Open this folder");
                            // egui reports a visible link's text as a plain label (text
                            // selection); say again that it is a link, for VoiceOver and tests.
                            link.widget_info(|| {
                                egui::WidgetInfo::labeled(egui::WidgetType::Link, true, &c.name)
                            });
                            if link.clicked() {
                                open_folder(app, c.path.clone());
                            }
                        } else {
                            ui.label(RichText::new(&c.name).font(font(13.0, Weight::Regular)));
                        }
                        if let Some(why) = &locked {
                            ui.label(
                                RichText::new(format!("locked: {why}"))
                                    .font(font(11.0, Weight::Regular))
                                    .color(t.text_faint),
                            );
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(human_size(c.size))
                                    .font(font(13.0, Weight::SemiBold)),
                            );
                        });
                    });
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
        Method::Quarantine => "the CleanCraft quarantine",
    };
    let mut go = false;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("disk-map-confirm")).show(ui.ctx(), |ui| {
        ui.set_width(640.0);
        ui.label(
            RichText::new(format!(
                "Move {} items ({}) to {dest}?",
                items.len(),
                human_size(total)
            ))
            .font(font(18.0, Weight::Bold)),
        );
        kit::dim(
            ui,
            &t,
            "These exact items will be moved. You can put them back with Undo or from History.",
        );
        egui::ScrollArea::vertical()
            .max_height(340.0)
            .show(ui, |ui| {
                egui::Grid::new("disk-map-confirm-list")
                    .striped(true)
                    .num_columns(2)
                    .show(ui, |ui| {
                        for (p, s) in &items {
                            ui.label(app.env.display(p));
                            ui.label(human_size(*s));
                            ui.end_row();
                        }
                    });
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let label = match app.settings.method {
                Method::Trash => "Move to Trash",
                Method::Quarantine => "Move to Quarantine",
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
    let t = app.t;
    let Phase::Done(rep) = &app.disk_map.phase else {
        return;
    };
    let rep = rep.clone();
    ui.label(
        RichText::new(format!("{} freed", human_size(rep.freed)))
            .font(font(30.0, Weight::Bold))
            .color(t.accent),
    );
    ui.label(format!(
        "{} items moved. {} skipped.",
        rep.moved.len(),
        rep.skipped.len()
    ));
    if !rep.skipped.is_empty() {
        kit::card(&t).show(ui, |ui| {
            ui.label(RichText::new("Not moved").font(font(14.0, Weight::SemiBold)));
            for (p, why) in &rep.skipped {
                ui.horizontal(|ui| {
                    ui.label(app.env.display(p));
                    ui.label(RichText::new(why.to_string()).color(t.warning));
                });
            }
        });
    }
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if !rep.moved.is_empty() && kit::secondary(ui, &t, "Undo") {
            app.act();
            start_undo(app, rep.clone());
        }
        if kit::secondary(ui, &t, "Show History") {
            app.act();
            app.navigate("history");
        }
        if kit::primary(ui, &t, "Done") {
            app.act();
            app.disk_map.phase = Phase::Map;
        }
    });
}
