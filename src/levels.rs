//! The shipped levels, compiled into the binary. All are original designs for
//! Marble Recovery; none reproduces a Marble Madness course.

use crate::level::Level;

pub const LEVELS: &[(&str, &str)] = &[
    ("01-first-descent", include_str!("../levels/01-first-descent.lvl")),
    ("02-slime-terraces", include_str!("../levels/02-slime-terraces.lvl")),
    ("03-the-gulper-yard", include_str!("../levels/03-the-gulper-yard.lvl")),
    ("04-low-tide", include_str!("../levels/04-low-tide.lvl")),
    ("05-gulper-bog", include_str!("../levels/05-gulper-bog.lvl")),
    ("06-bully-ridge", include_str!("../levels/06-bully-ridge.lvl")),
    ("07-thin-ice", include_str!("../levels/07-thin-ice.lvl")),
    ("08-piston-steps", include_str!("../levels/08-piston-steps.lvl")),
    ("09-spring-fever", include_str!("../levels/09-spring-fever.lvl")),
    ("10-weightless", include_str!("../levels/10-weightless.lvl")),
    ("11-switchback-spire", include_str!("../levels/11-switchback-spire.lvl")),
    ("12-the-long-way-home", include_str!("../levels/12-the-long-way-home.lvl")),
];

/// A level's stable id for saved data: its file name without the order number, so
/// reordering levels never orphans best times or ghosts ("08-piston-steps" is
/// "piston-steps").
pub fn id(i: usize) -> &'static str {
    strip_order(LEVELS[i % LEVELS.len()].0)
}

/// "07-piston-steps" to "piston-steps"; ids without a number pass through.
pub fn strip_order(name: &str) -> &str {
    match name.split_once('-') {
        Some((n, rest)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => rest,
        _ => name,
    }
}

pub fn load(i: usize) -> Level {
    let (file, text) = LEVELS[i % LEVELS.len()];
    Level::load(text).unwrap_or_else(|e| panic!("level {file}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Game, State};

    /// Every shipped level parses and its own route reaches the goal without losing
    /// the marble once, with all hazards live: the level is proven completable.
    #[test]
    fn every_level_is_completable_by_its_routes() {
        for (i, (file, _)) in LEVELS.iter().enumerate() {
            let routes = load(i).routes.len();
            assert!(routes > 0, "{file}: no route");
            for r in 0..routes {
                let level = load(i);
                let time = level.time_limit;
                let mut g = Game::new(level);
                let hits = g.play_route(r, time, |_| {});
                assert_eq!(
                    g.state,
                    State::Won,
                    "{file} route {r}: ended {:?} at {:.2},{:.2} after waypoints {hits:?}, losses {}",
                    g.state,
                    g.marble.pos.x,
                    g.marble.pos.y,
                    g.falls
                );
                assert_eq!(g.falls, 0, "{file} route {r}: lost the marble");
                println!("{file} route {r}: recovered in {:.2}s of {time}s", g.elapsed);
            }
        }
    }
}




/// The route lab, for designing levels (ignored by a plain `cargo test`):
///
/// ```text
/// MR_LEVEL=7 MR_ROUTE="2,2 w? 10,2 ..." cargo test --release route_lab -- --ignored --nocapture
/// ```
///
/// Plays the route on that level and reports where it got to and how hard each hazard
/// pressed (peak slime soak, closest gulper and bully, how long they hunted). Every
/// `w?` is searched in turn (0 to 8 s in quarter seconds): the shortest wait that
/// gets past the following step without losing the marble is kept.
#[cfg(test)]
mod lab {
    use super::*;
    use crate::game::{Game, State, STEP};

    struct Outcome {
        won: bool,
        steps_done: usize,
        falls: u32,
        time: f32,
        soak: f32,
        gulper: f32,
        bully: f32,
        hunted: f32,
        at: (f32, f32),
    }

    fn play(i: usize, route: &str) -> Outcome {
        let text = LEVELS[i].1;
        let t = text.lines().filter(|l| !l.starts_with("route:")).collect::<Vec<_>>().join("\n");
        let t = t.replace("grid:", &format!("route: {route}\ngrid:"));
        let mut g = Game::new(crate::level::Level::load(&t).unwrap());
        let (mut soak, mut gulper, mut bully, mut hunted) = (0.0f32, f32::MAX, f32::MAX, 0.0f32);
        let hits = g.play_route(0, 120.0, |g| {
            soak = soak.max(g.marble.slime);
            for e in &g.eaters {
                gulper = gulper.min((e.pos - g.marble.pos).len());
                if e.hunting {
                    hunted += STEP;
                }
            }
            for b in &g.bullies {
                bully = bully.min((b.ball.pos - g.marble.pos).len());
                if b.angry {
                    hunted += STEP;
                }
            }
        });
        Outcome {
            won: g.state == State::Won,
            steps_done: hits.len(),
            falls: g.falls,
            time: g.elapsed,
            soak,
            gulper,
            bully,
            hunted,
            at: (g.marble.pos.x, g.marble.pos.y),
        }
    }

    #[test]
    #[ignore]
    fn route_lab() {
        let i: usize = std::env::var("MR_LEVEL").expect("MR_LEVEL").parse::<usize>().unwrap() - 1;
        let mut toks: Vec<String> = std::env::var("MR_ROUTE").expect("MR_ROUTE").split_whitespace().map(str::to_string).collect();
        while let Some(k) = toks.iter().position(|t| t == "w?") {
            let mut chosen = None;
            for q in 0..=32 {
                let w = q as f32 * 0.25;
                let mut cand = toks.clone();
                cand[k] = format!("w{w}");
                // later unknown waits count as zero while this one is searched
                let route = cand.iter().map(|t| if t == "w?" { "w0".to_string() } else { t.clone() }).collect::<Vec<_>>().join(" ");
                let o = play(i, &route);
                if o.falls == 0 && (o.won || o.steps_done >= k + 2) {
                    chosen = Some(w);
                    break;
                }
            }
            match chosen {
                Some(w) => {
                    println!("wait {k}: {w}s");
                    toks[k] = format!("w{w}");
                }
                None => {
                    println!("wait {k}: nothing in 0 to 8 s gets past step {}", k + 1);
                    toks[k] = "w0".into();
                }
            }
        }
        let route = toks.join(" ");
        let o = play(i, &route);
        println!("route: {route}");
        println!(
            "won {} steps {} falls {} time {:.2} at {:.2},{:.2} | soak {:.2} gulper {:.2} bully {:.2} hunted {:.2}s",
            o.won, o.steps_done, o.falls, o.time, o.at.0, o.at.1, o.soak, o.gulper, o.bully, o.hunted
        );
    }
}
