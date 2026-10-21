//! The list toolbar every list page shares: a search field, a Filter pop-up with counts, a
//! Sort pop-up with its direction, and the line under them that says what is shown ("Showing
//! 12 of 135 apps") with the filters that are on as chips and Clear Filters.
//!
//! Filters and sort orders are remembered per list in `Settings::lists` (searches are not).
//! Filters never decide what moves: a page's bottom bar keeps counting every selected item
//! and says how many of them the filters hide ([`hidden_note`]); confirmations always list
//! everything that will move, whatever the filters.
//!
//! Keys: Cmd+F focuses the search field, Esc in it clears it. The Filter and Sort buttons
//! are reached with Tab and open with Space or Enter; every entry is announced with its state
//! ("App Store (12)", ticked or not).
//!
//! Filters combine like Finder's search rules: filters of one group marked `any` match when
//! any of them does (App Store or Setapp), every other filter must match too (Unused and
//! Large).

use std::collections::BTreeMap;

use cleancraft_core::Settings;
use cleancraft_core::settings::ListPrefs;
use egui::{
    Align, Align2, Color32, Key, Layout, Modifiers, PopupCloseBehavior, Rect, Response, RichText,
    Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::icons::{self, Icon};
use crate::kit;
use crate::theme::{Tokens, Weight, font, hit, size, space};

/// One filter of a list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filter {
    /// Stable key, remembered in the settings ("app_store").
    pub key: String,
    /// What the menu shows ("App Store").
    pub label: String,
    /// The heading it is listed under in the menu ("Source").
    pub group: String,
    /// Filters of one group with `any` set match when any of them does.
    pub any: bool,
    /// How many items of the whole list it matches (shown in the menu).
    pub count: usize,
}

impl Filter {
    /// A filter that must match on its own (AND with the others).
    pub fn only(
        group: impl Into<String>,
        key: impl Into<String>,
        label: impl Into<String>,
        count: usize,
    ) -> Self {
        Filter {
            key: key.into(),
            label: label.into(),
            group: group.into(),
            any: false,
            count,
        }
    }

    /// One of several choices of a group, any of which may match (OR within the group).
    pub fn any(
        group: impl Into<String>,
        key: impl Into<String>,
        label: impl Into<String>,
        count: usize,
    ) -> Self {
        Filter {
            any: true,
            ..Filter::only(group, key, label, count)
        }
    }

    /// The name the menu entry is announced with: "App Store (12)".
    pub fn entry_name(&self) -> String {
        format!("{} ({})", self.label, self.count)
    }

    fn logic_group(&self) -> &str {
        if self.any { &self.group } else { &self.key }
    }
}

/// One sort order of a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortKey {
    /// Stable key, remembered in the settings ("size").
    pub key: &'static str,
    /// The menu entry and the announced value ("Sort by size").
    pub label: &'static str,
    /// Its usual direction ("Largest first").
    pub first: &'static str,
    /// The other direction ("Smallest first").
    pub second: &'static str,
    /// The usual direction goes from high to low (an arrow pointing down).
    pub descending: bool,
}

pub const BY_SIZE: SortKey = SortKey {
    key: "size",
    label: "Sort by size",
    first: "Largest first",
    second: "Smallest first",
    descending: true,
};
pub const BY_NAME: SortKey = SortKey {
    key: "name",
    label: "Sort by name",
    first: "A to Z",
    second: "Z to A",
    descending: false,
};
/// Oldest first: what has not been touched for longest (Large & Old Files).
pub const BY_DATE: SortKey = SortKey {
    key: "date",
    label: "Sort by date",
    first: "Oldest first",
    second: "Newest first",
    descending: false,
};

/// What a list remembers, read from the settings (defaults when it has never been changed).
pub fn prefs(settings: &Settings, list: &str) -> ListPrefs {
    settings.lists.get(list).cloned().unwrap_or_default()
}

/// Store a list's preferences; true when they changed (the caller saves the settings).
pub fn store(settings: &mut Settings, list: &str, p: ListPrefs) -> bool {
    if prefs(settings, list) == p {
        return false;
    }
    if p == ListPrefs::default() {
        settings.lists.remove(list);
    } else {
        settings.lists.insert(list.to_string(), p);
    }
    true
}

/// Is this filter on?
pub fn on(p: &ListPrefs, key: &str) -> bool {
    p.filters.iter().any(|k| k == key)
}

