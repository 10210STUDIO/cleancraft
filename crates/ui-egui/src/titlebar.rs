//! Window chrome on macOS: transparent title bar over a full-size content view
//! (REVIEW-A 12). The traffic lights float over the top left of the sidebar, and a
//! strip along the top of the window keeps it draggable.
//!
//! Layout contract for the shell:
//! - Nothing interactive in the top [`HEIGHT`] points of the window: that strip moves
//!   the window (drag) and zooms it (double click), like a native title bar.
//! - In the sidebar, the first row starts at [`SIDEBAR_TOP`] or lower, so the traffic
//!   lights (x 0 to [`LIGHTS_WIDTH`], y 0 to [`HEIGHT`]) never sit on top of it.

use egui::{Id, Rect, Response, Sense, Ui, ViewportBuilder, ViewportCommand, pos2};

/// Height of the macOS title bar, in points: the traffic lights are centred in it.
pub const HEIGHT: f32 = 28.0;

/// Width the three traffic lights take from the left edge, margins included.
pub const LIGHTS_WIDTH: f32 = 76.0;

/// Where the first sidebar row may start: the title bar plus a 12 pt gap.
pub const SIDEBAR_TOP: f32 = HEIGHT + 12.0;

/// Whether this platform draws the content behind a transparent title bar.
pub const ACTIVE: bool = cfg!(target_os = "macos");

/// The window flags: the title bar becomes transparent, the content fills the whole
/// window, the title text is hidden and the traffic lights stay. Other platforms
/// ignore these flags and keep their normal decorations.
pub fn viewport(builder: ViewportBuilder) -> ViewportBuilder {
    builder
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
        .with_titlebar_buttons_shown(true)
        .with_decorations(true)
}

/// The drag strip across the top of `ui`: drag moves the window, double click zooms it.
///
/// Call it before drawing anything else, so any control later placed on top of the
/// strip still gets its clicks.
pub fn drag_strip(ui: &mut Ui) -> Response {
    let full = ui.max_rect();
    let strip = Rect::from_min_max(full.min, pos2(full.max.x, full.min.y + HEIGHT));
    let r = ui.interact(
        strip,
        Id::new("cleancraft.titlebar"),
        Sense::click_and_drag(),
    );
    if r.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    } else if r.double_clicked() {
        let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!max));
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Modifiers, PointerButton, RawInput, Vec2, ViewportId, vec2};

    #[test]
    fn viewport_has_a_transparent_title_bar_over_full_size_content() {
        let v = viewport(ViewportBuilder::default());
        assert_eq!(v.fullsize_content_view, Some(true));
        assert_eq!(v.titlebar_shown, Some(false));
        assert_eq!(v.title_shown, Some(false));
        assert_eq!(v.titlebar_buttons_shown, Some(true));
        assert_eq!(v.decorations, Some(true));
        // Dragging the background would fight sliders and drag to scroll.
        assert_eq!(v.movable_by_window_background, None);
    }

    #[test]
    fn layout_leaves_room_for_the_traffic_lights() {
        const { assert!(SIDEBAR_TOP >= HEIGHT + 8.0) };
        const { assert!(LIGHTS_WIDTH >= 68.0) };
    }

    fn input(events: Vec<Event>, time: f64) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1060.0, 680.0))),
            time: Some(time),
            events,
            ..Default::default()
        }
    }

    fn button(pos: egui::Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    /// Runs the given frames and returns every viewport command sent, plus whether the
    /// test button on top of the strip was clicked.
    fn run(frames: Vec<Vec<Event>>, with_button: bool) -> (Vec<ViewportCommand>, bool) {
        let ctx = Context::default();
        let mut cmds = Vec::new();
        let mut clicked = false;
        for (i, ev) in frames.into_iter().enumerate() {
            let mut out = ctx.run_ui(input(ev, i as f64 * 0.05), |ui| {
                drag_strip(ui);
                if with_button {
                    let r = Rect::from_min_size(pos2(900.0, 2.0), Vec2::splat(24.0));
                    clicked |= ui.interact(r, Id::new("btn"), Sense::click()).clicked();
                }
            });
            out.textures_delta.clear();
            if let Some(v) = out.viewport_output.get(&ViewportId::ROOT) {
                cmds.extend(v.commands.iter().cloned());
            }
        }
        (cmds, clicked)
    }

    #[test]
    fn dragging_the_top_strip_moves_the_window() {
        let p = pos2(500.0, 12.0);
        let (cmds, _) = run(
            vec![
                vec![Event::PointerMoved(p)],
                vec![button(p, true)],
                vec![Event::PointerMoved(p + vec2(30.0, 10.0))],
                vec![button(p + vec2(30.0, 10.0), false)],
            ],
            false,
        );
        assert!(cmds.contains(&ViewportCommand::StartDrag), "{cmds:?}");
    }

    #[test]
    fn dragging_below_the_strip_does_not_move_the_window() {
        let p = pos2(500.0, HEIGHT + 6.0);
        let (cmds, _) = run(
            vec![
                vec![Event::PointerMoved(p)],
                vec![button(p, true)],
                vec![Event::PointerMoved(p + vec2(30.0, 10.0))],
                vec![button(p + vec2(30.0, 10.0), false)],
            ],
            false,
        );
        assert!(!cmds.contains(&ViewportCommand::StartDrag), "{cmds:?}");
    }

    #[test]
    fn double_click_on_the_strip_zooms_the_window() {
        let p = pos2(500.0, 12.0);
        let (cmds, _) = run(
            vec![
                vec![Event::PointerMoved(p)],
                vec![button(p, true)],
                vec![button(p, false)],
                vec![button(p, true)],
                vec![button(p, false)],
            ],
            false,
        );
        assert!(cmds.contains(&ViewportCommand::Maximized(true)), "{cmds:?}");
    }

    #[test]
    fn a_control_on_the_strip_still_gets_its_click() {
        let p = pos2(912.0, 14.0);
        let (cmds, clicked) = run(
            vec![
                vec![Event::PointerMoved(p)],
                vec![button(p, true)],
                vec![button(p, false)],
            ],
            true,
        );
        assert!(clicked);
        assert!(!cmds.contains(&ViewportCommand::StartDrag), "{cmds:?}");
    }
}
