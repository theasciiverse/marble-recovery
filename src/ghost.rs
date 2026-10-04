//! Ghost runs: the inputs of your best run on each level, replayed as a pale marble
//! to race. The physics is deterministic, so replaying the same per-step pushes into
//! a fresh game reproduces the run exactly; pushes are quantized to hundredths before
//! they reach the live game too, so recording loses nothing.
//!
//! File format (`ghosts/<level id>.txt` in the data directory): a version line, then
//! run-length lines `count x y` with x and y in hundredths.

use crate::game::Game;
use crate::math::V3;
use log::{info, warn};
use std::path::PathBuf;

/// Bump when the physics changes, so old ghosts are dropped instead of drifting.
const FORMAT: &str = "marble-recovery ghost v2";

pub type Push = (i8, i8);

pub fn quantize(p: V3) -> Push {
    let q = |v: f32| (v * 100.0).round().clamp(-100.0, 100.0) as i8;
    (q(p.x), q(p.y))
}

pub fn dequantize(q: Push) -> V3 {
    V3::new(q.0 as f32 / 100.0, q.1 as f32 / 100.0, 0.0)
}

fn path(id: &str) -> Option<PathBuf> {
    crate::progress::data_dir().map(|d| d.join("marble-recovery").join("ghosts").join(format!("{id}.txt")))
}

pub fn encode(pushes: &[Push]) -> String {
    let mut out = format!("{FORMAT}\n");
    let mut i = 0;
    while i < pushes.len() {
        let run = pushes[i..].iter().take_while(|&&p| p == pushes[i]).count();
        out += &format!("{run} {} {}\n", pushes[i].0, pushes[i].1);
        i += run;
    }
    out
}

pub fn decode(text: &str) -> Option<Vec<Push>> {
    let mut lines = text.lines();
    if lines.next()? != FORMAT {
        return None;
    }
    let mut pushes = Vec::new();
    for l in lines {
        let mut it = l.split_whitespace().map(str::parse::<i32>);
        let (Some(Ok(n)), Some(Ok(x)), Some(Ok(y))) = (it.next(), it.next(), it.next()) else { return None };
        pushes.extend(std::iter::repeat_n((x as i8, y as i8), n.max(0) as usize));
    }
    Some(pushes)
}

pub fn save(id: &str, pushes: &[Push]) {
    let Some(p) = path(id) else { return };
    let result = p.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&p, encode(pushes)));
    match result {
        Ok(()) => info!("ghost saved for {id}: {} steps", pushes.len()),
        Err(e) => warn!("could not save ghost to {}: {e}", p.display()),
    }
}

pub fn load(id: &str) -> Option<Vec<Push>> {
    decode(&std::fs::read_to_string(path(id)?).ok()?)
}

/// A replaying run, stepped in lockstep with the player's game.
pub struct Ghost {
    pub game: Game,
    pushes: Vec<Push>,
    at: usize,
}

impl Ghost {
    pub fn new(game: Game, pushes: Vec<Push>) -> Ghost {
        Ghost { game, pushes, at: 0 }
    }

    pub fn step(&mut self) {
        if let Some(&q) = self.pushes.get(self.at) {
            self.game.step(dequantize(q));
            self.at += 1;
        }
    }

    /// Still worth drawing: the recording has not run out.
    pub fn running(&self) -> bool {
        self.at < self.pushes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Pilot, State};
    use crate::levels;

    #[test]
    fn a_recorded_run_replays_exactly() {
        // record the autopilot on a level with every kind of moving hazard
        for (i, (id, _)) in levels::LEVELS.iter().enumerate() {
            let mut live = Game::new(levels::load(i));
            let mut pilot = Pilot::new(&live, 0);
            let mut rec = Vec::new();
            while live.state != State::Won && rec.len() < 240 * 70 {
                let q = quantize(pilot.push(&live));
                rec.push(q);
                live.step(dequantize(q));
            }
            assert_eq!(live.state, State::Won, "{id}: the quantized autopilot should still finish");
            let back = decode(&encode(&rec)).unwrap();
            assert_eq!(back, rec, "{id}: encoding round trip");
            let mut ghost = Ghost::new(Game::new(levels::load(i)), back);
            while ghost.running() {
                ghost.step();
            }
            assert_eq!(ghost.game.marble.pos, live.marble.pos, "{id}: replay drifted");
            assert_eq!(ghost.game.state, State::Won);
        }
    }

    #[test]
    fn foreign_or_old_files_are_ignored() {
        assert!(decode("marble-recovery ghost v0\n3 1 1\n").is_none());
        assert!(decode("hello").is_none());
        assert!(decode(&format!("{FORMAT}\n3 x 1\n")).is_none());
    }
}
