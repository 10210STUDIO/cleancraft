//! "Connect an AI agent": docs/guide-ai-agents.md shown in the app (opened from Settings).
//! A small reader for the Markdown the guide uses: headings, paragraphs, lists, code blocks,
//! tables, **bold** and `code` spans. The text is built into the app, so it is always there.

use egui::text::LayoutJob;
use egui::{Color32, RichText, TextFormat, Ui};

use crate::app::CleanApp;
use crate::icons::Icon;
use crate::kit;
use crate::theme::{Tokens, Weight, font, mono, size, space};
use cleancraft_core::route::AppRoute;

/// The guide's text.
pub const GUIDE: &str = include_str!("../../../../docs/guide-ai-agents.md");

#[derive(Default)]
pub struct State {}

/// One block of the guide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Heading(u8, String),
    Para(String),
    Bullet(u8, String),
    Numbered(u8, String, String),
    Code(String),
    Table(Vec<Vec<String>>),
}

/// Split the Markdown into blocks.
pub fn blocks(md: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut lines = md.lines().peekable();
    let mut para = String::new();
    let flush = |para: &mut String, out: &mut Vec<Block>| {
        if !para.trim().is_empty() {
            out.push(Block::Para(para.trim().to_string()));
        }
        para.clear();
    };
    while let Some(l) = lines.next() {
        let indent = (l.len() - l.trim_start().len()) as u8 / 2;
        let tl = l.trim();
        if tl.starts_with("```") {
            flush(&mut para, &mut out);
            let mut code = Vec::new();
            for c in lines.by_ref() {
                if c.trim().starts_with("```") {
                    break;
                }
                code.push(c);
            }
            out.push(Block::Code(code.join("\n")));
        } else if tl.starts_with('|') {
            flush(&mut para, &mut out);
            let mut rows = vec![cells(tl)];
            while let Some(n) = lines.peek() {
                if !n.trim().starts_with('|') {
                    break;
                }
                let r = cells(n.trim());
                if !r.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':')) {
                    rows.push(r);
                }
                lines.next();
            }
            out.push(Block::Table(rows));
        } else if let Some(h) = tl.strip_prefix('#') {
            flush(&mut para, &mut out);
            let level = 1 + h.chars().take_while(|c| *c == '#').count() as u8;
            out.push(Block::Heading(
                level,
                h.trim_start_matches('#').trim().to_string(),
            ));
        } else if let Some(b) = tl.strip_prefix("- ") {
            flush(&mut para, &mut out);
            out.push(Block::Bullet(indent, b.to_string()));
        } else if let Some((n, rest)) = tl.split_once(". ")
            && !n.is_empty()
            && n.chars().all(|c| c.is_ascii_digit())
        {
            flush(&mut para, &mut out);
            out.push(Block::Numbered(indent, n.to_string(), rest.to_string()));
        } else if tl.is_empty() {
            flush(&mut para, &mut out);
        } else {
            if !para.is_empty() {
                para.push(' ');
            }
            para.push_str(tl);
        }
    }
    flush(&mut para, &mut out);
    out
}

fn cells(row: &str) -> Vec<String> {
    row.trim_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

/// A line with **bold** and `code` spans.
pub fn inline(t: &Tokens, text: &str, size_pt: f32, color: Color32, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let mut bold = false;
    let mut code = false;
    let mut cur = String::new();
    let push = |job: &mut LayoutJob, s: &mut String, bold: bool, code: bool| {
        if s.is_empty() {
            return;
        }
        let fmt = if code {
            TextFormat {
                font_id: mono(size_pt - 1.0),
                color: t.text,
                background: Color32::from_white_alpha(14),
                ..Default::default()
            }
        } else {
            TextFormat {
                font_id: font(
                    size_pt,
                    if bold {
                        Weight::SemiBold
                    } else {
                        Weight::Regular
                    },
                ),
                color: if bold { t.text } else { color },
                ..Default::default()
            }
        };
        job.append(s, 0.0, fmt);
        s.clear();
    };
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '`' {
            push(&mut job, &mut cur, bold, code);
            code = !code;
        } else if c == '*' && !code && chars.peek() == Some(&'*') {
            chars.next();
            push(&mut job, &mut cur, bold, code);
            bold = !bold;
        } else {
            cur.push(c);
        }
    }
    push(&mut job, &mut cur, bold, code);
    job
}

pub fn ui(app: &mut CleanApp, ui: &mut Ui) {
    let t = app.t;
    ui.horizontal(|ui| {
        if kit::ghost(ui, &t, "Settings", Some(Icon::Back)).clicked() {
            app.act();
            app.open_route(&AppRoute::Settings);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if kit::primary(ui, &t, "Connect AI Apps") {
                app.act();
                app.open_route(&AppRoute::ConnectAi);
            }
        });
    });
    ui.add_space(space::M);
    let out = egui::ScrollArea::vertical()
        .id_salt("guide")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let w = ui.available_width().min(space::MEASURE + 260.0);
            ui.set_max_width(w);
            for b in blocks(GUIDE) {
                block(ui, &t, &b, w);
            }
            ui.add_space(space::XL);
        });
    kit::scroll_fade_page(ui, &out, &t);
}