/// The filters of `filters` that are on (filters a list no longer has are left out).
pub fn active<'a>(p: &ListPrefs, filters: &'a [Filter]) -> Vec<&'a Filter> {
    filters.iter().filter(|f| on(p, &f.key)).collect()
}

/// Is any filter of `filters` on?
pub fn filtering(p: &ListPrefs, filters: &[Filter]) -> bool {
    filters.iter().any(|f| on(p, &f.key))
}

/// Does an item pass the filters that are on? `has(key)` says whether the item matches the
/// filter `key`.
pub fn passes(p: &ListPrefs, filters: &[Filter], has: impl Fn(&str) -> bool) -> bool {
    let mut groups: BTreeMap<&str, bool> = BTreeMap::new();
    for f in filters.iter().filter(|f| on(p, &f.key)) {
        let g = groups.entry(f.logic_group()).or_insert(false);
        *g = *g || has(&f.key);
    }
    groups.values().all(|ok| *ok)
}

/// The sort order chosen (the list's first when none or an unknown one is remembered) and
/// whether it runs the other way round.
pub fn sort_of(p: &ListPrefs, sorts: &[SortKey]) -> (SortKey, bool) {
    match sorts.iter().find(|s| s.key == p.sort) {
        Some(s) => (*s, p.reverse),
        None => (sorts.first().copied().unwrap_or(BY_NAME), false),
    }
}

/// Does `q` (already lower case and trimmed) appear in any of `fields`? An empty query
/// matches everything.
pub fn matches(q: &str, fields: &[&str]) -> bool {
    q.is_empty() || fields.iter().any(|f| f.to_lowercase().contains(q))
}

/// The search as compared: trimmed, lower case.
pub fn query(search: &str) -> String {
    search.trim().to_lowercase()
}

/// " (1 hidden by filters)" when some selected items are not shown, else nothing. Appended to
/// a bottom bar's "3 selected" so the filters never hide what will move.
pub fn hidden_note(hidden: usize) -> String {
    if hidden == 0 {
        String::new()
    } else {
        format!(" ({hidden} hidden by filters)")
    }
}

/// "3 selected (1 hidden by filters)".
pub fn selected_text(selected: &str, hidden: usize) -> String {
    format!("{selected}{}", hidden_note(hidden))
}

// ---------------------------------------------------------------------------------------
// Widgets
// ---------------------------------------------------------------------------------------

/// What a toolbar did this frame.
#[derive(Default)]
pub struct Changes {
    /// The search text changed (typed, or cleared with Esc or Clear Filters).
    pub search: bool,
    /// Filters or sort changed: save the settings.
    pub prefs: bool,
}

impl Changes {
    pub fn any(&self) -> bool {
        self.search || self.prefs
    }
}

/// The search field, named `name` for VoiceOver ("Search apps"). Cmd+F focuses it (when no
/// dialog is open) and Esc clears it. Returns its response and whether the text changed.
pub fn search(
    ui: &mut Ui,
    t: &Tokens,
    text: &mut String,
    name: &str,
    hint: &str,
    width: f32,
) -> (Response, bool) {
    let r = kit::search_field(ui, t, text, hint, width);
    let label = name.to_string();
    r.widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, &label));
    let mut changed = r.changed();
    // Cmd+F, as in every Mac app with a search field.
    if ui.ctx().memory(|m| m.top_modal_layer()).is_none()
        && ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::F))
    {
        r.request_focus();
    }
    // Esc clears the search (egui also gives up the focus on Esc).
    if (r.has_focus() || r.lost_focus())
        && !text.is_empty()
        && ui.input(|i| i.key_pressed(Key::Escape))
    {
        text.clear();
        changed = true;
    }
    (r, changed)
}

