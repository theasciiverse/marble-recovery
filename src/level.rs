//! Block-grid levels, the CGmadness model: every cell is one block whose top is a
//! plane (height plus a slope), with walls down to whatever is next to it.
//!
//! World axes: +x runs down-right on screen, +y down-left, +z up. One cell is one
//! unit square. Heights in the level text are in half units.
//!
//! Level files (`levels/*.lvl`) are plain text:
//!
//! ```text
//! name: Slime Terraces
//! time: 70
//! eater: 3,7 6,7 speed 1.2 chase 2.5    # patrol path through cell centres
//! bully: 5,5 chase 4                    # an iron marble that rolls at you
//! tide: 5 0.5 0.25                      # tidal slime: period, wet fraction, lag per cell
//! piston: 4 1.5 0.45                    # pistons: period, rise, lag per cell
//! gravity: 7                            # this world's pull (default 18)
//! spring: 9.5                           # launch speed of `!` cells (default 9.5)
//! theme: madness                        # palette: classic, madness, dawn
//! act: Act II: The Madness              # shown on the story card when an act begins
//! story: The first line of the card.    # one line each, shown before the level
//! ending: ...                           # lines for the card after the last level
//! route: 2,2 4,5 w2 6,5 ...             # a known-good line (w2 = wait 2 s); tests play all
//! grid:
//! .  KS K  K~ ...
//! ```
//!
//! Grid tokens: a height char (0-9, A-Z = 10-35 half units) then optional modifiers:
//! `>` `<` `v` `^` slope down half a unit toward +x, -x, +y, -y (`}` `{` `V` `A` a whole
//! unit), `S` start, `G` goal, `~` slime, `%` tidal slime (wet only part of each
//! tide, rolling across the level as a wave), `|` piston (rises and falls by the
//! `piston:` timing, from its written height up), `*` ice (almost no grip), `!`
//! spring (throws you up at the `spring:` speed). `.` is void. `#` starts a
//! comment (so story text cannot contain `#`).

use crate::math::V3;

/// Where a void cell's walls (and a fall) bottom out.
pub const FLOOR_Z: f32 = -8.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Plain,
    Start,
    Goal,
    Slime,
    Tidal,
    Piston,
    Ice,
    Spring,
}

/// A level's palette, by act: the arc from the classic look, through violet
/// madness, to a warm dawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Theme {
    Classic,
    Madness,
    Dawn,
}

/// Piston timing: each piston rises `rise` units above its written height and back
/// down, once per `period` seconds, a cell `lag` seconds behind the one before it
/// along x + y (so a row of them can climb like an escalator).
#[derive(Clone, Copy, Debug)]
pub struct PistonTiming {
    pub period: f32,
    pub rise: f32,
    pub lag: f32,
}

impl PistonTiming {
    /// How far above its rest height a piston at this cell is (eased, 0 to `rise`).
    pub fn lift(&self, x: i32, y: i32, clock: f32) -> f32 {
        let phase = (clock - (x + y) as f32 * self.lag).rem_euclid(self.period) / self.period;
        self.rise * 0.5 * (1.0 - (phase * std::f32::consts::TAU).cos())
    }
}

/// Tidal slime timing: each cell is wet for `wet` of every `period` seconds, a cell
/// `lag` seconds behind the one before it along x + y, so the tide rolls as a wave.
#[derive(Clone, Copy, Debug)]
pub struct Tide {
    pub period: f32,
    pub wet: f32,
    pub lag: f32,
}

/// How long before rising a tidal cell starts to bubble (the player's warning).
pub const TIDE_WARNING: f32 = 0.9;

impl Tide {
    /// Seconds into this cell's cycle; wet while under `wet * period`.
    pub fn phase(&self, x: i32, y: i32, clock: f32) -> f32 {
        (clock - (x + y) as f32 * self.lag).rem_euclid(self.period)
    }

    pub fn is_wet(&self, x: i32, y: i32, clock: f32) -> bool {
        self.phase(x, y, clock) < self.wet * self.period
    }

    /// 0 when dry and calm, rising to 1 as the slime is about to come up.
    pub fn warning(&self, x: i32, y: i32, clock: f32) -> f32 {
        let until = self.period - self.phase(x, y, clock);
        if self.is_wet(x, y, clock) || until > TIDE_WARNING { 0.0 } else { 1.0 - until / TIDE_WARNING }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    /// Top corner heights: [x0y0, x1y0, x0y1, x1y1].
    pub h: [f32; 4],
    pub kind: Kind,
}

impl Cell {
    pub fn height_at(&self, fx: f32, fy: f32) -> f32 {
        let a = self.h[0] + (self.h[1] - self.h[0]) * fx;
        let b = self.h[2] + (self.h[3] - self.h[2]) * fx;
        a + (b - a) * fy
    }

