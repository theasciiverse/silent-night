//! Silent Night: a snowy forest at night.
//!
//! Snow at three depths blows in the wind and its gusts and settles on the
//! ground, the spruces and a cabin roof; the wind lifts it off the open
//! snow and lays it down again in the lee of the cabin and the trees, so
//! drifts build and move. Stars, a faint aurora, a moon, a lit cabin with
//! smoke from the chimney, a decorated spruce, and now and then a sleigh
//! across the sky. The forest's sound (sound.rs) follows the wind on
//! screen. m turns the sound off and on; any other key quits.

mod sound;

use std::io::{self, BufWriter, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    queue,
    style::{Color, Print, SetBackgroundColor, SetForegroundColor},
    terminal,
};

const FPS: f32 = 30.0;
/// Seconds of weather run before the first frame, so the snow has already
/// settled and drifted when the picture appears.
const WARMUP: f32 = 480.0;

type Rgb = [f32; 3];

fn add(a: Rgb, b: Rgb) -> Rgb {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: Rgb, k: f32) -> Rgb {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn hash(seed: u32, i: i32) -> f32 {
    let mut h = (i as u32).wrapping_mul(0x9E37_79B1) ^ seed.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h & 0xFF_FFFF) as f32 / 16_777_215.0
}
fn hash2(seed: u32, x: i32, y: i32) -> f32 {
    hash(seed, x.wrapping_mul(7919) ^ y.wrapping_mul(104_729))
}
/// Smooth value noise, 0 to 1.
fn noise1(seed: u32, x: f32) -> f32 {
    let i = x.floor();
    let u = smooth(x - i);
    let a = hash(seed, i as i32);
    a + (hash(seed, i as i32 + 1) - a) * u
}

// ---------------------------------------------------------------- palette

const SKY_TOP: Rgb = [0.012, 0.016, 0.05];
const SKY_LOW: Rgb = [0.05, 0.07, 0.15];
const HAZE: Rgb = [0.16, 0.18, 0.26];
const MOON: Rgb = [0.96, 0.94, 0.84];
// farther is lighter and bluer, as in the haze of a winter night
const MOUNTAIN: Rgb = [0.13, 0.15, 0.24];
const MOUNTAIN_SNOW: Rgb = [0.38, 0.42, 0.56];
const FAR_FOREST: Rgb = [0.085, 0.105, 0.16];
const MID_FOREST: Rgb = [0.05, 0.07, 0.10];
const SPRUCE: Rgb = [0.02, 0.085, 0.055];
const FIR: Rgb = [0.03, 0.15, 0.08];
const TRUNK: Rgb = [0.11, 0.065, 0.04];
const SNOW_LIT: Rgb = [0.80, 0.85, 0.97];
const SNOW_DEEP: Rgb = [0.46, 0.52, 0.72];
const FLAKE: Rgb = [0.94, 0.96, 1.0];
const LOG: Rgb = [0.23, 0.125, 0.065];
const LOG_SEAM: Rgb = [0.11, 0.055, 0.03];
const ROOF: Rgb = [0.16, 0.09, 0.07];
const STONE: Rgb = [0.22, 0.20, 0.21];
const DOOR: Rgb = [0.15, 0.08, 0.04];
const WINDOW: Rgb = [1.0, 0.66, 0.28];
const SMOKE: Rgb = [0.42, 0.42, 0.48];
const AURORA_GREEN: Rgb = [0.10, 0.85, 0.45];
const AURORA_VIOLET: Rgb = [0.32, 0.14, 0.55];
const SILHOUETTE: Rgb = [0.01, 0.01, 0.025];
const NOSE: Rgb = [1.0, 0.18, 0.12];
const LIGHTS: [Rgb; 5] = [
    [1.0, 0.16, 0.14],
    [1.0, 0.72, 0.18],
    [0.25, 0.45, 1.0],
    [0.2, 0.9, 0.35],
    [0.95, 0.9, 0.82],
];
const STAR_GOLD: Rgb = [1.0, 0.85, 0.35];

const LOWER: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

// ---------------------------------------------------------------- things

#[derive(Clone, Copy, PartialEq)]
enum Layer {
    Far,
    Mid,
    Near,
}

struct Flake {
    x: f32,
    y: f32,
    vy: f32,
    phase: f32,
    flutter: f32,
    layer: Layer,
    ch: char,
    end_y: f32,
}

/// Snow on the move near the ground (drift) or falling in a clump.
struct Grain {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    mass: f32,
    life: f32,
    clump: bool,
    shade: f32,
}

struct Puff {
    x: f32,
    y: f32,
    vx: f32,
    age: f32,
    life: f32,
}

struct Star {
    x: usize,
    y: usize,
    b: f32,
    phase: f32,
    speed: f32,
    ch: char,
    col: Rgb,
}

struct Tree {
    cx: f32,
    base: f32,
    height: f32,
    half: f32,
    tiers: f32,
    load: f32,
    sway: f32,
    seed: u32,
    decorated: bool,
    lights: Vec<(i32, i32, usize, f32)>,
}

#[derive(PartialEq)]
enum TreeCell {
    None,
    Trunk,
    Foliage,
}

impl Tree {
    fn cell(&self, x: i32, y: i32) -> TreeCell {
        let v = (self.base - (y as f32 + 0.5)) / self.height;
        if !(0.0..=1.0).contains(&v) {
            return TreeCell::None;
        }
        let dx = x as f32 + 0.5 - (self.cx + self.sway * v * v);
        if v >= 0.07 {
            let u = (v - 0.07) / 0.93;
            let frac = ((1.0 - u) * self.tiers).fract();
            let hw = self.half * (1.0 - u).powf(0.95) * (0.5 + 0.5 * frac) + 0.45;
            if dx.abs() < hw {
                return TreeCell::Foliage;
            }
        }
        if v < 0.12 && dx.abs() < 0.8 {
            return TreeCell::Trunk;
        }
        TreeCell::None
    }
    fn top(&self) -> f32 {
        self.base - self.height
    }
    fn left(&self) -> f32 {
        self.cx - self.half - 1.0
    }
    fn right(&self) -> f32 {
        self.cx + self.half + 1.0
    }
}

struct Cabin {
    x0: i32,
    w: i32,
    base: f32,
    wall_top: f32,
    roof_h: f32,
    chimney: i32,
    chimney_top: f32,
    windows: Vec<(i32, i32)>, // left column, top row; 4 by 2 cells
    door: i32,                // left column; 3 wide
}

impl Cabin {
    fn x1(&self) -> i32 {
        self.x0 + self.w - 1
    }
    fn roof_span(&self) -> (i32, i32) {
        (self.x0 - 1, self.x1() + 1)
    }
    /// The top of the roof at a column (rows), or None off the roof.
    fn roof_line(&self, x: i32) -> Option<f32> {
        let (a, b) = self.roof_span();
        if x < a || x > b {
            return None;
        }
        let half = (b - a + 1) as f32 / 2.0;
        let d = (x as f32 + 0.5 - (a as f32 + half)).abs();
        Some(self.wall_top - self.roof_h * (1.0 - d / half) + 0.15)
    }
    fn height(&self) -> f32 {
        self.base - self.wall_top + self.roof_h * 0.6
    }
}

struct Gust {
    t0: f32,
    rise: f32,
    hold: f32,
    fall: f32,
    peak: f32,
}

impl Gust {
    fn at(&self, t: f32) -> f32 {
        let u = t - self.t0;
        let e = if u < 0.0 {
            0.0
        } else if u < self.rise {
            smooth(u / self.rise)
        } else if u < self.rise + self.hold {
            1.0
        } else {
            1.0 - smooth((u - self.rise - self.hold) / self.fall)
        };
        e * self.peak
    }
    fn end(&self) -> f32 {
        self.t0 + self.rise + self.hold + self.fall
    }
}

struct Sleigh {
    x: f32,
    y: f32,
    dir: f32,
    speed: f32,
}

const SLEIGH: [&str; 2] = ["  ▗▖     Y   Y   Y ", "▝▀▀▀▀▘~~~m~~~m~~~m●"];

fn mirror(c: char) -> char {
    match c {
        '▝' => '▘',
        '▘' => '▝',
        '▗' => '▖',
        '▖' => '▗',
        c => c,
    }
}

// ---------------------------------------------------------------- the scene

struct Scene {
    w: usize,
    h: usize,
    t: f32,
    rng: fastrand::Rng,
    seed: u32,
    base_y: f32,
    horizon: f32,
    ridge: Vec<f32>,
    far_top: Vec<f32>,
    mid_top: Vec<f32>,
    /// Snow depth on the ground, rows; the surface is at base_y - snow.
    snow: Vec<f32>,
    shelter: Vec<f32>,
    shelter_dir: f32,
    max_depth: f32,
    trees: Vec<Tree>,
    cabin: Option<Cabin>,
    roof_snow: Vec<f32>,
    stars: Vec<Star>,
    moon: (f32, f32, f32), // x, y, radius in rows
    aurora: (f32, f32, f32, f32), // span start, end, two phases
    flakes: Vec<Flake>,
    grains: Vec<Grain>,
    puffs: Vec<Puff>,
    glints: Vec<(usize, f32)>,
    wind_base: f32,
    wind_target: f32,
    next_wind_change: f32,
    gusts: Vec<Gust>,
    next_gust: f32,
    snowfall: f32,
    snowfall_target: f32,
    next_snowfall_change: f32,
    aurora_amp: f32,
    aurora_target: f32,
    next_aurora_change: f32,
    sleigh: Option<Sleigh>,
    next_sleigh: f32,
    next_owl: f32,
    events: Vec<sound::Event>,
    clumps: u32,
}