/// The Filter button and its pop-up: the filters by group, each with a check mark when on
/// and how many items it matches; Clear Filters at the end. The pop-up stays open while
/// filters are switched. Returns true when a filter changed.
pub fn filter_menu(
    ui: &mut Ui,
    t: &Tokens,
    id: &str,
    filters: &[Filter],
    p: &mut ListPrefs,
) -> bool {
    let n_on = active(p, filters).len();
    let label = if n_on == 0 {
        "Filter".to_string()
    } else {
        format!("Filter ({n_on})")
    };
    let resp = filter_button(ui, t, &label, n_on > 0);
    let value = if n_on == 0 {
        "None".to_string()
    } else {
        active(p, filters)
            .iter()
            .map(|f| f.label.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    resp.widget_info(|| {
        let mut i = WidgetInfo::labeled(WidgetType::Button, true, "Filter");
        i.current_text_value = Some(value.clone());
        i
    });
    let mut changed = false;
    egui::Popup::menu(&resp)
        .id(egui::Id::new(("list-filter-menu", id)))
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            // As wide as the longest entry (label, gap, count), at least 240.
            let fid = font(size::CALLOUT, Weight::Medium);
            let w = filters
                .iter()
                .map(|f| {
                    kit::galley(ui, &f.label, fid.clone(), t.text).size().x
                        + kit::galley(ui, &f.count.to_string(), fid.clone(), t.text)
                            .size()
                            .x
                })
                .fold(0.0f32, f32::max)
                + 32.0
                + 24.0
                + 12.0;
            let w = w.max(240.0);
            ui.set_min_width(w);
            ui.set_max_width(w);
            ui.spacing_mut().item_spacing.y = 2.0;
            let mut last_group = "";
            for f in filters {
                if f.group != last_group && !f.group.is_empty() {
                    if !last_group.is_empty() {
                        ui.add_space(space::XS);
                    }
                    ui.horizontal(|ui| {
                        ui.add_space(10.0);
                        kit::eyebrow(ui, t, &f.group);
                    });
                }
                last_group = &f.group;
                let was = on(p, &f.key);
                if filter_entry(ui, t, f, was) {
                    toggle(p, &f.key);
                    changed = true;
                }
            }
            if n_on > 0 || changed {
                ui.separator();
                let r = clear_entry(ui, t);
                if r.clicked() {
                    clear_filters(p, filters);
                    changed = true;
                    ui.close();
                }
            }
        });
    changed
}

/// Turn a filter on or off.
pub fn toggle(p: &mut ListPrefs, key: &str) {
    if on(p, key) {
        p.filters.retain(|k| k != key);
    } else {
        p.filters.push(key.to_string());
    }
}

/// Turn off every filter of `filters`.
pub fn clear_filters(p: &mut ListPrefs, filters: &[Filter]) {
    p.filters.retain(|k| !filters.iter().any(|f| &f.key == k));
}

/// "Smallest first" -> "smallest first"; "Z to A" stays.
fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match (c.next(), c.clone().next()) {
        (Some(f), Some(n)) if n.is_lowercase() => f.to_lowercase().chain(c).collect(),
        _ => s.to_string(),
    }
}

/// The Sort pop-up: the orders, then the two directions of the chosen one. Announced as
/// "Sort" with the order as its value ("Sort by size", "Sort by size, smallest first").
/// Returns true when the order or its direction changed.
pub fn sort_menu(ui: &mut Ui, id: &str, sorts: &[SortKey], p: &mut ListPrefs) -> bool {
    let (cur, rev) = sort_of(p, sorts);
    let value = if rev {
        format!("{}, {}", cur.label, lower_first(cur.second))
    } else {
        cur.label.to_string()
    };
    let down = cur.descending != rev;
    let mut changed = false;
    ui.scope(|ui| {
        let v = &mut ui.style_mut().visuals.widgets;
        for w in [&mut v.inactive, &mut v.hovered, &mut v.active, &mut v.open] {
            w.corner_radius = egui::CornerRadius::same((hit::FIELD / 2.0) as u8);
        }
        ui.spacing_mut().interact_size.y = hit::FIELD;
        ui.spacing_mut().button_padding = vec2(14.0, 7.0);
        let r = egui::ComboBox::from_id_salt(("list-sort", id))
            .selected_text(RichText::new(cur.label).font(font(size::CALLOUT, Weight::Medium)))
            .icon(move |ui, rect, visuals, _open| {
                let r = Rect::from_center_size(rect.center(), Vec2::splat(13.0));
                arrow(ui.painter(), r, down, visuals.fg_stroke.color);
            })
            .show_ui(ui, |ui| {
                for s in sorts {
                    if kit::menu_choice(ui, s.label, s.key == cur.key) {
                        if s.key != cur.key {
                            p.sort = s.key.to_string();
                            p.reverse = false;
                            changed = true;
                        }
                        ui.close();
                    }
                }
                ui.separator();
                for (text, reverse) in [(cur.first, false), (cur.second, true)] {
                    if kit::menu_choice(ui, text, rev == reverse) {
                        if rev != reverse {
                            p.sort = cur.key.to_string();
                            p.reverse = reverse;
                            changed = true;
                        }
                        ui.close();
                    }
                }
            })
            .response;
        kit::pointer(&r);
        r.widget_info(|| {
            let mut i = WidgetInfo::labeled(WidgetType::ComboBox, true, "Sort");
            i.current_text_value = Some(value.clone());
            i
        });
    });
    changed
}