    pub fn is_flat(&self) -> bool {
        self.h.iter().all(|&h| (h - self.h[0]).abs() < 1e-4)
    }
}

/// A face for both the renderer and the physics.
#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub v: [V3; 3],
    pub face: Face,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Face {
    Top { kind: Kind, normal: V3 },
    /// Outward normal along one axis: 0 = +x, 1 = +y, 2 = -x, 3 = -y.
    Wall { side: u8 },
}

/// A marble-eater's patrol, in world coordinates (cell centres).
#[derive(Clone, Debug)]
pub struct EaterSpec {
    pub path: Vec<(f32, f32)>,
    pub speed: f32,
    /// Starts hunting the marble inside this distance (0 = never leaves its path).
    pub chase: f32,
}

/// A bully's home and how far it will come for the marble.
#[derive(Clone, Debug)]
pub struct BullySpec {
    pub home: (f32, f32),
    pub chase: f32,
}

pub struct Level {
    pub name: String,
    pub w: usize,
    pub h: usize,
    cells: Vec<Option<Cell>>,
    pub start: (usize, usize),
    pub time_limit: f32,
    pub eaters: Vec<EaterSpec>,
    pub bullies: Vec<BullySpec>,
    pub tide: Tide,
    pub piston: PistonTiming,
    pub gravity: f32,
    pub spring: f32,
    pub theme: Theme,
    /// The act this level opens, if it opens one (shown on its story card).
    pub act: Option<String>,
    pub story: Vec<String>,
    pub ending: Vec<String>,
    /// Piston cells and their rest corner heights.
    pistons: Vec<(usize, usize, [f32; 4])>,
    /// Lines of cell centres a steering autopilot follows to the goal, one per
    /// `route:` line (each a solvability proof; the tests play them all).
    pub routes: Vec<Vec<RouteStep>>,
    /// Triangles grouped by the cell that owns them, for local physics queries.
    tris: Vec<Vec<Tri>>,
}

/// One step of a proof route: roll to a cell centre, or hold position for a while
/// (`w2.5`), for timing a tide or letting a hazard pass.
#[derive(Clone, Copy, Debug)]
pub enum RouteStep {
    Go(f32, f32),
    Wait(f32),
}

fn route_step(tok: &str) -> Result<RouteStep, String> {
    if let Some(secs) = tok.strip_prefix('w') {
        return secs.parse().map(RouteStep::Wait).map_err(|e| format!("route wait {tok:?}: {e}"));
    }
    cell_point(tok).map(|(x, y)| RouteStep::Go(x, y))
}

fn cell_point(tok: &str) -> Result<(f32, f32), String> {
    let (a, b) = tok.split_once(',').ok_or_else(|| format!("expected x,y, got {tok:?}"))?;
    let p = |v: &str| v.trim().parse::<f32>().map_err(|e| format!("{tok:?}: {e}"));
    Ok((p(a)? + 0.5, p(b)? + 0.5))
}

impl Level {
    pub fn empty() -> Level {
        Level {
            name: String::new(),
            w: 0,
            h: 0,
            cells: Vec::new(),
            start: (0, 0),
            time_limit: 0.0,
            eaters: Vec::new(),
            bullies: Vec::new(),
            tide: Tide { period: 5.0, wet: 0.5, lag: 0.25 },
            piston: PistonTiming { period: 4.0, rise: 1.0, lag: 0.0 },
            gravity: 18.0,
            spring: 9.5,
            theme: Theme::Classic,
            act: None,
            story: Vec::new(),
            ending: Vec::new(),
            pistons: Vec::new(),
            routes: Vec::new(),
            tris: Vec::new(),
        }
    }