impl Scene {
    fn new(w: usize, h: usize, seed: u64) -> Scene {
        let mut rng = fastrand::Rng::with_seed(seed);
        let s32 = rng.u32(..);
        let (wf, hf) = (w as f32, h as f32);
        let base_y = hf - 1.5 - hf * 0.06;
        let horizon = (hf * 0.62).max(4.0);

        let ridge = (0..w)
            .map(|x| {
                let xf = x as f32;
                let n = 0.55 * noise1(s32 ^ 11, xf * 0.035) + 0.3 * noise1(s32 ^ 12, xf * 0.09) + 0.15 * noise1(s32 ^ 13, xf * 0.25);
                horizon - 1.0 - n.powf(1.7) * hf * 0.42
            })
            .collect();
        let spiky = |rng: &mut fastrand::Rng, base: f32, lo: f32, hi: f32, step: (usize, usize)| {
            let mut top = vec![base; w];
            let mut x = -(rng.usize(0..4) as i32);
            while x < w as i32 + 4 {
                let ht = lo + rng.f32() * (hi - lo);
                let half = ht * 0.7 + 0.5;
                for cx in (x - half.ceil() as i32)..=(x + half.ceil() as i32) {
                    if (0..w as i32).contains(&cx) {
                        let d = (cx - x).abs() as f32;
                        let y = base - ht * (1.0 - d / half).max(0.0);
                        let c = &mut top[cx as usize];
                        *c = c.min(y);
                    }
                }
                x += rng.usize(step.0..=step.1) as i32;
            }
            top
        };
        let far_top = spiky(&mut rng, horizon + 0.6, 0.8, 2.6, (1, 3));
        let mid_base = horizon + (base_y - horizon) * 0.55;
        let mid_top = spiky(&mut rng, mid_base, 1.5, (hf * 0.16).max(2.0), (2, 4));

        let snow: Vec<f32> = (0..w).map(|x| 2.0 + 0.4 * noise1(s32 ^ 21, x as f32 * 0.07)).collect();

        // the cabin, and the decorated spruce beside it
        let mut cabin = None;
        let mut trees = Vec::new();
        if w >= 44 && h >= 16 {
            let cw = if w >= 80 && h >= 26 { 16 } else { 12 };
            let wall = if h >= 26 { 6.0 } else { 5.0 };
            let x0 = (wf * (0.42 + rng.f32() * 0.16)) as i32 - cw / 2;
            let wall_top = (base_y - wall).floor();
            let roof_h = (cw as f32 / 2.0 + 1.0) * 0.5;
            let chimney = x0 + cw * 3 / 4 - 1;
            let mut c = Cabin {
                x0,
                w: cw,
                base: base_y,
                wall_top,
                roof_h,
                chimney,
                chimney_top: 0.0,
                windows: if cw >= 16 { vec![(x0 + 2, wall_top as i32 + 1), (x0 + cw - 6, wall_top as i32 + 1)] } else { vec![(x0 + 2, wall_top as i32 + 1)] },
                door: if cw >= 16 { x0 + cw / 2 - 1 } else { x0 + cw - 4 },
            };
            c.chimney_top = c.roof_line(chimney).unwrap().min(c.roof_line(chimney + 1).unwrap()) - 1.6;
            let side = if rng.bool() { 1.0 } else { -1.0 };
            let ht = (hf * 0.42).clamp(7.0, 14.0);
            let cx = if side > 0.0 { c.x1() as f32 + 3.5 + ht * 0.5 } else { x0 as f32 - 2.5 - ht * 0.5 };
            let mut deco = Tree {
                cx,
                base: base_y,
                height: ht,
                half: ht * 0.5,
                tiers: 4.0,
                load: 0.5,
                sway: 0.0,
                seed: rng.u32(..),
                decorated: true,
                lights: Vec::new(),
            };
            for y in (deco.top() as i32)..(base_y as i32) {
                for x in (deco.left() as i32)..=(deco.right() as i32) {
                    if deco.cell(x, y) == TreeCell::Foliage && rng.f32() < 0.28 {
                        deco.lights.push((x, y, rng.usize(..LIGHTS.len()), rng.f32() * 6.28));
                    }
                }
            }
            trees.push(deco);
            cabin = Some(c);
        }

        // spruces in front: some at the edges, a few between
        let mut tries = 0;
        let count = 2 + (w / 40).min(4);
        while trees.iter().filter(|t| !t.decorated).count() < count && tries < 200 {
            tries += 1;
            let ht = hf * (0.32 + rng.f32() * 0.3);
            let half = ht * 0.42;
            let cx = match trees.iter().filter(|t| !t.decorated).count() {
                0 => rng.f32() * 6.0 + 1.0,
                1 => wf - 1.0 - rng.f32() * 6.0,
                _ => rng.f32() * wf,
            };
            let t = Tree {
                cx,
                base: base_y,
                height: ht,
                half,
                tiers: (ht / 2.6).round().max(3.0),
                load: 0.4 + rng.f32() * 0.3,
                sway: 0.0,
                seed: rng.u32(..),
                decorated: false,
                lights: Vec::new(),
            };
            let clear_cabin = cabin.as_ref().is_none_or(|c| t.right() < c.x0 as f32 - 2.0 || t.left() > c.x1() as f32 + 2.0);
            let clear_trees = trees.iter().all(|o| (o.cx - t.cx).abs() > (o.half + t.half) * 0.8);
            if clear_cabin && clear_trees {
                trees.push(t);
            }
        }
        trees.sort_by(|a, b| a.height.partial_cmp(&b.height).unwrap().reverse());

        let moon_left = rng.bool();
        let moon = (
            (if moon_left { wf * (0.1 + rng.f32() * 0.12) } else { wf * (0.78 + rng.f32() * 0.12) }).round(),
            (hf * 0.14).max(2.0) + 0.5,
            if h >= 26 { 2.1 } else { 1.4 },
        );
        let aurora = if moon_left { (wf * 0.32, wf * 1.05, rng.f32() * 6.0, rng.f32() * 6.0) } else { (-wf * 0.05, wf * 0.68, rng.f32() * 6.0, rng.f32() * 6.0) };

        let mut stars = Vec::new();
        let n = (wf * horizon * 0.05) as usize;
        for _ in 0..n {
            let x = rng.usize(..w);
            let y = rng.usize(..(horizon as usize).max(1));
            let r = rng.f32();
            stars.push(Star {
                x,
                y,
                b: 0.25 + rng.f32().powi(2) * 0.75,
                phase: rng.f32() * 6.28,
                speed: 0.5 + rng.f32() * 2.5,
                ch: if r < 0.04 { '✦' } else if r < 0.2 { '+' } else if r < 0.5 { '·' } else { '.' },
                col: mix([0.75, 0.82, 1.0], [1.0, 0.92, 0.75], rng.f32()),
            });
        }

        let mut s = Scene {
            w,
            h,
            t: 0.0,
            rng,
            seed: s32,
            base_y,
            horizon,
            ridge,
            far_top,
            mid_top,
            snow,
            shelter: vec![0.0; w],
            shelter_dir: 0.0,
            max_depth: (hf * 0.22).max(3.0),
            trees,
            cabin,
            roof_snow: vec![0.5; w],
            stars,
            moon,
            aurora,
            flakes: Vec::new(),
            grains: Vec::new(),
            puffs: Vec::new(),
            glints: Vec::new(),
            wind_base: 0.3,
            wind_target: 0.3,
            next_wind_change: 20.0,
            gusts: Vec::new(),
            next_gust: 3.0,
            snowfall: 0.7,
            snowfall_target: 0.7,
            next_snowfall_change: 30.0,
            aurora_amp: 0.12,
            aurora_target: 0.12,
            next_aurora_change: 40.0,
            sleigh: None,
            next_sleigh: WARMUP + 14.0,
            next_owl: WARMUP + 6.0,
            events: Vec::new(),
            clumps: 0,
        };
        if s.rng.f32() < 0.2 {
            s.wind_base = -0.3;
            s.wind_target = -0.3;
        }
        s.compute_shelter();
        let dt = 1.0 / 15.0;
        while s.t < WARMUP {
            s.update(dt);
        }
        s.events.clear();
        s
    }

    fn surface(&self, x: usize) -> f32 {
        self.base_y - self.snow[x]
    }

