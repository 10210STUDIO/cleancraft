//! One original animated 3D object per module, drawn with [`crate::render3d`].
//!
//! Every hero has three states:
//! - **Idle**: turns or sways slowly, floats, an occasional glint.
//! - **Scanning**: moves faster, particles orbit it, its glow follows the progress.
//! - **Done**: springs into its rest pose with a burst of particles and a shock ring.
//!
//! Colours come from the module's section in [`crate::theme::Section`]. Timings come from
//! [`crate::motion`]; with `REDUCE_MOTION` the heroes hold a still pose.
//!
//! Pages call [`show`] (allocates the space and tracks the time since the state changed) or
//! [`draw`] (paints into a given rect with an explicit [`HeroState`]).

use crate::motion::{self, ENTER, Easing, Loop, Motion, lp, tr};
use crate::render3d::{
    BLACK, Camera, Lin, Material, Mesh3, Rig, Scene, SpriteKind, Stats, V3, WHITE, Xf, lin, mix,
    scale, v3,
};
use crate::theme::Section;
use cleancraft_core::FeatureId;
use egui::{Mesh, Rect, Response, Sense, Ui, Vec2};
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::sync::OnceLock;

/// Which of the three looks the hero shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HeroPhase {
    Idle,
    Scanning,
    Done,
}

/// What to draw this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeroState {
    pub phase: HeroPhase,
    /// Scan progress 0..1 (used while scanning; ignored otherwise).
    pub progress: f32,
    /// A running clock in seconds (drives the loops).
    pub t: f64,
    /// Seconds since `phase` started (drives the transitions and the done burst).
    pub since: f32,
}

impl HeroState {
    pub fn idle(t: f64) -> HeroState {
        HeroState {
            phase: HeroPhase::Idle,
            progress: 0.0,
            t,
            since: t as f32,
        }
    }
}

/// Allocate `size`, track how long `phase` has been showing and draw the hero. Requests a
/// repaint while anything moves.
pub fn show(
    ui: &mut Ui,
    feature: FeatureId,
    phase: HeroPhase,
    progress: f32,
    size: Vec2,
) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let id = ui.id().with(("hero", feature.as_str()));
    let since = motion::since(ui.ctx(), id, phase as u64) as f32;
    let t = ui.input(|i| i.time);
    draw(
        ui,
        feature,
        HeroState {
            phase,
            progress,
            t,
            since,
        },
        rect,
    );
    resp
}

/// Paint the hero of `feature` into `rect`.
pub fn draw(ui: &mut Ui, feature: FeatureId, state: HeroState, rect: Rect) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let ppp = ui.ctx().pixels_per_point();
    let (mesh, _) = mesh(feature, state, rect, ppp);
    ui.painter().add(egui::Shape::mesh(mesh));
    // Full frame rate only while something visibly travels (a scan, the done burst);
    // the idle float and turn run at about 30 fps and stop in the background.
    let busy = state.phase == HeroPhase::Scanning
        || (state.phase == HeroPhase::Done && state.since < tr::HERO_BURST.duration + 1.0);
    if busy && !motion::reduced() {
        ui.ctx().request_repaint();
    } else {
        motion::ambient_repaint(ui.ctx());
    }
}

/// The hero as an egui mesh (no `Ui` needed: tests, icons, offscreen rendering).
pub fn mesh(feature: FeatureId, state: HeroState, rect: Rect, ppp: f32) -> (Mesh, Stats) {
    let a = assets();
    let an = Ani::new(feature, state);
    let mut sc = Scene::new(Camera::front(6.6, an.look.elevation), Rig::studio(an.a));
    backdrop(&mut sc, &an);
    let root = an.root();
    match feature {
        FeatureId::SmartCare => quick_sweep(&mut sc, a, &an, root),
        // Developer Clutter shares System Junk's object until it has its own.
        FeatureId::SystemJunk | FeatureId::DevClutter => system_junk(&mut sc, a, &an, root),
        FeatureId::MailAttachments => mail(&mut sc, a, &an, root),
        FeatureId::TrashBins => trash(&mut sc, a, &an, root),
        FeatureId::Malware => threat(&mut sc, a, &an, root),
        FeatureId::Privacy => privacy(&mut sc, a, &an, root),
        FeatureId::Maintenance => tune_up(&mut sc, a, &an, root),
        FeatureId::LoginItems => startup(&mut sc, a, &an, root),
        FeatureId::Uninstaller => app_remover(&mut sc, a, &an, root),
        FeatureId::Updater => app_updates(&mut sc, a, &an, root),
        FeatureId::LargeOldFiles => large_old(&mut sc, a, &an, root),
        FeatureId::Duplicates => duplicates(&mut sc, a, &an, root),
        FeatureId::SimilarPhotos => similar(&mut sc, a, &an, root),
        FeatureId::SpaceLens => disk_map(&mut sc, a, &an, root),
        FeatureId::Monitor => monitor(&mut sc, a, &an, root),
    }
    effects(&mut sc, a, &an);
    sc.render(rect, ppp)
}

// ---------------------------------------------------------------- animation state

/// How a hero moves around its vertical axis.
#[derive(Clone, Copy)]
enum Turn {
    /// Full turns; `sym` = how many identical views per turn (for settling to the nearest).
    Spin { sym: f32 },
    /// Back and forth by `amp` radians.
    Sway { amp: f32 },
}

#[derive(Clone, Copy)]
struct Look {
    turn: Turn,
    /// Rest yaw (radians).
    yaw: f32,
    /// Forward tilt of the object (radians, positive shows the top).
    pitch: f32,
    /// Camera elevation (radians).
    elevation: f32,
    /// Where the glint flashes (world units).
    glint: V3,
}

fn look(f: FeatureId) -> Look {
    let spin = |sym| Turn::Spin { sym };
    let sway = |amp| Turn::Sway { amp };
    let (turn, yaw, pitch, elevation, glint) = match f {
        FeatureId::SmartCare => (spin(8.0), 0.2, 0.0, 0.22, v3(-0.32, 0.62, 0.8)),
        FeatureId::SystemJunk | FeatureId::DevClutter => {
            (spin(4.0), -0.62, 0.0, 0.42, v3(-0.55, 0.85, 0.7))
        }
        FeatureId::MailAttachments => (sway(0.42), -0.28, 0.12, 0.2, v3(-0.7, 0.5, 0.5)),
        FeatureId::TrashBins => (spin(10.0), 0.0, 0.0, 0.3, v3(-0.45, 0.55, 0.8)),
        FeatureId::Malware => (sway(0.5), -0.25, 0.05, 0.16, v3(-0.4, 0.62, 0.6)),
        FeatureId::Privacy => (sway(0.4), -0.32, 0.08, 0.16, v3(0.55, -0.1, 0.8)),
        FeatureId::Maintenance => (sway(0.35), -0.3, 0.18, 0.2, v3(-0.75, 0.75, 0.4)),
        FeatureId::LoginItems => (sway(0.45), -0.3, 0.22, 0.2, v3(-0.45, 0.55, 0.7)),
        FeatureId::Uninstaller => (sway(0.35), -0.3, 0.06, 0.18, v3(0.5, 0.52, 0.3)),
        FeatureId::Updater => (sway(0.45), -0.3, 0.1, 0.18, v3(-0.6, 0.55, 0.5)),
        FeatureId::LargeOldFiles => (spin(2.0), -0.45, 0.0, 0.42, v3(-0.5, 0.62, 0.6)),
        FeatureId::Duplicates => (sway(0.3), 0.0, 0.08, 0.18, v3(-0.75, 0.62, 0.5)),
        FeatureId::SimilarPhotos => (sway(0.35), -0.2, 0.08, 0.18, v3(-0.7, 0.62, 0.5)),
        FeatureId::SpaceLens => (spin(1.0), 0.4, 0.0, 0.55, v3(-0.55, 0.45, 0.6)),
        FeatureId::Monitor => (sway(0.4), -0.25, 0.08, 0.16, v3(-0.55, 0.6, 0.5)),
    };
    Look {
        turn,
        yaw,
        pitch,
        elevation,
        glint,
    }
}

struct Ani {
    look: Look,
    phase: HeroPhase,
    t: f64,
    since: f32,
    progress: f32,
    /// Scanning energy 0..1 (eases in when a scan starts).
    scan: f32,
    /// Done spring 0..1 (overshoots); 1 outside Done.
    settle: f32,
    /// Done burst progress 0..1 (linear); 1 outside Done.
    burst: f32,
    /// Breathing 0..1 while scanning.
    breath: f32,
    a: Lin,
    b: Lin,
}

fn reduce() -> bool {
    motion::reduced()
}

impl Ani {
    fn new(f: FeatureId, st: HeroState) -> Ani {
        let (ca, cb) = Section::of(f).colors();
        let since = if reduce() { 1e3 } else { st.since.max(0.0) };
        let t = if reduce() { 0.0 } else { st.t };
        let scanning = st.phase == HeroPhase::Scanning;
        let done = st.phase == HeroPhase::Done;
        Ani {
            look: look(f),
            phase: st.phase,
            t,
            since,
            progress: st.progress.clamp(0.0, 1.0),
            scan: if scanning {
                tr::HERO_PHASE.progress(since)
            } else {
                0.0
            },
            settle: if done {
                Motion::spring(tr::HERO_SETTLE).progress(since)
            } else {
                1.0
            },
            burst: if done {
                Motion::new(tr::HERO_BURST.duration, Easing::Linear).progress(since)
            } else {
                1.0
            },
            breath: if scanning {
                lp::HERO_BREATH.wave(t)
            } else {
                0.0
            },
            a: lin(ca),
            b: lin(cb),
        }
    }

    fn scanning(&self) -> bool {
        self.phase == HeroPhase::Scanning
    }
    fn done(&self) -> bool {
        self.phase == HeroPhase::Done
    }

