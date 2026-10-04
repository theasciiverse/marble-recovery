//! Two layers fused per cell:
//! - the world, rasterized with a depth buffer into half-block pixels (`▀`, fg = top
//!   pixel, bg = bottom), about square, full colour each;
//! - round things (the marble, gulpers, shards) in a Braille dot layer (2x4 per cell, so
//!   2x2 per world pixel). Each dot is ray-tested against a real sphere, so they shade,
//!   roll, and hide behind walls and each other.
//!
//! Projection (pixel space, `s` = zoom): px = (x - y)·2s, py = (x + y)·s - z·2s.
//! The view direction is then (1, 1, 1), so depth (larger = nearer) is x + y + z.

use crate::game::{Eater, Game, Marble, State, BULLY_RADIUS, EATER_RADIUS, RADIUS, SLIME_LIMIT};
use crate::level::{Face, Kind, Tide};
use crate::math::V3;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

type Rgb = [f32; 3];

const BG: Rgb = [6.0, 7.0, 18.0];
const TOP: Rgb = [196.0, 198.0, 206.0];
const GRID: Rgb = [120.0, 122.0, 136.0];
const WALL_X: Rgb = [28.0, 86.0, 190.0];
const WALL_Y: Rgb = [70.0, 158.0, 236.0];
const MARBLE: Rgb = [70.0, 130.0, 255.0];
const MARBLE_DARK: Rgb = [14.0, 26.0, 70.0];
const SLIME: Rgb = [70.0, 200.0, 40.0];
const SLIME_DEEP: Rgb = [20.0, 110.0, 30.0];
const GULPER: Rgb = [190.0, 50.0, 120.0];
const GULPER_DARK: Rgb = [45.0, 0.0, 20.0];
const SILT: Rgb = [112.0, 118.0, 84.0];
const IRON: Rgb = [92.0, 96.0, 108.0];
const BULLY_EYE: Rgb = [255.0, 60.0, 30.0];
const XRAY: Rgb = [120.0, 190.0, 255.0];

fn light() -> V3 {
    // from the upper left, a little toward the viewer, so the visible faces are lit
    V3::new(0.15, 1.0, 1.6).norm()
}

#[derive(Clone, Copy)]
struct Px {
    col: Rgb,
    depth: f32,
    world: V3,
    top: bool,
}

pub struct View {
    pub scale: f32,
    pub cam: (f32, f32),
}

impl View {
    pub fn new() -> View {
        View { scale: 6.0, cam: (0.0, 0.0) }
    }

    pub fn proj(&self, p: V3) -> (f32, f32) {
        let s = self.scale;
        ((p.x - p.y) * 2.0 * s, (p.x + p.y) * s - p.z * 2.0 * s)
    }

    /// Ease the camera toward the marble.
    pub fn follow(&mut self, target: V3, dt: f32, snap: bool) {
        let (tx, ty) = self.proj(target);
        let k = if snap { 1.0 } else { (dt * 5.0).min(1.0) };
        self.cam.0 += (tx - self.cam.0) * k;
        self.cam.1 += (ty - self.cam.1) * k;
    }

    /// Where the marble centre lands, in (fractional) terminal cells within `area`.
    pub fn marble_cell(&self, game: &Game, area: Rect) -> (f32, f32) {
        let (px, py) = self.proj(game.marble.pos);
        let (ox, oy) = self.origin(area);
        (area.x as f32 + px + ox, area.y as f32 + (py + oy) / 2.0)
    }

    fn origin(&self, area: Rect) -> (f32, f32) {
        (area.width as f32 / 2.0 - self.cam.0, area.height as f32 - self.cam.1)
    }

    /// Screen push (x right, y down; square units) to a world ground-plane direction.
    pub fn screen_to_world(&self, d: (f32, f32)) -> V3 {
        // invert px = (x - y)·2, py = (x + y)·1 at z = 0, then keep the input's length
        let (dx, dy) = d;
        let len = (dx * dx + dy * dy).sqrt();
        if len == 0.0 {
            return V3::ZERO;
        }
        let w = V3::new(dx / 2.0 + dy, dy - dx / 2.0, 0.0).norm();
        w * len
    }
}

