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
    ("07-piston-steps", include_str!("../levels/07-piston-steps.lvl")),
    ("08-switchback-spire", include_str!("../levels/08-switchback-spire.lvl")),
];

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