/// The Filter and Sort pop-ups side by side (after a search field the page lays out).
pub fn menus(
    ui: &mut Ui,
    t: &Tokens,
    id: &str,
    filters: &[Filter],
    sorts: &[SortKey],
    p: &mut ListPrefs,
) -> bool {
    let mut changed = false;
    if !filters.is_empty() {
        changed |= filter_menu(ui, t, id, filters, p);
    }
    if !sorts.is_empty() {
        changed |= sort_menu(ui, id, sorts, p);
    }
    changed
}

/// The whole toolbar in one row: search, Filter, Sort. `list` names the remembered
/// preferences in the settings; changed preferences are stored there (the caller saves).
#[allow(clippy::too_many_arguments)]
pub fn toolbar(
    ui: &mut Ui,
    t: &Tokens,
    settings: &mut Settings,
    list: &str,
    search_text: &mut String,
    search_name: &str,
    filters: &[Filter],
    sorts: &[SortKey],
    search_w: f32,
) -> Changes {
    let mut out = Changes::default();
    let mut p = prefs(settings, list);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S;
        let (_, ch) = search(ui, t, search_text, search_name, "Search", search_w);
        out.search = ch;
        if menus(ui, t, list, filters, sorts, &mut p) {
            out.prefs = store(settings, list, p.clone());
        }
    });
    out
}

/// Should the "Showing" line be drawn? When a search or a filter is on.
pub fn status_shown(p: &ListPrefs, filters: &[Filter], search: &str) -> bool {
    !search.trim().is_empty() || filtering(p, filters)
}

/// The line under a toolbar while a search or a filter is on: "Showing 12 of 135 apps", a
/// chip per filter that is on (click to turn it off) and Clear Filters (clears the search
/// too). Nothing is drawn otherwise. `noun` is (one, many): ("app", "apps").
#[allow(clippy::too_many_arguments)]
pub fn status(
    ui: &mut Ui,
    t: &Tokens,
    shown: usize,
    total: usize,
    noun: (&str, &str),
    filters: &[Filter],
    p: &mut ListPrefs,
    search_text: &mut String,
) -> Changes {
    let mut out = Changes::default();
    if !status_shown(p, filters, search_text) {
        return out;
    }
    let line = showing(shown, total, noun);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 28.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = space::S;
            ui.label(
                RichText::new(&line)
                    .font(font(size::CALLOUT, Weight::Medium))
                    .color(if shown == 0 { t.warning } else { t.text_dim }),
            );
            // A Filter or Sort pop-up opens over this line: its controls wait until it
            // closes (a click there would land in the pop-up).
            ui.add_enabled_ui(!egui::Popup::is_any_open(ui.ctx()), |ui| {
                for f in active(p, filters) {
                    if chip(ui, t, &f.label).clicked() {
                        toggle(p, &f.key);
                        out.prefs = true;
                    }
                }
                let r = kit::ghost(ui, t, "Clear Filters", Some(Icon::Close));
                if r.clicked() {
                    clear_filters(p, filters);
                    if !search_text.is_empty() {
                        search_text.clear();
                        out.search = true;
                    }
                    out.prefs = true;
                }
            });
        },
    );
    out
}

/// [`status`] for a list whose preferences live in the settings under `list`.
#[allow(clippy::too_many_arguments)]
pub fn status_line(
    ui: &mut Ui,
    t: &Tokens,
    settings: &mut Settings,
    list: &str,
    shown: usize,
    total: usize,
    noun: (&str, &str),
    filters: &[Filter],
    search_text: &mut String,
) -> Changes {
    let mut p = prefs(settings, list);
    let mut out = status(ui, t, shown, total, noun, filters, &mut p, search_text);
    if out.prefs {
        out.prefs = store(settings, list, p);
    }
    out
}

/// "Showing 12 of 135 apps", "Showing all 3 apps", "Showing 1 of 1 app".
pub fn showing(shown: usize, total: usize, noun: (&str, &str)) -> String {
    let word = if total == 1 { noun.0 } else { noun.1 };
    if shown == total && total > 1 {
        format!("Showing all {total} {word}")
    } else {
        format!("Showing {shown} of {total} {word}")
    }
}

// ---------------------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------------------