fn shade(c: Rgb, k: f32) -> Rgb {
    [c[0] * k, c[1] * k, c[2] * k]
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn to_color(c: Rgb) -> Color {
    let q = |v: f32| v.clamp(0.0, 255.0) as u8;
    Color::Rgb(q(c[0]), q(c[1]), q(c[2]))
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn frac(v: f32) -> f32 {
    v - v.floor()
}

fn slime_ripple(w: V3, t: f32, l: f32) -> Rgb {
    // slow interfering ripples, so pools visibly crawl
    let r = (w.x * 5.0 + t * 2.1).sin() + (w.y * 4.3 - t * 1.7).sin() + ((w.x + w.y) * 3.1 + t * 2.9).sin();
    let k = smoothstep(-1.5, 2.5, r);
    shade(mix(SLIME_DEEP, SLIME, k), 0.75 + 0.25 * l)
}

fn surface_colour(face: Face, w: V3, t: f32, tide: &Tide) -> Rgb {
    match face {
        Face::Top { kind, normal } => {
            let l = 0.5 + 0.5 * normal.dot(light()).max(0.0);
            // thin grid lines alias into stair-steps at this resolution; a checker of
            // half tiles reads as the same grid and cannot
            let odd = ((w.x * 2.0 + 1e-3).floor() as i32 + (w.y * 2.0 + 1e-3).floor() as i32) & 1 == 1;
            let c = match kind {
                Kind::Goal => {
                    if odd { [30.0, 30.0, 30.0] } else { [240.0, 240.0, 240.0] }
                }
                Kind::Slime => return slime_ripple(w, t, l),
                Kind::Tidal => {
                    // the cell this pixel belongs to (nudged off shared edges)
                    let (cx, cy) = ((w.x - 1e-3).floor() as i32, (w.y - 1e-3).floor() as i32);
                    if tide.is_wet(cx, cy, t) {
                        return slime_ripple(w, t, l);
                    }
                    let silt = if odd { mix(SILT, GRID, 0.35) } else { SILT };
                    let warn = tide.warning(cx, cy, t);
                    if warn > 0.0 {
                        // bubbles: sparse, fast, growing in number as the slime comes up
                        let b = (w.x * 13.0 + t * 9.0).sin() * (w.y * 11.0 - t * 7.0).sin();
                        if b > 1.0 - 0.9 * warn {
                            return shade(SLIME, 0.8 + 0.2 * l);
                        }
                        return shade(mix(silt, SLIME_DEEP, warn * 0.6), l);
                    }
                    silt
                }
                Kind::Start => {
                    if odd { mix([176.0, 196.0, 220.0], GRID, 0.45) } else { [176.0, 196.0, 220.0] }
                }
                Kind::Plain => {
                    if odd { mix(TOP, GRID, 0.45) } else { TOP }
                }
                Kind::Piston => {
                    // steel plates with amber caution edges, so a moving block reads as one
                    let (fx, fy) = (frac(w.x), frac(w.y));
                    let edge = fx < 0.12 || fx > 0.88 || fy < 0.12 || fy > 0.88;
                    if edge { [230.0, 160.0, 30.0] } else if odd { [150.0, 162.0, 182.0] } else { [176.0, 188.0, 206.0] }
                }
            };
            shade(c, l)
        }
        Face::Wall { side } => {
            let (c, along) = if side == 0 { (WALL_X, w.y) } else { (WALL_Y, w.x) };
            let stripe = if frac(along * 4.0) < 0.5 { 1.0 } else { 0.82 };
            let depth = (1.0 + w.z / 10.0).clamp(0.35, 1.0);
            shade(c, stripe * depth)
        }
    }
}

/// One Braille dot of a round thing: its depth, colour, and whether it is drawn
/// (`lit`) or is a dark part of the body (band, mouth) that still covers the cell.
#[derive(Clone, Copy)]
struct Dot {
    depth: f32,
    col: Rgb,
    lit: bool,
}

struct DotLayer {
    cols: usize,
    rows: usize,
    dots: Vec<Option<Dot>>,
}

impl DotLayer {
    fn put(&mut self, dx: usize, dy: usize, d: Dot) {
        if dx >= self.cols * 2 || dy >= self.rows * 4 {
            return;
        }
        let slot = &mut self.dots[dy * self.cols * 2 + dx];
        if slot.is_none_or(|o| d.depth > o.depth) {
            *slot = Some(d);
        }
    }

    fn cell(&self, c: usize, r: usize) -> Option<DotCell> {
        let mut dc = DotCell::default();
        for dy in 0..4 {
            for dx in 0..2 {
                if let Some(d) = self.dots[(r * 4 + dy) * self.cols * 2 + c * 2 + dx] {
                    dc.set(dx, dy, d.col, d.lit);
                }
            }
        }
        (dc.covered > 0).then_some(dc)
    }
}

/// Render the whole game into `buf` within `area`, with the ghost of a best run if given.
pub fn draw(game: &Game, view: &View, ghost: Option<&Marble>, area: Rect, buf: &mut Buffer) {
    let (w, h) = (area.width as usize, area.height as usize * 2);
    if w == 0 || h == 0 {
        return;
    }
    let (ox, oy) = view.origin(area);
    let mut px = vec![Px { col: BG, depth: f32::NEG_INFINITY, world: V3::ZERO, top: false }; w * h];

    // 1. world, z-buffered
    for t in game.level.all_tris() {
        if let Face::Wall { side } = t.face {
            if side >= 2 {
                continue; // faces away from the camera
            }
        }
        let p: Vec<(f32, f32)> = t
            .v
            .iter()
            .map(|&v| {
                let (x, y) = view.proj(v);
                (x + ox, y + oy)
            })
            .collect();
        let d = [t.v[0].x + t.v[0].y + t.v[0].z, t.v[1].x + t.v[1].y + t.v[1].z, t.v[2].x + t.v[2].y + t.v[2].z];
        let area2 = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        if area2.abs() < 1e-6 {
            continue;
        }
        let minx = p.iter().map(|q| q.0).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
        let maxx = p.iter().map(|q| q.0).fold(f32::MIN, f32::max).ceil().min(w as f32) as usize;
        let miny = p.iter().map(|q| q.1).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
        let maxy = p.iter().map(|q| q.1).fold(f32::MIN, f32::max).ceil().min(h as f32) as usize;
        for y in miny..maxy {
            for x in minx..maxx {
                let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
                let e = |a: (f32, f32), b: (f32, f32)| (b.0 - a.0) * (cy - a.1) - (cx - a.0) * (b.1 - a.1);
                let w0 = e(p[1], p[2]) / area2;
                let w1 = e(p[2], p[0]) / area2;
                let w2 = 1.0 - w0 - w1;
                if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                    continue;
                }
                let depth = d[0] * w0 + d[1] * w1 + d[2] * w2;
                let cell = &mut px[y * w + x];
                if depth <= cell.depth {
                    continue;
                }
                let world = t.v[0] * w0 + t.v[1] * w1 + t.v[2] * w2;
                *cell = Px {
                    col: surface_colour(t.face, world, game.clock, &game.level.tide),
                    depth,
                    world,
                    top: matches!(t.face, Face::Top { .. }),
                };
            }
        }
    }

    // 2. shadows: darken the surface right under each round thing, softer the higher
    let m = &game.marble;
    let marble_shown = matches!(game.state, State::Playing | State::Won | State::TimeUp | State::Falling(_));
    let mut casters: Vec<(V3, f32)> = game.eaters.iter().map(|e| (e.pos, EATER_RADIUS)).collect();
    casters.extend(game.bullies.iter().filter(|b| b.out == 0.0).map(|b| (b.ball.pos, BULLY_RADIUS)));
    if marble_shown {
        casters.push((m.pos, RADIUS));
    }
    for (pos, r) in casters {
        let Some(gz) = game.level.ground(pos.x, pos.y).filter(|&gz| gz <= pos.z) else { continue };
        let hgt = (pos.z - r - gz).max(0.0);
        let rs = r * (0.95 + 0.35 * hgt);
        let strength = 0.6 / (1.0 + 0.7 * hgt);
        for c in px.iter_mut().filter(|c| c.top) {
            if (c.world.z - gz).abs() > 0.3 {
                continue;
            }
            let d = ((c.world.x - pos.x).powi(2) + (c.world.y - pos.y).powi(2)).sqrt();
            let k = strength * smoothstep(rs, rs * 0.5, d);
            c.col = shade(c.col, 1.0 - k);
        }
    }

    // 3. the dot layer
    let cols = area.width as usize;
    let rows = area.height as usize;
    let mut layer = DotLayer { cols, rows, dots: vec![None; cols * 2 * rows * 4] };
    let lt = light();
    let half = (lt + V3::new(1.0, 1.0, 1.0).norm()).norm();
    let lit = |base: Rgb, n: V3, gloss: f32| {
        let diff = 0.35 + 0.75 * n.dot(lt).max(0.0);
        let spec = n.dot(half).max(0.0).powi(24) * gloss;
        mix(shade(base, diff), [255.0, 255.0, 255.0], spec)
    };
    if marble_shown {
        let tint = (m.slime / SLIME_LIMIT).clamp(0.0, 1.0);
        let base = mix(MARBLE, SLIME, tint * 0.8);
        sphere(view, (ox, oy), &px, w, &mut layer, m.pos, RADIUS, true, |n| {
            let body = m.rot.to_body(n);
            let band = body.z.abs() < 0.2 || body.x.abs() > 0.9;
            (lit(base, n, 1.0), !band)
        });
    }
    if let Some(gm) = ghost {
        // pale and shadowless; a hair smaller, so the real marble wins where they overlap
        sphere(view, (ox, oy), &px, w, &mut layer, gm.pos, RADIUS * 0.98, false, |n| {
            let body = gm.rot.to_body(n);
            let band = body.z.abs() < 0.2;
            (lit([150.0, 160.0, 200.0], n, 0.4), !band)
        });
    }
    for e in &game.eaters {
        sphere(view, (ox, oy), &px, w, &mut layer, e.pos, EATER_RADIUS, false, |n| gulper_dot(e, n, game.clock, &lit));
    }
    for b in game.bullies.iter().filter(|b| b.out == 0.0 || b.ball.pos.z > -6.0) {
        let glow = if b.angry { 0.5 + 0.5 * (game.clock * 10.0).sin().abs() } else { 0.0 };
        sphere(view, (ox, oy), &px, w, &mut layer, b.ball.pos, BULLY_RADIUS, false, |n| {
            let body = b.ball.rot.to_body(n);
            if body.z.abs() < 0.16 {
                // the band: a dull seam at rest, a hot red glare when it is coming for you
                return (mix([40.0, 40.0, 46.0], BULLY_EYE, glow), true);
            }
            (lit(IRON, n, 1.6), true)
        });
    }
    for p in &game.particles {
        let (x, y) = view.proj(p.pos);
        let (dx, dy) = (((x + ox) * 2.0).floor(), ((y + oy) * 2.0).floor());
        if dx < 0.0 || dy < 0.0 {
            continue;
        }
        let col = if p.slime { SLIME } else { MARBLE };
        let d = Dot { depth: p.pos.x + p.pos.y + p.pos.z, col: shade(col, 0.6 + 0.6 * p.life.min(1.0)), lit: true };
        layer.put(dx as usize, dy as usize, d);
    }

    // 4. compose
    for r in 0..rows {
        for c in 0..cols {
            let top = px[(r * 2) * w + c].col;
            let bot = px[(r * 2 + 1) * w + c].col;
            let Some(cell) = buf.cell_mut((area.x + c as u16, area.y + r as u16)) else { continue };
            if let Some(dc) = layer.cell(c, r) {
                let (sym, fg, bg) = dc.resolve(top, bot);
                cell.set_char(sym).set_fg(to_color(fg)).set_bg(to_color(bg));
            } else {
                let (t, b) = (to_color(top), to_color(bot));
                if t == b {
                    cell.set_char(' ').set_bg(b);
                } else {
                    cell.set_char('▀').set_fg(t).set_bg(b);
                }
            }
        }
    }
}

/// A gulper's surface: a glossy body, a mouth on the facing side that chomps (fast
/// while hunting or chewing, a lazy idle otherwise), a rim of teeth, and two eyes.
fn gulper_dot(e: &Eater, n: V3, clock: f32, lit: &impl Fn(Rgb, V3, f32) -> Rgb) -> (Rgb, bool) {
    let up = V3::new(0.0, 0.0, 1.0);
    let f = e.facing;
    let side = f.cross(up).norm();
    let rate = if e.hunting || e.full > 0.0 { 9.0 } else { 3.0 };
    let open = 0.08 + 0.22 * (clock * rate).sin().abs();
    let front = n.dot(f);
    let vert = n.dot(up);
    if front > 0.35 && vert.abs() < open {
        return (GULPER_DARK, false);
    }
    if front > 0.35 && vert.abs() < open + 0.09 {
        return ([235.0, 235.0, 225.0], true);
    }
    for s in [-1.0, 1.0] {
        let eye = (f * 0.62 + up * 0.62 + side * (0.42 * s)).norm();
        let k = n.dot(eye);
        if k > 0.985 {
            return ([10.0, 10.0, 10.0], true);
        }
        if k > 0.955 {
            return ([250.0, 250.0, 240.0], true);
        }
    }
    (lit(GULPER, n, 0.6), true)
}

/// Ray-test every Braille dot near a sphere and put the visible ones in the layer.
/// `surface` gives each hit's colour and whether it is a lit dot, from its normal.
#[allow(clippy::too_many_arguments)]
fn sphere(view: &View, (ox, oy): (f32, f32), px: &[Px], w: usize, layer: &mut DotLayer, c: V3, r: f32, xray: bool, surface: impl Fn(V3) -> (Rgb, bool)) {
    let s = view.scale;
    let (cx, cy) = view.proj(c);
    let (cx, cy) = (cx + ox, cy + oy);
    let reach = r * 3.0 * s + 2.0;
    let u = V3::new(1.0, 1.0, 1.0);
    let uu = u.dot(u);
    let x0 = ((cx - reach) * 2.0).floor().max(0.0) as usize;
    let x1 = ((cx + reach) * 2.0).ceil().clamp(0.0, layer.cols as f32 * 2.0) as usize;
    let y0 = ((cy - reach) * 2.0).floor().max(0.0) as usize;
    let y1 = ((cy + reach) * 2.0).ceil().clamp(0.0, layer.rows as f32 * 4.0) as usize;
    for dy in y0..y1 {
        for dx in x0..x1 {
            // dot centre back into world space: the line of points that project here
            let sx = (dx as f32 + 0.5) / 2.0 - ox;
            let sy = (dy as f32 + 0.5) / 2.0 - oy;
            let (a, b) = (sx / (2.0 * s), sy / s);
            let p0 = V3::new((a + b) / 2.0, (b - a) / 2.0, 0.0);
            let rel = c - p0;
            let t = rel.dot(u) / uu;
            let off = rel - u * t;
            let d2 = off.dot(off);
            if d2 >= r * r {
                continue;
            }
            let t_front = t + (r * r - d2).sqrt() / uu.sqrt();
            let hit = p0 + u * t_front;
            let depth = hit.x + hit.y + hit.z;
            if depth < px[(dy / 2) * w + dx / 2].depth - 0.02 {
                // behind the world: hidden, except the outline of an x-rayed sphere, so a
                // marble behind a wall or a risen piston is never lost from view
                if xray && d2 > r * r * 0.55 {
                    layer.put(dx, dy, Dot { depth: f32::NEG_INFINITY, col: XRAY, lit: true });
                }
                continue;
            }
            let (col, lit) = surface((hit - c) * (1.0 / r));
            layer.put(dx, dy, Dot { depth, col, lit });
        }
    }
}

#[derive(Clone, Default)]
struct DotCell {
    /// Every dot something round occupies.
    bits: u8,
    covered: u8,
    covered_top: u8,
    /// Colour sums for the top and bottom half of the cell (dot rows 0-1, 2-3).
    sum: [Rgb; 2],
}

const BRAILLE_BIT: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

impl DotCell {
    fn set(&mut self, dx: usize, dy: usize, col: Rgb, lit: bool) {
        let bit = BRAILLE_BIT[dx][dy];
        if self.bits & bit != 0 {
            return;
        }
        self.bits |= bit;
        self.covered += 1;
        if dy < 2 {
            self.covered_top += 1;
        }
        let col = if lit { col } else { mix(col, MARBLE_DARK, 0.75) };
        let s = &mut self.sum[dy / 2];
        *s = [s[0] + col[0], s[1] + col[1], s[2] + col[2]];
    }

    /// One glyph, one fg, one bg. A cell that is fully covered becomes a half-block
    /// with each half shaded on its own (bands and mouths show as darker shading); a
    /// rim cell is Braille dots, for the sub-cell silhouette, over whichever world
    /// pixel is covered less.
    fn resolve(&self, top: Rgb, bot: Rgb) -> (char, Rgb, Rgb) {
        let avg = |s: Rgb, n: u8| {
            let n = n.max(1) as f32;
            [s[0] / n, s[1] / n, s[2] / n]
        };
        let n_bot = self.covered - self.covered_top;
        if self.covered >= 8 {
            return ('▀', avg(self.sum[0], self.covered_top), avg(self.sum[1], n_bot));
        }
        // a half that is (nearly) covered reads better as a solid half block than as
        // dots, which many fonts draw small and grey; only sparse rim cells stay Braille
        if self.covered_top >= 3 && n_bot <= 1 {
            return ('▀', avg(self.sum[0], self.covered_top), bot);
        }
        if n_bot >= 3 && self.covered_top <= 1 {
            return ('▄', avg(self.sum[1], n_bot), top);
        }
        let all = [self.sum[0][0] + self.sum[1][0], self.sum[0][1] + self.sum[1][1], self.sum[0][2] + self.sum[1][2]];
        let bg = if self.covered_top * 2 > self.covered { bot } else { top };
        (char::from_u32(0x2800 + self.bits as u32).unwrap_or(' '), avg(all, self.covered), bg)
    }
}
