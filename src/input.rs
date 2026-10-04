//! Every way to push the marble, merged into one screen-space vector.
//!
//! Keyboard: two tiers, adapted from Entity's `input.rs`. With the kitty keyboard
//! protocol presses and releases are real; otherwise a key counts as held while its
//! OS repeats keep arriving, with the repeat timing learned from the events.
//! Mouse: hold the left button and the marble is pushed toward the pointer, harder the
//! further away it is (a virtual trackball). Gamepad: left stick (analog) or d-pad.
//!
//! gilrs counts any device with buttons and two axes as a gamepad (drawing-tablet
//! key pads and 3D mice included), so only devices that matched SDL's controller
//! database are used; `MARBLE_ANY_PAD=1` accepts everything.

use log::debug;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::time::{Duration, Instant};

const PRE_REPEAT_CAP: Duration = Duration::from_millis(220);
const REPEAT_DELAY_DEFAULT: Duration = Duration::from_millis(500);
const REPEAT_INTERVAL_DEFAULT: Duration = Duration::from_millis(33);
const RELEASE_MIN: Duration = Duration::from_millis(60);
const RELEASE_MAX: Duration = Duration::from_millis(160);
/// Pointer distance (in cells) that gives full push.
const MOUSE_FULL: f32 = 12.0;
#[cfg(feature = "gamepad")]
const STICK_DEADZONE: f32 = 0.15;

#[derive(Clone, Copy, Debug)]
struct Repeat {
    delay: Duration,
    interval: Duration,
}

#[derive(Clone, Copy, Default, Debug)]
struct Hold {
    last: Option<Instant>,
    events: u32,
}

impl Hold {
    fn event(&mut self, now: Instant, rep: &mut Repeat) {
        if let Some(last) = self.last {
            let gap = now.duration_since(last);
            let continues = if self.events == 1 {
                gap >= rep.delay.mul_f32(0.5) && gap <= rep.delay.mul_f32(1.6)
            } else {
                gap <= RELEASE_MAX.max(rep.interval * 4)
            };
            if continues {
                self.events += 1;
                let blend = |old: Duration, new: Duration| (old.mul_f32(0.5) + new.mul_f32(0.5)).max(Duration::from_millis(10));
                if self.events == 2 {
                    rep.delay = blend(rep.delay, gap.clamp(Duration::from_millis(100), Duration::from_millis(1000)));
                } else if gap < Duration::from_millis(200) {
                    rep.interval = blend(rep.interval, gap);
                }
                self.last = Some(now);
                return;
            }
        }
        self.last = Some(now);
        self.events = 1;
    }

    fn held(&self, now: Instant, rep: &Repeat) -> bool {
        let Some(last) = self.last else { return false };
        let timeout = if self.events >= 2 {
            (rep.interval * 5 / 2).clamp(RELEASE_MIN, RELEASE_MAX)
        } else {
            PRE_REPEAT_CAP.min(rep.delay + rep.interval * 2)
        };
        now.duration_since(last) <= timeout
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg_attr(not(feature = "gamepad"), allow(dead_code))]
pub enum Source {
    Keys,
    Mouse,
    Pad,
}

pub struct Input {
    exact: bool,
    down: [bool; 4],
    holds: [Hold; 4],
    rep: Repeat,
    mouse_down: bool,
    mouse_at: (u16, u16),
    pad: (f32, f32),
    pub last_source: Source,
    pub quit: bool,
    pub restart: bool,
    pub zoom: i32,
    /// Levels to move by (n / p).
    pub level_step: i32,
    /// Enter, space or pad A: carry on (next level after a win, pick on the title).
    pub confirm: bool,
    /// Esc or pad B: back to the title.
    pub back: bool,
    /// Up/down presses for menus (+1 = down).
    pub menu_moves: i32,
    #[cfg(feature = "gamepad")]
    gilrs: Option<gilrs::Gilrs>,
    pub pad_name: Option<String>,
}

/// Directions: 0 up, 1 down, 2 left, 3 right (screen space).
fn dir_of(code: KeyCode) -> Option<usize> {
    match code {
        KeyCode::Up | KeyCode::Char('w') | KeyCode::Char('k') => Some(0),
        KeyCode::Down | KeyCode::Char('s') | KeyCode::Char('j') => Some(1),
        KeyCode::Left | KeyCode::Char('a') | KeyCode::Char('h') => Some(2),
        KeyCode::Right | KeyCode::Char('d') | KeyCode::Char('l') => Some(3),
        _ => None,
    }
}

#[cfg(feature = "gamepad")]
fn usable_pad(p: &gilrs::Gamepad) -> bool {
    p.mapping_source() == gilrs::MappingSource::SdlMappings || std::env::var("MARBLE_ANY_PAD").is_ok_and(|v| v == "1")
}

impl Input {
    pub fn new(exact: bool) -> Input {
        #[cfg(feature = "gamepad")]
        let (gilrs, pad_name) = match gilrs::Gilrs::new() {
            Ok(g) => {
                let all: Vec<_> = g.gamepads().map(|(_, p)| (p.name().to_string(), usable_pad(&p))).collect();
                log::info!("gamepads (name, used): {all:?}");
                let name = all.into_iter().find(|(_, used)| *used).map(|(n, _)| n);
                (Some(g), name)
            }
            Err(e) => {
                log::warn!("gamepad support unavailable: {e}");
                (None, None)
            }
        };
        #[cfg(not(feature = "gamepad"))]
        let pad_name = None;
        Input {
            exact,
            down: [false; 4],
            holds: [Hold::default(); 4],
            rep: Repeat { delay: REPEAT_DELAY_DEFAULT, interval: REPEAT_INTERVAL_DEFAULT },
            mouse_down: false,
            mouse_at: (0, 0),
            pad: (0.0, 0.0),
            last_source: Source::Keys,
            quit: false,
            restart: false,
            zoom: 0,
            level_step: 0,
            confirm: false,
            back: false,
            menu_moves: 0,
            #[cfg(feature = "gamepad")]
            gilrs,
            pad_name,
        }
    }