/// The Filter button: a glass pill with a funnel, tinted when filters are on.
fn filter_button(ui: &mut Ui, t: &Tokens, label: &str, lit: bool) -> Response {
    let fid = font(size::CALLOUT, Weight::Medium);
    let g = kit::galley(ui, label, fid, t.text);
    let w = g.size().x + 16.0 + 6.0 + 2.0 * 14.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, hit::FIELD), Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let f = kit::feel(ui, &resp);
    let p = ui.painter();
    let round = rect.height() / 2.0;
    if f.focus {
        kit::focus_ring(p, t, rect, round);
    }
    let fill = if lit {
        icons::lerp(t.glass, t.sec_a, 0.28 + 0.1 * f.hov)
    } else {
        icons::lerp(t.glass, Color32::WHITE, 0.06 * f.hov)
    };
    p.rect_filled(rect, round, fill);
    p.rect_stroke(
        rect,
        round,
        Stroke::new(
            1.0,
            if lit {
                t.sec_a.gamma_multiply(0.7)
            } else {
                t.glass_edge
            },
        ),
        egui::StrokeKind::Inside,
    );
    let ir = Rect::from_center_size(
        pos2(rect.left() + 14.0 + 8.0, rect.center().y),
        Vec2::splat(14.0),
    );
    funnel(p, ir, if lit { t.text } else { t.text_dim });
    p.galley(
        pos2(
            rect.left() + 14.0 + 22.0,
            rect.center().y - g.size().y / 2.0,
        ),
        g,
        t.text,
    );
    resp
}

/// A filter in the pop-up: check mark, label, count at the right.
fn filter_entry(ui: &mut Ui, t: &Tokens, f: &Filter, is_on: bool) -> bool {
    let fid = font(size::CALLOUT, Weight::Medium);
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
    let name = f.entry_name();
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, is_on, &name));
    let feel = kit::feel(ui, &resp);
    let p = ui.painter();
    if feel.hov > 0.0 {
        p.rect_filled(
            r,
            6.0,
            icons::lerp(t.sec_a, Color32::BLACK, 0.25 + 0.15 * feel.down).gamma_multiply(feel.hov),
        );
    }
    if feel.focus {
        kit::focus_ring(p, t, r, 6.0);
    }
    if is_on {
        icons::paint_with(
            p,
            Rect::from_center_size(pos2(r.left() + 16.0, r.center().y), Vec2::splat(12.0)),
            Icon::Check,
            t.text,
            1.8,
        );
    }
    p.text(
        pos2(r.left() + 32.0, r.center().y),
        Align2::LEFT_CENTER,
        &f.label,
        fid.clone(),
        t.text,
    );
    p.text(
        pos2(r.right() - 12.0, r.center().y),
        Align2::RIGHT_CENTER,
        f.count.to_string(),
        fid,
        if f.count == 0 {
            t.text_faint
        } else {
            t.text_dim
        },
    );
    resp.clicked()
}

fn clear_entry(ui: &mut Ui, t: &Tokens) -> Response {
    let fid = font(size::CALLOUT, Weight::Medium);
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Clear Filters"));
    let feel = kit::feel(ui, &resp);
    let p = ui.painter();
    if feel.hov > 0.0 {
        p.rect_filled(
            r,
            6.0,
            icons::lerp(t.sec_a, Color32::BLACK, 0.25).gamma_multiply(feel.hov),
        );
    }
    p.text(
        pos2(r.left() + 32.0, r.center().y),
        Align2::LEFT_CENTER,
        "Clear Filters",
        fid,
        t.text,
    );
    resp
}

/// A filter that is on, under the toolbar: its name and an x. Announced "Remove filter
/// App Store".
fn chip(ui: &mut Ui, t: &Tokens, label: &str) -> Response {
    let fid = font(size::FOOTNOTE, Weight::Medium);
    let g = kit::galley(ui, label, fid, t.text);
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 12.0 + 24.0, 24.0), Sense::click());
    let name = format!("Remove filter {label}");
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &name));
    let f = kit::feel(ui, &resp);
    let p = ui.painter();
    p.rect_filled(
        r,
        r.height() / 2.0,
        t.sec_a.gamma_multiply(0.18 + 0.1 * f.hov),
    );
    p.rect_stroke(
        r,
        r.height() / 2.0,
        Stroke::new(1.0, t.sec_a.gamma_multiply(0.5)),
        egui::StrokeKind::Inside,
    );
    if f.focus {
        kit::focus_ring(p, t, r, r.height() / 2.0);
    }
    p.galley(
        pos2(r.left() + 12.0, r.center().y - g.size().y / 2.0),
        g,
        t.text,
    );
    icons::paint_with(
        p,
        Rect::from_center_size(pos2(r.right() - 13.0, r.center().y), Vec2::splat(9.0)),
        Icon::Close,
        t.text_dim,
        1.5,
    );
    resp
}

