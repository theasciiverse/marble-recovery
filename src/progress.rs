//! Best times, kept between sessions in a small text file (`<level id> <seconds>` per
//! line) under the user's data directory. Losing or failing to write it only costs
//! the records, never the game.

use log::{info, warn};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub struct Progress {
    best: BTreeMap<String, f32>,
    path: Option<PathBuf>,
}

pub fn data_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    if let Some(d) = std::env::var_os("APPDATA").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share"))
}

impl Progress {
    pub fn load() -> Progress {
        let path = data_dir().map(|d| d.join("marble-recovery").join("best-times.txt"));
        let best = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|text| Progress::parse(&text))
            .unwrap_or_default();
        Progress { best, path }
    }

    fn parse(text: &str) -> BTreeMap<String, f32> {
        text.lines()
            .filter_map(|l| {
                let (id, t) = l.trim().split_once(' ')?;
                Some((id.to_string(), t.trim().parse().ok()?))
            })
            .collect()
    }

    pub fn best(&self, id: &str) -> Option<f32> {
        self.best.get(id).copied()
    }

    /// Record a finish; returns true when it beats the old best (or is the first).
    pub fn record(&mut self, id: &str, secs: f32) -> bool {
        if self.best(id).is_some_and(|b| b <= secs) {
            return false;
        }
        self.best.insert(id.to_string(), secs);
        info!("new best on {id}: {secs:.2}s");
        self.save();
        true
    }

    fn save(&self) {
        let Some(path) = &self.path else { return };
        let text: String = self.best.iter().map(|(id, t)| format!("{id} {t:.3}\n")).collect();
        let result = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(path, text));
        if let Err(e) = result {
            warn!("could not save best times to {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_only_improve_and_round_trip() {
        let mut p = Progress { best: BTreeMap::new(), path: None };
        assert!(p.record("01-first-descent", 30.0));
        assert!(!p.record("01-first-descent", 31.0));
        assert!(p.record("01-first-descent", 27.5));
        let text: String = p.best.iter().map(|(id, t)| format!("{id} {t:.3}\n")).collect();
        assert_eq!(Progress::parse(&text).get("01-first-descent"), Some(&27.5));
        assert!(Progress::parse("garbage\n\nx y\n").is_empty());
    }
}