    pub fn load(text: &str) -> Result<Level, String> {
        let mut level = Level::empty();
        level.time_limit = 60.0;
        let mut rows: Vec<Vec<&str>> = Vec::new();
        let mut in_grid = false;
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            if in_grid {
                rows.push(line.split_whitespace().collect());
                continue;
            }
            let (key, val) = line.split_once(':').ok_or_else(|| format!("expected key: value, got {line:?}"))?;
            let val = val.trim();
            match key.trim() {
                "name" => level.name = val.to_string(),
                "time" => level.time_limit = val.parse().map_err(|e| format!("time: {e}"))?,
                "route" => level.routes.push(val.split_whitespace().map(route_step).collect::<Result<_, _>>()?),
                "eater" => {
                    let mut spec = EaterSpec { path: Vec::new(), speed: 1.2, chase: 0.0 };
                    let mut it = val.split_whitespace();
                    while let Some(tok) = it.next() {
                        let mut num = |name: &str| {
                            it.next().and_then(|v| v.parse::<f32>().ok()).ok_or_else(|| format!("eater: {name} needs a number"))
                        };
                        match tok {
                            "speed" => spec.speed = num("speed")?,
                            "chase" => spec.chase = num("chase")?,
                            _ => spec.path.push(cell_point(tok)?),
                        }
                    }
                    if spec.path.is_empty() {
                        return Err("eater without a path".into());
                    }
                    level.eaters.push(spec);
                }
                "bully" => {
                    let mut it = val.split_whitespace();
                    let home = cell_point(it.next().ok_or("bully: needs a home cell")?)?;
                    let chase = match (it.next(), it.next()) {
                        (Some("chase"), Some(v)) => v.parse().map_err(|e| format!("bully chase: {e}"))?,
                        (None, _) => 4.0,
                        _ => return Err(format!("bully: expected `chase N`, got {val:?}")),
                    };
                    level.bullies.push(BullySpec { home, chase });
                }
                "gravity" => level.gravity = val.parse().map_err(|e| format!("gravity: {e}"))?,
                "spring" => level.spring = val.parse().map_err(|e| format!("spring: {e}"))?,
                "theme" => {
                    level.theme = match val {
                        "classic" => Theme::Classic,
                        "madness" => Theme::Madness,
                        "dawn" => Theme::Dawn,
                        t => return Err(format!("unknown theme {t:?}")),
                    }
                }
                "act" => level.act = Some(val.to_string()),
                "story" => level.story.push(val.to_string()),
                "ending" => level.ending.push(val.to_string()),
                "piston" => {
                    let v: Vec<f32> = val.split_whitespace().map(str::parse).collect::<Result<_, _>>().map_err(|e| format!("piston: {e}"))?;
                    let [period, rise, lag] = v[..] else { return Err("piston: expected period rise lag".into()) };
                    level.piston = PistonTiming { period, rise, lag };
                }
                "tide" => {
                    let v: Vec<f32> = val.split_whitespace().map(str::parse).collect::<Result<_, _>>().map_err(|e| format!("tide: {e}"))?;
                    let [period, wet, lag] = v[..] else { return Err("tide: expected period wet lag".into()) };
                    level.tide = Tide { period, wet, lag };
                }
                "grid" => in_grid = true,
                k => return Err(format!("unknown key {k:?}")),
            }
        }
        let h = rows.len();
        let w = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut cells = vec![None; w * h];
        let mut start = None;
        let mut goal = false;
        for (y, row) in rows.iter().enumerate() {
            for (x, tok) in row.iter().enumerate() {
                let mut ch = tok.chars();
                let Some(hc) = ch.next() else { continue };
                if hc == '.' {
                    continue;
                }
                let base = match hc {
                    '0'..='9' => hc as u32 - '0' as u32,
                    'A'..='Z' => hc as u32 - 'A' as u32 + 10,
                    _ => return Err(format!("bad height {hc:?} at {x},{y}")),
                } as f32
                    * 0.5;
                let mut c = Cell { h: [base; 4], kind: Kind::Plain };
                for m in ch {
                    let (d, s) = match m {
                        '>' => (0, 0.5),
                        '}' => (0, 1.0),
                        '<' => (1, 0.5),
                        '{' => (1, 1.0),
                        'v' => (2, 0.5),
                        'V' => (2, 1.0),
                        '^' => (3, 0.5),
                        'A' => (3, 1.0),
                        'S' => {
                            c.kind = Kind::Start;
                            start = Some((x, y));
                            continue;
                        }
                        'G' => {
                            c.kind = Kind::Goal;
                            goal = true;
                            continue;
                        }
                        '~' => {
                            c.kind = Kind::Slime;
                            continue;
                        }
                        '%' => {
                            c.kind = Kind::Tidal;
                            continue;
                        }
                        '|' => {
                            c.kind = Kind::Piston;
                            continue;
                        }
                        '*' => {
                            c.kind = Kind::Ice;
                            continue;
                        }
                        '!' => {
                            c.kind = Kind::Spring;
                            continue;
                        }
                        _ => return Err(format!("bad modifier {m:?} at {x},{y}")),
                    };
                    // corners: 0 = x0y0, 1 = x1y0, 2 = x0y1, 3 = x1y1
                    let low: [usize; 2] = match d {
                        0 => [1, 3],
                        1 => [0, 2],
                        2 => [2, 3],
                        _ => [0, 1],
                    };
                    for i in low {
                        c.h[i] -= s;
                    }
                }
                cells[y * w + x] = Some(c);
            }
        }
        level.start = start.ok_or("no start cell (S)")?;
        if !goal {
            return Err("no goal cell (G)".into());
        }
        level.w = w;
        level.h = h;
        level.cells = cells;
        level.pistons = (0..w * h)
            .filter_map(|i| level.cells[i].filter(|c| c.kind == Kind::Piston).map(|c| (i % w, i / w, c.h)))
            .collect();
        level.tris = (0..w * h).map(|i| level.build_cell(i % w, i / w)).collect();
        level.set_clock(0.0);
        Ok(level)
    }