    fn gust_at(&self, t: f32) -> f32 {
        self.gusts.iter().map(|g| g.at(t)).sum()
    }

    fn dir(&self) -> f32 {
        if self.wind_base.abs() > 0.03 {
            self.wind_base.signum()
        } else {
            self.wind_target.signum()
        }
    }

    /// The wind at a point: the steady wind plus gusts that sweep across
    /// from the windward side, weaker near the ground, a little turbulent.
    fn wind_at(&self, x: f32, y: f32) -> f32 {
        let dir = self.dir();
        let travel = (if dir > 0.0 { x } else { self.w as f32 - x }) / 28.0;
        let g = self.gust_at(self.t - travel) * dir;
        let turb = 0.10 * (noise1(self.seed ^ 7, x * 0.08 - self.t * 0.9 * dir) - 0.5)
            + 0.07 * (noise1(self.seed ^ 9, y * 0.4 + self.t * 1.7) - 0.5);
        let height = 0.55 + 0.45 * ((self.base_y - y) / 8.0).clamp(0.0, 1.0);
        (self.wind_base + g + turb) * height
    }

    /// How much each ground column is kept out of the wind: long lee zones
    /// behind the cabin and the trees, a little in front of them.
    fn compute_shelter(&mut self) {
        let dir = self.dir();
        self.shelter_dir = dir;
        // left, right, how far the lee reaches, how much wind it stops; a
        // spruce lets much of the wind through, so its lee is short
        let mut obstacles: Vec<(f32, f32, f32, f32)> = Vec::new();
        if let Some(c) = &self.cabin {
            obstacles.push((c.x0 as f32, c.x1() as f32 + 1.0, c.height() * 2.2, 0.9));
        }
        for t in &self.trees {
            obstacles.push((t.cx - t.half * 0.8, t.cx + t.half * 0.8, t.height * 0.5, 0.6));
        }
        for x in 0..self.w {
            let xf = x as f32 + 0.5;
            let mut s: f32 = 0.0;
            for &(l, r, reach, k) in &obstacles {
                let (front, back) = if dir > 0.0 { (l, r) } else { (r, l) };
                let behind = (xf - back) * dir;
                let before = (front - xf) * dir;
                if xf >= l && xf <= r {
                    s = s.max(k * 0.7);
                } else if behind > 0.0 {
                    s = s.max(k * (-behind / reach).exp());
                } else if before > 0.0 && before < 3.5 {
                    s = s.max(k * 0.6 * (1.0 - before / 3.5));
                }
            }
            self.shelter[x] = s.min(0.95);
        }
    }

    fn cabin_hit(&self, x: i32, y: f32) -> bool {
        self.cabin.as_ref().is_some_and(|c| x >= c.x0 && x <= c.x1() && y >= c.wall_top && y < c.base)
    }

    fn roof_hit(&self, x: i32, y: f32) -> bool {
        match self.cabin.as_ref().and_then(|c| c.roof_line(x)) {
            Some(line) => y >= line - self.roof_snow[x as usize] && y < line + 1.0,
            None => false,
        }
    }

    fn tree_hit(&self, x: i32, y: f32) -> Option<usize> {
        self.trees.iter().position(|t| t.cell(x, y as i32) == TreeCell::Foliage)
    }

