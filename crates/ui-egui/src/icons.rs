//! CleanCraft's own vector icons, painted with egui shapes (no font glyphs), so they stay
//! crisp at every size and scale factor and never show as empty boxes.
//!
//! Every icon is drawn on a 24 x 24 grid scaled to the target rectangle. Lines use round ends
//! and a stroke that grows with the size. `illustration` composes an icon into the large
//! picture shown on each module's start screen.

use std::f32::consts::{PI, TAU};

use egui::epaint::{Mesh, PathShape, PathStroke, Vertex, WHITE_UV};
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

use crate::theme::motion;

/// Every icon CleanCraft draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    // Modules.
    Sweep,
    Junk,
    Mail,
    Trash,
    LargeOld,
    Duplicates,
    Similar,
    Remover,
    Updates,
    Startup,
    Tasks,
    Shield,
    Privacy,
    DiskMap,
    Monitor,
    // Shell.
    History,
    Settings,
    Logo,
    // Small UI icons.
    Check,
    Warning,
    Folder,
    File,
    App,
    Undo,
    Reveal,
    Search,
    Stop,
    Close,
    Chevron,
    Picture,
    Gear,
}

impl Icon {
    /// Every icon, for tests and the gallery.
    pub const ALL: [Icon; 31] = [
        Icon::Sweep,
        Icon::Junk,
        Icon::Mail,
        Icon::Trash,
        Icon::LargeOld,
        Icon::Duplicates,
        Icon::Similar,
        Icon::Remover,
        Icon::Updates,
        Icon::Startup,
        Icon::Tasks,
        Icon::Shield,
        Icon::Privacy,
        Icon::DiskMap,
        Icon::Monitor,
        Icon::History,
        Icon::Settings,
        Icon::Logo,
        Icon::Check,
        Icon::Warning,
        Icon::Folder,
        Icon::File,
        Icon::App,
        Icon::Undo,
        Icon::Reveal,
        Icon::Search,
        Icon::Stop,
        Icon::Close,
        Icon::Chevron,
        Icon::Picture,
        Icon::Gear,
    ];
}

/// A pen on the 24-unit grid of one icon.
struct Pen<'a> {
    p: &'a Painter,
    r: Rect,
    c: Color32,
    w: f32,
}