    /// Sine of a loop (-1..1) with `period` seconds and phase offset.
    fn sin(&self, period: f32, offset: f32) -> f32 {
        if reduce() {
            return 0.0;
        }
        ((self.t / period as f64 * std::f64::consts::TAU) as f32 + offset).sin()
    }

    /// Seconds since the phase started, as f32 time for scan-only loops.
    fn scan_angle(&self, l: Loop) -> f32 {
        self.scan * l.angle(self.since as f64)
    }

    fn yaw(&self) -> f32 {
        let lk = &self.look;
        let live = match lk.turn {
            Turn::Spin { .. } => {
                lp::HERO_IDLE_TURN.angle(self.t) + self.scan_angle(lp::HERO_SCAN_TURN)
            }
            Turn::Sway { amp } => {
                amp * self.sin(7.5, 0.0) + amp * 0.7 * self.scan * self.sin(2.2, 1.0)
            }
        };
        if !self.done() {
            return lk.yaw + live;
        }
        // Spring from the live pose (plus one quick extra step) into the rest pose.
        let off = match lk.turn {
            Turn::Spin { sym } => {
                let step = TAU / sym;
                let d = (live + step * 0.5).rem_euclid(step) - step * 0.5;
                d + step
            }
            Turn::Sway { amp } => live + amp,
        };
        let rest_sway = 0.12 * self.sin(9.0, 0.0) * self.settle.min(1.0);
        lk.yaw + off * (1.0 - self.settle) + rest_sway
    }

    fn root(&self) -> Xf {
        let float = 0.06 * self.sin(lp::HERO_FLOAT.period, 0.0) * (1.0 - 0.4 * self.scan);
        let wobble = 0.035 * self.sin(5.3, 0.7);
        let pop = if self.done() {
            motion::lerp(0.86, 1.0, self.settle)
        } else {
            1.0 + 0.03 * self.scan * self.breath
        };
        Xf::scale(pop)
            .rot_y(self.yaw())
            .rot_x(self.look.pitch + wobble)
            .at(v3(0.0, float, 0.0))
    }

    /// Emissive glow for the parts that light up with the scan.
    fn glow(&self, k: f32) -> Lin {
        let s = self.scan * (0.55 + 0.45 * self.breath) * (0.35 + 0.65 * self.progress);
        let d = if self.done() {
            (1.0 - self.burst).powi(2) * 0.8
        } else {
            0.0
        };
        scale(mix(self.a, WHITE, 0.2), k * (s * 0.55 + d))
    }

    fn mat_a(&self) -> Material {
        Material::gloss(mix(self.a, WHITE, 0.08), self.b)
    }
    fn mat_b(&self) -> Material {
        Material::gloss(self.b, scale(mix(self.b, self.a, 0.3), 0.7))
    }
}