    fn update(&mut self, dt: f32) {
        self.t += dt;
        let t = self.t;
        let (w, wf) = (self.w, self.w as f32);

        // --- weather
        if t >= self.next_wind_change {
            // mostly the same way; now and then the wind turns
            let strength = 0.08 + self.rng.f32() * 0.5;
            let turn = self.rng.f32() < 0.12;
            self.wind_target = strength * if turn { -self.dir() } else { self.dir() };
            self.next_wind_change = t + 30.0 + self.rng.f32() * 60.0;
        }
        self.wind_base += (self.wind_target - self.wind_base) * (dt / 20.0);
        if t >= self.next_gust {
            let peak = 0.25 + self.rng.f32().powf(1.5) * 1.0;
            let g = Gust { t0: t, rise: 1.2 + self.rng.f32() * 2.0, hold: 0.5 + self.rng.f32() * 3.5, fall: 2.5 + self.rng.f32() * 4.0, peak };
            if t > WARMUP {
                log::debug!("gust peak {peak:.2} over {:.1}s", g.end() - t);
            }
            self.gusts.push(g);
            self.next_gust = t + (4.0 + self.rng.f32() * 16.0) / (0.6 + self.wind_base.abs());
        }
        let span = (wf / 28.0) + 1.0;
        self.gusts.retain(|g| g.end() + span > t);
        if t >= self.next_snowfall_change {
            self.snowfall_target = 0.12 + self.rng.f32() * 0.88;
            self.next_snowfall_change = t + 40.0 + self.rng.f32() * 80.0;
        }
        self.snowfall += (self.snowfall_target - self.snowfall) * (dt / 25.0);
        if t >= self.next_aurora_change {
            self.aurora_target = if self.rng.f32() < 0.2 { 0.03 } else { 0.08 + self.rng.f32() * 0.16 };
            self.next_aurora_change = t + 50.0 + self.rng.f32() * 90.0;
        }
        self.aurora_amp += (self.aurora_target - self.aurora_amp) * (dt / 30.0);
        if self.dir() != self.shelter_dir {
            self.compute_shelter();
        }

        // --- trees sway
        for i in 0..self.trees.len() {
            let tr = &self.trees[i];
            let target = self.wind_at(tr.cx, tr.base - tr.height * 0.7) * 0.22 * tr.height / 10.0;
            self.trees[i].sway += (target - self.trees[i].sway) * (dt * 1.5).min(1.0);
        }

        // --- falling snow
        let area = wf * self.h as f32;
        let targets = [
            (Layer::Far, area * 0.022 * (0.3 + self.snowfall)),
            (Layer::Mid, area * 0.014 * self.snowfall),
            (Layer::Near, area * 0.008 * self.snowfall),
        ];
        let mean_wind = self.wind_base + self.gust_at(t) * self.dir();
        for (layer, target) in targets {
            let have = self.flakes.iter().filter(|f| f.layer == layer).count() as f32;
            let mut spawn = ((target - have) * dt * 1.5).max(0.0);
            while spawn > 0.0 {
                if self.rng.f32() < spawn.min(1.0) {
                    let (vy, end_y, ch) = match layer {
                        Layer::Far => (1.0 + self.rng.f32() * 0.6, self.horizon + self.rng.f32() * 3.0, if self.rng.bool() { '·' } else { '.' }),
                        Layer::Mid => (
                            2.0 + self.rng.f32() * 1.0,
                            self.horizon + (self.base_y - self.horizon) * (0.4 + self.rng.f32() * 0.5),
                            ['•', '·', '∙'][self.rng.usize(..3)],
                        ),
                        Layer::Near => (3.2 + self.rng.f32() * 2.0, f32::MAX, ['*', '❄', '•', '*', '•'][self.rng.usize(..5)]),
                    };
                    let margin = (mean_wind.abs() * 14.0 / vy) * self.h as f32 * 0.6 + 2.0;
                    let lo = if mean_wind > 0.0 { -margin } else { 0.0 };
                    let hi = if mean_wind > 0.0 { wf } else { wf + margin };
                    self.flakes.push(Flake {
                        x: lo + self.rng.f32() * (hi - lo),
                        y: -1.0 - self.rng.f32() * 2.0,
                        vy,
                        phase: self.rng.f32() * 6.28,
                        flutter: 0.6 + self.rng.f32() * 1.6,
                        layer,
                        ch,
                        end_y,
                    });
                }
                spawn -= 1.0;
            }
        }
        let mut flakes = std::mem::take(&mut self.flakes);
        flakes.retain_mut(|f| {
            let k = match f.layer {
                Layer::Far => 5.0,
                Layer::Mid => 9.0,
                Layer::Near => 14.0,
            };
            let vx = self.wind_at(f.x, f.y) * k + (t * f.flutter + f.phase).sin() * 0.7;
            f.x += vx * dt;
            f.y += f.vy * (1.0 + 0.15 * (t * f.flutter * 0.7 + f.phase).cos()) * dt;
            if f.x < -60.0 || f.x > wf + 60.0 {
                return false;
            }
            if f.layer != Layer::Near {
                return f.y < f.end_y;
            }
            let col = f.x.floor() as i32;
            if col < 0 || col >= w as i32 || f.y < 0.0 {
                return f.y < self.base_y;
            }
            if self.roof_hit(col, f.y) {
                self.roof_snow[col as usize] += 0.05;
                return false;
            }
            if let Some(i) = self.tree_hit(col, f.y) {
                if self.rng.f32() < 0.12 {
                    self.trees[i].load = (self.trees[i].load + 0.01).min(1.0);
                    return false;
                }
            }
            if f.y >= self.surface(col as usize) {
                self.snow[col as usize] += 0.05;
                return false;
            }
            true
        });
        self.flakes = flakes;
        // the fine snow too small to draw
        for x in 0..w {
            self.snow[x] += self.snowfall * 0.003 * dt;
        }
        for tr in &mut self.trees {
            tr.load = (tr.load + self.snowfall * 0.006 * dt).min(1.0);
        }

        // --- drifting snow: lifted off the open snow, laid down out of the wind
        const THR: f32 = 0.42;
        // a bump keeps the wind off the snow just behind it, so ripples and
        // dunes grow and creep downwind
        let dir = self.dir();
        let lee: Vec<f32> = (0..w)
            .map(|x| {
                let mut s: f32 = 0.0;
                for k in 1..=5 {
                    let u = x as i32 - dir as i32 * k;
                    if (0..w as i32).contains(&u) {
                        s = s.max((self.snow[u as usize] - self.snow[x]) * 0.5 / k as f32);
                    }
                }
                s.min(0.8)
            })
            .collect();
        for x in 0..w {
            let y = self.surface(x);
            let wg = self.wind_at(x as f32 + 0.5, y).abs() * (1.0 - self.shelter[x]) * (1.0 - lee[x]);
            // below the loose top the snow is packed hard and stays
            if wg > THR && self.snow[x] > 0.7 && self.rng.f32() < (wg - THR) * 16.0 * dt {
                self.snow[x] -= 0.02;
                let gx = x as f32 + self.rng.f32();
                let g = self.grain(gx, y - 0.1, wg);
                self.grains.push(g);
            }
        }
        // in a strong gust the loose powder streams along the ground: there
        // to be seen, it carries no snow from place to place
        for x in 0..w {
            let y = self.surface(x);
            let wg = self.wind_at(x as f32 + 0.5, y).abs() * (1.0 - self.shelter[x]) * (1.0 - lee[x]);
            if wg > 0.33 && self.rng.f32() < (wg - 0.33) * 30.0 * dt {
                let gx = x as f32 + self.rng.f32();
                let mut g = self.grain(gx, y - 0.1, wg);
                g.mass = 0.0;
                g.life = 1.5 + self.rng.f32() * 2.5;
                g.vy -= self.rng.f32() * 3.0 * wg;
                self.grains.push(g);
            }
        }
        // and off the roof
        for (x, line) in self.roof_columns() {
            let wr = self.wind_at(x as f32 + 0.5, line).abs();
            if wr > 0.75 && self.roof_snow[x as usize] > 0.3 && self.rng.f32() < (wr - 0.75) * 2.0 * dt {
                self.roof_snow[x as usize] -= 0.02;
                let mut g = self.grain(x as f32 + 0.5, line - self.roof_snow[x as usize] - 0.2, wr);
                g.vy -= 1.0;
                self.grains.push(g);
            }
        }

        let mut grains = std::mem::take(&mut self.grains);
        grains.retain_mut(|g| {
            g.life -= dt;
            let col = g.x.floor() as i32;
            let inside = col >= 0 && col < w as i32;
            let ci = col.clamp(0, w as i32 - 1) as usize;
            let surf = self.surface(ci);
            let near_ground = (1.0 - (surf - g.y) / 4.0).clamp(0.0, 1.0);
            let cover = 1.0 - (1.0 - self.shelter[ci]) * (1.0 - lee[ci]);
            let local = self.wind_at(g.x, g.y) * (1.0 - cover * near_ground);
            if g.clump {
                g.vx += (local * 3.0 - g.vx) * (dt * 1.5).min(1.0);
                g.vy += 9.0 * dt;
            } else {
                g.vx += (local * 14.0 - g.vx) * (dt * 4.0).min(1.0);
                g.vy += 7.0 * dt;
                if local.abs() > 0.6 {
                    g.vy += (self.rng.f32() - 0.56) * local.abs() * 22.0 * dt;
                }
                // spindrift rises a few rows at most
                if surf - g.y > 2.5 {
                    g.vy += 6.0 * dt;
                }
            }
            g.x += g.vx * dt;
            g.y += g.vy * dt;
            // the field goes on past the edges: what blows out comes in again
            if !g.clump {
                g.x = g.x.rem_euclid(wf);
            }
            if !inside {
                return g.x > -2.0 && g.x < wf + 2.0 && g.life > 0.0;
            }
            if self.cabin_hit(col, g.y) {
                let c = self.cabin.as_ref().unwrap();
                // piled against the wall, spread over a few columns
                let side = if g.vx > 0.0 { c.x0 - 1 - self.rng.i32(0..3) } else { c.x1() + 1 + self.rng.i32(0..3) };
                if (0..w as i32).contains(&side) {
                    self.snow[side as usize] += g.mass;
                }
                return false;
            }
            if self.roof_hit(col, g.y) && g.vy > 0.0 {
                self.roof_snow[ci] += g.mass * 0.6;
                return false;
            }
            if !g.clump {
                if let Some(i) = self.tree_hit(col, g.y) {
                    if self.rng.f32() < 0.1 {
                        self.trees[i].load = (self.trees[i].load + 0.003).min(1.0);
                        return false;
                    }
                }
            }
            if g.y >= surf {
                let wg = local.abs();
                if g.mass == 0.0 && g.life <= 0.0 {
                    return false;
                }
                if g.clump || g.life <= 0.0 || self.rng.f32() < (1.15 - wg / THR).clamp(0.04, 1.0) {
                    self.snow[ci] += g.mass;
                    return false;
                }
                g.y = surf - 0.05;
                g.vy = -(0.3 + self.rng.f32() * 1.5) * wg * 2.0;
                g.vx *= 0.7;
            }
            true
        });
        self.grains = grains;

        // --- the snow settles: no steeper than it can stand, and packs down
        for _ in 0..3 {
            for x in 0..w.saturating_sub(1) {
                let d = self.snow[x] - self.snow[x + 1];
                if d.abs() > 0.7 {
                    let m = (d.abs() - 0.7) * 0.25 * d.signum();
                    self.snow[x] -= m;
                    self.snow[x + 1] += m;
                }
            }
        }
        for x in 0..w {
            let s = &mut self.snow[x];
            if *s > 1.0 {
                *s -= (*s - 1.0) * 0.0015 * dt;
            }
            *s = s.clamp(0.3, self.max_depth);
        }
        // never over the windows
        if let Some(c) = &self.cabin {
            let cap = c.base - c.wall_top - 2.6;
            for x in (c.x0 - 1).max(0)..=(c.x1() + 1).min(w as i32 - 1) {
                self.snow[x as usize] = self.snow[x as usize].min(cap);
            }
        }
        for x in 0..w {
            self.roof_snow[x] = self.roof_snow[x].min(1.4);
            self.roof_snow[x] += 0.004 * self.snowfall * dt;
        }

        // --- clumps off the branches in a gust, and off the roof
        for i in 0..self.trees.len() {
            let (cx, base, height, half, load) = {
                let tr = &self.trees[i];
                (tr.cx, tr.base, tr.height, tr.half, tr.load)
            };
            let wl = self.wind_at(cx, base - height * 0.6).abs();
            if load > 0.35 && wl > 0.62 && self.rng.f32() < (wl - 0.58) * 0.5 * dt {
                let v = 0.3 + self.rng.f32() * 0.6;
                let x = cx + (self.rng.f32() - 0.5) * half * (1.0 - v) * 1.8;
                let y = base - height * v;
                let n = 8 + self.rng.usize(..10);
                self.clump(x, y, n, 0.012);
                self.trees[i].load -= 0.12;
                self.events.push(sound::Event::Thump { pan: x / wf, size: 0.5 + self.rng.f32() * 0.4 });
            }
        }
        if let Some((a, b, mid, wall_top)) = self.cabin.as_ref().map(|c| (c.roof_span().0, c.roof_span().1, c.x0 + c.w / 2, c.wall_top)) {
            for (lo, hi, eave) in [(a, mid - 1, a - 1), (mid, b, b + 1)] {
                let cols: Vec<usize> = (lo.max(0)..=hi.min(w as i32 - 1)).map(|x| x as usize).collect();
                if cols.is_empty() {
                    continue;
                }
                let mean = cols.iter().map(|&x| self.roof_snow[x]).sum::<f32>() / cols.len() as f32;
                let gust = self.wind_at(eave as f32, wall_top).abs();
                if mean > 0.9 && self.rng.f32() < (0.002 + (gust - 0.6).max(0.0) * 0.08) * dt {
                    let mut mass = 0.0;
                    for &x in &cols {
                        mass += self.roof_snow[x] - 0.15;
                        self.roof_snow[x] = 0.15;
                    }
                    let n = (mass / 0.03) as usize;
                    let y = wall_top - 0.3;
                    self.clump(eave as f32 + 0.5, y, n.clamp(10, 60), mass / n.max(1) as f32);
                    self.events.push(sound::Event::Thump { pan: eave as f32 / wf, size: 1.3 });
                }
            }
        }

        // --- chimney smoke
        if let Some(c) = &self.cabin {
            if self.rng.f32() < 5.0 * dt {
                self.puffs.push(Puff {
                    x: c.chimney as f32 + 0.5 + self.rng.f32(),
                    y: c.chimney_top - 0.2,
                    vx: 0.0,
                    age: 0.0,
                    life: 4.0 + self.rng.f32() * 3.0,
                });
            }
        }
        let mut puffs = std::mem::take(&mut self.puffs);
        puffs.retain_mut(|p| {
            p.age += dt;
            p.vx += (self.wind_at(p.x, p.y) * 7.0 - p.vx) * (dt * 1.5).min(1.0);
            p.x += (p.vx + (p.age * 1.7 + p.life).sin() * 0.4) * dt;
            p.y -= (1.1 / (1.0 + p.age * 0.4)) * dt;
            p.age < p.life && p.y > 0.0
        });
        self.puffs = puffs;

        // --- glints on the snow
        if self.rng.f32() < wf * 0.15 * dt {
            let x = self.rng.usize(..w);
            self.glints.push((x, 0.0));
        }
        self.glints.retain_mut(|g| {
            g.1 += dt;
            g.1 < 0.6
        });

        // --- the sleigh and the owl
        if self.sleigh.is_none() && t >= self.next_sleigh {
            let dir = if self.rng.bool() { 1.0 } else { -1.0 };
            let len = SLEIGH[1].chars().count() as f32;
            self.sleigh = Some(Sleigh {
                x: if dir > 0.0 { -len } else { wf },
                y: (self.moon.1 - 1.0 + self.rng.f32() * 2.5).max(0.0),
                dir,
                speed: 8.0 + self.rng.f32() * 4.0,
            });
            if t > WARMUP {
                log::info!("sleigh at {t:.1}s");
            }
            self.next_sleigh = t + 150.0 + self.rng.f32() * 150.0;
        }
        if let Some(s) = &mut self.sleigh {
            s.x += s.dir * s.speed * dt;
            if s.x < -30.0 || s.x > wf + 10.0 {
                self.sleigh = None;
            }
        }
        if t >= self.next_owl {
            if self.wind_at(wf / 2.0, self.base_y - 5.0).abs() > 0.6 {
                self.next_owl = t + 10.0;
            } else {
                let pan = self.rng.f32();
                self.events.push(sound::Event::Owl { pan });
                if t > WARMUP {
                    log::info!("owl at {t:.1}s pan {pan:.2}");
                }
                self.next_owl = t + 50.0 + self.rng.f32() * 130.0;
            }
        }
    }

