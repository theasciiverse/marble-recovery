//! Marble physics, hazards and game state. The collision is the CGmadness approach
//! (and the Neverball / Marble Blast family's): the marble is a sphere, the world is
//! triangles, push out along closest-point normals, bounce the normal velocity.

use crate::level::{Kind, Level, RouteStep};
use crate::math::{closest_on_tri, Rot, V3};
use log::info;

pub const RADIUS: f32 = 0.28;
const GRAVITY: f32 = 18.0;
/// Acceleration from full input, on the ground and in the air.
const PUSH: f32 = 6.0;
const AIR_PUSH: f32 = 2.5;
const ROLL_DRAG: f32 = 0.35;
const MAX_SPEED: f32 = 14.0;
/// Bounciness for impacts faster than `BOUNCE_MIN`; slower ones just stick.
const RESTITUTION: f32 = 0.35;
const BOUNCE_MIN: f32 = 2.5;
/// An impact this hard (normal speed) shatters the marble, as in Marble Madness.
pub const SHATTER_SPEED: f32 = 10.5;
pub const STEP: f32 = 1.0 / 240.0;

/// Slime: thick drag, weak push, and a marble left in it this long dissolves.
const SLIME_DRAG: f32 = 3.2;
const SLIME_PUSH: f32 = 0.6;
pub const SLIME_LIMIT: f32 = 1.5;
/// How fast the dissolve meter drains again on clean ground (per second).
const SLIME_RECOVER: f32 = 0.8;

pub const EATER_RADIUS: f32 = 0.34;
/// Bullies: iron marbles, heavier than yours, a little slower off the mark.
pub const BULLY_RADIUS: f32 = 0.31;
const BULLY_MASS: f32 = 2.5;
const BULLY_PUSH: f32 = 4.2;
/// How bouncy marble-on-marble knocks are.
const KNOCK_RESTITUTION: f32 = 0.7;
/// A bully that falls off comes back home after this long.
const BULLY_RETURN: f32 = 3.0;
/// Eaters leave a freshly respawned marble alone this long.
const GRACE: f32 = 1.5;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum State {
    Playing,
    /// Rolled off into the void; respawns when the timer runs out.
    Falling(f32),
    Shattered(f32),
    Dissolved(f32),
    Eaten(f32),
    Won,
    TimeUp,
}

pub struct Marble {
    pub pos: V3,
    pub vel: V3,
    pub rot: Rot,
    pub grounded: bool,
    /// Seconds of slime soaked up (dissolves at `SLIME_LIMIT`).
    pub slime: f32,
}

pub struct Particle {
    pub pos: V3,
    pub vel: V3,
    pub life: f32,
    pub slime: bool,
}

/// The marble-eater: a squat round thing that walks its patrol and, if it has a
/// hunting range, goes for a marble that comes close. It never steps off its level.
pub struct Eater {
    pub pos: V3,
    /// Unit direction it faces (mouth side), on the ground plane.
    pub facing: V3,
    path: Vec<(f32, f32)>,
    speed: f32,
    chase: f32,
    next: usize,
    dir: i32,
    /// After a meal it sits still and chews.
    pub full: f32,
    pub hunting: bool,
}

/// The bully: an iron marble that rolls straight at yours when it is near and
/// shoves it with real momentum. It does not care about edges, so it can be lured off.
pub struct Bully {
    pub ball: Marble,
    home: V3,
    chase: f32,
    /// Seconds until a fallen bully is back (0 = in play).
    pub out: f32,
    pub angry: bool,
}

impl Bully {
    fn at_home(home: V3) -> Marble {
        Marble { pos: home, vel: V3::ZERO, rot: Rot::ID, grounded: false, slime: 0.0 }
    }
}

pub struct Game {
    pub level: Level,
    pub marble: Marble,
    pub eaters: Vec<Eater>,
    pub bullies: Vec<Bully>,
    pub state: State,
    pub time_left: f32,
    pub elapsed: f32,
    /// Always runs (animation), even after the level ends.
    pub clock: f32,
    pub particles: Vec<Particle>,
    pub falls: u32,
    grace: f32,
    safe: (i32, i32),
    /// Cells inside a gulper's or bully's reach, never used as respawn points.
    menaced: Vec<bool>,
    rng: u32,
}