/// A funnel: the Filter button's picture.
fn funnel(p: &egui::Painter, r: Rect, c: Color32) {
    let s = Stroke::new(1.5, c);
    let (l, rt, top) = (r.left() + 0.5, r.right() - 0.5, r.top() + 1.5);
    let mid = r.top() + r.height() * 0.52;
    let cx = r.center().x;
    let pts = vec![
        pos2(l, top),
        pos2(rt, top),
        pos2(cx + 1.8, mid),
        pos2(cx + 1.8, r.bottom() - 2.5),
        pos2(cx - 1.8, r.bottom() - 0.5),
        pos2(cx - 1.8, mid),
    ];
    p.add(egui::Shape::closed_line(pts, s));
}

/// An arrow up or down: the sort direction.
fn arrow(p: &egui::Painter, r: Rect, down: bool, c: Color32) {
    let s = Stroke::new(1.6, c);
    let (a, b) = if down {
        (r.center_top(), r.center_bottom())
    } else {
        (r.center_bottom(), r.center_top())
    };
    p.line_segment([a, b], s);
    let dir = if down { -1.0 } else { 1.0 };
    let head = r.width() * 0.32;
    p.line_segment([b, pos2(b.x - head, b.y + dir * head)], s);
    p.line_segment([b, pos2(b.x + head, b.y + dir * head)], s);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(on: &[&str]) -> ListPrefs {
        ListPrefs {
            filters: on.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    fn defs() -> Vec<Filter> {
        vec![
            Filter::any("Source", "store", "App Store", 2),
            Filter::any("Source", "setapp", "Setapp", 1),
            Filter::only("Show only", "unused", "Unused", 1),
            Filter::only("Show only", "large", "Large", 1),
        ]
    }

    #[test]
    fn filters_of_one_group_match_any_and_the_others_all() {
        let d = defs();
        let item = |keys: &'static [&'static str]| move |k: &str| keys.contains(&k);
        assert!(passes(&p(&[]), &d, item(&[])));
        assert!(passes(&p(&["store", "setapp"]), &d, item(&["setapp"])));
        assert!(!passes(&p(&["store"]), &d, item(&["setapp"])));
        assert!(passes(
            &p(&["unused", "large"]),
            &d,
            item(&["unused", "large"])
        ));
        assert!(!passes(&p(&["unused", "large"]), &d, item(&["unused"])));
        assert!(!passes(&p(&["store", "unused"]), &d, item(&["store"])));
        // A remembered filter the list no longer has is ignored.
        assert!(passes(&p(&["gone"]), &d, item(&[])));
    }

    #[test]
    fn sort_falls_back_to_the_first_and_directions_read_well() {
        let sorts = [BY_SIZE, BY_NAME];
        assert_eq!(sort_of(&ListPrefs::default(), &sorts), (BY_SIZE, false));
        let q = ListPrefs {
            sort: "name".into(),
            reverse: true,
            ..Default::default()
        };
        assert_eq!(sort_of(&q, &sorts), (BY_NAME, true));
        let gone = ListPrefs {
            sort: "colour".into(),
            reverse: true,
            ..Default::default()
        };
        assert_eq!(sort_of(&gone, &sorts), (BY_SIZE, false));
    }

    #[test]
    fn texts() {
        assert_eq!(showing(12, 135, ("app", "apps")), "Showing 12 of 135 apps");
        assert_eq!(showing(3, 3, ("app", "apps")), "Showing all 3 apps");
        assert_eq!(showing(0, 1, ("app", "apps")), "Showing 0 of 1 app");
        assert_eq!(
            selected_text("3 selected", 1),
            "3 selected (1 hidden by filters)"
        );
        assert_eq!(selected_text("3 selected", 0), "3 selected");
        assert!(matches("edi", &["Editor"]));
        assert!(!matches("zzz", &["Editor", "com.example"]));
    }

    #[test]
    fn stored_prefs_round_trip_and_defaults_leave_no_entry() {
        let mut s = Settings::default();
        assert!(store(&mut s, "apps", p(&["store"])));
        assert!(!store(&mut s, "apps", p(&["store"])));
        assert_eq!(prefs(&s, "apps"), p(&["store"]));
        assert!(store(&mut s, "apps", ListPrefs::default()));
        assert!(s.lists.is_empty());
    }
}