    /// Each roof column and the top of the roof there.
    fn roof_columns(&self) -> Vec<(i32, f32)> {
        let Some(c) = &self.cabin else { return Vec::new() };
        let (a, b) = c.roof_span();
        (a.max(0)..=b.min(self.w as i32 - 1)).map(|x| (x, c.roof_line(x).unwrap())).collect()
    }

    fn grain(&mut self, x: f32, y: f32, wg: f32) -> Grain {
        Grain {
            x,
            y,
            vx: self.wind_at(x, y) * 8.0,
            vy: -(0.4 + self.rng.f32() * 1.8 * wg),
            mass: 0.02,
            life: 5.0 + self.rng.f32() * 4.0,
            clump: false,
            shade: 0.35 + self.rng.f32() * 0.5,
        }
    }

    fn clump(&mut self, x: f32, y: f32, n: usize, mass: f32) {
        self.clumps += 1;
        if self.t > WARMUP {
            log::debug!("snow falls at {x:.0},{y:.0} ({n} grains)");
        }
        for _ in 0..n {
            self.grains.push(Grain {
                x: x + (self.rng.f32() - 0.5) * 2.0,
                y: y + (self.rng.f32() - 0.5),
                vx: (self.rng.f32() - 0.5) * 2.0,
                vy: self.rng.f32() * 1.5,
                mass,
                life: 8.0,
                clump: true,
                shade: 1.0,
            });
        }
    }

    /// The wind the sound should play: the gusts as they reach the middle.
    fn sound_wind(&self) -> f32 {
        let wf = self.w as f32;
        let mut v = 0.0;
        for x in [wf * 0.25, wf * 0.5, wf * 0.75] {
            v += self.wind_at(x, self.base_y - 6.0).abs();
        }
        v / 3.0
    }

    fn sleigh_sound(&self) -> Option<(f32, f32)> {
        let s = self.sleigh.as_ref()?;
        let wf = self.w as f32;
        let mid = s.x + SLEIGH[1].chars().count() as f32 / 2.0;
        let pan = (mid / wf).clamp(0.0, 1.0);
        let vol = (1.0 - ((mid / wf) - 0.5).abs() * 1.6).clamp(0.0, 1.0).max(0.15);
        Some((pan, vol * 0.8))
    }

    // ------------------------------------------------------------ drawing

    fn visibility(&self) -> f32 {
        1.0 - 0.55 * self.snowfall
    }

    fn aurora_at(&self, x: f32, y: f32) -> Rgb {
        let amp = self.aurora_amp * self.visibility();
        if amp < 0.005 {
            return [0.0; 3];
        }
        let (a0, a1, p1, p2) = self.aurora;
        let t = self.t;
        let env = smooth((x - a0) / 14.0) * smooth((a1 - x) / 14.0) * (0.55 + 0.45 * noise1(self.seed ^ 3, x * 0.03 + t * 0.02));
        if env <= 0.0 {
            return [0.0; 3];
        }
        let hf = self.h as f32;
        let yb = hf * 0.3 + 2.5 * (x * 0.045 + t * 0.035 + p1).sin() + 1.4 * (x * 0.11 - t * 0.05 + p2).sin();
        let r = 0.5 + 0.5 * (x * 0.55 + 2.2 * (x * 0.07 + t * 0.09).sin() + t * 0.15).sin();
        let rays = 0.4 + 0.6 * r * r;
        let d = y - yb;
        let len = 3.5 + 4.5 * rays;
        let v = if d > 0.0 { (-d * 1.3).exp() } else { (d / len).exp() };
        let c = mix(AURORA_GREEN, AURORA_VIOLET, -d / len * 0.8 - 0.1);
        scale(c, amp * env * rays * v)
    }

    fn snow_color(&self, x: usize, depth: f32) -> Rgb {
        let w = self.w;
        let l = self.snow[x.saturating_sub(1)];
        let r = self.snow[(x + 1).min(w - 1)];
        // faces turned to the moon are lit, the others in blue shadow; the
        // crest of the snow catches the most light
        let moon_side = if self.moon.0 < w as f32 / 2.0 { 1.0 } else { -1.0 };
        let face = (1.0 - depth / 1.6).max(0.0);
        let lit = 0.8 + ((r - l) * moon_side * 0.45).clamp(-0.35, 0.22) * face + 0.12 * face;
        let base = mix(SNOW_LIT, SNOW_DEEP, 0.35 + (depth / 6.0).min(0.35));
        let c = scale(base, lit * (0.85 + 0.15 * self.visibility()));
        add(c, scale(AURORA_GREEN, self.aurora_amp * self.visibility() * 0.12))
    }

    fn window_light(&self) -> Option<(Vec<(f32, f32)>, f32)> {
        let c = self.cabin.as_ref()?;
        let flicker = 0.88 + 0.12 * noise1(self.seed ^ 5, self.t * 3.0);
        Some((c.windows.iter().map(|&(x, y)| (x as f32 + 2.0, y as f32 + 2.0)).collect(), flicker))
    }