impl Pen<'_> {
    fn pt(&self, x: f32, y: f32) -> Pos2 {
        pos2(
            self.r.left() + x / 24.0 * self.r.width(),
            self.r.top() + y / 24.0 * self.r.height(),
        )
    }

    fn u(&self, v: f32) -> f32 {
        v / 24.0 * self.r.width()
    }

    fn stroke(&self) -> Stroke {
        Stroke::new(self.w, self.c)
    }

    /// An open line with round ends.
    fn line(&self, pts: &[(f32, f32)]) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.pt(*x, *y)).collect();
        self.poly(v, false);
    }

    fn poly(&self, v: Vec<Pos2>, closed: bool) {
        if v.len() < 2 {
            return;
        }
        if !closed {
            let r = self.w / 2.0;
            self.p.circle_filled(v[0], r, self.c);
            self.p.circle_filled(*v.last().unwrap_or(&v[0]), r, self.c);
            // Round joins: a dot on every corner.
            for q in &v[1..v.len() - 1] {
                self.p.circle_filled(*q, r * 0.98, self.c);
            }
        }
        let shape = if closed {
            PathShape::closed_line(v, PathStroke::new(self.w, self.c))
        } else {
            PathShape::line(v, PathStroke::new(self.w, self.c))
        };
        self.p.add(Shape::Path(shape));
    }

    fn closed(&self, pts: &[(f32, f32)]) {
        let v: Vec<Pos2> = pts.iter().map(|(x, y)| self.pt(*x, *y)).collect();
        self.poly(v, true);
    }

    fn circle(&self, x: f32, y: f32, r: f32) {
        self.p
            .circle_stroke(self.pt(x, y), self.u(r), self.stroke());
    }

    fn dot(&self, x: f32, y: f32, r: f32) {
        self.p.circle_filled(self.pt(x, y), self.u(r), self.c);
    }

    /// Points of an arc, angles in degrees, 0 = right, growing clockwise (screen y is down).
    fn arc_pts(&self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32) -> Vec<Pos2> {
        let n = (((a1 - a0).abs() / 8.0).ceil() as usize).max(2);
        (0..=n)
            .map(|i| {
                let a = (a0 + (a1 - a0) * i as f32 / n as f32).to_radians();
                self.pt(cx + r * a.cos(), cy + r * a.sin())
            })
            .collect()
    }

    fn arc(&self, cx: f32, cy: f32, r: f32, a0: f32, a1: f32) {
        let v = self.arc_pts(cx, cy, r, a0, a1);
        self.poly(v, false);
    }

    /// Rounded rectangle outline.
    fn rrect(&self, x: f32, y: f32, w: f32, h: f32, rad: f32) {
        let r = Rect::from_min_max(self.pt(x, y), self.pt(x + w, y + h));
        self.p
            .rect_stroke(r, self.u(rad), self.stroke(), egui::StrokeKind::Middle);
    }

    fn rrect_fill(&self, x: f32, y: f32, w: f32, h: f32, rad: f32) {
        let r = Rect::from_min_max(self.pt(x, y), self.pt(x + w, y + h));
        self.p.rect_filled(r, self.u(rad), self.c);
    }

    /// Fill a star-shaped polygon (every point visible from `c`).
    fn fan(&self, c: (f32, f32), pts: &[Pos2]) {
        let mut m = Mesh::default();
        m.vertices.push(Vertex {
            pos: self.pt(c.0, c.1),
            uv: WHITE_UV,
            color: self.c,
        });
        for q in pts {
            m.vertices.push(Vertex {
                pos: *q,
                uv: WHITE_UV,
                color: self.c,
            });
        }
        let n = pts.len() as u32;
        for i in 0..n {
            m.add_triangle(0, 1 + i, 1 + (i + 1) % n);
        }
        self.p.add(Shape::mesh(m));
    }

    /// A four-pointed sparkle.
    fn sparkle(&self, cx: f32, cy: f32, big: f32, small: f32) {
        let pts: Vec<Pos2> = (0..8)
            .map(|i| {
                let a = i as f32 * PI / 4.0 - PI / 2.0;
                let r = if i % 2 == 0 { big } else { small };
                self.pt(cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();
        self.fan((cx, cy), &pts);
    }

    /// A quadratic curve's points from a to b bending toward c.
    fn quad(&self, a: (f32, f32), c: (f32, f32), b: (f32, f32)) -> Vec<Pos2> {
        (0..=12)
            .map(|i| {
                let t = i as f32 / 12.0;
                let x = (1.0 - t).powi(2) * a.0 + 2.0 * (1.0 - t) * t * c.0 + t * t * b.0;
                let y = (1.0 - t).powi(2) * a.1 + 2.0 * (1.0 - t) * t * c.1 + t * t * b.1;
                self.pt(x, y)
            })
            .collect()
    }
}

/// Stroke width that suits an icon of `side` points.
pub fn default_stroke(side: f32) -> f32 {
    (side * 0.083).clamp(1.25, 8.0)
}

/// Paint `icon` in `rect` (square works best) with the default stroke.
pub fn paint(p: &Painter, rect: Rect, icon: Icon, color: Color32) {
    paint_with(p, rect, icon, color, default_stroke(rect.width()));
}

/// Paint `icon` in `rect` with a given stroke width.
pub fn paint_with(p: &Painter, rect: Rect, icon: Icon, color: Color32, width: f32) {
    let pen = Pen {
        p,
        r: rect,
        c: color,
        w: width,
    };
    draw(&pen, icon);
}

fn gear_points(pen: &Pen<'_>, cx: f32, cy: f32, outer: f32, inner: f32, teeth: usize) -> Vec<Pos2> {
    let mut v = Vec::new();
    let step = TAU / teeth as f32;
    for i in 0..teeth {
        let a = i as f32 * step;
        // Each tooth: flat top between two shoulders.
        for (da, r) in [(-0.30, inner), (-0.17, outer), (0.17, outer), (0.30, inner)] {
            let ang = a + da * step * 1.6;
            v.push(pen.pt(cx + r * ang.cos(), cy + r * ang.sin()));
        }
    }
    v
}

fn draw(pen: &Pen<'_>, icon: Icon) {
    match icon {
        Icon::Sweep => {
            pen.sparkle(10.0, 13.5, 8.5, 2.3);
            pen.sparkle(18.5, 5.5, 3.8, 1.1);
            pen.dot(19.0, 17.5, 1.3);
        }
        Icon::Junk => {
            // An open-topped box of loose pieces.
            pen.closed(&[
                (12.0, 3.0),
                (20.0, 7.3),
                (20.0, 16.7),
                (12.0, 21.0),
                (4.0, 16.7),
                (4.0, 7.3),
            ]);
            pen.line(&[(4.0, 7.3), (12.0, 11.6), (20.0, 7.3)]);
            pen.line(&[(12.0, 11.6), (12.0, 21.0)]);
            pen.line(&[(8.0, 5.2), (16.0, 9.5)]);
        }
        Icon::Mail => {
            pen.rrect(3.0, 6.0, 18.0, 13.0, 2.5);
            pen.line(&[(3.8, 7.5), (12.0, 13.5), (20.2, 7.5)]);
        }
        Icon::Trash => {
            pen.line(&[(4.0, 6.5), (20.0, 6.5)]);
            pen.line(&[(9.5, 6.5), (9.5, 4.0), (14.5, 4.0), (14.5, 6.5)]);
            pen.line(&[(6.2, 6.5), (7.2, 20.0), (16.8, 20.0), (17.8, 6.5)]);
            pen.line(&[(10.2, 10.5), (10.2, 16.2)]);
            pen.line(&[(13.8, 10.5), (13.8, 16.2)]);
        }
        Icon::LargeOld => {
            pen.closed(&[
                (5.5, 3.0),
                (14.0, 3.0),
                (19.5, 8.5),
                (19.5, 21.0),
                (5.5, 21.0),
            ]);
            pen.line(&[(14.0, 3.0), (14.0, 8.5), (19.5, 8.5)]);
            pen.circle(12.5, 14.8, 3.6);
            pen.line(&[(12.5, 13.0), (12.5, 14.8), (14.0, 15.8)]);
        }
        Icon::Duplicates => {
            pen.line(&[
                (8.5, 6.5),
                (8.5, 3.0),
                (20.5, 3.0),
                (20.5, 16.5),
                (16.5, 16.5),
            ]);
            pen.rrect(3.5, 7.0, 12.5, 14.0, 2.0);
            pen.line(&[(6.8, 11.5), (12.7, 11.5)]);
            pen.line(&[(6.8, 15.0), (11.0, 15.0)]);
        }
        Icon::Similar | Icon::Picture => {
            pen.rrect(3.0, 5.0, 18.0, 14.5, 2.5);
            pen.line(&[
                (3.8, 17.2),
                (9.0, 11.8),
                (13.0, 15.5),
                (15.5, 13.0),
                (20.2, 17.2),
            ]);
            pen.circle(15.8, 9.3, 1.6);
            if icon == Icon::Similar {
                pen.line(&[(6.0, 2.6), (20.5, 2.6)]);
            }
        }
        Icon::Remover => {
            pen.rrect(3.5, 3.5, 7.5, 7.5, 2.0);
            pen.rrect(13.0, 3.5, 7.5, 7.5, 2.0);
            pen.rrect(3.5, 13.0, 7.5, 7.5, 2.0);
            pen.line(&[(14.0, 14.0), (19.5, 19.5)]);
            pen.line(&[(19.5, 14.0), (14.0, 19.5)]);
        }
        Icon::Updates => {
            pen.arc(12.0, 12.0, 8.0, -60.0, 225.0);
            // Arrow head at the start of the arc (upper right).
            let a = (-60.0f32).to_radians();
            let (hx, hy) = (12.0 + 8.0 * a.cos(), 12.0 + 8.0 * a.sin());
            pen.line(&[(hx - 4.2, hy - 0.6), (hx, hy), (hx + 0.4, hy + 4.2)]);
            pen.line(&[(12.0, 8.0), (12.0, 15.5)]);
            pen.line(&[(9.0, 12.7), (12.0, 15.7), (15.0, 12.7)]);
        }
        Icon::Startup => {
            pen.arc(12.0, 13.0, 7.8, -55.0, 235.0);
            pen.line(&[(12.0, 3.2), (12.0, 11.5)]);
        }
        Icon::Tasks => {
            pen.arc(12.0, 15.0, 8.5, 180.0, 360.0);
            for a in [200.0f32, 235.0, 270.0, 305.0, 340.0] {
                let r = a.to_radians();
                pen.line(&[
                    (12.0 + 6.2 * r.cos(), 15.0 + 6.2 * r.sin()),
                    (12.0 + 7.0 * r.cos(), 15.0 + 7.0 * r.sin()),
                ]);
            }
            pen.line(&[(12.0, 15.0), (16.6, 9.6)]);
            pen.dot(12.0, 15.0, 1.9);
            pen.line(&[(5.5, 19.5), (18.5, 19.5)]);
        }
        Icon::Shield => {
            let mut v = vec![pen.pt(12.0, 2.8), pen.pt(19.8, 5.8)];
            v.extend(pen.quad((19.8, 5.8), (20.2, 17.0), (12.0, 21.2)));
            v.extend(pen.quad((12.0, 21.2), (3.8, 17.0), (4.2, 5.8)));
            pen.poly(v, true);
            pen.line(&[(8.3, 12.2), (11.0, 14.9), (15.9, 9.6)]);
        }
        Icon::Privacy => {
            let mut v = pen.quad((2.5, 12.0), (12.0, 3.0), (21.5, 12.0));
            v.extend(pen.quad((21.5, 12.0), (12.0, 21.0), (2.5, 12.0)));
            pen.poly(v, true);
            pen.circle(12.0, 12.0, 3.4);
            pen.dot(12.0, 12.0, 1.2);
        }
        Icon::DiskMap => {
            pen.rrect(3.0, 4.0, 18.0, 16.0, 2.5);
            pen.line(&[(11.0, 4.0), (11.0, 20.0)]);
            pen.line(&[(11.0, 12.5), (21.0, 12.5)]);
            pen.line(&[(16.0, 12.5), (16.0, 20.0)]);
            pen.line(&[(3.0, 14.5), (11.0, 14.5)]);
        }
        Icon::Monitor => {
            pen.line(&[
                (2.5, 12.5),
                (7.0, 12.5),
                (9.5, 6.5),
                (13.0, 18.5),
                (15.5, 10.0),
                (17.2, 12.5),
                (21.5, 12.5),
            ]);
        }
        Icon::History => {
            pen.arc(12.5, 12.0, 8.3, 200.0, 520.0);
            let a = 200.0f32.to_radians();
            let (hx, hy) = (12.5 + 8.3 * a.cos(), 12.0 + 8.3 * a.sin());
            pen.line(&[(hx - 2.6, hy - 3.2), (hx, hy), (hx + 3.4, hy - 1.6)]);
            pen.line(&[(12.5, 7.8), (12.5, 12.0), (15.6, 14.0)]);
        }
        Icon::Settings | Icon::Gear => {
            let v = gear_points(pen, 12.0, 12.0, 9.4, 7.2, 8);
            pen.poly(v, true);
            pen.circle(12.0, 12.0, 3.0);
        }
        Icon::Logo => {
            // A crisp C with a spark in its opening.
            pen.arc(11.5, 12.0, 7.8, 40.0, 320.0);
            pen.sparkle(18.6, 12.0, 3.6, 1.0);
        }
        Icon::Check => pen.line(&[(5.0, 12.5), (10.0, 17.5), (19.0, 7.0)]),
        Icon::Warning => {
            let mut v = Vec::new();
            v.extend([pen.pt(12.0, 3.5), pen.pt(21.0, 19.5), pen.pt(3.0, 19.5)]);
            pen.poly(v, true);
            pen.line(&[(12.0, 9.5), (12.0, 14.0)]);
            pen.dot(12.0, 16.9, 1.15);
        }
        Icon::Folder => pen.closed(&[
            (3.0, 6.5),
            (9.5, 6.5),
            (11.5, 8.8),
            (21.0, 8.8),
            (21.0, 19.0),
            (3.0, 19.0),
        ]),
        Icon::File => {
            pen.closed(&[
                (5.5, 3.0),
                (14.0, 3.0),
                (19.0, 8.0),
                (19.0, 21.0),
                (5.5, 21.0),
            ]);
            pen.line(&[(14.0, 3.0), (14.0, 8.0), (19.0, 8.0)]);
        }
        Icon::App => {
            pen.rrect(4.0, 4.0, 16.0, 16.0, 4.5);
            pen.line(&[(8.5, 15.5), (12.0, 8.0), (15.5, 15.5)]);
            pen.line(&[(9.8, 13.0), (14.2, 13.0)]);
        }
        Icon::Undo => {
            pen.line(&[(8.5, 4.8), (4.5, 8.8), (8.5, 12.8)]);
            let mut v = vec![pen.pt(4.5, 8.8), pen.pt(14.0, 8.8)];
            v.extend(pen.arc_pts(14.0, 14.0, 5.2, -90.0, 90.0));
            v.push(pen.pt(9.0, 19.2));
            pen.poly(v, false);
        }
        Icon::Reveal => {
            pen.line(&[
                (11.0, 5.5),
                (4.5, 5.5),
                (4.5, 19.5),
                (18.5, 19.5),
                (18.5, 13.0),
            ]);
            pen.line(&[(11.5, 12.5), (19.5, 4.5)]);
            pen.line(&[(14.0, 4.5), (19.5, 4.5), (19.5, 10.0)]);
        }
        Icon::Search => {
            pen.circle(10.5, 10.5, 6.0);
            pen.line(&[(15.0, 15.0), (20.0, 20.0)]);
        }
        Icon::Stop => pen.rrect_fill(6.5, 6.5, 11.0, 11.0, 2.5),
        Icon::Close => {
            pen.line(&[(6.5, 6.5), (17.5, 17.5)]);
            pen.line(&[(17.5, 6.5), (6.5, 17.5)]);
        }
        Icon::Chevron => pen.line(&[(9.5, 5.5), (16.0, 12.0), (9.5, 18.5)]),
    }
}

// ---------------------------------------------------------------------------------------
// Gradients and glows (shared with kit).
// ---------------------------------------------------------------------------------------

/// A rectangle filled with a vertical gradient.
pub fn vertical_gradient(p: &Painter, r: Rect, top: Color32, bottom: Color32) {
    let mut m = Mesh::default();
    let i = m.vertices.len() as u32;
    for (pos, c) in [
        (r.left_top(), top),
        (r.right_top(), top),
        (r.right_bottom(), bottom),
        (r.left_bottom(), bottom),
    ] {
        m.vertices.push(Vertex {
            pos,
            uv: WHITE_UV,
            color: c,
        });
    }
    m.add_triangle(i, i + 1, i + 2);
    m.add_triangle(i, i + 2, i + 3);
    p.add(Shape::mesh(m));
}

/// A soft radial glow: `color` in the middle fading to nothing at `radius` (an ellipse when
/// `squash` differs from 1).
pub fn glow(p: &Painter, center: Pos2, radius: f32, color: Color32, squash: f32) {
    let mut m = Mesh::default();
    let rings = [
        (0.0, 1.0),
        (0.25, 0.82),
        (0.5, 0.5),
        (0.75, 0.2),
        (1.0, 0.0),
    ];
    let seg = 64u32;
    m.vertices.push(Vertex {
        pos: center,
        uv: WHITE_UV,
        color,
    });
    for (ri, (f, a)) in rings.iter().enumerate().skip(1) {
        let c = color.gamma_multiply(*a);
        for s in 0..seg {
            let ang = s as f32 / seg as f32 * TAU;
            m.vertices.push(Vertex {
                pos: center + vec2(ang.cos(), ang.sin() * squash) * radius * *f,
                uv: WHITE_UV,
                color: c,
            });
        }
        let base = 1 + (ri as u32 - 1) * seg;
        for s in 0..seg {
            let n = (s + 1) % seg;
            if ri == 1 {
                m.add_triangle(0, base + s, base + n);
            } else {
                let prev = base - seg;
                m.add_triangle(prev + s, base + s, base + n);
                m.add_triangle(prev + s, base + n, prev + n);
            }
        }
    }
    p.add(Shape::mesh(m));
}

/// A disc filled with a diagonal two-colour gradient (top-left `a`, bottom-right `b`).
pub fn gradient_disc(p: &Painter, center: Pos2, radius: f32, a: Color32, b: Color32) {
    let mut m = Mesh::default();
    let seg = 72u32;
    m.vertices.push(Vertex {
        pos: center,
        uv: WHITE_UV,
        color: lerp(a, b, 0.5),
    });
    for s in 0..seg {
        let ang = s as f32 / seg as f32 * TAU;
        let d = vec2(ang.cos(), ang.sin());
        // Position along the top-left to bottom-right diagonal, 0..1.
        let t = ((d.x + d.y) / std::f32::consts::SQRT_2 + 1.0) / 2.0;
        m.vertices.push(Vertex {
            pos: center + d * radius,
            uv: WHITE_UV,
            color: lerp(a, b, t),
        });
    }
    for s in 0..seg {
        m.add_triangle(0, 1 + s, 1 + (s + 1) % seg);
    }
    p.add(Shape::mesh(m));
}

/// A rounded square filled with a diagonal gradient (used for icon tiles).
pub fn gradient_tile(p: &Painter, r: Rect, rounding: f32, a: Color32, b: Color32) {
    let mut m = Mesh::default();
    let c = r.center();
    let rad = rounding.min(r.width() / 2.0).min(r.height() / 2.0);
    let mut outline = Vec::new();
    let corners = [
        (r.right_bottom() - vec2(rad, rad), 0.0f32),
        (r.left_bottom() + vec2(rad, -rad), 90.0),
        (r.left_top() + vec2(rad, rad), 180.0),
        (r.right_top() + vec2(-rad, rad), 270.0),
    ];
    for (cc, start) in corners {
        for i in 0..=8 {
            let a = (start + 90.0 * i as f32 / 8.0).to_radians();
            outline.push(cc + vec2(a.cos(), a.sin()) * rad);
        }
    }
    let diag = |q: Pos2| {
        let v = q - r.min;
        ((v.x / r.width() + v.y / r.height()) / 2.0).clamp(0.0, 1.0)
    };
    m.vertices.push(Vertex {
        pos: c,
        uv: WHITE_UV,
        color: lerp(a, b, 0.5),
    });
    for q in &outline {
        m.vertices.push(Vertex {
            pos: *q,
            uv: WHITE_UV,
            color: lerp(a, b, diag(*q)),
        });
    }
    let n = outline.len() as u32;
    for i in 0..n {
        m.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    p.add(Shape::mesh(m));
}

/// Linear mix of two colours (premultiplied space).
pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        m(a.r(), b.r()),
        m(a.g(), b.g()),
        m(a.b(), b.b()),
        m(a.a(), b.a()),
    )
}

/// A module's start-screen picture: a glowing orb in the section colours, two thin orbit
/// rings, drifting sparkles and the module's icon in white. `time` drives the drift.
pub fn illustration(
    p: &Painter,
    center: Pos2,
    side: f32,
    icon: Icon,
    a: Color32,
    b: Color32,
    time: f64,
) {
    let r = side / 2.0;
    glow(p, center, r * 1.25, a.gamma_multiply(0.28), 1.0);
    // Orbit rings.
    p.circle_stroke(
        center,
        r * 0.98,
        Stroke::new(1.0, Color32::from_white_alpha(18)),
    );
    p.circle_stroke(
        center,
        r * 0.80,
        Stroke::new(1.0, Color32::from_white_alpha(26)),
    );
    // The orb.
    let orb = r * 0.60;
    p.add(Shape::circle_filled(
        center + vec2(0.0, orb * 0.12),
        orb * 1.02,
        Color32::from_black_alpha(90),
    ));
    gradient_disc(p, center, orb, a, b);
    // Glass highlight on the upper left.
    glow(
        p,
        center + vec2(-orb * 0.35, -orb * 0.45),
        orb * 0.7,
        Color32::from_white_alpha(46),
        0.65,
    );
    p.circle_stroke(center, orb, Stroke::new(1.2, Color32::from_white_alpha(60)));
    // The icon.
    let isz = orb * 1.0;
    let ir = Rect::from_center_size(center, Vec2::splat(isz));
    paint_with(
        p,
        ir,
        icon,
        Color32::from_black_alpha(60),
        default_stroke(isz) * 1.05,
    );
    let ir2 = ir.translate(vec2(0.0, -side * 0.006));
    paint_with(p, ir2, icon, Color32::WHITE, default_stroke(isz) * 0.95);
    // Satellites on the orbits.
    let phase = motion::looped(time, motion::DRIFT * 4.0) * TAU;
    for (k, (rad, off, size, col)) in [
        (0.98, 0.0, 0.035, a),
        (0.80, 2.4, 0.026, b),
        (0.98, 3.7, 0.022, Color32::WHITE),
    ]
    .into_iter()
    .enumerate()
    {
        let dir = if k % 2 == 0 { 1.0 } else { -1.0 };
        let ang = off + phase * dir;
        let q = center + vec2(ang.cos(), ang.sin()) * r * rad;
        glow(p, q, side * size * 2.6, col.gamma_multiply(0.45), 1.0);
        p.circle_filled(q, side * size * 0.6, col);
    }
    // Sparkles that breathe.
    let pen = Pen {
        p,
        r: Rect::from_center_size(center, Vec2::splat(side)),
        c: Color32::WHITE,
        w: 1.0,
    };
    let breathe = |o: f32| {
        0.55 + 0.45
            * motion::ease_in_out(
                ((motion::looped(time, motion::DRIFT / 3.0) + o) * 2.0)
                    .fract()
                    .min(1.0),
            )
    };
    let s1 = Pen {
        c: Color32::from_white_alpha((200.0 * breathe(0.0)) as u8),
        ..pen
    };
    s1.sparkle(19.6, 5.2, 1.5, 0.35);
    let s2 = Pen {
        c: a.gamma_multiply(breathe(0.4)),
        ..pen
    };
    s2.sparkle(4.6, 17.8, 1.1, 0.28);
}