/// Ease 0..1 over [from, from + len] of `x`.
fn window(x: f32, from: f32, len: f32) -> f32 {
    ((x - from) / len).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------- shared effects

fn backdrop(sc: &mut Scene<'static>, an: &Ani) {
    let glow = 0.30 + 0.25 * an.scan * (0.6 + 0.4 * an.breath) + 0.3 * (1.0 - an.burst);
    // Straight behind the object as the camera sees it, so a camera looking down (Disk Map,
    // System Junk) does not lift the halo above the object.
    let (se, ce) = an.look.elevation.sin_cos();
    sc.sprite(
        v3(0.0, 0.05 - 2.2 * se, -2.2 * ce),
        3.2,
        mix(an.a, an.b, 0.35),
        glow * 0.55,
        SpriteKind::Glow,
    );
    let lift = 0.06 * an.sin(lp::HERO_FLOAT.period, 0.0);
    sc.sprite(
        v3(0.0, -1.32, 0.0),
        1.15 - lift,
        BLACK,
        0.5 - lift * 1.5,
        SpriteKind::Shadow(0.22),
    );
}

fn effects(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani) {
    let spark = mix(an.a, WHITE, 0.45);
    // Twinkles around the hero.
    for (i, p) in [
        v3(-1.35, 0.95, 0.1),
        v3(1.3, 0.55, -0.3),
        v3(1.05, -0.85, 0.5),
    ]
    .into_iter()
    .enumerate()
    {
        let w = 0.5 + 0.5 * an.sin(3.1 + i as f32 * 0.9, i as f32 * 2.1);
        let k = if reduce() { 0.45 } else { w * w * w };
        sc.sprite(p, 0.2, spark, 0.75 * k, SpriteKind::Star(0.0));
    }
    // A glint across the object now and then.
    let g = if reduce() {
        0.0
    } else {
        lp::HERO_GLINT.wave(an.t).powi(12)
    };
    let flash = if an.done() {
        (1.0 - window(an.since, 0.05, 0.6)).powi(2)
    } else {
        0.0
    };
    sc.sprite(
        an.look.glint,
        0.3 + 0.25 * flash,
        WHITE,
        (g * 0.9 + flash).min(1.0),
        SpriteKind::Star(0.3),
    );
    // Comets orbiting on two tilted rings while scanning, passing in front and behind.
    if an.scan > 0.0 {
        for (ring, count, r, tilt, roll, speed) in [
            (0, 7, 1.42f32, 0.52f32, 0.2f32, 1.0f32),
            (1, 5, 1.68, -0.38, -0.25, -0.75),
        ] {
            let orbit = Xf::ID.rot_x(tilt).rot_z(roll);
            for i in 0..count {
                let head = speed * lp::HERO_ORBIT.angle(an.since as f64)
                    + TAU * i as f32 / count as f32
                    + ring as f32 * 0.4;
                for k in 0..6 {
                    let ang = head - speed.signum() * k as f32 * 0.022;
                    let p = orbit.apply(v3(r * ang.cos(), 0.0, r * ang.sin()));
                    let fall = 1.0 - k as f32 / 6.0;
                    let size = (0.05 + 0.02 * (i % 2) as f32) * (0.4 + 0.6 * fall);
                    let col = mix(an.a, WHITE, 0.35 + 0.4 * fall);
                    sc.sprite(p, size, col, an.scan * fall, SpriteKind::Dot);
                    if k == 0 {
                        sc.sprite(p, 0.28, an.a, an.scan * 0.22, SpriteKind::Glow);
                    }
                }
            }
        }
    }
    // The done burst: particles flying out, a flash and a shock ring.
    if an.done() && an.burst < 1.0 {
        let k = an.burst;
        let e = ENTER.apply(k);
        let fade = (1.0 - k).powf(1.4);
        sc.sprite(
            V3::ZERO,
            0.8 + 2.2 * e,
            mix(an.a, WHITE, 0.3),
            fade * 0.45,
            SpriteKind::Glow,
        );
        let n = 22;
        for i in 0..n {
            // Golden-angle directions over a sphere, flattened a little.
            let y = 1.0 - 2.0 * (i as f32 + 0.5) / n as f32;
            let rr = (1.0 - y * y).sqrt();
            let th = i as f32 * 2.399_963;
            let dir = v3(rr * th.cos(), y * 0.75, rr * th.sin());
            let speed = 1.1 + 0.6 * ((i * 7 % 5) as f32 / 4.0);
            let p = dir * (0.5 + speed * e);
            let col = if i % 3 == 0 {
                WHITE
            } else {
                mix(an.a, an.b, (i % 4) as f32 / 3.0)
            };
            if i % 4 == 0 {
                sc.sprite(p, 0.22 * (1.0 - 0.5 * k), col, fade, SpriteKind::Star(th));
            } else {
                sc.sprite(p, 0.05 * (1.0 - 0.6 * k), col, fade, SpriteKind::Dot);
            }
        }
        // The ring stops inside the slot margin and is gone by the time it gets there.
        let ring = Xf::scale(0.8 + 0.55 * e).rot_x(FRAC_PI_2 - 0.35);
        sc.add_faded(
            &a.shock,
            ring,
            Material::glass(mix(an.a, WHITE, 0.5), 0.5).with_emissive(scale(an.a, 0.9)),
            fade * fade * 0.6 * (1.0 - e).sqrt(),
        );
    }
}

// ---------------------------------------------------------------- the heroes

fn quick_sweep(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    let m = Material {
        spec: 1.0,
        shine: 60.0,
        rim: 0.8,
        ..Material::gloss(mix(an.a, WHITE, 0.25), an.b)
    }
    .with_emissive(an.glow(0.6));
    sc.add(&a.gem, Xf::ID.at(v3(0.0, 0.05, 0.0)).then(root), m);
    // Three small sparks circling the crystal.
    for i in 0..3 {
        let ang = lp::HERO_IDLE_TURN.angle(an.t) * -3.0
            + an.scan_angle(lp::HERO_ORBIT) * 1.5
            + TAU * i as f32 / 3.0;
        let r = 1.12;
        let y = 0.35 * (ang * 2.0).sin() * 0.4 + [0.4, -0.25, 0.05][i];
        let p = v3(r * ang.cos(), y, r * ang.sin());
        let spin = an.t as f32 * 1.7 + i as f32;
        sc.add(
            &a.spark,
            Xf::scale(0.16).rot_y(spin).rot_x(0.4).at(p),
            Material::gloss(mix(an.a, WHITE, 0.6), an.a).with_emissive(scale(an.a, 0.2)),
        );
    }
}

fn system_junk(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    let cube = Xf::ID.rot_x(0.0).then(root);
    sc.add(
        &a.glass_cube,
        cube,
        Material::glass(mix(an.a, WHITE, 0.3), 0.14).with_emissive(an.glow(0.2)),
    );
    let colors = [mix(an.a, WHITE, 0.15), an.b, mix(an.a, an.b, 0.5)];
    let mut i = 0;
    for x in -1..=1 {
        for y in -1..=1 {
            for z in -1..=1 {
                let fi = i as f32;
                let mut p = v3(x as f32, y as f32, z as f32) * 0.47;
                let mut s = 1.0;
                if an.scanning() {
                    let w = an.sin(1.2, fi * 0.7 + x as f32 + y as f32 * 0.5);
                    p.y += 0.05 * w * an.scan;
                    s = 1.0 - 0.12 * an.scan * (0.5 + 0.5 * w) * an.progress;
                }
                if an.done() {
                    // Tiles collapse to the centre one after another and vanish.
                    let k = Easing::CubicBezier(0.5, 0.0, 0.75, 0.0).apply(window(
                        an.since,
                        0.03 * fi,
                        0.35,
                    ));
                    p = p * (1.0 - k);
                    s *= 1.0 - k;
                }
                if s > 0.01 {
                    let col = colors[i % 3];
                    let m = Material::gloss(mix(col, WHITE, 0.15), col).with_emissive(an.glow(0.5));
                    let spin = 0.2 * ((i * 5 % 7) as f32 - 3.0) / 3.0;
                    sc.add(
                        &a.tile,
                        Xf::scale(s).rot_y(spin).rot_x(spin * 0.7).at(p).then(cube),
                        m,
                    );
                }
                i += 1;
            }
        }
    }
    // Once the tiles are gone a clean core shines in the middle.
    if an.done() {
        let k = window(an.since, 0.8, 0.5);
        sc.sprite(
            root.apply(V3::ZERO),
            1.4 * k,
            mix(an.a, WHITE, 0.4),
            0.6 * k,
            SpriteKind::Glow,
        );
    }
}

fn mail(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    let body = Material::satin(mix(an.a, WHITE, 0.62), mix(an.b, WHITE, 0.25));
    sc.add(&a.env_body, root, body);
    // A letter rises out while scanning.
    let rise = if an.scanning() {
        an.scan * (0.25 + 0.45 * an.progress)
    } else if an.done() {
        0.7 * (1.0 - an.settle).max(0.0)
    } else {
        0.0
    };
    if rise > 0.01 {
        sc.add(
            &a.letter,
            Xf::ID.at(v3(0.0, -0.12 + rise, 0.02)).then(root),
            Material::satin(WHITE, mix(WHITE, an.a, 0.2)).with_emissive(an.glow(0.15)),
        );
        // Inside the envelope: drawn before the body so only the part above it shows.
        sc.bias_last(-0.4);
    }
    // The flap opens around its top hinge while scanning, closes with a bounce when done.
    let open = if an.scanning() {
        an.scan * 2.3
    } else if an.done() {
        2.3 * (1.0 - an.settle)
    } else {
        0.0
    };
    let hinge = v3(0.0, 0.6, 0.18);
    let flap = Xf::ID.at(-hinge).rot_x(-open).at(hinge).then(root);
    sc.add(
        &a.env_flap,
        Xf::ID.at(v3(0.0, 0.0, 0.18)).then(flap),
        Material::gloss(mix(an.a, WHITE, 0.2), an.b).with_emissive(an.glow(0.3)),
    );
    let seal_pop = if an.done() {
        motion::lerp(0.6, 1.0, an.settle)
    } else {
        1.0
    };
    sc.add(
        &a.seal,
        Xf::scale(seal_pop).at(v3(0.0, 0.0, 0.23)).then(flap),
        Material::gloss(mix(an.b, WHITE, 0.1), scale(an.b, 0.7)),
    );
}

fn trash(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    sc.add(
        &a.bin,
        root,
        Material::satin(mix(an.a, WHITE, 0.1), scale(an.b, 0.75)).with_emissive(an.glow(0.15)),
    );
    for i in 0..10 {
        let ang = TAU * i as f32 / 10.0;
        // Ribs follow the tapered wall.
        let rib = Xf::ID.rot_z(-0.115).at(v3(0.66, -0.15, 0.0)).rot_y(ang);
        sc.add(&a.rib, rib.then(root), an.mat_a());
        // Ribs sit on the wall: let the ones at the edge draw over it.
        sc.bias_last(0.12);
    }
    let (lift, tilt) = if an.scanning() {
        let w = 0.5 + 0.5 * an.sin(1.1, 0.0);
        (an.scan * (0.02 + 0.02 * w), an.scan * (0.1 + 0.1 * w))
    } else if an.done() {
        let k = 1.0 - an.settle;
        (0.3 * k, 0.6 * k)
    } else {
        (0.0, 0.0)
    };
    let hinge = v3(0.0, 0.66, -0.8);
    let lid = Xf::ID
        .at(-hinge)
        .rot_x(-tilt)
        .at(hinge + v3(0.0, lift, 0.0))
        .then(root);
    sc.add(
        &a.lid,
        Xf::ID.at(v3(0.0, 0.74, 0.0)).then(lid),
        Material::gloss(mix(an.a, WHITE, 0.25), an.a),
    );
    sc.add(
        &a.handle,
        Xf::ID.at(v3(0.0, 0.81, 0.0)).then(lid),
        an.mat_b(),
    );
    // Scraps hop out of the open bin.
    if an.scanning() {
        for i in 0..5 {
            let ph = ((an.since * 0.9 + i as f32 * 0.2) % 1.0).max(0.0);
            let ang = i as f32 * 1.3;
            let p = v3(
                0.35 * ang.cos() * ph,
                0.7 + 1.0 * ph - 1.2 * ph * ph,
                0.35 * ang.sin() * ph,
            );
            sc.sprite(
                root.apply(p),
                0.06,
                mix(an.a, WHITE, 0.4),
                an.scan * (1.0 - ph),
                SpriteKind::Dot,
            );
        }
    }
}

fn threat(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    sc.add(
        &a.shield,
        root,
        Material::gloss(mix(an.a, WHITE, 0.12), an.b).with_emissive(an.glow(0.3)),
    );
    sc.add(
        &a.shield_inner,
        Xf::ID.at(v3(0.0, 0.03, 0.2)).then(root),
        Material::satin(mix(WHITE, an.a, 0.15), mix(WHITE, an.b, 0.35)),
    );
    let check = if an.done() {
        motion::POP.apply(window(an.since, 0.15, 0.45))
    } else if an.scanning() {
        0.0
    } else {
        1.0
    };
    if check > 0.01 {
        sc.add(
            &a.check,
            Xf::scale(check).at(v3(0.0, 0.0, 0.3)).then(root),
            Material::gloss(mix(an.a, WHITE, 0.1), an.b).with_emissive(an.glow(0.4)),
        );
    }
    // A scanning beam sweeping down the shield.
    if an.scanning() {
        let y = 0.9 - 1.9 * motion::Loop::new(1.6, Easing::Linear).phase(an.since as f64);
        let w = 1.0 - (y.abs() / 1.0).powi(4);
        sc.add_faded(
            &a.beam,
            Xf::scale3(v3(0.85 * w.max(0.2), 1.0, 1.0))
                .at(v3(0.0, y, 0.36))
                .then(root),
            Material::glass(mix(an.a, WHITE, 0.6), 0.5).with_emissive(scale(an.a, 1.2)),
            an.scan,
        );
        sc.sprite(
            root.apply(v3(0.0, y, 0.4)),
            0.9,
            an.a,
            0.4 * an.scan,
            SpriteKind::Glow,
        );
    }
}

/// Privacy Traces: a padlock in front of a browser window whose history lines are the
/// traces. Scanning unlocks the shackle and the lines flicker; done clears the lines and
/// the shackle snaps shut.
fn privacy(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    // The browser window behind, up and to the right.
    let win = Xf::scale(0.82)
        .rot_z(0.08)
        .at(v3(0.42, 0.42, -0.6))
        .then(root);
    sc.add(
        &a.browser,
        win,
        Material::satin(mix(an.b, WHITE, 0.35), scale(an.b, 0.7)),
    );
    sc.add(
        &a.browser_bar,
        Xf::ID.at(v3(0.0, 0.47, 0.06)).then(win),
        Material::satin(scale(an.b, 0.55), scale(an.b, 0.4)),
    );
    for i in 0..3 {
        sc.add(
            &a.bead,
            Xf::scale3(v3(0.045, 0.045, 0.03))
                .at(v3(-0.66 + 0.13 * i as f32, 0.47, 0.1))
                .then(win),
            Material::gloss(WHITE, mix(WHITE, an.a, 0.3)),
        );
    }
    // History lines: the traces. They flicker while scanning and clear when done.
    let cleared = if an.done() {
        ENTER.apply(window(an.since, 0.0, 0.6))
    } else {
        0.0
    };
    for i in 0..3 {
        let fi = i as f32;
        let flick = if an.scanning() {
            1.0 - 0.6 * an.scan * (0.5 + 0.5 * an.sin(0.9, fi * 1.7))
        } else {
            1.0
        };
        let fade = flick * (1.0 - cleared);
        sc.add_faded(
            &a.card_line,
            Xf::scale3(v3(2.0 - 0.45 * fi, 1.5, 1.0))
                .at(v3(-0.08 - 0.13 * fi, 0.2 - 0.2 * fi, 0.08))
                .then(win),
            Material::satin(WHITE, mix(WHITE, an.b, 0.4)),
            fade,
        );
    }
    // The padlock in front, lower left.
    let lock = Xf::scale(1.12).at(v3(-0.16, -0.36, 0.3)).then(root);
    let open = if an.scanning() {
        an.scan * (0.75 + 0.25 * an.sin(1.6, 0.0))
    } else if an.done() {
        // From open to shut with the done spring (overshoot reads as a snap).
        (1.0 - an.settle).max(-0.15)
    } else {
        0.0
    };
    let body =
        Material::gloss(mix(an.a, WHITE, 0.12), scale(an.a, 0.62)).with_emissive(an.glow(0.35));
    sc.add(&a.lock_body, lock, body);
    // The shackle lifts straight up when open, and its right leg swings out.
    let shackle = Xf::ID
        .at(v3(0.0, 0.2 * open, 0.0))
        .rot_y(-0.5 * open)
        .at(v3(0.0, 0.0, 0.0))
        .then(lock);
    let steel = Material::metal(mix(WHITE, an.a, 0.12), scale(mix(WHITE, an.b, 0.2), 0.45));
    sc.add(&a.lock_shackle, shackle, steel);
    sc.add(
        &a.lock_keyhole,
        Xf::ID.at(v3(0.0, -0.06, 0.18)).then(lock),
        Material::gloss([0.03, 0.02, 0.03], [0.01, 0.01, 0.02]),
    );
}

fn tune_up(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    let base = if reduce() { 0.0 } else { an.t as f32 * 0.25 }
        + an.scan * an.since * 2.0
        + if an.done() {
            1.2 * (1.0 - an.settle)
        } else {
            0.0
        };
    let (n1, n2) = (GEAR_BIG.0 as f32, GEAR_SMALL.0 as f32);
    let c1 = v3(-0.36, 0.2, 0.0);
    let c2 = c1
        + v3(
            GEAR_CENTRES * (-0.6f32).cos(),
            GEAR_CENTRES * (-0.6f32).sin(),
            0.0,
        );
    let line = -0.6f32;
    let a1 = line + base;
    let a2 = line + PI - PI / n2 - base * n1 / n2;
    sc.add(
        &a.gear_big,
        Xf::ID.rot_z(a1).at(c1).then(root),
        Material::metal(mix(an.a, WHITE, 0.2), an.b).with_emissive(an.glow(0.3)),
    );
    sc.add(
        &a.gear_small,
        Xf::ID.rot_z(a2).at(c2 + v3(0.0, 0.0, 0.0)).then(root),
        Material::gloss(mix(an.b, WHITE, 0.15), scale(an.b, 0.7)).with_emissive(an.glow(0.3)),
    );
    for (c, r) in [(c1, 1.0), (c2, 0.75)] {
        sc.add(
            &a.hub,
            Xf::scale(r).at(c + v3(0.0, 0.0, 0.13)).then(root),
            Material::metal(WHITE, mix(WHITE, an.a, 0.4)),
        );
    }
}

fn startup(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    let press = if an.scanning() {
        1.0 - 0.12 * an.scan * (0.5 + 0.5 * an.sin(1.3, 0.0))
    } else if an.done() {
        motion::lerp(0.8, 1.0, an.settle)
    } else {
        1.0
    };
    let body = Xf::scale3(v3(1.0, 1.0, press)).then(root);
    sc.add(&a.puck, body, Material::gloss(mix(an.a, WHITE, 0.1), an.b));
    sc.add(
        &a.groove,
        Xf::ID.at(v3(0.0, 0.0, 0.215)).then(body),
        Material::metal(mix(an.a, WHITE, 0.5), an.a),
    );
    let lit = 0.25
        + an.scan * (0.5 + 0.5 * an.breath)
        + if an.done() {
            1.2 * (1.0 - an.burst) + 0.4
        } else {
            0.0
        };
    let sym = Material::gloss(WHITE, mix(WHITE, an.a, 0.25))
        .with_emissive(scale(mix(an.a, WHITE, 0.5), 0.35 * lit));
    sc.add(
        &a.power_arc,
        Xf::ID.at(v3(0.0, -0.04, 0.27)).then(body),
        sym,
    );
    sc.add(&a.power_bar, Xf::ID.at(v3(0.0, 0.2, 0.27)).then(body), sym);
    sc.sprite(
        body.apply(v3(0.0, 0.0, 0.4)),
        1.1,
        an.a,
        0.25 * lit,
        SpriteKind::Glow,
    );
}

/// App Remover: an app tile in "remove mode" with a minus badge. Scanning wiggles it and
/// its leftover files orbit it; done shrinks the app away and leaves an empty glass slot.
fn app_remover(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    let gone = if an.done() {
        ENTER.apply(window(an.since, 0.05, 0.7))
    } else {
        0.0
    };
    let wiggle = if an.scanning() {
        0.07 * an.scan * an.sin(0.32, 0.0)
    } else {
        0.0
    };
    let size = 1.0 - 0.9 * gone;
    let tile = Xf::scale(size).rot_z(wiggle + 0.5 * gone).then(root);
    let face =
        Material::gloss(mix(an.a, WHITE, 0.18), scale(an.b, 0.75)).with_emissive(an.glow(0.3));
    sc.add_faded(&a.remover_tile, tile, face, 1.0 - gone);
    // The app's own glyph: a 2 by 2 grid of rounded squares.
    let ink = Material::gloss(WHITE, mix(WHITE, an.a, 0.25));
    for (x, y) in [(-1.0, 1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
        sc.add_faded(
            &a.remover_cell,
            Xf::ID.at(v3(0.19 * x, 0.19 * y, 0.12)).then(tile),
            ink,
            1.0 - gone,
        );
    }
    // The minus badge on the top left corner: pops in when scanning, out when done.
    let pop = if an.scanning() {
        1.0 + 0.12 * an.scan * (0.5 + 0.5 * an.sin(0.8, 0.0))
    } else {
        1.0
    } * (1.0 - ENTER.apply(window(gone, 0.0, 0.5)));
    if pop > 0.01 {
        let badge = Xf::scale(pop)
            .at(v3(-0.5, 0.5, 0.2))
            .then(Xf::scale(size).then(root));
        sc.add(
            &a.badge,
            badge,
            Material::gloss(
                lin(egui::Color32::from_rgb(0xFF, 0x6B, 0x6B)),
                lin(egui::Color32::from_rgb(0xC8, 0x2A, 0x3A)),
            ),
        );
        sc.add(
            &a.badge_minus,
            Xf::ID.at(v3(0.0, 0.0, 0.09)).then(badge),
            Material::gloss(WHITE, mix(WHITE, [0.9, 0.5, 0.5], 0.3)),
        );
    }
    // Leftover files: small chips that orbit while scanning and fly into the app when done.
    if an.scanning() || (an.done() && gone < 1.0) {
        let k = if an.scanning() { an.scan } else { 1.0 - gone };
        let spin = an.scan_angle(Loop::new(2.4, Easing::Linear));
        for i in 0..4 {
            let ang = spin + TAU * i as f32 / 4.0 + 0.4;
            let r = 0.95 * k + 0.2;
            sc.add_faded(
                &a.tile,
                Xf::ID
                    .rot_z(ang)
                    .at(v3(
                        r * ang.cos(),
                        r * ang.sin() * 0.8,
                        0.25 * (ang * 1.0).sin(),
                    ))
                    .then(root),
                Material::satin(mix(an.b, WHITE, 0.5), an.b),
                k,
            );
        }
    }
    // The empty place the app leaves behind: a dashed outline of its tile.
    if gone > 0.0 {
        let m = Material::satin(mix(an.a, WHITE, 0.45), mix(an.a, an.b, 0.5))
            .with_emissive(scale(an.a, 0.15));
        for &(p, ang) in &a.slot_path {
            sc.add_faded(
                &a.slot_dash,
                Xf::ID.rot_z(ang).at(v3(p[0], p[1], 0.0)).then(root),
                m,
                gone,
            );
        }
    }
}

fn app_updates(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    let spin = -(lp::HERO_IDLE_TURN.angle(an.t) * 1.5
        + an.scan_angle(Loop::new(1.6, Easing::Linear)))
        - if an.done() {
            PI * (1.0 - an.settle)
        } else {
            0.0
        };
    let ring = Xf::ID.rot_z(spin).then(root);
    let m = Material::gloss(mix(an.a, WHITE, 0.15), an.b).with_emissive(an.glow(0.4));
    for half in 0..2 {
        let x = Xf::ID.rot_z(PI * half as f32).then(ring);
        sc.add(&a.refresh_arc, x, m);
        sc.add(&a.arrow, x, m);
    }
    sc.add(
        &a.ball,
        Xf::scale(0.3).then(root),
        Material::gloss(mix(an.b, WHITE, 0.3), an.b).with_emissive(an.glow(0.6)),
    );
}

fn large_old(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    for (i, slab) in a.slabs.iter().enumerate() {
        let fi = i as f32;
        let mut y = -0.6 + fi * 0.33;
        if an.scanning() {
            let w = 0.5 + 0.5 * an.sin(1.6, -fi * 0.8);
            y += an.scan * (fi * 0.09 + 0.07 * w);
        }
        if an.done() {
            let k = 1.0
                - Motion::spring(tr::HERO_SETTLE)
                    .delayed(fi * 0.06)
                    .progress(an.since);
            y += k * fi * 0.2;
        }
        let turn = [0.0, 0.16, -0.1, 0.22][i];
        let t = fi / 3.0;
        let top = mix(mix(an.a, WHITE, 0.15), an.b, t * 0.6);
        let m = Material::gloss(top, scale(mix(an.b, an.a, t), 0.72))
            .with_emissive(an.glow(0.25 + 0.2 * fi));
        sc.add(slab, Xf::ID.rot_y(turn).at(v3(0.0, y, 0.0)).then(root), m);
    }
}

fn duplicates(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    let swing = if an.scanning() {
        an.scan * 0.2 * an.sin(1.5, 0.0)
    } else {
        0.0
    };
    let merge = if an.done() {
        ENTER.apply(window(an.since, 0.0, 0.7))
    } else {
        0.0
    };
    for side in [-1.0f32, 1.0] {
        let open = 0.42 + swing * side;
        let mut x = side * 0.5;
        let mut turn = -side * open;
        let mut fade = 1.0;
        if side > 0.0 && merge > 0.0 {
            x = motion::lerp(x, -0.5, merge);
            turn = motion::lerp(turn, 0.42, merge);
            fade = 1.0 - merge;
        }
        if side < 0.0 && merge > 0.0 {
            x = motion::lerp(x, 0.0, merge);
            turn = motion::lerp(turn, 0.0, merge);
        }
        let card = Xf::ID.rot_y(turn).at(v3(x, 0.0, 0.0)).then(root);
        let body = Material::satin(mix(WHITE, an.a, 0.14), mix(WHITE, an.b, 0.38));
        let ink = Material::gloss(mix(an.a, WHITE, 0.1), an.b).with_emissive(an.glow(0.4));
        let soft = Material::satin(mix(an.a, WHITE, 0.55), mix(an.b, WHITE, 0.45));
        let parts: [(&'static Mesh3, Xf, Material); 4] = [
            (&a.card, Xf::ID, body),
            (&a.card_pic, Xf::ID.at(v3(0.0, 0.25, 0.06)), ink),
            (&a.card_line, Xf::ID.at(v3(-0.05, -0.15, 0.055)), soft),
            (
                &a.card_line,
                Xf::scale3(v3(0.7, 1.0, 1.0)).at(v3(-0.13, -0.33, 0.055)),
                soft,
            ),
        ];
        for (mesh, xf, m) in parts {
            sc.add_faded(mesh, xf.then(card), m, fade);
        }
    }
    if an.scanning() {
        let k = an.scan * (0.5 + 0.5 * an.breath);
        sc.sprite(
            root.apply(v3(0.0, 0.0, 0.3)),
            0.8,
            an.a,
            0.5 * k,
            SpriteKind::Glow,
        );
    }
}

fn similar(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    let fan = if an.scanning() {
        1.0 + an.scan * (0.5 + 0.35 * an.sin(1.7, 0.0))
    } else if an.done() {
        motion::lerp(0.0, 1.0, an.settle)
    } else {
        1.0
    };
    let skies = [
        mix(an.a, WHITE, 0.25),
        mix(an.b, an.a, 0.4),
        mix(an.b, [0.6, 0.3, 0.9], 0.3),
    ];
    for (i, sky) in skies.into_iter().enumerate() {
        let fi = i as f32 - 1.0;
        let frame = Xf::ID
            .rot_z(-fi * 0.2 * fan)
            .at(v3(
                fi * 0.32 * fan,
                -fi.abs() * 0.05,
                -0.18 * (2 - i) as f32,
            ))
            .then(root);
        sc.add(
            &a.frame,
            frame,
            Material::satin(mix(WHITE, an.a, 0.08), mix(WHITE, an.b, 0.3)),
        );
        sc.add(
            &a.photo,
            Xf::ID.at(v3(0.0, 0.06, 0.05)).then(frame),
            Material::satin(sky, scale(sky, 0.55)).with_emissive(an.glow(0.3)),
        );
        sc.add(
            &a.hills,
            Xf::ID.at(v3(0.0, -0.08, 0.075)).then(frame),
            Material::satin(scale(an.b, 0.8), scale(an.b, 0.45)),
        );
        sc.add(
            &a.sun,
            Xf::ID.at(v3(0.24, 0.3, 0.075)).then(frame),
            Material::gloss(mix(WHITE, an.a, 0.25), WHITE).with_emissive(scale(an.a, 0.2)),
        );
    }
}

fn disk_map(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    let n = a.sectors.len();
    let tones = [
        mix(an.a, WHITE, 0.1),
        an.b,
        mix(an.a, an.b, 0.5),
        mix(an.b, [0.85, 0.3, 0.4], 0.35),
        mix(an.a, [1.0, 0.9, 0.5], 0.3),
        scale(an.b, 0.8),
    ];
    for (i, (mesh, mid)) in a.sectors.iter().enumerate() {
        let fi = i as f32;
        let mut lift = 0.0;
        let mut out = 0.04;
        if an.scanning() {
            let w = 0.5 + 0.5 * an.sin(1.4, -fi * TAU / n as f32);
            lift = an.scan * 0.22 * w * (0.4 + 0.6 * an.progress);
        }
        if an.done() {
            let k = 1.0
                - Motion::spring(tr::HERO_SETTLE)
                    .delayed(fi * 0.05)
                    .progress(an.since);
            lift = 0.35 * k;
            out += 0.15 * k;
        }
        let dir = v3(mid.cos(), 0.0, -mid.sin());
        let c = tones[i % tones.len()];
        let m = Material::gloss(mix(c, WHITE, 0.12), scale(c, 0.6)).with_emissive(an.glow(0.3));
        sc.add(
            mesh,
            Xf::ID.at(dir * out + v3(0.0, lift - 0.1, 0.0)).then(root),
            m,
        );
    }
    sc.add(
        &a.disk_hub,
        Xf::ID.at(v3(0.0, 0.05, 0.0)).then(root),
        Material::metal(WHITE, mix(WHITE, an.a, 0.4)),
    );
}

fn monitor(sc: &mut Scene<'static>, a: &'static Assets, an: &Ani, root: Xf) {
    // Heartbeat: two quick bumps every 1.2 s (faster while scanning).
    let period: f32 = if an.scanning() { 0.85 } else { 1.25 };
    let ph = if reduce() {
        0.5
    } else {
        ((an.t / period as f64).fract()) as f32
    };
    let beat = (-((ph - 0.1) / 0.05).powi(2)).exp() + 0.6 * (-((ph - 0.28) / 0.05).powi(2)).exp();
    let pop = 1.0 + 0.04 * beat;
    let body = Xf::scale(pop).then(root);
    sc.add(
        &a.gauge_track,
        body,
        Material::satin(scale(an.b, 0.35), scale(an.b, 0.2)),
    );
    let value = if an.scanning() {
        0.15 + 0.85 * an.progress
    } else if an.done() {
        an.settle.min(1.05)
    } else {
        0.62 + 0.08 * an.sin(3.0, 0.0)
    };
    let shown =
        ((value.clamp(0.0, 1.0) * a.gauge.len() as f32).round() as usize).min(a.gauge.len());
    for (i, seg) in a.gauge.iter().take(shown).enumerate() {
        let t = i as f32 / a.gauge.len() as f32;
        let c = mix(an.a, an.b, t);
        sc.add(
            seg,
            body,
            Material::gloss(mix(c, WHITE, 0.15), scale(c, 0.7)).with_emissive(an.glow(0.3)),
        );
        // The lit gauge wraps its track.
        sc.bias_last(0.3);
    }
    sc.add(
        &a.pulse_line,
        Xf::ID.at(v3(0.0, 0.0, 0.05)).then(body),
        Material::gloss(WHITE, mix(WHITE, an.a, 0.4)).with_emissive(scale(an.a, 0.25 + 0.6 * beat)),
    );
    sc.sprite(
        body.apply(V3::ZERO),
        1.5 + 0.4 * beat,
        an.a,
        0.25 * beat,
        SpriteKind::Glow,
    );
}

// ---------------------------------------------------------------- geometry, built once

const GEAR_BIG: (usize, f32) = (12, 0.6);
const GEAR_SMALL: (usize, f32) = (8, 0.4);
const GEAR_CENTRES: f32 = 1.0; // pitch radii 0.6 + 0.4

struct Assets {
    shock: Mesh3,
    gem: Mesh3,
    spark: Mesh3,
    glass_cube: Mesh3,
    tile: Mesh3,
    env_body: Mesh3,
    env_flap: Mesh3,
    seal: Mesh3,
    letter: Mesh3,
    bin: Mesh3,
    rib: Mesh3,
    lid: Mesh3,
    handle: Mesh3,
    shield: Mesh3,
    shield_inner: Mesh3,
    check: Mesh3,
    beam: Mesh3,
    bead: Mesh3,
    gear_big: Mesh3,
    gear_small: Mesh3,
    hub: Mesh3,
    puck: Mesh3,
    groove: Mesh3,
    power_arc: Mesh3,
    power_bar: Mesh3,
    browser: Mesh3,
    browser_bar: Mesh3,
    lock_body: Mesh3,
    lock_shackle: Mesh3,
    lock_keyhole: Mesh3,
    remover_tile: Mesh3,
    remover_cell: Mesh3,
    badge: Mesh3,
    badge_minus: Mesh3,
    slot_dash: Mesh3,
    /// Dash centres and directions, evenly spaced along the app tile's outline.
    slot_path: Vec<([f32; 2], f32)>,
    refresh_arc: Mesh3,
    arrow: Mesh3,
    ball: Mesh3,
    slabs: Vec<Mesh3>,
    card: Mesh3,
    card_pic: Mesh3,
    card_line: Mesh3,
    frame: Mesh3,
    photo: Mesh3,
    hills: Mesh3,
    sun: Mesh3,
    sectors: Vec<(Mesh3, f32)>,
    disk_hub: Mesh3,
    gauge_track: Mesh3,
    gauge: Vec<Mesh3>,
    pulse_line: Mesh3,
    icon_ring: Mesh3,
}

fn assets() -> &'static Assets {
    static A: OnceLock<Assets> = OnceLock::new();
    A.get_or_init(build_assets)
}

/// Lay a mesh built around Y so it faces the camera (+Z).
pub(crate) fn facing(m: Mesh3) -> Mesh3 {
    let mut m = m.transformed(Xf::ID.rot_x(FRAC_PI_2));
    m.tint.clear();
    m.finish()
}

/// Recompute the vertical gradient after a transform.
pub(crate) fn retint(mut m: Mesh3) -> Mesh3 {
    m.tint.clear();
    m.finish()
}

pub(crate) fn ccw(mut p: Vec<[f32; 2]>) -> Vec<[f32; 2]> {
    let n = p.len();
    let area: f32 = (0..n)
        .map(|i| p[i][0] * p[(i + 1) % n][1] - p[(i + 1) % n][0] * p[i][1])
        .sum();
    if area < 0.0 {
        p.reverse();
    }
    p
}

pub(crate) fn rounded_rect(w: f32, h: f32, r: f32, seg: usize) -> Vec<[f32; 2]> {
    let (hw, hh) = (w * 0.5 - r, h * 0.5 - r);
    let mut p = Vec::new();
    for (cx, cy, a0) in [
        (hw, -hh, -FRAC_PI_2),
        (hw, hh, 0.0),
        (-hw, hh, FRAC_PI_2),
        (-hw, -hh, PI),
    ] {
        for k in 0..=seg {
            let a = a0 + FRAC_PI_2 * k as f32 / seg as f32;
            p.push([cx + r * a.cos(), cy + r * a.sin()]);
        }
    }
    p
}

pub(crate) fn squircle(r: f32, n: f32, seg: usize) -> Vec<[f32; 2]> {
    (0..seg)
        .map(|i| {
            let a = TAU * i as f32 / seg as f32;
            let (s, c) = a.sin_cos();
            [
                r * c.signum() * c.abs().powf(2.0 / n),
                r * s.signum() * s.abs().powf(2.0 / n),
            ]
        })
        .collect()
}

fn quad_bezier(p0: [f32; 2], c: [f32; 2], p1: [f32; 2], steps: usize, out: &mut Vec<[f32; 2]>) {
    for k in 0..steps {
        let t = k as f32 / steps as f32;
        let u = 1.0 - t;
        out.push([
            u * u * p0[0] + 2.0 * u * t * c[0] + t * t * p1[0],
            u * u * p0[1] + 2.0 * u * t * c[1] + t * t * p1[1],
        ]);
    }
}

pub(crate) fn shield_outline(s: f32) -> Vec<[f32; 2]> {
    let mut right = Vec::new();
    quad_bezier([0.0, 0.8], [0.42, 0.8], [0.82, 0.94], 8, &mut right);
    quad_bezier([0.82, 0.94], [0.88, 0.5], [0.8, 0.12], 8, &mut right);
    quad_bezier([0.8, 0.12], [0.72, -0.55], [0.0, -1.0], 12, &mut right);
    let mut p: Vec<[f32; 2]> = right.clone();
    p.push([0.0, -1.0]);
    for q in right.iter().skip(1).rev() {
        p.push([-q[0], q[1]]);
    }
    ccw(p.into_iter().map(|q| [q[0] * s, q[1] * s]).collect())
}

/// Outline of a thick polyline (miter joins, limited).
pub(crate) fn stroke(points: &[[f32; 2]], w: f32) -> Vec<[f32; 2]> {
    let n = points.len();
    let dir = |a: [f32; 2], b: [f32; 2]| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
        [dx / l, dy / l]
    };
    let mut left = Vec::new();
    let mut right = Vec::new();
    for i in 0..n {
        let d0 = if i > 0 {
            dir(points[i - 1], points[i])
        } else {
            dir(points[0], points[1])
        };
        let d1 = if i + 1 < n {
            dir(points[i], points[i + 1])
        } else {
            d0
        };
        let n0 = [-d0[1], d0[0]];
        let n1 = [-d1[1], d1[0]];
        let mut m = [n0[0] + n1[0], n0[1] + n1[1]];
        let ml = (m[0] * m[0] + m[1] * m[1]).sqrt().max(1e-6);
        m = [m[0] / ml, m[1] / ml];
        let k = (w * 0.5 / (m[0] * n1[0] + m[1] * n1[1]).max(0.5)).min(w);
        left.push([points[i][0] + m[0] * k, points[i][1] + m[1] * k]);
        right.push([points[i][0] - m[0] * k, points[i][1] - m[1] * k]);
    }
    right.reverse();
    left.extend(right);
    ccw(left)
}

pub(crate) fn gear_outline(teeth: usize, pitch_r: f32) -> Vec<[f32; 2]> {
    let m = 2.0 * pitch_r / teeth as f32;
    let (ra, rr) = (pitch_r + m * 0.95, pitch_r - m * 1.15);
    let pa = TAU / teeth as f32;
    let mut p = Vec::new();
    let at = |r: f32, a: f32| [r * a.cos(), r * a.sin()];
    for k in 0..teeth {
        let c = pa * k as f32;
        for (r, f) in [
            (rr, -0.5),
            (rr, -0.38),
            (rr, -0.27),
            (ra, -0.14),
            (ra, -0.05),
            (ra, 0.05),
            (ra, 0.14),
            (rr, 0.27),
            (rr, 0.38),
        ] {
            p.push(at(r, c + f * pa));
        }
    }
    ccw(p)
}

fn sector_outline(r0: f32, r1: f32, a0: f32, a1: f32) -> Vec<[f32; 2]> {
    let steps = ((a1 - a0) / 0.06).ceil().max(2.0) as usize;
    let mut p = Vec::new();
    for k in 0..=steps {
        let a = a0 + (a1 - a0) * k as f32 / steps as f32;
        p.push([r1 * a.cos(), r1 * a.sin()]);
    }
    for k in (0..=steps).rev() {
        let a = a0 + (a1 - a0) * k as f32 / steps as f32;
        p.push([r0 * a.cos(), r0 * a.sin()]);
    }
    ccw(p)
}

/// A padlock shackle: an arch with two straight legs, as one bent tube (the left leg
/// is longer, so it stays in the body when the shackle lifts).
fn lock_shackle() -> Mesh3 {
    let (r, w) = (0.3f32, 0.08f32);
    let arch = facing(Mesh3::torus(r, w, 40, 12, PI, PI));
    let mut m = arch.transformed(Xf::ID.at(v3(0.0, 0.62, 0.0)));
    for x in [-r, r] {
        let leg = Mesh3::rounded_box(v3(w, 0.17, w), w * 0.99, 4);
        m.merge(&leg.transformed(Xf::ID.at(v3(x, 0.47, 0.0))));
    }
    retint(m)
}

/// `n` points evenly spaced by arc length along a closed path, with the direction there.
fn even_points(path: &[[f32; 2]], n: usize) -> Vec<([f32; 2], f32)> {
    let len = path.len();
    let seg = |i: usize| {
        let (p, q) = (path[i], path[(i + 1) % len]);
        (
            (q[0] - p[0]).hypot(q[1] - p[1]),
            (q[1] - p[1]).atan2(q[0] - p[0]),
        )
    };
    let total: f32 = (0..len).map(|i| seg(i).0).sum();
    let mut out = Vec::with_capacity(n);
    let (mut i, mut walked) = (0, 0.0f32);
    for k in 0..n {
        let want = total * k as f32 / n as f32;
        while i < len - 1 && walked + seg(i).0 < want {
            walked += seg(i).0;
            i += 1;
        }
        let (l, ang) = seg(i);
        let t = ((want - walked) / l.max(1e-6)).clamp(0.0, 1.0);
        let (p, q) = (path[i], path[(i + 1) % len]);
        out.push(([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t], ang));
    }
    out
}

/// A keyhole: a round head over a tapered slot.
fn keyhole() -> Vec<[f32; 2]> {
    let (r, cy) = (0.085f32, 0.04f32);
    let mut p = Vec::new();
    // From the right side of the slot top, around the head, to the left side.
    let a0 = (-0.35f32).asin();
    let a1 = PI - a0;
    let n = 24;
    for k in 0..=n {
        let a = a0 + (a1 - a0) * k as f32 / n as f32;
        p.push([r * a.cos(), cy + r * a.sin()]);
    }
    p.push([-0.06, -0.2]);
    p.push([0.06, -0.2]);
    ccw(p)
}

fn build_assets() -> Assets {
    let puck_z = |r: f32, h: f32, b: f32, seg: usize| facing(Mesh3::puck(r, h, b, seg));
    // Bin profile: rounded bottom, tapered wall, rolled rim, flat top under the lid.
    let mut bin = vec![[0.0, -0.9], [0.46, -0.9]];
    for k in 1..5 {
        let a = -FRAC_PI_2 + FRAC_PI_2 * k as f32 / 5.0;
        bin.push([0.46 + 0.1 * a.cos(), -0.8 + 0.1 * a.sin()]);
    }
    bin.extend([
        [0.56, -0.8],
        [0.7, 0.5],
        [0.7, 0.5],
        [0.76, 0.54],
        [0.78, 0.6],
        [0.74, 0.66],
        [0.74, 0.66],
        [0.0, 0.66],
    ]);
    let check = stroke(&[[-0.26, 0.02], [-0.07, -0.18], [0.28, 0.22]], 0.13);
    let pulse = stroke(
        &[
            [-0.52, 0.0],
            [-0.22, 0.0],
            [-0.12, 0.26],
            [0.0, -0.3],
            [0.1, 0.14],
            [0.18, 0.0],
            [0.52, 0.0],
        ],
        0.075,
    );
    // Envelope flap: a rounded triangle hanging from the top edge.
    let mut flap = Vec::new();
    quad_bezier([-0.9, 0.6], [0.0, 0.6], [0.9, 0.6], 2, &mut flap);
    quad_bezier([0.9, 0.6], [0.6, 0.38], [0.08, -0.04], 8, &mut flap);
    quad_bezier([0.08, -0.04], [0.0, -0.1], [-0.08, -0.04], 4, &mut flap);
    quad_bezier([-0.08, -0.04], [-0.6, 0.38], [-0.9, 0.6], 8, &mut flap);
    let hills = ccw(vec![
        [-0.27, -0.16],
        [0.27, -0.16],
        [0.27, -0.02],
        [0.12, 0.14],
        [0.02, 0.04],
        [-0.08, 0.2],
    ]);
    // Disk platter sectors (fractions of a turn) with heights.
    let parts = [
        (0.31, 0.32),
        (0.22, 0.24),
        (0.17, 0.4),
        (0.13, 0.2),
        (0.1, 0.3),
        (0.07, 0.17),
    ];
    let mut sectors = Vec::new();
    let mut start = 0.0f32;
    for (frac, h) in parts {
        let a0 = start + 0.025;
        let a1 = start + frac * TAU - 0.025;
        let m = Mesh3::extrude(&sector_outline(0.34, 1.02, a0, a1), h, 0.05, 2)
            .transformed(Xf::ID.rot_x(-FRAC_PI_2).at(v3(0.0, h * 0.5, 0.0)));
        sectors.push((retint(m), (a0 + a1) * 0.5));
        start += frac * TAU;
    }
    let gauge_n = 28;
    let gauge_arc = 1.5 * PI;
    let gauge_start = -0.75 * PI - FRAC_PI_2; // opening at the bottom
    let gauge = (0..gauge_n)
        .map(|i| {
            let a0 = gauge_start + gauge_arc * i as f32 / gauge_n as f32;
            facing(Mesh3::torus(
                0.82,
                0.135,
                4,
                14,
                a0,
                gauge_arc / gauge_n as f32 * 1.02,
            ))
        })
        .collect();
    // Arrow head for the refresh ring, at the end of a 150 degree arc (ring in XY).
    let arc_end = 0.3 + 150f32.to_radians();
    let cone = Mesh3::lathe(
        &[[0.0, -0.02], [0.24, -0.02], [0.24, -0.02], [0.0, 0.3]],
        32,
        0.0,
        TAU,
    );
    let arrow = cone.transformed(
        Xf::ID
            .rot_z(-FRAC_PI_2)
            .rot_y(-(arc_end + FRAC_PI_2))
            .at(v3(0.78 * arc_end.cos(), 0.0, 0.78 * arc_end.sin()))
            .rot_x(FRAC_PI_2),
    );
    Assets {
        shock: Mesh3::torus(1.0, 0.018, 72, 6, 0.0, TAU),
        gem: Mesh3::gem(9, 0.78, 0.5, 1.0).transformed(Xf::ID.at(v3(0.0, 0.15, 0.0))),
        spark: Mesh3::gem(4, 1.0, 0.7, 0.7),
        glass_cube: Mesh3::rounded_box(v3(0.82, 0.82, 0.82), 0.16, 9),
        tile: Mesh3::rounded_box(v3(0.17, 0.17, 0.17), 0.055, 3),
        env_body: Mesh3::extrude(&rounded_rect(1.9, 1.26, 0.14, 5), 0.3, 0.08, 3),
        env_flap: Mesh3::extrude(&ccw(flap), 0.06, 0.025, 2),
        seal: puck_z(0.15, 0.06, 0.025, 32),
        letter: Mesh3::extrude(&rounded_rect(1.5, 0.9, 0.06, 3), 0.03, 0.01, 1),
        bin: Mesh3::lathe(&bin, 56, 0.0, TAU),
        rib: Mesh3::rounded_box(v3(0.035, 0.52, 0.035), 0.034, 3),
        lid: Mesh3::puck(0.85, 0.14, 0.06, 56),
        handle: Mesh3::tube(0.22, 0.055, 24, 10, 0.0, PI).transformed(Xf::ID.rot_x(-FRAC_PI_2)),
        shield: Mesh3::extrude(&shield_outline(1.0), 0.34, 0.12, 4),
        shield_inner: Mesh3::extrude(&shield_outline(0.66), 0.08, 0.035, 2),
        check: Mesh3::extrude(&check, 0.1, 0.04, 2),
        beam: Mesh3::rounded_box(v3(1.0, 0.025, 0.025), 0.024, 3),
        bead: Mesh3::icosphere(1),
        gear_big: Mesh3::extrude(&gear_outline(GEAR_BIG.0, GEAR_BIG.1), 0.22, 0.035, 2),
        gear_small: Mesh3::extrude(&gear_outline(GEAR_SMALL.0, GEAR_SMALL.1), 0.2, 0.035, 2),
        hub: puck_z(0.16, 0.12, 0.04, 32),
        puck: puck_z(0.95, 0.42, 0.16, 72),
        groove: facing(Mesh3::torus(0.78, 0.03, 72, 8, 0.0, TAU)),
        power_arc: facing(Mesh3::tube(
            0.42,
            0.075,
            56,
            12,
            -FRAC_PI_2 + 0.62,
            TAU - 1.24,
        )),
        power_bar: Mesh3::rounded_box(v3(0.075, 0.24, 0.075), 0.074, 4),
        browser: Mesh3::extrude(&rounded_rect(1.62, 1.2, 0.12, 4), 0.1, 0.035, 2),
        browser_bar: Mesh3::rounded_box(v3(0.78, 0.075, 0.02), 0.02, 3),
        lock_body: Mesh3::extrude(&rounded_rect(1.1, 0.86, 0.17, 5), 0.34, 0.1, 3),
        lock_shackle: lock_shackle(),
        lock_keyhole: Mesh3::extrude(&keyhole(), 0.06, 0.02, 1),
        remover_tile: retint(Mesh3::extrude(&ccw(squircle(0.66, 4.5, 72)), 0.2, 0.06, 3)),
        remover_cell: Mesh3::rounded_box(v3(0.13, 0.13, 0.04), 0.039, 3),
        badge: puck_z(0.23, 0.12, 0.05, 40),
        badge_minus: Mesh3::rounded_box(v3(0.13, 0.035, 0.025), 0.024, 3),
        slot_dash: Mesh3::rounded_box(v3(0.055, 0.022, 0.022), 0.021, 3),
        slot_path: even_points(&squircle(0.62, 4.5, 720), 22),
        refresh_arc: facing(Mesh3::tube(0.78, 0.12, 48, 14, 0.3, 150f32.to_radians())),
        arrow: retint(arrow),
        ball: Mesh3::icosphere(3),
        slabs: [(0.95, 0.68), (0.85, 0.6), (0.74, 0.52), (0.6, 0.42)]
            .iter()
            .map(|&(w, d)| Mesh3::rounded_box(v3(w, 0.13, d), 0.08, 7))
            .collect(),
        card: Mesh3::extrude(&rounded_rect(0.84, 1.18, 0.1, 4), 0.1, 0.035, 2),
        card_pic: Mesh3::rounded_box(v3(0.26, 0.2, 0.02), 0.02, 3),
        card_line: Mesh3::rounded_box(v3(0.28, 0.035, 0.015), 0.015, 3),
        frame: Mesh3::extrude(&rounded_rect(1.0, 0.84, 0.05, 3), 0.08, 0.025, 2),
        photo: Mesh3::extrude(&rounded_rect(0.82, 0.6, 0.03, 2), 0.03, 0.01, 1),
        hills: Mesh3::extrude(&hills, 0.025, 0.008, 1),
        sun: puck_z(0.07, 0.03, 0.012, 24),
        sectors,
        disk_hub: Mesh3::puck(0.26, 0.5, 0.08, 40),
        gauge_track: facing(Mesh3::torus(0.82, 0.11, 96, 12, 0.0, TAU)),
        gauge,
        pulse_line: Mesh3::extrude(&pulse, 0.08, 0.025, 2),
        icon_ring: facing(Mesh3::tube(
            0.78,
            0.235,
            96,
            24,
            ICON_GAP,
            TAU - 2.0 * ICON_GAP,
        )),
    }
}

/// The CleanCraft app icon at `px` x `px` points: a macOS-style squircle tile with a drop
/// shadow and a top sheen, holding a glossy 3D "C" ring around a faceted crystal. `ppp` sets
/// the feather width (pass the supersampling factor when rasterising on the CPU).
pub fn app_icon(px: f32, ppp: f32) -> Mesh {
    use egui::epaint::{Vertex, WHITE_UV};
    use egui::{Color32, Pos2, pos2, vec2};
    let mut out = Mesh::default();
    let c = pos2(px * 0.5, px * 0.5);
    // Apple's grid: an 824 / 1024 tile, superellipse corners.
    let half = px * 0.4023;
    let n = 160;
    let outline: Vec<Vec2> = squircle(1.0, 5.0, n)
        .into_iter()
        .map(|q| vec2(q[0], -q[1]) * half)
        .collect();
    let push = |out: &mut Mesh, pos: Pos2, color: Color32| {
        out.vertices.push(Vertex {
            pos,
            uv: WHITE_UV,
            color,
        });
        (out.vertices.len() - 1) as u32
    };
    // Drop shadow: rings from the outline outward, fading.
    let shadow_off = vec2(0.0, px * 0.014);
    let rings = [(0.0f32, 0.42f32), (0.012, 0.3), (0.028, 0.12), (0.05, 0.0)];
    let base = out.vertices.len() as u32;
    for (grow, a) in rings {
        for q in &outline {
            let d = q.normalized();
            push(
                &mut out,
                c + shadow_off + *q + d * grow * px,
                Color32::from_black_alpha((a * 255.0) as u8),
            );
        }
    }
    let nn = n as u32;
    // Fill the inside of the shadow too (it shows below the tile where it is offset).
    let shadow_c = push(
        &mut out,
        c + shadow_off,
        Color32::from_black_alpha((rings[0].1 * 255.0) as u8),
    );
    for j in 0..nn {
        out.indices
            .extend_from_slice(&[shadow_c, base + j, base + (j + 1) % nn]);
    }
    for r in 0..rings.len() as u32 - 1 {
        for j in 0..nn {
            let j1 = (j + 1) % nn;
            let (a, b) = (base + r * nn + j, base + r * nn + j1);
            let (d, e) = (base + (r + 1) * nn + j, base + (r + 1) * nn + j1);
            out.indices.extend_from_slice(&[a, b, e, a, e, d]);
        }
    }
    // The tile: vertical gradient from a deep teal ink to near black.
    let top = Color32::from_rgb(0x1A, 0x3C, 0x44);
    let bottom = Color32::from_rgb(0x08, 0x10, 0x18);
    let grad = |y: f32| {
        let t = ((y - (c.y - half)) / (2.0 * half)).clamp(0.0, 1.0);
        crate::icons::lerp(top, bottom, t)
    };
    let centre = push(&mut out, c, grad(c.y));
    let first = out.vertices.len() as u32;
    for q in &outline {
        push(&mut out, c + *q, grad(c.y + q.y));
    }
    for j in 0..nn {
        out.indices
            .extend_from_slice(&[centre, first + j, first + (j + 1) % nn]);
    }
    // Feathered tile edge and a thin top sheen.
    let fw = 1.0 / ppp.max(0.25);
    let edge = out.vertices.len() as u32;
    for q in &outline {
        let d = q.normalized();
        push(&mut out, c + *q, grad(c.y + q.y));
        push(&mut out, c + *q + d * fw, Color32::TRANSPARENT);
    }
    for j in 0..nn {
        let j1 = (j + 1) % nn;
        let (a, b, cc, d) = (
            edge + 2 * j,
            edge + 2 * j1,
            edge + 2 * j1 + 1,
            edge + 2 * j + 1,
        );
        out.indices.extend_from_slice(&[a, b, cc, a, cc, d]);
    }
    let sheen = out.vertices.len() as u32;
    for q in &outline {
        let d = q.normalized();
        let up = (-q.y / half).max(0.0).powf(1.5);
        let a = (up * 70.0) as u8;
        push(
            &mut out,
            c + *q - d * px * 0.004,
            Color32::from_white_alpha(a),
        );
        push(&mut out, c + *q - d * px * 0.03, Color32::TRANSPARENT);
    }
    for j in 0..nn {
        let j1 = (j + 1) % nn;
        let (a, b, cc, d) = (
            sheen + 2 * j,
            sheen + 2 * j1,
            sheen + 2 * j1 + 1,
            sheen + 2 * j + 1,
        );
        out.indices.extend_from_slice(&[a, b, cc, a, cc, d]);
    }
    // The 3D mark.
    let ast = assets();
    let (ca, cb) = Section::Overview.colors();
    let (a, b) = (lin(ca), lin(cb));
    let mut sc = Scene::new(Camera::front(6.0, 0.18), Rig::studio(a));
    sc.sprite(
        v3(0.0, -0.05, -1.5),
        1.55,
        mix(a, b, 0.3),
        0.6,
        SpriteKind::Glow,
    );
    sc.sprite(
        v3(0.0, -1.32, 0.0),
        1.15,
        BLACK,
        0.55,
        SpriteKind::Shadow(0.2),
    );
    let root = Xf::ID.rot_y(-0.42).rot_x(0.16).at(v3(0.0, 0.08, 0.0));
    let ring = Material {
        spec: 0.9,
        shine: 60.0,
        ..Material::gloss(mix(a, WHITE, 0.2), b)
    };
    sc.add(&ast.icon_ring, root, ring);

    let gem = Material {
        spec: 1.1,
        shine: 70.0,
        rim: 0.9,
        ..Material::gloss(mix(a, WHITE, 0.35), mix(a, b, 0.55))
    }
    .with_emissive(scale(a, 0.12));
    sc.add(
        &ast.gem,
        Xf::scale(0.5)
            .rot_y(0.5)
            .rot_x(0.15)
            .at(v3(0.05, 0.02, 0.15))
            .then(root),
        gem,
    );
    sc.sprite(
        v3(-0.35, 0.68, 0.9),
        0.36,
        WHITE,
        1.0,
        SpriteKind::Star(0.0),
    );
    sc.sprite(
        v3(1.05, 0.95, 0.3),
        0.22,
        [1.0, 0.85, 0.55],
        0.9,
        SpriteKind::Star(0.0),
    );
    sc.sprite(v3(0.12, 0.25, 0.7), 0.16, WHITE, 0.8, SpriteKind::Star(0.4));
    let tile = Rect::from_center_size(c, Vec2::splat(half * 2.0));
    let (mark, _) = sc.render(tile, ppp);
    out.append(mark);
    out
}

/// Half the opening of the icon's "C", in radians.
const ICON_GAP: f32 = 0.78;

/// Build every hero's geometry now (otherwise the first frame that shows a hero does).
pub fn warm_up() {
    let _ = assets();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(phase: HeroPhase, progress: f32, since: f32) -> HeroState {
        HeroState {
            phase,
            progress,
            t: 3.7,
            since,
        }
    }

    #[test]
    fn every_hero_renders_in_every_state_inside_its_rect() {
        let rect = Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(240.0, 240.0));
        for f in FeatureId::ALL {
            for s in [
                st(HeroPhase::Idle, 0.0, 3.7),
                st(HeroPhase::Scanning, 0.5, 2.0),
                st(HeroPhase::Done, 1.0, 0.3),
                st(HeroPhase::Done, 1.0, 5.0),
            ] {
                let (m, stats) = mesh(f, s, rect, 2.0);
                assert!(stats.triangles > 40, "{f:?} {s:?}: {stats:?}");
                assert!(stats.silhouette_edges > 10, "{f:?} {s:?}: {stats:?}");
                // The object (not the soft glows) stays well inside a margin around the rect.
                let grown = rect.expand(rect.width() * 0.5);
                for v in &m.vertices {
                    assert!(grown.contains(v.pos), "{f:?} {s:?} {:?}", v.pos);
                }
                assert!(m.is_valid());
            }
        }
    }

    #[test]
    fn nothing_reaches_the_outer_margin_of_the_slot() {
        // REVIEW-B B3: the slot may be clipped, so glows, shadows and particles must reach
        // zero well inside the rect or a lit box shows. Checked on a CPU raster at 96 and
        // 200 pt, every state, including the burst.
        let ink = egui::Color32::from_rgb(0x0E, 0x13, 0x1B);
        for side in [96usize, 200] {
            let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(side as f32, side as f32));
            for f in FeatureId::ALL {
                for s in [
                    st(HeroPhase::Idle, 0.0, 3.7),
                    st(HeroPhase::Scanning, 0.9, 2.0),
                    st(HeroPhase::Done, 1.0, 0.12),
                    st(HeroPhase::Done, 1.0, 0.3),
                    st(HeroPhase::Done, 1.0, 0.6),
                    st(HeroPhase::Done, 1.0, 5.0),
                ] {
                    let mut all = Mesh::default();
                    all.add_colored_rect(rect, ink);
                    all.append(mesh(f, s, rect, 1.0).0);
                    let px = crate::render3d::rasterize(&all, side, side, 1.0, 1);
                    let m = crate::render3d::SPRITE_MARGIN as usize - 2;
                    let mut worst = 0u8;
                    for y in 0..side {
                        for x in 0..side {
                            if x < m || y < m || x >= side - m || y >= side - m {
                                let p = &px[(y * side + x) * 4..(y * side + x) * 4 + 3];
                                let d = p
                                    .iter()
                                    .zip([ink.r(), ink.g(), ink.b()])
                                    .map(|(&a, b)| a.abs_diff(b))
                                    .max()
                                    .unwrap_or(0);
                                worst = worst.max(d);
                            }
                        }
                    }
                    assert!(
                        worst <= 2,
                        "{f:?} {s:?} at {side} pt: edge differs by {worst}"
                    );
                }
            }
        }
    }

    #[test]
    fn states_look_different() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 200.0));
        for f in FeatureId::ALL {
            let idle = mesh(f, st(HeroPhase::Idle, 0.0, 3.7), rect, 1.0).1;
            let scan = mesh(f, st(HeroPhase::Scanning, 0.6, 2.0), rect, 1.0).1;
            let burst = mesh(f, st(HeroPhase::Done, 1.0, 0.3), rect, 1.0).1;
            assert!(scan.sprites > idle.sprites, "{f:?}: orbiting particles");
            assert!(burst.sprites > idle.sprites, "{f:?}: burst particles");
        }
    }

    #[test]
    fn done_settles_to_a_still_rest_pose() {
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(200.0, 200.0));
        for f in FeatureId::ALL {
            let a = mesh(f, st(HeroPhase::Done, 1.0, 6.0), rect, 1.0).1;
            let b = mesh(f, st(HeroPhase::Done, 1.0, 6.5), rect, 1.0).1;
            // After the burst only the hero itself and the ambient sprites remain.
            assert_eq!(a.sprites, b.sprites, "{f:?}");
        }
    }

    #[test]
    fn motion_is_continuous_frame_to_frame() {
        // Vertices move less than a few points between two frames 1/60 s apart, so nothing
        // jumps or flickers (same vertex order frame to frame).
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(220.0, 220.0));
        for f in FeatureId::ALL {
            for (phase, since) in [
                (HeroPhase::Idle, 3.0f32),
                (HeroPhase::Scanning, 1.5),
                (HeroPhase::Done, 2.0),
            ] {
                let a = mesh(
                    f,
                    HeroState {
                        phase,
                        progress: 0.5,
                        t: 3.0,
                        since,
                    },
                    rect,
                    1.0,
                )
                .0;
                let b = mesh(
                    f,
                    HeroState {
                        phase,
                        progress: 0.5,
                        t: 3.0 + 1.0 / 60.0,
                        since: since + 1.0 / 60.0,
                    },
                    rect,
                    1.0,
                )
                .0;
                // Every sampled vertex of one frame has a close neighbour in the next (the
                // silhouette strips change order, so match by position).
                let worst = a
                    .vertices
                    .iter()
                    .step_by(23)
                    .map(|p| {
                        b.vertices
                            .iter()
                            .map(|q| (p.pos - q.pos).length_sq())
                            .fold(f32::MAX, f32::min)
                            .sqrt()
                    })
                    .fold(0.0f32, f32::max);
                assert!(worst < 12.0, "{f:?} {phase:?}: {worst}");
            }
        }
    }
}