    fn render(&self, f: &mut Frame) {
        let (w, h) = (self.w, self.h);
        let t = self.t;
        let vis = self.visibility();
        let (mx, my, mr) = self.moon;

        // sky, moon glow, aurora, haze
        for y in 0..h {
            let yf = y as f32 + 0.5;
            let grad = (yf / self.horizon).clamp(0.0, 1.0).powf(1.3);
            for x in 0..w {
                let xf = x as f32 + 0.5;
                let mut c = mix(SKY_TOP, SKY_LOW, grad);
                let d = (((xf - mx) / 2.0).powi(2) + (yf - my).powi(2)).sqrt();
                c = add(c, scale(MOON, 0.09 * vis * (-(d - mr).max(0.0) / 3.5).exp()));
                c = add(c, self.aurora_at(xf, yf));
                c = mix(c, HAZE, self.snowfall * 0.22 * grad);
                f.put(x, y, ' ', c, c);
            }
        }
        for s in &self.stars {
            if (s.y as f32 + 0.5) >= self.ridge[s.x] {
                continue;
            }
            let tw = 0.55 + 0.45 * (t * s.speed + s.phase).sin();
            let b = s.b * tw * vis;
            let bg = f.bg(s.x, s.y);
            f.fg(s.x, s.y, s.ch, mix(bg, s.col, b));
        }
        // the moon, round in half blocks
        let moon_col = mix(SKY_LOW, MOON, 0.55 + 0.45 * vis);
        for y in (my - mr - 1.0).max(0.0) as usize..((my + mr + 1.0) as usize).min(h) {
            for x in (mx - mr * 2.0 - 1.0).max(0.0) as usize..((mx + mr * 2.0 + 1.0) as usize).min(w) {
                let inside = |yy: f32| (((x as f32 + 0.5 - mx) / 2.0).powi(2) + (yy - my).powi(2)) <= mr * mr;
                let (top, bottom) = (inside(y as f32 + 0.25), inside(y as f32 + 0.75));
                let col = moon_col;
                match (top, bottom) {
                    (true, true) => f.put(x, y, '█', col, col),
                    (true, false) => f.fg(x, y, '▀', col),
                    (false, true) => f.fg(x, y, '▄', col),
                    _ => {}
                }
            }
        }
        // the sleigh, a shadow across the sky (and the moon)
        if let Some(s) = &self.sleigh {
            for (row, line) in SLEIGH.iter().enumerate() {
                let chars: Vec<char> = if s.dir > 0.0 { line.chars().collect() } else { line.chars().rev().map(mirror).collect() };
                let y = s.y as i32 + row as i32;
                for (i, &ch) in chars.iter().enumerate() {
                    let x = s.x as i32 + i as i32;
                    if ch == ' ' || x < 0 || x >= w as i32 || y < 0 || y >= h as i32 {
                        continue;
                    }
                    let (x, y) = (x as usize, y as usize);
                    if ch == '●' {
                        let glow = 0.7 + 0.3 * (t * 9.0).sin();
                        f.fg(x, y, '•', scale(NOSE, glow));
                    } else {
                        f.fg(x, y, ch, SILHOUETTE);
                    }
                }
            }
        }

        self.draw_flakes(f, Layer::Far);

        // mountains, then the far and the near forest lines
        let haze = 0.3 + self.snowfall * 0.35;
        for x in 0..w {
            let top = self.ridge[x];
            column(f, x, top, h, |depth| {
                let c = if depth < 1.4 { mix(MOUNTAIN_SNOW, MOUNTAIN, depth / 1.4) } else { MOUNTAIN };
                mix(c, SKY_LOW, haze)
            });
        }
        for x in 0..w {
            column(f, x, self.far_top[x], h, |depth| {
                let c = if depth < 0.6 { mix(scale(SNOW_DEEP, 0.45), FAR_FOREST, depth / 0.6) } else { FAR_FOREST };
                mix(c, HAZE, haze * 0.6)
            });
        }
        self.draw_flakes(f, Layer::Mid);
        for x in 0..w {
            column(f, x, self.mid_top[x], h, |depth| {
                let c = if depth < 0.7 { mix(scale(SNOW_DEEP, 0.5), MID_FOREST, depth / 0.7) } else { MID_FOREST };
                mix(c, HAZE, self.snowfall * 0.15)
            });
        }

        for tr in self.trees.iter().filter(|t| !t.decorated) {
            self.draw_tree(f, tr);
        }
        self.draw_cabin(f);
        for p in &self.puffs {
            let k = p.age / p.life;
            let r = 0.6 + p.age * 0.3;
            let a = 0.16 * (1.0 - k) * (1.0 - k);
            for y in (p.y - r).floor() as i32..=(p.y + r).ceil() as i32 {
                for x in (p.x - r * 2.0).floor() as i32..=(p.x + r * 2.0).ceil() as i32 {
                    if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                        continue;
                    }
                    let d2 = ((x as f32 + 0.5 - p.x) / 2.0).powi(2) + (y as f32 + 0.5 - p.y).powi(2);
                    let fall = (1.0 - d2 / (r * r)).max(0.0);
                    if fall > 0.0 {
                        f.tint(x as usize, y as usize, SMOKE, a * fall);
                    }
                }
            }
        }
        for tr in self.trees.iter().filter(|t| t.decorated) {
            self.draw_tree(f, tr);
        }

        // the snow on the ground
        let light = self.window_light();
        for x in 0..w {
            let s = self.surface(x);
            let top = s.floor().max(0.0) as usize;
            for y in top..h {
                let depth = (y as f32 + 0.5 - s).max(0.0);
                let mut c = scale(self.snow_color(x, depth), 0.97 + 0.05 * hash2(self.seed ^ 51, x as i32, y as i32));
                if let Some((wins, flicker)) = &light {
                    for &(wx, wy) in wins {
                        if (y as f32) >= wy {
                            let d2 = ((x as f32 + 0.5 - wx) / 2.2).powi(2) + (y as f32 + 0.5 - wy).powi(2);
                            c = add(c, scale(WINDOW, 0.28 * flicker * (-d2 / 7.0).exp()));
                        }
                    }
                }
                if y == top {
                    let k = ((y as f32 + 1.0 - s) * 8.0).round().clamp(0.0, 8.0) as usize;
                    let bg = f.bg(x, y);
                    if k == 8 {
                        f.put(x, y, ' ', c, c);
                    } else if k > 0 {
                        f.put(x, y, LOWER[k], c, bg);
                    }
                } else {
                    f.put(x, y, ' ', c, c);
                }
            }
        }
        for &(x, age) in &self.glints {
            let y = self.surface(x).floor() as usize + 1;
            if y < h {
                let b = (1.0 - (age / 0.6 - 0.5).abs() * 2.0).max(0.0);
                let bg = f.bg(x, y);
                f.fg(x, y, if b > 0.8 { '✧' } else { '·' }, mix(bg, [1.0, 1.0, 1.0], b));
            }
        }