    pub fn keys_note(&self) -> &'static str {
        if self.exact { "keys: exact" } else { "keys: timing" }
    }

    pub fn on_key(&mut self, k: KeyEvent, now: Instant) {
        debug!("key {:?} {:?} {:?}", k.code, k.kind, k.modifiers);
        let down = k.kind != KeyEventKind::Release;
        let press = k.kind == KeyEventKind::Press;
        if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        if let Some(d) = dir_of(k.code) {
            self.last_source = Source::Keys;
            if press && d < 2 {
                self.menu_moves += if d == 0 { -1 } else { 1 };
            }
            if self.exact {
                self.down[d] = down;
            } else if down {
                self.holds[d].event(now, &mut self.rep);
                // a press of the opposite direction ends that one at once
                self.holds[d ^ 1] = Hold::default();
            }
            return;
        }
        if !press {
            return;
        }
        match k.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => self.back = true,
            KeyCode::Char('r') => self.restart = true,
            KeyCode::Char('+') | KeyCode::Char('=') => self.zoom += 1,
            KeyCode::Char('-') => self.zoom -= 1,
            KeyCode::Char('n') => self.level_step += 1,
            KeyCode::Char('p') => self.level_step -= 1,
            KeyCode::Enter | KeyCode::Char(' ') => self.confirm = true,
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left) => {
                if !self.mouse_down {
                    debug!("mouse down at {},{}", m.column, m.row);
                }
                self.mouse_down = true;
                self.last_source = Source::Mouse;
            }
            MouseEventKind::Up(MouseButton::Left) => {
                debug!("mouse up at {},{}", m.column, m.row);
                self.mouse_down = false;
            }
            _ => {}
        }
        self.mouse_at = (m.column, m.row);
    }

    /// Drain gamepad events (call once per frame).
    pub fn poll_pad(&mut self) {
        #[cfg(feature = "gamepad")]
        if let Some(g) = &mut self.gilrs {
            use gilrs::{Axis, Button, EventType};
            while let Some(ev) = g.next_event() {
                let pad = g.gamepad(ev.id);
                if !usable_pad(&pad) {
                    continue;
                }
                match ev.event {
                    EventType::Connected => {
                        self.pad_name = Some(pad.name().to_string());
                        log::info!("gamepad connected: {:?}", self.pad_name);
                    }
                    EventType::ButtonPressed(b, _) => {
                        debug!("pad button {b:?}");
                        match b {
                            Button::Start => self.restart = true,
                            Button::South => self.confirm = true,
                            Button::East => self.back = true,
                            Button::DPadUp => self.menu_moves -= 1,
                            Button::DPadDown => self.menu_moves += 1,
                            _ => {}
                        }
                    }
                    _ => {}
                }
                let axis = |a: Axis| pad.axis_data(a).map_or(0.0, |d| d.value());
                let btn = |b: Button| pad.is_pressed(b) as i32 as f32;
                let mut x = axis(Axis::LeftStickX) + btn(Button::DPadRight) - btn(Button::DPadLeft);
                // stick up is positive; screen up is negative
                let mut y = -axis(Axis::LeftStickY) + btn(Button::DPadDown) - btn(Button::DPadUp);
                let len = (x * x + y * y).sqrt();
                if len < STICK_DEADZONE {
                    (x, y) = (0.0, 0.0);
                } else if len > 1.0 {
                    (x, y) = (x / len, y / len);
                }
                if (x, y) != self.pad {
                    debug!("pad stick {x:.2},{y:.2}");
                    self.pad = (x, y);
                    if len >= STICK_DEADZONE {
                        self.last_source = Source::Pad;
                    }
                }
            }
        }
    }

    /// Wanted push in screen space (x right, y down, in terminal-cell proportions
    /// corrected to square), length 0 to 1. `marble_cell` is where the marble is drawn.
    pub fn push(&self, now: Instant, marble_cell: (f32, f32)) -> (f32, f32) {
        if self.mouse_down {
            // a cell is about twice as tall as wide
            let dx = self.mouse_at.0 as f32 + 0.5 - marble_cell.0;
            let dy = (self.mouse_at.1 as f32 + 0.5 - marble_cell.1) * 2.0;
            let len = (dx * dx + dy * dy).sqrt();
            if len < 0.5 {
                return (0.0, 0.0);
            }
            let s = (len / MOUSE_FULL).min(1.0) / len;
            return (dx * s, dy * s);
        }
        if self.pad != (0.0, 0.0) {
            return self.pad;
        }
        let held = |d: usize| if self.exact { self.down[d] } else { self.holds[d].held(now, &self.rep) };
        let x = held(3) as i32 - held(2) as i32;
        let y = held(1) as i32 - held(0) as i32;
        let len = ((x * x + y * y) as f32).sqrt();
        if len == 0.0 { (0.0, 0.0) } else { (x as f32 / len, y as f32 / len) }
    }
}