impl Game {
    pub fn new(level: Level) -> Game {
        let safe = (level.start.0 as i32, level.start.1 as i32);
        let time_left = level.time_limit;
        let eaters = level
            .eaters
            .iter()
            .map(|s| {
                let (x, y) = s.path[0];
                let z = level.ground(x, y).unwrap_or(0.0) + EATER_RADIUS;
                Eater {
                    pos: V3::new(x, y, z),
                    facing: V3::new(1.0, 0.0, 0.0),
                    path: s.path.clone(),
                    speed: s.speed,
                    chase: s.chase,
                    next: 1.min(s.path.len() - 1),
                    dir: 1,
                    full: 0.0,
                    hunting: false,
                }
            })
            .collect();
        let bullies = level
            .bullies
            .iter()
            .map(|s| {
                let (x, y) = s.home;
                let home = V3::new(x, y, level.ground(x, y).unwrap_or(0.0) + BULLY_RADIUS + 0.05);
                Bully { ball: Bully::at_home(home), home, chase: s.chase, out: 0.0, angry: false }
            })
            .collect();
        let menaced = menace_map(&level);
        let mut g = Game {
            menaced,
            level,
            marble: Marble { pos: V3::ZERO, vel: V3::ZERO, rot: Rot::ID, grounded: false, slime: 0.0 },
            eaters,
            bullies,
            state: State::Playing,
            time_left,
            elapsed: 0.0,
            clock: 0.0,
            particles: Vec::new(),
            falls: 0,
            grace: 0.0,
            safe,
            rng: 0x2545_f491,
        };
        g.respawn();
        g
    }

    /// A cell good to come back to after a loss: flat plain ground, no slime, tide or
    /// piston next to it, and out of every gulper's and bully's reach. The start always
    /// qualifies (there is nowhere further back).
    pub fn can_respawn_at(&self, x: i32, y: i32) -> bool {
        let Some(c) = self.level.cell(x, y) else { return false };
        if c.kind == Kind::Start {
            return true;
        }
        let calm_neighbours = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .iter()
            .all(|(dx, dy)| self.level.cell(x + dx, y + dy).is_some_and(|n| !matches!(n.kind, Kind::Slime | Kind::Tidal | Kind::Piston)));
        c.is_flat() && calm_neighbours && c.kind == Kind::Plain && !self.menaced[y as usize * self.level.w + x as usize]
    }