fn block(ui: &mut Ui, t: &Tokens, b: &Block, w: f32) {
    match b {
        Block::Heading(1, s) => {
            ui.label(
                RichText::new(s)
                    .font(font(size::TITLE, Weight::Bold))
                    .color(t.text),
            );
            ui.add_space(space::S);
        }
        Block::Heading(_, s) => {
            ui.add_space(space::M);
            ui.label(
                RichText::new(s)
                    .font(font(size::TITLE3, Weight::SemiBold))
                    .color(t.text),
            );
            ui.add_space(space::XS);
        }
        Block::Para(s) => {
            ui.label(inline(t, s, size::BODY, t.text_dim, w));
            ui.add_space(space::S);
        }
        Block::Bullet(lvl, s) | Block::Numbered(lvl, _, s) => {
            let mark = match b {
                Block::Numbered(_, n, _) => format!("{n}."),
                _ => "\u{2022}".to_string(),
            };
            ui.horizontal_top(|ui| {
                ui.add_space(8.0 + 18.0 * f32::from(*lvl));
                ui.add_sized(
                    [18.0, 18.0],
                    egui::Label::new(
                        RichText::new(mark)
                            .font(font(size::BODY, Weight::Medium))
                            .color(t.sec_a),
                    ),
                );
                let wrap = ui.available_width();
                ui.label(inline(t, s, size::BODY, t.text_dim, wrap));
            });
            ui.add_space(space::XS);
        }
        Block::Code(s) => {
            kit::well(t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(
                    egui::Label::new(RichText::new(s).font(mono(size::CALLOUT)).color(t.text))
                        .wrap(),
                );
            });
            ui.add_space(space::S);
        }
        Block::Table(rows) => {
            kit::card(t).show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Grid::new(("guide-table", rows.len(), rows.first().map(|r| r.join(""))))
                    .num_columns(rows.first().map_or(0, |r| r.len()))
                    .spacing([space::L, space::S])
                    .striped(false)
                    .show(ui, |ui| {
                        let cols = rows.first().map_or(1, |r| r.len()).max(1);
                        let cw =
                            (ui.available_width() - space::L * (cols as f32 - 1.0)) / cols as f32;
                        for (i, r) in rows.iter().enumerate() {
                            for c in r {
                                if i == 0 {
                                    ui.label(
                                        RichText::new(c)
                                            .font(font(size::CALLOUT, Weight::SemiBold))
                                            .color(t.text),
                                    );
                                } else {
                                    ui.label(inline(
                                        t,
                                        c,
                                        size::CALLOUT,
                                        t.text_dim,
                                        cw.max(120.0),
                                    ));
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
            ui.add_space(space::S);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guide_parses_into_blocks() {
        let b = blocks(GUIDE);
        assert!(matches!(&b[0], Block::Heading(1, s) if s == "Connect an AI agent to CleanCraft"));
        assert!(
            b.iter()
                .any(|x| matches!(x, Block::Code(c) if c.contains("serve")))
        );
        let table = b
            .iter()
            .find_map(|x| match x {
                Block::Table(r) => Some(r),
                _ => None,
            })
            .expect("tools table");
        assert_eq!(table[0], vec!["Tool", "What it does", "Changes anything?"]);
        assert!(table.iter().any(|r| r[0] == "`execute_plan`"));
        assert!(
            b.iter()
                .any(|x| matches!(x, Block::Numbered(0, n, _) if n == "1"))
        );
        assert!(b.iter().any(|x| matches!(x, Block::Bullet(1, _))));
    }

    /// Every MCP tool is in the guide's table.
    #[test]
    fn guide_lists_every_mcp_tool() {
        let srcs = [
            include_str!("../../../mcp/src/lib.rs"),
            include_str!("../../../mcp/src/apps.rs"),
            include_str!("../../../mcp/src/monitor.rs"),
            include_str!("../../../mcp/src/perf.rs"),
        ];
        let mut n = 0;
        for src in srcs {
            let mut after_tool = false;
            for l in src.lines() {
                let l = l.trim();
                if l.starts_with("#[tool(") {
                    after_tool = true;
                } else if after_tool && let Some(rest) = l.strip_prefix("async fn ") {
                    let name = rest.split('(').next().unwrap_or("");
                    assert!(
                        GUIDE.contains(&format!("| `{name}` |")),
                        "{name} missing from the guide"
                    );
                    n += 1;
                    after_tool = false;
                }
            }
        }
        assert!(n >= 16, "{n} tools found");
    }

    #[test]
    fn guide_wording() {
        assert!(!GUIDE.contains('\u{2014}'));
        assert!(!GUIDE.to_lowercase().split_whitespace().any(|w| w == "nor"));
    }
}