    pub fn cell(&self, x: i32, y: i32) -> Option<&Cell> {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return None;
        }
        self.cells[y as usize * self.w + x as usize].as_ref()
    }

    /// Move the pistons to where they are at `clock`, and rebuild the faces they touch
    /// (their own, and their neighbours' walls).
    pub fn set_clock(&mut self, clock: f32) {
        if self.pistons.is_empty() {
            return;
        }
        let mut dirty = Vec::new();
        for &(x, y, rest) in &self.pistons {
            let up = self.piston.lift(x as i32, y as i32, clock);
            if let Some(c) = self.cells[y * self.w + x].as_mut() {
                c.h = rest.map(|h| h + up);
            }
            for (dx, dy) in [(0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx >= 0 && ny >= 0 && (nx as usize) < self.w && (ny as usize) < self.h {
                    dirty.push((nx as usize, ny as usize));
                }
            }
        }
        dirty.sort_unstable();
        dirty.dedup();
        for (x, y) in dirty {
            self.tris[y * self.w + x] = self.build_cell(x, y);
        }
    }

    /// Is this cell slime right now (always for `~`, by the tide for `%`)?
    pub fn slime_at(&self, x: i32, y: i32, clock: f32) -> bool {
        match self.cell(x, y).map(|c| c.kind) {
            Some(Kind::Slime) => true,
            Some(Kind::Tidal) => self.tide.is_wet(x, y, clock),
            _ => false,
        }
    }

    /// Top surface height under a world point, if there is a block there.
    pub fn ground(&self, x: f32, y: f32) -> Option<f32> {
        let (cx, cy) = (x.floor() as i32, y.floor() as i32);
        self.cell(cx, cy).map(|c| c.height_at(x - cx as f32, y - cy as f32))
    }

    pub fn tris_of(&self, x: i32, y: i32) -> &[Tri] {
        if x < 0 || y < 0 || x as usize >= self.w || y as usize >= self.h {
            return &[];
        }
        &self.tris[y as usize * self.w + x as usize]
    }

    pub fn all_tris(&self) -> impl Iterator<Item = &Tri> {
        self.tris.iter().flatten()
    }

    fn build_cell(&self, x: usize, y: usize) -> Vec<Tri> {
        let Some(c) = self.cell(x as i32, y as i32) else { return Vec::new() };
        let (fx, fy) = (x as f32, y as f32);
        let p = |i: usize| V3::new(fx + (i & 1) as f32, fy + (i >> 1) as f32, c.h[i]);
        let mut out = Vec::new();
        // top, split along the 0-3 diagonal; both halves get their own normal
        for t in [[0, 1, 3], [0, 3, 2]] {
            let v = [p(t[0]), p(t[1]), p(t[2])];
            let normal = (v[1] - v[0]).cross(v[2] - v[0]).norm();
            out.push(Tri { v, face: Face::Top { kind: c.kind, normal } });
        }
        // walls: from this block's edge down to the neighbour's matching edge
        // side 0 = +x edge (corners 1,3), 1 = +y (3,2), 2 = -x (2,0), 3 = -y (0,1);
        // corner order keeps the wall's outward normal pointing away from the block
        let sides: [(i32, i32, [usize; 2], [usize; 2]); 4] =
            [(1, 0, [1, 3], [0, 2]), (0, 1, [3, 2], [1, 0]), (-1, 0, [2, 0], [3, 1]), (0, -1, [0, 1], [2, 3])];
        for (side, (dx, dy, mine, theirs)) in sides.into_iter().enumerate() {
            let n = self.cell(x as i32 + dx, y as i32 + dy);
            let bottom = |k: usize| n.map_or(FLOOR_Z, |n| n.h[theirs[k]]);
            let (b0, b1) = (bottom(0), bottom(1));
            let (t0, t1) = (p(mine[0]), p(mine[1]));
            if t0.z <= b0 + 1e-4 && t1.z <= b1 + 1e-4 {
                continue;
            }
            let (l0, l1) = (V3::new(t0.x, t0.y, b0.min(t0.z)), V3::new(t1.x, t1.y, b1.min(t1.z)));
            let face = Face::Wall { side: side as u8 };
            out.push(Tri { v: [t0, l0, l1], face });
            out.push(Tri { v: [t0, l1, t1], face });
        }
        // a wall that tapers to nothing at one corner has a zero-area half; the
        // closest-point test divides by that area, so drop it here
        out.retain(|t| (t.v[1] - t.v[0]).cross(t.v[2] - t.v[0]).len() > 1e-6);
        out
    }
}