    fn respawn(&mut self) {
        let (x, y) = self.safe;
        let top = self.level.cell(x, y).map_or(0.0, |c| c.height_at(0.5, 0.5));
        self.marble.pos = V3::new(x as f32 + 0.5, y as f32 + 0.5, top + RADIUS + 0.4);
        self.marble.vel = V3::ZERO;
        self.marble.slime = 0.0;
        self.grace = GRACE;
        self.state = State::Playing;
        info!("respawn at cell {x},{y}");
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// Advance one fixed step. `push` is the wanted direction on the ground plane,
    /// length 0 to 1.
    pub fn step(&mut self, push: V3) {
        let dt = STEP;
        self.clock += dt;
        self.level.set_clock(self.clock);
        self.particles.retain_mut(|p| {
            p.vel.z -= GRAVITY * dt;
            p.pos += p.vel * dt;
            p.life -= dt;
            p.life > 0.0
        });
        self.move_eaters(dt);
        self.move_bullies(dt);
        match self.state {
            State::Won | State::TimeUp => return,
            State::Playing => {}
            s => {
                let t = match s {
                    State::Falling(t) | State::Shattered(t) | State::Dissolved(t) | State::Eaten(t) => t - dt,
                    _ => unreachable!(),
                };
                self.state = match s {
                    _ if t <= 0.0 => {
                        self.respawn();
                        State::Playing
                    }
                    State::Falling(_) => State::Falling(t),
                    State::Shattered(_) => State::Shattered(t),
                    State::Dissolved(_) => State::Dissolved(t),
                    _ => State::Eaten(t),
                };
                if let State::Falling(_) = self.state {
                    self.integrate(V3::ZERO, dt);
                }
                self.tick_clock(dt);
                return;
            }
        }
        self.grace = (self.grace - dt).max(0.0);
        let impact = self.integrate(push, dt);
        self.tick_clock(dt);
        let m = &self.marble;
        if !(m.pos.x.is_finite() && m.pos.y.is_finite() && m.pos.z.is_finite()) {
            log::error!("non-finite marble position {:?}, respawning", m.pos);
            self.respawn();
            return;
        }
        if impact > SHATTER_SPEED {
            info!("shattered: impact {impact:.1}");
            self.burst(30, false);
            self.falls += 1;
            self.state = State::Shattered(1.6);
            return;
        }
        if self.grace == 0.0 {
            let mp = self.marble.pos;
            if let Some(e) = self.eaters.iter_mut().find(|e| e.full == 0.0 && (e.pos - mp).len() < RADIUS + EATER_RADIUS * 0.85) {
                info!("eaten at {:.2},{:.2}", mp.x, mp.y);
                e.full = 2.2;
                e.facing = V3::new(mp.x - e.pos.x, mp.y - e.pos.y, 0.0).norm();
                self.falls += 1;
                self.state = State::Eaten(1.6);
                return;
            }
        }
        if self.marble.slime >= SLIME_LIMIT {
            info!("dissolved in slime");
            self.burst(24, true);
            self.falls += 1;
            self.state = State::Dissolved(1.6);
            return;
        }
        let m = &self.marble;
        let (cx, cy) = (m.pos.x.floor() as i32, m.pos.y.floor() as i32);
        if m.grounded {
            if self.level.cell(cx, cy).is_some_and(|c| c.kind == Kind::Goal) {
                info!("recovered in {:.2}s", self.elapsed);
                self.state = State::Won;
                return;
            }
            if self.can_respawn_at(cx, cy) {
                self.safe = (cx, cy);
            }
        }
        let floor = self.level.ground(m.pos.x, m.pos.y);
        if floor.is_none() && m.pos.z < -1.0 || m.pos.z < -3.0 {
            info!("fell off at {:.2},{:.2}", m.pos.x, m.pos.y);
            self.falls += 1;
            self.state = State::Falling(1.2);
        }
    }

    fn tick_clock(&mut self, dt: f32) {
        self.elapsed += dt;
        self.time_left -= dt;
        if self.time_left <= 0.0 {
            self.time_left = 0.0;
            info!("time up");
            self.state = State::TimeUp;
        }
    }

    fn burst(&mut self, n: usize, slime: bool) {
        let c = self.marble.pos;
        for _ in 0..n {
            let d = V3::new(self.rand(), self.rand(), self.rand().abs() + 0.3).norm();
            let speed = if slime { 1.0 + 1.5 * self.rand().abs() } else { 2.0 + 3.0 * self.rand().abs() };
            let p = Particle { pos: c + d * RADIUS, vel: d * speed, life: 0.8 + 0.6 * self.rand().abs(), slime };
            self.particles.push(p);
        }
    }

    fn move_eaters(&mut self, dt: f32) {
        let m = self.marble.pos;
        let marble_there = self.state == State::Playing && self.grace == 0.0;
        for e in &mut self.eaters {
            if e.full > 0.0 {
                e.full = (e.full - dt).max(0.0);
                continue;
            }
            let to_marble = V3::new(m.x - e.pos.x, m.y - e.pos.y, 0.0);
            e.hunting = marble_there && e.chase > 0.0 && to_marble.len() < e.chase && (m.z - e.pos.z).abs() < 0.8;
            let (tx, ty) = if e.hunting { (m.x, m.y) } else { e.path[e.next] };
            let to = V3::new(tx - e.pos.x, ty - e.pos.y, 0.0);
            let dist = to.len();
            if !e.hunting && dist < 0.05 {
                if e.path.len() > 1 {
                    let n = e.next as i32 + e.dir;
                    if n < 0 || n >= e.path.len() as i32 {
                        e.dir = -e.dir;
                    }
                    e.next = (e.next as i32 + e.dir) as usize;
                }
                continue;
            }
            let dirv = to * (1.0 / dist.max(1e-6));
            // turn toward where it is going, a little lazily
            e.facing = (e.facing + (dirv - e.facing) * (dt * 6.0).min(1.0)).norm();
            let speed = if e.hunting { e.speed * 1.25 } else { e.speed };
            let stepv = dirv * (speed * dt).min(dist);
            let (nx, ny) = (e.pos.x + stepv.x, e.pos.y + stepv.y);
            // an eater keeps its footing: no stepping off an edge or onto another level
            let here = self.level.ground(e.pos.x, e.pos.y).unwrap_or(e.pos.z - EATER_RADIUS);
            let footing = |x: f32, y: f32| self.level.ground(x, y).filter(|g| (g - here).abs() < 0.3);
            let probe = (nx + dirv.x * EATER_RADIUS, ny + dirv.y * EATER_RADIUS);
            if let (Some(g), Some(_)) = (footing(nx, ny), footing(probe.0, probe.1)) {
                e.pos = V3::new(nx, ny, g + EATER_RADIUS);
            }
        }
    }

    /// Returns the hardest impact (normal speed) this step.
    fn integrate(&mut self, push: V3, dt: f32) -> f32 {
        roll(&self.level, self.clock, &mut self.marble, RADIUS, push, PUSH, dt)
    }

    fn move_bullies(&mut self, dt: f32) {
        let marble_there = self.state == State::Playing && self.grace == 0.0;
        let m = self.marble.pos;
        for b in &mut self.bullies {
            if b.out > 0.0 {
                b.out -= dt;
                if b.out <= 0.0 {
                    b.out = 0.0;
                    b.ball = Bully::at_home(b.home);
                    info!("bully back home at {:.1},{:.1}", b.home.x, b.home.y);
                }
                continue;
            }
            let to_marble = V3::new(m.x - b.ball.pos.x, m.y - b.ball.pos.y, 0.0);
            b.angry = marble_there && to_marble.len() < b.chase && (m.z - b.ball.pos.z).abs() < 1.2;
            let push = if b.angry {
                to_marble.norm()
            } else {
                // drift home, braking as it gets there
                let to_home = V3::new(b.home.x - b.ball.pos.x, b.home.y - b.ball.pos.y, 0.0);
                let want = to_home.norm() * to_home.len().min(1.0) - V3::new(b.ball.vel.x, b.ball.vel.y, 0.0) * 0.6;
                if want.len() > 1.0 { want.norm() } else { want }
            };
            roll(&self.level, self.clock, &mut b.ball, BULLY_RADIUS, push, BULLY_PUSH, dt);
            let p = b.ball.pos;
            if self.level.ground(p.x, p.y).is_none() && p.z < -1.0 || p.z < -3.0 {
                info!("a bully fell off at {:.2},{:.2}", p.x, p.y);
                b.out = BULLY_RETURN;
            }
        }
        // knocks: bully on bully always, bully on marble while it is in play
        let n = self.bullies.len();
        for i in 0..n {
            for j in i + 1..n {
                let (a, rest) = self.bullies.split_at_mut(j);
                let (a, b) = (&mut a[i], &mut rest[0]);
                if a.out == 0.0 && b.out == 0.0 {
                    knock(&mut a.ball, BULLY_RADIUS, BULLY_MASS, &mut b.ball, BULLY_RADIUS, BULLY_MASS);
                }
            }
        }
        if self.state == State::Playing {
            for b in self.bullies.iter_mut().filter(|b| b.out == 0.0) {
                if knock(&mut self.marble, RADIUS, 1.0, &mut b.ball, BULLY_RADIUS, BULLY_MASS) {
                    log::debug!("knocked by a bully");
                }
            }
        }
    }

    /// Autopilot for tests and snapshots: steer toward `target` (world x, y) with a
    /// little velocity damping so it arrives instead of overshooting.
    pub fn steer(&self, target: (f32, f32)) -> V3 {
        let m = &self.marble;
        let to = V3::new(target.0 - m.pos.x, target.1 - m.pos.y, 0.0);
        let want = to.norm() * 1.6 - V3::new(m.vel.x, m.vel.y, 0.0) * 0.5;
        if want.len() > 1.0 { want.norm() } else { want }
    }

    /// Play one of the level's own routes with the autopilot for up to `secs`. Returns
    /// the time each step was completed.
    pub fn play_route(&mut self, which: usize, secs: f32, mut each: impl FnMut(&Game)) -> Vec<f32> {
        let mut pilot = Pilot::new(self, which);
        for _ in 0..(secs / STEP) as usize {
            if matches!(self.state, State::Won | State::TimeUp) {
                break;
            }
            let push = pilot.push(self);
            self.step(push);
            each(self);
        }
        pilot.hits
    }
}

/// The autopilot, one physics step at a time: follows a level's proof route (tests,
/// snapshots, and the title screen's attract mode).
pub struct Pilot {
    route: Vec<RouteStep>,
    next: usize,
    hold: (f32, f32),
    waited: f32,
    /// When each route step was completed.
    pub hits: Vec<f32>,
}

impl Pilot {
    pub fn new(game: &Game, which: usize) -> Pilot {
        Pilot {
            route: game.level.routes.get(which).cloned().unwrap_or_default(),
            next: 0,
            hold: (game.marble.pos.x, game.marble.pos.y),
            waited: 0.0,
            hits: Vec::new(),
        }
    }