        // drifting snow and clumps, then the near flakes
        for g in &self.grains {
            let (x, y) = (g.x.floor() as i32, g.y.floor() as i32);
            if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                continue;
            }
            let (x, y) = (x as usize, y as usize);
            let bg = f.bg(x, y);
            if g.clump {
                f.fg(x, y, '•', FLAKE);
            } else {
                let low = self.surface(x) - g.y < 1.5;
                let ch = if low && g.vx.abs() > 9.0 && g.shade > 0.6 { '-' } else { '·' };
                f.fg(x, y, ch, mix(bg, FLAKE, g.shade * if low { 1.0 } else { 0.8 }));
            }
        }
        self.draw_flakes(f, Layer::Near);
    }

    fn draw_flakes(&self, f: &mut Frame, layer: Layer) {
        let k = match layer {
            Layer::Far => 0.38,
            Layer::Mid => 0.62,
            Layer::Near => 0.95,
        };
        for fl in self.flakes.iter().filter(|fl| fl.layer == layer) {
            let (x, y) = (fl.x.floor() as i32, fl.y.floor() as i32);
            if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
                continue;
            }
            let (x, y) = (x as usize, y as usize);
            // a flake does not cut into the moon's edge or a snow cap
            if ('\u{2580}'..='\u{259F}').contains(&f.cells[y * f.w + x].ch) {
                continue;
            }
            let bg = f.bg(x, y);
            f.fg(x, y, fl.ch, mix(bg, FLAKE, k));
        }
    }

    fn draw_tree(&self, f: &mut Frame, tr: &Tree) {
        let t = self.t;
        let y0 = tr.top().floor().max(0.0) as i32;
        let y1 = (tr.base.ceil() as i32).min(self.h as i32 - 1);
        let x0 = (tr.left() - 2.0).floor().max(0.0) as i32;
        let x1 = ((tr.right() + 2.0).ceil() as i32).min(self.w as i32 - 1);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (ux, uy) = (x as usize, y as usize);
                match tr.cell(x, y) {
                    TreeCell::None => {}
                    TreeCell::Trunk => f.put(ux, uy, ' ', TRUNK, TRUNK),
                    TreeCell::Foliage => {
                        let shade = 0.7 + 0.3 * hash2(tr.seed, x, y) + 0.15 * (1.0 - (tr.base - y as f32) / tr.height);
                        let mut green = scale(if tr.decorated { FIR } else { SPRUCE }, shade);
                        if tr.decorated {
                            for &(lx, ly, li, ph) in &tr.lights {
                                if (lx - x).abs() == 1 && ly == y {
                                    let b = 0.5 + 0.5 * (t * 1.3 + ph).sin();
                                    green = add(green, scale(LIGHTS[li], 0.035 * b));
                                }
                            }
                        }
                        // snow lies on the tops of the tiers and along their edges
                        let exposed = tr.cell(x, y - 1) != TreeCell::Foliage;
                        let edge = tr.cell(x - 1, y) != TreeCell::Foliage || tr.cell(x + 1, y) != TreeCell::Foliage;
                        let need = if exposed { 0.15 } else if edge { 0.45 } else { 0.8 };
                        if tr.load > need + 0.5 * hash2(tr.seed ^ 1, x, y) {
                            f.put(ux, uy, '▀', scale(SNOW_LIT, 0.86 * (0.85 + 0.15 * self.visibility())), green);
                        } else {
                            f.put(ux, uy, ' ', green, green);
                        }
                    }
                }
            }
        }
        if tr.decorated {
            for &(lx, ly, li, ph) in &tr.lights {
                if tr.cell(lx, ly) != TreeCell::Foliage || lx < 0 || lx >= self.w as i32 || ly < 0 || ly >= self.h as i32 {
                    continue;
                }
                let b = 0.45 + 0.55 * (0.5 + 0.5 * (t * 1.3 + ph).sin());
                f.fg(lx as usize, ly as usize, '•', scale(LIGHTS[li], b));
            }
            let sx = (tr.cx + tr.sway).floor() as i32;
            let sy = tr.top().floor() as i32 - 1;
            if sx >= 0 && sx < self.w as i32 && sy >= 0 {
                let b = 0.85 + 0.15 * (t * 2.1).sin();
                f.fg(sx as usize, sy as usize, '★', scale(STAR_GOLD, b));
            }
        }
    }

    fn draw_cabin(&self, f: &mut Frame) {
        let Some(c) = &self.cabin else { return };
        let (w, h) = (self.w as i32, self.h as i32);
        let flicker = 0.88 + 0.12 * noise1(self.seed ^ 5, self.t * 3.0);
        let (a, b) = c.roof_span();
        let peak = c.wall_top - c.roof_h - 2.0;
        let snow_top = scale(SNOW_LIT, 0.9 * (0.85 + 0.15 * self.visibility()));
        // what is at a point of the cabin, from the front
        let mat = |x: i32, y: f32| -> Option<Rgb> {
            let roof = c.roof_line(x);
            if (x == c.chimney || x == c.chimney + 1) && y >= c.chimney_top - 0.35 && roof.is_some_and(|r| y < r + 0.5) {
                if y < c.chimney_top {
                    return if self.roof_snow[x as usize] > 0.4 { Some(snow_top) } else { None };
                }
                return Some(STONE);
            }
            if let Some(line) = roof {
                let s = self.roof_snow[x as usize];
                if y >= line - s && y < line {
                    return Some(snow_top);
                }
                if y >= line && y < c.wall_top + 0.6 {
                    return Some(ROOF);
                }
            }
            if x >= c.x0 && x <= c.x1() && y >= c.wall_top && y < c.base {
                return Some(LOG);
            }
            None
        };
        for y in (peak.floor() as i32).max(0)..(c.base.ceil() as i32).min(h) {
            for x in a.max(0)..=b.min(w - 1) {
                let samples: Vec<Option<Rgb>> = (0..8).map(|i| mat(x, y as f32 + (i as f32 + 0.5) / 8.0)).collect();
                let top = samples[0];
                let bottom = samples[7];
                if top.is_none() && bottom.is_none() {
                    continue;
                }
                // the cell's top material and the one just under it (snow
                // over roof keeps its snow), split at the first change
                let first = samples.iter().position(|s| *s != top).unwrap_or(8);
                let bottom = if first < 8 { samples[first] } else { top };
                let k = 8 - first;
                let bg_here = f.bg(x as usize, y as usize);
                let top_c = top.unwrap_or(bg_here);
                match bottom {
                    // something above, the scene below: the underside of the eaves
                    None => f.put(x as usize, y as usize, LOWER[k], bg_here, top_c),
                    Some(bc) if k >= 8 || first == 0 => f.put(x as usize, y as usize, ' ', bc, bc),
                    Some(bc) => f.put(x as usize, y as usize, LOWER[k], bc, top_c),
                }
            }
        }
        // logs, door and windows on the wall
        let wall = (c.wall_top as i32).max(0)..(c.base.ceil() as i32).min(h);
        for y in wall.clone() {
            for x in c.x0.max(0)..=c.x1().min(w - 1) {
                let (ux, uy) = (x as usize, y as usize);
                let shade = 0.85 + 0.15 * hash2(self.seed ^ 31, x / 3, y);
                f.put(ux, uy, '▁', LOG_SEAM, scale(LOG, shade));
            }
        }
        for y in (c.wall_top as i32 + 1).max(0)..(c.base.ceil() as i32).min(h) {
            for x in c.door..c.door + 3 {
                if (0..w).contains(&x) {
                    let ch = if x == c.door + 2 && y == c.wall_top as i32 + 3 { '•' } else { ' ' };
                    f.put(x as usize, y as usize, ch, [0.6, 0.5, 0.2], DOOR);
                }
            }
        }
        if (0..w).contains(&(c.door + 1)) && c.wall_top as i32 + 1 < h {
            f.put((c.door + 1) as usize, (c.wall_top as i32 + 1) as usize, '❂', [0.15, 0.55, 0.2], DOOR);
        }
        for &(wx, wy) in &c.windows {
            for dy in 0..2 {
                for dx in 0..4 {
                    let (x, y) = (wx + dx, wy + dy);
                    if !(0..w).contains(&x) || !(0..h).contains(&y) {
                        continue;
                    }
                    let glow = scale(WINDOW, flicker * (0.85 + 0.15 * hash2(self.seed ^ 41, x, y)));
                    // the frame's middle bar, half in each of the two middle cells
                    let ch = match dx {
                        1 => '▕',
                        2 => '▏',
                        _ => ' ',
                    };
                    f.put(x as usize, y as usize, ch, LOG_SEAM, glow);
                }
            }
        }
    }
}

/// Fills a column from `top` (rows, fractional) to the bottom, the top
/// cell as a partial block over what is already there.
fn column(f: &mut Frame, x: usize, top: f32, h: usize, color: impl Fn(f32) -> Rgb) {
    let first = top.floor().max(0.0) as usize;
    for y in first..h {
        let depth = (y as f32 + 0.5 - top).max(0.0);
        let c = color(depth);
        if y == first {
            let k = ((y as f32 + 1.0 - top) * 8.0).round().clamp(0.0, 8.0) as usize;
            let bg = f.bg(x, y);
            if k >= 8 {
                f.put(x, y, ' ', c, c);
            } else if k > 0 {
                f.put(x, y, LOWER[k], c, bg);
            }
        } else {
            f.put(x, y, ' ', c, c);
        }
    }
}

// ---------------------------------------------------------------- screen

#[derive(Clone, Copy, PartialEq)]
struct Cell {
    ch: char,
    fg: [u8; 3],
    bg: [u8; 3],
}

struct Frame {
    w: usize,
    h: usize,
    cells: Vec<Cell>,
    bgs: Vec<Rgb>,
}

fn to8(c: Rgb) -> [u8; 3] {
    let q = |v: f32| (v.clamp(0.0, 1.0).powf(1.0 / 1.15) * 255.0) as u8;
    [q(c[0]), q(c[1]), q(c[2])]
}

impl Frame {
    fn new(w: usize, h: usize) -> Frame {
        Frame { w, h, cells: vec![Cell { ch: ' ', fg: [0; 3], bg: [0; 3] }; w * h], bgs: vec![[0.0; 3]; w * h] }
    }
    fn put(&mut self, x: usize, y: usize, ch: char, fg: Rgb, bg: Rgb) {
        if x < self.w && y < self.h {
            let i = y * self.w + x;
            self.cells[i] = Cell { ch, fg: to8(fg), bg: to8(bg) };
            self.bgs[i] = bg;
        }
    }
    /// Draws a glyph over whatever background is already there.
    fn fg(&mut self, x: usize, y: usize, ch: char, fg: Rgb) {
        if x < self.w && y < self.h {
            let c = &mut self.cells[y * self.w + x];
            c.ch = ch;
            c.fg = to8(fg);
        }
    }
    /// Blends a colour into a cell, glyph and background both.
    fn tint(&mut self, x: usize, y: usize, col: Rgb, a: f32) {
        if x < self.w && y < self.h {
            let i = y * self.w + x;
            self.bgs[i] = mix(self.bgs[i], col, a);
            let c = &mut self.cells[i];
            c.bg = to8(self.bgs[i]);
            let fg = c.fg.map(|v| (v as f32 / 255.0).powf(1.15));
            c.fg = to8(mix(fg, col, a));
        }
    }
    fn bg(&self, x: usize, y: usize) -> Rgb {
        if x < self.w && y < self.h {
            self.bgs[y * self.w + x]
        } else {
            [0.0; 3]
        }
    }
}

struct Screen {
    out: BufWriter<io::Stdout>,
    prev: Option<Frame>,
    truecolor: bool,
}

fn ansi256(c: [u8; 3]) -> u8 {
    let [r, g, b] = c;
    if r.abs_diff(g) < 10 && g.abs_diff(b) < 10 {
        let v = (r as u16 + g as u16 + b as u16) / 3;
        if v < 8 {
            return 16;
        }
        if v > 238 {
            return 231;
        }
        return 232 + ((v - 8) / 10).min(23) as u8;
    }
    let s = |v: u8| if v < 48 { 0 } else if v < 115 { 1 } else { (v - 35) / 40 };
    16 + 36 * s(r) + 6 * s(g) + s(b)
}

impl Screen {
    fn color(&self, c: [u8; 3]) -> Color {
        if self.truecolor {
            Color::Rgb { r: c[0], g: c[1], b: c[2] }
        } else {
            Color::AnsiValue(ansi256(c))
        }
    }