    /// The push for this step.
    pub fn push(&mut self, game: &Game) -> V3 {
        if game.state != State::Playing {
            return V3::ZERO;
        }
        match self.route.get(self.next) {
            Some(&RouteStep::Go(tx, ty)) => {
                let m = &game.marble;
                if V3::new(tx - m.pos.x, ty - m.pos.y, 0.0).len() < 0.35 {
                    self.hits.push(game.elapsed);
                    self.next += 1;
                    self.hold = (tx, ty);
                }
                game.steer((tx, ty))
            }
            Some(&RouteStep::Wait(t)) => {
                self.waited += STEP;
                if self.waited >= t {
                    self.hits.push(game.elapsed);
                    self.next += 1;
                    self.waited = 0.0;
                }
                game.steer(self.hold)
            }
            None => V3::ZERO,
        }
    }
}

/// Which cells a gulper or bully can reach: within its hunting range (plus both
/// radii and a margin to get away) of anywhere on its patrol, or of a bully's home.
fn menace_map(level: &Level) -> Vec<bool> {
    const MARGIN: f32 = 0.9;
    let seg_dist = |p: (f32, f32), a: (f32, f32), b: (f32, f32)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len2 = dx * dx + dy * dy;
        let t = if len2 > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0) } else { 0.0 };
        ((p.0 - a.0 - dx * t).powi(2) + (p.1 - a.1 - dy * t).powi(2)).sqrt()
    };
    let mut out = vec![false; level.w * level.h];
    for y in 0..level.h {
        for x in 0..level.w {
            let p = (x as f32 + 0.5, y as f32 + 0.5);
            let by_gulper = level.eaters.iter().any(|e| {
                let reach = e.chase + EATER_RADIUS + RADIUS + MARGIN;
                let path = &e.path;
                let d = if path.len() == 1 {
                    seg_dist(p, path[0], path[0])
                } else {
                    path.windows(2).map(|w| seg_dist(p, w[0], w[1])).fold(f32::MAX, f32::min)
                };
                d < reach
            });
            let by_bully = level.bullies.iter().any(|b| seg_dist(p, b.home, b.home) < b.chase + BULLY_RADIUS + RADIUS + MARGIN);
            out[y * level.w + x] = by_gulper || by_bully;
        }
    }
    out
}

/// Two balls colliding: separate them by mass and exchange momentum along the line
/// between their centres. Returns whether they touched.
fn knock(a: &mut Marble, ra: f32, ma: f32, b: &mut Marble, rb: f32, mb: f32) -> bool {
    let d = a.pos - b.pos;
    let dist = d.len();
    if dist >= ra + rb || dist < 1e-6 {
        return false;
    }
    let n = d * (1.0 / dist);
    let overlap = ra + rb - dist;
    let (wa, wb) = (1.0 / ma, 1.0 / mb);
    a.pos += n * (overlap * wa / (wa + wb));
    b.pos -= n * (overlap * wb / (wa + wb));
    let vrel = (a.vel - b.vel).dot(n);
    if vrel < 0.0 {
        let j = -(1.0 + KNOCK_RESTITUTION) * vrel / (wa + wb);
        a.vel += n * (j * wa);
        b.vel -= n * (j * wb);
    }
    true
}

/// One physics step for a rolling ball (the marble or a bully) against the level.
/// Returns the hardest impact (normal speed) this step.
fn roll(level: &Level, clock: f32, m: &mut Marble, radius: f32, push: V3, push_accel: f32, dt: f32) -> f32 {
    let (cx, cy) = (m.pos.x.floor() as i32, m.pos.y.floor() as i32);
    let in_slime = m.grounded && level.slime_at(cx, cy, clock);
    let accel = if !m.grounded {
        AIR_PUSH * push_accel / PUSH
    } else if in_slime {
        push_accel * SLIME_PUSH
    } else {
        push_accel
    };
    m.vel += push * (accel * dt);
    m.vel.z -= GRAVITY * dt;
    let speed = m.vel.len();
    if speed > MAX_SPEED {
        m.vel = m.vel * (MAX_SPEED / speed);
    }
    m.pos += m.vel * dt;

    let mut impact = 0.0f32;
    let mut ground_n = V3::ZERO;
    // impacts are measured against the velocity from before this step's contacts:
    // on a seam the first contact's bounce would otherwise hide the second's hit
    let v_in = m.vel;
    let (cx, cy) = (m.pos.x.floor() as i32, m.pos.y.floor() as i32);
    for y in cy - 1..=cy + 1 {
        for x in cx - 1..=cx + 1 {
            for t in level.tris_of(x, y) {
                let q = closest_on_tri(m.pos, t.v[0], t.v[1], t.v[2]);
                let d = m.pos - q;
                let dist = d.len();
                if dist >= radius || dist < 1e-6 {
                    continue;
                }
                let n = d * (1.0 / dist);
                m.pos += n * (radius - dist);
                let vn = m.vel.dot(n);
                impact = impact.max(-v_in.dot(n));
                if vn < 0.0 {
                    let e = if -vn > BOUNCE_MIN { RESTITUTION } else { 0.0 };
                    m.vel -= n * (vn * (1.0 + e));
                }
                if n.z > 0.5 {
                    ground_n += n;
                }
            }
        }
    }
    m.grounded = ground_n.z > 0.0;
    if in_slime {
        m.slime += dt;
    } else {
        m.slime = (m.slime - SLIME_RECOVER * dt).max(0.0);
    }
    if m.grounded {
        let n = ground_n.norm();
        // a rolling solid sphere feels 5/7 of the slope's pull; give back the rest
        let g = V3::new(0.0, 0.0, -GRAVITY);
        let gt = g - n * g.dot(n);
        m.vel -= gt * (2.0 / 7.0 * dt);
        let vt = m.vel - n * m.vel.dot(n);
        let drag = if in_slime { SLIME_DRAG } else { ROLL_DRAG };
        m.vel -= vt * (drag * dt).min(1.0);
        let v = vt.len();
        if v > 1e-4 {
            m.rot.rotate(n.cross(vt).norm(), v * dt / radius);
        }
    }
    impact
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(extra: &str, grid: &str) -> Level {
        Level::load(&format!("name: test\ntime: 30\n{extra}\ngrid:\n{grid}")).unwrap()
    }

    fn run(g: &mut Game, secs: f32, push: V3) {
        for _ in 0..(secs / STEP) as usize {
            g.step(push);
        }
    }

    #[test]
    fn a_marble_left_in_slime_dissolves() {
        let mut g = Game::new(level("", "4S~ 4~ 4G"));
        run(&mut g, SLIME_LIMIT + 0.6, V3::ZERO);
        assert!(matches!(g.state, State::Dissolved(_)), "{:?}", g.state);
        assert_eq!(g.falls, 1);
    }

    #[test]
    fn slime_drags_hard() {
        let clean = |grid| {
            let mut g = Game::new(level("", grid));
            run(&mut g, 2.0, V3::new(1.0, 0.0, 0.0));
            g.marble.pos.x
        };
        let dry = clean("4S 4 4 4 4 4 4 4 4G");
        let wet = clean("4S 4~ 4~ 4~ 4~ 4~ 4~ 4~ 4G");
        assert!(wet < dry - 0.3, "slime {wet:.2} vs clean {dry:.2}");
    }

    #[test]
    fn rolling_out_of_slime_in_time_is_survivable() {
        // a short crossing soaks some, then drains on clean ground
        let mut g = Game::new(level("", "4S 4~ 4 4 4 4 4 4 4G"));
        run(&mut g, 3.0, V3::new(1.0, 0.0, 0.0));
        assert_eq!(g.falls, 0, "{:?}", g.state);
    }

    #[test]
    fn a_gulper_in_the_way_eats_the_marble() {
        let mut g = Game::new(level("eater: 4,0", "4S 4 4 4 4 4 4 4G"));
        run(&mut g, 4.0, V3::new(1.0, 0.0, 0.0));
        assert!(g.falls >= 1, "never eaten: {:?} at {:?}", g.state, g.marble.pos);
    }

    #[test]
    fn a_hunting_gulper_comes_for_a_still_marble() {
        let mut g = Game::new(level("eater: 4,0 chase 4.5", "4S 4 4 4 4 4 4G"));
        let start = g.eaters[0].pos.x;
        // grace runs out, then the gulper closes in on a marble that just sits there
        run(&mut g, 2.0, V3::ZERO);
        assert!(g.eaters[0].pos.x < start - 0.3, "gulper did not move: {:?}", g.eaters[0].pos);
        run(&mut g, 4.0, V3::ZERO);
        assert!(g.falls >= 1, "{:?}", g.state);
    }

    #[test]
    fn gulpers_never_step_off_their_level() {
        // a drop between it and the marble: it stops at the ledge
        let mut g = Game::new(level("eater: 4,0 chase 6", "2S 2 2 4 4 4 4G"));
        run(&mut g, 6.0, V3::ZERO);
        let e = &g.eaters[0];
        assert!(e.pos.x > 3.0 + EATER_RADIUS * 0.5, "left its level: {:?}", e.pos);
        assert_eq!(g.falls, 0);
    }

    #[test]
    fn a_long_fall_shatters_and_a_short_one_does_not() {
        // dropping off a ledge 4 units high onto the floor below
        let mut g = Game::new(level("", "CS C 4 4 4 4G"));
        run(&mut g, 3.0, V3::new(1.0, 0.0, 0.0));
        assert!(g.falls >= 1, "survived a 4 unit drop: {:?}", g.state);
        let mut g = Game::new(level("", "6S 6 4 4 4 4 4 4 4 4G"));
        for _ in 0..(3.0 / STEP) as usize {
            g.step(V3::new(1.0, 0.0, 0.0));
            assert!(!matches!(g.state, State::Shattered(_)), "a 1 unit drop shattered");
        }
    }

    #[test]
    fn the_tide_comes_and_goes_as_a_wave() {
        let l = level("tide: 4 0.5 1", "4S 4% 4% 4G");
        assert!(l.slime_at(1, 0, 1.5) && !l.slime_at(1, 0, 3.5));
        // the next cell along is one second behind
        assert!(!l.slime_at(2, 0, 1.5) && l.slime_at(2, 0, 2.5));
        assert!(!l.slime_at(0, 0, 1.5), "plain ground is never slime");
    }

    #[test]
    fn tidal_slime_only_dissolves_while_wet() {
        // wet a tenth of the time: never long enough to dissolve
        let mut g = Game::new(level("tide: 6 0.1 0", "4S% 4 4G"));
        run(&mut g, 5.0, V3::ZERO);
        assert_eq!(g.falls, 0, "{:?}", g.state);
        // wet most of the time: it gets you
        let mut g = Game::new(level("tide: 6 0.6 0", "4S% 4 4G"));
        run(&mut g, 3.0, V3::ZERO);
        assert!(matches!(g.state, State::Dissolved(_)), "{:?}", g.state);
    }

    #[test]
    fn a_bully_shoves_a_still_marble_off_the_edge() {
        let mut g = Game::new(level("bully: 4,0 chase 5", "4S 4 4 4 4 4G"));
        run(&mut g, 6.0, V3::ZERO);
        assert!(g.falls >= 1, "never shoved off: {:?} at {:?}", g.state, g.marble.pos);
    }

    #[test]
    fn a_heavy_bully_wins_a_head_on_knock() {
        let mut a = Marble { pos: V3::new(0.0, 0.0, 1.0), vel: V3::new(2.0, 0.0, 0.0), rot: Rot::ID, grounded: true, slime: 0.0 };
        let mut b = Marble { pos: V3::new(0.55, 0.0, 1.0), vel: V3::new(-2.0, 0.0, 0.0), ..a };
        b.rot = Rot::ID;
        assert!(knock(&mut a, RADIUS, 1.0, &mut b, BULLY_RADIUS, BULLY_MASS));
        assert!(a.vel.x < -2.0, "marble should be thrown back hard: {:?}", a.vel);
        assert!(b.vel.x > -1.0 && b.vel.x < 0.5, "bully mostly stopped: {:?}", b.vel);
    }

    #[test]
    fn a_bully_chasing_over_a_gap_falls_in_and_comes_back() {
        let mut g = Game::new(level("bully: 4,0 chase 6", "4S 4 . 4 4 4G"));
        let mut fell = false;
        for _ in 0..(4.0 / STEP) as usize {
            g.step(V3::ZERO);
            fell |= g.bullies[0].out > 0.0;
        }
        assert!(fell, "bully never went over: {:?}", g.bullies[0].ball.pos);
        assert_eq!(g.falls, 0);
        // run until it is back, then it must be at home, not where it fell
        let mut back = false;
        for _ in 0..((BULLY_RETURN + 1.0) / STEP) as usize {
            let was_out = g.bullies[0].out > 0.0;
            g.step(V3::ZERO);
            if was_out && g.bullies[0].out == 0.0 {
                let p = g.bullies[0].ball.pos;
                assert!((p.x - 4.5).abs() < 0.1 && (p.y - 0.5).abs() < 0.1, "returned to {p:?}");
                back = true;
                break;
            }
        }
        assert!(back, "never came back");
    }

    #[test]
    fn pistons_rise_and_fall_on_their_timing() {
        let mut l = level("piston: 4 1.5 1", "4S 4| 4| 4G");
        // the cell at x + y = 1 runs one second behind clock zero
        l.set_clock(1.0);
        assert!((l.ground(1.5, 0.5).unwrap() - 2.0).abs() < 1e-4, "at rest");
        l.set_clock(3.0);
        assert!((l.ground(1.5, 0.5).unwrap() - 3.5).abs() < 1e-4, "top half a period later");
        // the next piston along is a further second behind: a quarter of the way up
        assert!((l.ground(2.5, 0.5).unwrap() - 2.75).abs() < 1e-3);
        assert!((l.ground(0.5, 0.5).unwrap() - 2.0).abs() < 1e-4, "plain ground stays put");
    }

    #[test]
    fn a_marble_rides_a_piston_up_without_shattering() {
        let mut g = Game::new(level("piston: 4 1.5 0", "4S| 4 4G"));
        // settle (it spawns a little above the surface and drops in)
        run(&mut g, 0.3, V3::ZERO);
        let low = g.marble.pos.z;
        run(&mut g, 1.7, V3::ZERO);
        assert!(g.marble.pos.z > low + 1.3, "carried up: {low:.2} -> {:.2}", g.marble.pos.z);
        run(&mut g, 2.0, V3::ZERO);
        assert_eq!(g.falls, 0, "riding a full cycle is safe: {:?}", g.state);
        assert!(g.marble.pos.z < low + 0.3, "and back down: {:.2}", g.marble.pos.z);
    }

    fn finite(v: V3) -> bool {
        v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
    }

    #[test]
    fn random_play_never_breaks_the_simulation() {
        for i in 0..crate::levels::LEVELS.len() {
            for seed in 1..=3u32 {
                let mut g = Game::new(crate::levels::load(i));
                let mut rng = seed.wrapping_mul(2_654_435_761);
                let mut push = V3::ZERO;
                let mut lost_for = 0.0f32;
                for step in 0..(120.0 / STEP) as usize {
                    if step % 60 == 0 {
                        rng ^= rng << 13;
                        rng ^= rng >> 17;
                        rng ^= rng << 5;
                        let a = (rng % 6283) as f32 / 1000.0;
                        let k = ((rng >> 16) % 100) as f32 / 100.0;
                        push = V3::new(a.cos() * k, a.sin() * k, 0.0);
                    }
                    g.step(push);
                    if matches!(g.state, State::Won | State::TimeUp) {
                        break;
                    }
                    assert!(finite(g.marble.pos) && finite(g.marble.vel), "level {i} seed {seed}: marble {:?}", g.marble.pos);
                    assert!(g.eaters.iter().all(|e| finite(e.pos)), "level {i}: gulper went non-finite");
                    assert!(g.bullies.iter().all(|b| finite(b.ball.pos)), "level {i}: bully went non-finite");
                    assert!(g.particles.len() < 500, "level {i}: particles pile up");
                    lost_for = if g.state == State::Playing { 0.0 } else { lost_for + STEP };
                    assert!(lost_for < 2.0, "level {i} seed {seed}: stuck in {:?}", g.state);
                }
            }
        }
    }

    #[test]
    fn every_respawn_point_is_safe_to_sit_on() {
        // any cell the game may pick as a respawn point must survive 6 s untouched,
        // or a player can be caught in a lose-respawn-lose loop
        let mut bad = Vec::new();
        for i in 0..crate::levels::LEVELS.len() {
            let base = Game::new(crate::levels::load(i));
            let mut spots = Vec::new();
            for y in 0..base.level.h as i32 {
                for x in 0..base.level.w as i32 {
                    if base.can_respawn_at(x, y) {
                        spots.push((x, y));
                    }
                }
            }
            for (x, y) in spots {
                // the hazards are wherever they are at some point mid-level
                for warm in [0.0f32, 7.3, 15.1] {
                    let mut g = Game::new(crate::levels::load(i));
                    for _ in 0..(warm / STEP) as usize {
                        g.clock += STEP;
                        g.level.set_clock(g.clock);
                        g.move_eaters(STEP);
                        g.move_bullies(STEP);
                    }
                    g.safe = (x, y);
                    g.respawn();
                    run(&mut g, 6.0, V3::ZERO);
                    if g.falls > 0 {
                        bad.push(format!("{} cell {x},{y} after {warm}s: {:?}", crate::levels::LEVELS[i].0, g.state));
                        break;
                    }
                }
            }
        }
        assert!(bad.is_empty(), "unsafe respawn points:\n{}", bad.join("\n"));
    }
}