    fn draw(&mut self, f: Frame) -> io::Result<()> {
        let mut cur_fg = None;
        let mut cur_bg = None;
        let mut at: Option<(usize, usize)> = None;
        for y in 0..f.h {
            for x in 0..f.w {
                let c = f.cells[y * f.w + x];
                if let Some(p) = &self.prev {
                    if p.cells[y * f.w + x] == c {
                        continue;
                    }
                }
                if at != Some((x, y)) {
                    queue!(self.out, cursor::MoveTo(x as u16, y as u16))?;
                }
                if cur_bg != Some(c.bg) {
                    let col = self.color(c.bg);
                    queue!(self.out, SetBackgroundColor(col))?;
                    cur_bg = Some(c.bg);
                }
                if c.ch != ' ' && cur_fg != Some(c.fg) {
                    let col = self.color(c.fg);
                    queue!(self.out, SetForegroundColor(col))?;
                    cur_fg = Some(c.fg);
                }
                queue!(self.out, Print(c.ch))?;
                at = Some((x + 1, y));
            }
        }
        self.out.flush()?;
        self.prev = Some(f);
        Ok(())
    }
}

// ---------------------------------------------------------------- main

fn usage() {
    println!("silent-night: a snowy forest at night, with wind, drifting snow and its sound\n");
    println!("usage: silent-night [-s SEED] [--mute] [--no-sound]\n");
    println!("  -s, --seed N        the same forest, the weather starting the same");
    println!("      --mute          start with the sound off (m turns it on)");
    println!("      --no-sound      no sound at all");
    println!("      --render-sound FILE SECONDS");
    println!("                      write the sound of SECONDS of the night to a WAV file");
    println!("  -h, --help          this text\n");
    println!("m turns the sound off and on; any other key quits.");
}

fn main() -> io::Result<()> {
    let mut seed: Option<u64> = None;
    let mut mute = false;
    let mut no_sound = false;
    let mut render: Option<(String, f32)> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-s" | "--seed" => match args.next().and_then(|v| v.parse().ok()) {
                Some(v) => seed = Some(v),
                None => {
                    eprintln!("silent-night: --seed needs a number");
                    std::process::exit(2);
                }
            },
            "--mute" => mute = true,
            "--no-sound" => no_sound = true,
            "--render-sound" => match (args.next(), args.next().and_then(|v| v.parse().ok())) {
                (Some(p), Some(s)) => render = Some((p, s)),
                _ => {
                    eprintln!("silent-night: --render-sound needs a file and a number of seconds");
                    std::process::exit(2);
                }
            },
            "-h" | "--help" => {
                usage();
                return Ok(());
            }
            other => {
                eprintln!("silent-night: unknown option {other}");
                usage();
                std::process::exit(2);
            }
        }
    }
    let seed = seed.unwrap_or_else(|| fastrand::u64(..));
    // Only writes when RUST_LOG is set: RUST_LOG=debug silent-night 2>silent-night.log
    env_logger::init();

    if let Some((path, secs)) = render {
        return render_sound(&path, secs, seed);
    }

    let stop = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    for sig in [signal_hook::consts::SIGTERM, signal_hook::consts::SIGHUP, signal_hook::consts::SIGINT] {
        signal_hook::flag::register(sig, Arc::clone(&stop))?;
    }

    let truecolor = std::env::var("COLORTERM").map(|v| v.contains("truecolor") || v.contains("24bit")).unwrap_or(false)
        || std::env::var_os("WT_SESSION").is_some()
        || cfg!(windows);

    // Give the terminal back even if something panics.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let mut out = io::stdout();
        let _ = crossterm::execute!(out, crossterm::style::ResetColor, cursor::Show, terminal::LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
        hook(info);
    }));

    let sound = if no_sound { sound::Sound::none() } else { sound::Sound::start(mute, seed) };

    terminal::enable_raw_mode()?;
    let mut screen = Screen { out: BufWriter::with_capacity(1 << 16, io::stdout()), prev: None, truecolor };
    queue!(screen.out, terminal::EnterAlternateScreen, cursor::Hide, terminal::Clear(terminal::ClearType::All))?;
    screen.out.flush()?;

    let result = run(&mut screen, seed, &stop, &sound);
    drop(sound);

    queue!(screen.out, crossterm::style::ResetColor, cursor::Show, terminal::LeaveAlternateScreen)?;
    screen.out.flush()?;
    terminal::disable_raw_mode()?;
    result
}

fn run(screen: &mut Screen, seed: u64, stop: &AtomicBool, sound: &sound::Sound) -> io::Result<()> {
    let (w, h) = terminal::size()?;
    log::info!("start {w}x{h} seed {seed} truecolor {}", screen.truecolor);
    let mut scene = Scene::new(w as usize, h as usize, seed);
    let mut next_snapshot = scene.t + 1.0;
    let frame_time = Duration::from_secs_f32(1.0 / FPS);
    let mut last = Instant::now();
    loop {
        if stop.load(Ordering::Relaxed) {
            log::info!("signal, quitting");
            return Ok(());
        }
        let now = Instant::now();
        let dt = (now - last).as_secs_f32().min(0.1);
        last = now;
        scene.update(dt);
        sound.update(scene.sound_wind(), scene.sleigh_sound(), std::mem::take(&mut scene.events));
        if scene.t >= next_snapshot {
            next_snapshot = scene.t + 1.0;
            let mean = scene.snow.iter().sum::<f32>() / scene.snow.len().max(1) as f32;
            let max = scene.snow.iter().cloned().fold(0.0, f32::max);
            log::debug!(
                "t {:.1} wind {:.2} gust {:.2} snowfall {:.2} aurora {:.2} flakes {} drift {} snow mean {:.2} max {:.2} clumps {} sound {}",
                scene.t,
                scene.wind_base,
                scene.gust_at(scene.t),
                scene.snowfall,
                scene.aurora_amp,
                scene.flakes.len(),
                scene.grains.len(),
                mean,
                max,
                scene.clumps,
                if sound.muted() { "muted" } else { sound.player.as_deref().unwrap_or("none") }
            );
        }
        let mut f = Frame::new(scene.w, scene.h);
        scene.render(&mut f);
        screen.draw(f)?;

        let deadline = now + frame_time;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            if !event::poll(left)? {
                break;
            }
            let ev = event::read()?;
            log::debug!("event {ev:?}");
            match ev {
                Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                    KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
                    KeyCode::Char('m' | 'M') => {
                        let on = sound.toggle();
                        log::info!("sound {}", if on { "on" } else { "off" });
                    }
                    _ => return Ok(()),
                },
                Event::Resize(nw, nh) => {
                    log::info!("resize {nw}x{nh}");
                    scene = Scene::new(nw as usize, nh as usize, seed);
                    next_snapshot = scene.t + 1.0;
                    screen.prev = None;
                    queue!(screen.out, crossterm::style::ResetColor, terminal::Clear(terminal::ClearType::All))?;
                }
                _ => {}
            }
        }
    }
}

/// Runs the night without a screen and writes what it sounds like, for
/// listening to the mix (16-bit stereo WAV).
fn render_sound(path: &str, secs: f32, seed: u64) -> io::Result<()> {
    let shared = Arc::new(std::sync::Mutex::new(sound::Shared::default()));
    let mut synth = sound::Synth::new(sound::RATE as f32, Arc::clone(&shared), seed);
    let mut scene = Scene::new(100, 30, seed);
    let per_frame = (sound::RATE as f32 / FPS) as usize;
    let mut buf = vec![[0.0f32; 2]; per_frame];
    let mut data: Vec<u8> = Vec::new();
    let frames = (secs * FPS) as usize;
    let mut peak = 0.0f32;
    for i in 0..frames {
        scene.update(1.0 / FPS);
        if i % (FPS as usize) == 0 {
            log::trace!("t {:.0} wind {:.2} drift {}", scene.t, scene.sound_wind(), scene.grains.iter().filter(|g| !g.clump).count());
        }
        if i % (30 * FPS as usize) == 0 {
            let bars: String = scene.snow.iter().map(|d| LOWER[((d / scene.max_depth) * 8.0).round().clamp(1.0, 8.0) as usize]).collect();
            log::debug!("snow at {:.0}s: {bars}", scene.t);
        }
        {
            let mut s = shared.lock().unwrap();
            s.wind = scene.sound_wind();
            s.sleigh = scene.sleigh_sound();
            s.events.extend(std::mem::take(&mut scene.events));
        }
        synth.render(&mut buf);
        for s in &buf {
            for c in s {
                peak = peak.max(c.abs());
                data.extend_from_slice(&((c.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
            }
        }
    }
    let mut out = BufWriter::new(std::fs::File::create(path)?);
    let len = data.len() as u32;
    out.write_all(b"RIFF")?;
    out.write_all(&(36 + len).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?;
    out.write_all(&2u16.to_le_bytes())?;
    out.write_all(&sound::RATE.to_le_bytes())?;
    out.write_all(&(sound::RATE * 4).to_le_bytes())?;
    out.write_all(&4u16.to_le_bytes())?;
    out.write_all(&16u16.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&len.to_le_bytes())?;
    out.write_all(&data)?;
    println!("{path}: {secs} s, peak {peak:.2}");
    Ok(())
}
