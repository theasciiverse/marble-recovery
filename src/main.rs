//! Marble Recovery: isometric marble physics in the terminal. Half-block world,
//! Braille-edged marble, slime and gulpers.
//!
//! Run: `cargo run --release [-- --level N]` (`RUST_LOG=debug ... 2>marble.log` for a
//! session log). Snapshot without a terminal: `cargo run --release -- --snapshot
//! out.ppm [--level N] [--size 160x48] [--secs t] [--autoplay [--route R] | --push dx,dy]`
//! renders one frame as an image.

mod game;
mod ghost;
mod input;
mod level;
mod levels;
mod math;
mod progress;
mod render;

use game::{Game, Pilot, State, STEP};
use ghost::{Ghost, Push};
use input::Input;
use log::{debug, info};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::supports_keyboard_enhancement;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Widget};
use math::V3;
use progress::Progress;
use render::View;
use std::io;
use std::time::{Duration, Instant};

const FRAME: Duration = Duration::from_millis(16);

fn main() -> io::Result<()> {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--snapshot") {
        return snapshot(&args, args.get(i + 1).map_or("snapshot.ppm", String::as_str));
    }
    run()
}

/// Where the camera aims: a little ahead of the marble, so you see where you're going.
fn cam_target(game: &Game) -> V3 {
    let v = game.marble.vel;
    let ahead = V3::new(v.x, v.y, 0.0) * 0.3;
    let ahead = if ahead.len() > 1.5 { ahead.norm() * 1.5 } else { ahead };
    game.marble.pos + ahead
}

/// A fresh run of a level, with the ghost of the best run if one is saved.
fn fresh(level_no: usize) -> (Game, Option<Ghost>) {
    let (id, _) = levels::LEVELS[level_no];
    let ghost = ghost::load(id).map(|pushes| Ghost::new(Game::new(levels::load(level_no)), pushes));
    if ghost.is_some() {
        info!("racing the ghost of the best run on {id}");
    }
    (Game::new(levels::load(level_no)), ghost)
}

enum Screen {
    /// Level select over an attract-mode run of the highlighted level.
    Title { pilot: Pilot, over: f32 },
    Play,
}

fn run() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let kitty = supports_keyboard_enhancement().unwrap_or(false);
    if kitty {
        let flags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES | KeyboardEnhancementFlags::REPORT_EVENT_TYPES;
        execute!(io::stdout(), PushKeyboardEnhancementFlags(flags))?;
    }
    execute!(io::stdout(), EnableMouseCapture)?;
    info!("start: kitty keyboard = {kitty}");

    let direct = std::env::args().any(|a| a == "--level");
    let mut level_no = start_level();
    let mut game = Game::new(levels::load(level_no));
    let mut screen = if direct {
        info!("level {}: {}", level_no + 1, game.level.name);
        Screen::Play
    } else {
        Screen::Title { pilot: Pilot::new(&game, 0), over: 0.0 }
    };
    let mut progress = Progress::load();
    let mut ghost: Option<Ghost> = None;
    let mut rec: Vec<Push> = Vec::new();
    if direct {
        (game, ghost) = fresh(level_no);
    }
    let mut new_best = false;
    let mut recorded = false;
    let mut view = View::new();
    let mut input = Input::new(kitty);
    let mut acc = 0.0f32;
    let mut last = Instant::now();
    let mut last_log = Instant::now();
    let mut last_state = game.state;
    let mut snap_cam = true;
    let mut paused = false;

    let result = loop {
        let deadline = last + FRAME;
        while event::poll(deadline.saturating_duration_since(Instant::now()))? {
            match event::read()? {
                Event::Key(k) => input.on_key(k, Instant::now()),
                Event::Mouse(m) => input.on_mouse(m),
                Event::Resize(..) => snap_cam = true,
                _ => {}
            }
        }
        input.poll_pad();
        if input.quit {
            break Ok(());
        }
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.1);
        last = now;
        if input.zoom != 0 {
            view.scale = (view.scale + input.zoom as f32).clamp(2.0, 8.0);
            input.zoom = 0;
            snap_cam = true;
            info!("zoom {}", view.scale);
        }

        // switching levels (title selection, n/p, or Enter after a win)
        let mut goto: Option<usize> = None;
        match &mut screen {
            Screen::Title { .. } => {
                let n = levels::LEVELS.len() as i32;
                let moved = std::mem::take(&mut input.menu_moves) + std::mem::take(&mut input.level_step);
                if moved != 0 {
                    goto = Some((level_no as i32 + moved).rem_euclid(n) as usize);
                }
                if std::mem::take(&mut input.back) {
                    break Ok(());
                }
                if std::mem::take(&mut input.confirm) {
                    info!("title: play level {}", level_no + 1);
                    (game, ghost) = fresh(level_no);
                    rec.clear();
                    screen = Screen::Play;
                    snap_cam = true;
                    recorded = false;
                    new_best = false;
                    input.restart = false;
                    last_state = game.state;
                    info!("level {}: {}", level_no + 1, game.level.name);
                }
            }
            Screen::Play => {
                input.menu_moves = 0;
                if std::mem::take(&mut input.back) {
                    info!("back to title");
                    game = Game::new(levels::load(level_no));
                    ghost = None;
                    screen = Screen::Title { pilot: Pilot::new(&game, 0), over: 0.0 };
                    snap_cam = true;
                }
                if std::mem::take(&mut input.restart) {
                    info!("restart");
                    (game, ghost) = fresh(level_no);
                    rec.clear();
                    snap_cam = true;
                    recorded = false;
                    new_best = false;
                }
                if std::mem::take(&mut input.confirm) {
                    match game.state {
                        State::Won => input.level_step += 1,
                        State::Playing => {
                            paused = !paused;
                            info!("{}", if paused { "paused" } else { "resumed" });
                        }
                        _ => {}
                    }
                }
                if input.level_step != 0 {
                    let n = levels::LEVELS.len() as i32;
                    goto = Some((level_no as i32 + input.level_step).rem_euclid(n) as usize);
                    input.level_step = 0;
                }
            }
        }
        if goto.is_some() || !matches!(screen, Screen::Play) || game.state != State::Playing {
            paused = false;
        }
        if let Some(n) = goto {
            level_no = n;
            game = Game::new(levels::load(level_no));
            rec.clear();
            ghost = None;
            if matches!(screen, Screen::Play) {
                (game, ghost) = fresh(level_no);
            }
            last_state = game.state;
            snap_cam = true;
            recorded = false;
            new_best = false;
            if let Screen::Title { pilot, over } = &mut screen {
                *pilot = Pilot::new(&game, 0);
                *over = 0.0;
            }
            info!("level {}: {}", level_no + 1, game.level.name);
        }

        let size = terminal.size()?;
        let world_area = Rect::new(0, 0, size.width, size.height);
        let screen_push = input.push(now, view.marble_cell(&game, world_area));
        acc = if paused { 0.0 } else { acc + dt };
        while acc >= STEP {
            let push = match &mut screen {
                Screen::Title { pilot, .. } => pilot.push(&game),
                Screen::Play => {
                    // quantized before it reaches the game, so the recording is exact
                    let q = ghost::quantize(view.screen_to_world(screen_push));
                    if !matches!(game.state, State::Won | State::TimeUp) {
                        rec.push(q);
                    }
                    if let Some(g) = &mut ghost {
                        g.step();
                    }
                    ghost::dequantize(q)
                }
            };
            game.step(push);
            acc -= STEP;
        }
        if let Screen::Title { pilot, over } = &mut screen {
            // attract mode: once the demo run ends, start it again
            if matches!(game.state, State::Won | State::TimeUp) {
                *over += dt;
                if *over > 2.5 {
                    game = Game::new(levels::load(level_no));
                    *pilot = Pilot::new(&game, 0);
                    *over = 0.0;
                    snap_cam = true;
                }
            }
        }
        if std::mem::discriminant(&game.state) != std::mem::discriminant(&last_state) {
            info!("state {last_state:?} -> {:?}", game.state);
            last_state = game.state;
        }
        if matches!(screen, Screen::Play) && game.state == State::Won && !recorded {
            recorded = true;
            new_best = progress.record(levels::LEVELS[level_no].0, game.elapsed);
            if new_best {
                ghost::save(levels::LEVELS[level_no].0, &rec);
            }
        }
        view.follow(cam_target(&game), dt, snap_cam);
        snap_cam = false;
        if now.duration_since(last_log) >= Duration::from_millis(500) {
            last_log = now;
            let m = &game.marble;
            debug!(
                "snapshot: screen {} state {:?} pos {:.2},{:.2},{:.2} vel {:.2},{:.2},{:.2} grounded {} push {:.2},{:.2} via {:?} time {:.1}",
                if matches!(screen, Screen::Play) { "play" } else { "title" },
                game.state, m.pos.x, m.pos.y, m.pos.z, m.vel.x, m.vel.y, m.vel.z, m.grounded, screen_push.0, screen_push.1,
                input.last_source, game.time_left
            );
        }

        terminal.draw(|f| {
            let area = f.area();
            let shade = ghost.as_ref().filter(|g| g.running() && g.game.state == State::Playing).map(|g| &g.game.marble);
            render::draw(&game, &view, shade, area, f.buffer_mut());
            match screen {
                Screen::Title { .. } => title(&progress, level_no, area, f.buffer_mut()),
                Screen::Play => {
                    hud(&game, &input, level_no, new_best, area, f.buffer_mut());
                    if paused {
                        let msg = " PAUSED · Space or Enter to roll on · Esc menu ";
                        let w = (msg.chars().count() as u16).min(area.width);
                        let boxed = Rect { x: area.x + (area.width - w) / 2, y: area.y + area.height / 3, width: w, height: 1 };
                        Clear.render(boxed, f.buffer_mut());
                        Paragraph::new(msg)
                            .style(Style::default().bg(Color::Rgb(0, 0, 0)).fg(Color::Rgb(255, 176, 0)).add_modifier(Modifier::BOLD))
                            .render(boxed, f.buffer_mut());
                    }
                }
            }
        })?;
    };

    let _ = execute!(io::stdout(), DisableMouseCapture);
    if kitty {
        let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
    }
    ratatui::restore();
    result
}

/// The title card and level list, over the attract-mode run.
fn title(progress: &Progress, selected: usize, area: Rect, buf: &mut Buffer) {
    let amber = Color::Rgb(255, 176, 0);
    let grey = Color::Rgb(150, 150, 150);
    let bg = Style::default().bg(Color::Rgb(0, 0, 0));
    let mut lines = vec![
        Line::from(Span::styled("M A R B L E   R E C O V E R Y", bg.fg(amber).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("something went wrong. bring it back.", bg.fg(grey))),
        Line::from(""),
    ];
    for (i, (id, text)) in levels::LEVELS.iter().enumerate() {
        let name = text.lines().find_map(|l| l.strip_prefix("name:")).map_or(*id, str::trim);
        let best = progress.best(id).map_or("   --  ".to_string(), |t| format!("{t:6.2}s"));
        let sel = i == selected;
        let style = if sel { bg.fg(Color::Rgb(0, 0, 0)).bg(amber).add_modifier(Modifier::BOLD) } else { bg.fg(Color::Rgb(220, 220, 220)) };
        lines.push(Line::from(Span::styled(format!(" {} {}  {:<22} best {} ", if sel { '>' } else { ' ' }, i + 1, name, best), style)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled("up/down choose · Enter play · Esc in a level: back here · q quit", bg.fg(grey))));
    let w = (lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16 + 6).min(area.width);
    let h = (lines.len() as u16 + 2).min(area.height);
    let boxed = Rect { x: area.x + (area.width - w) / 2, y: area.y + area.height.saturating_sub(h) / 2, width: w, height: h };
    Clear.render(boxed, buf);
    Paragraph::new(lines)
        .alignment(ratatui::layout::Alignment::Center)
        .style(bg)
        .block(Block::bordered().border_type(BorderType::Rounded).border_style(bg.fg(amber)).style(bg))
        .render(boxed, buf);
}

/// `--level N` (1-based) picks the first level.
fn start_level() -> usize {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == "--level")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse::<usize>().ok())
        .map_or(0, |n| n.clamp(1, levels::LEVELS.len()) - 1)
}

fn hud(game: &Game, input: &Input, level_no: usize, new_best: bool, area: Rect, buf: &mut Buffer) {
    let bar = Style::default().bg(Color::Rgb(0, 0, 0)).fg(Color::Rgb(255, 176, 0));
    let t = game.time_left;
    let time_style = if t < 10.0 { bar.fg(Color::Rgb(255, 60, 60)) } else { bar };
    let pad = input.pad_name.as_deref().map_or(String::new(), |n| format!("  pad: {n}"));
    let grey = bar.fg(Color::Rgb(180, 180, 180));
    let mut spans = vec![
        Span::styled(" MARBLE RECOVERY ", bar.add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {}/{} {} ", level_no + 1, levels::LEVELS.len(), game.level.name), grey),
        Span::styled(format!(" TIME {t:5.1} "), time_style.add_modifier(Modifier::BOLD)),
        Span::styled(format!(" lost {} ", game.falls), grey),
    ];
    let soak = game.marble.slime / game::SLIME_LIMIT;
    if soak > 0.02 && game.state == State::Playing {
        let n = 10;
        let full = ((soak * n as f32).ceil() as usize).min(n);
        spans.push(Span::styled(" SLIME ", bar.fg(Color::Rgb(90, 220, 60)).add_modifier(Modifier::BOLD)));
        spans.push(Span::styled(format!("{}{}", "█".repeat(full), "░".repeat(n - full)), bar.fg(Color::Rgb(90, 220, 60))));
    }
    let top = Line::from(spans);
    // clear first: a world cell is a `▀` whose top half takes the fg colour, so text
    // styled over it would leave coloured half-blocks wherever there is no glyph
    let top_row = Rect { height: 1, ..area };
    Clear.render(top_row, buf);
    Paragraph::new(top).style(bar).render(top_row, buf);
    let help = format!(
        " arrows/wasd push · hold mouse button to pull toward pointer · gamepad stick · +/- zoom · space pause · r restart · n/p level · Esc menu · q quit   [{}{}] ",
        input.keys_note(),
        pad
    );
    if area.height > 2 {
        let bottom_row = Rect { y: area.y + area.height - 1, height: 1, ..area };
        Clear.render(bottom_row, buf);
        Paragraph::new(help).style(bar.fg(Color::Rgb(140, 140, 140))).render(bottom_row, buf);
    }
    let msg = match game.state {
        State::Won if new_best && level_no + 1 < levels::LEVELS.len() => {
            Some(format!(" NEW BEST! RECOVERED in {:.2}s · Enter: next level · r: run again ", game.elapsed))
        }
        State::Won if level_no + 1 == levels::LEVELS.len() => {
            Some(format!(" ALL MARBLES RECOVERED · last one in {:.2}s · Enter: back to the start · r: run again ", game.elapsed))
        }
        State::Won => Some(format!(" MARBLE RECOVERED in {:.2}s · Enter: next level · r: run again ", game.elapsed)),
        State::TimeUp => Some(" TIME UP · r to try again ".to_string()),
        State::Shattered(_) => Some(" SHATTERED ".to_string()),
        State::Falling(_) => Some(" LOST TO THE VOID ".to_string()),
        State::Dissolved(_) => Some(" DISSOLVED IN SLIME ".to_string()),
        State::Eaten(_) => Some(" GULPED! ".to_string()),
        State::Playing => None,
    };
    if let Some(msg) = msg {
        // a box just the size of the message, on black
        let w = (msg.chars().count() as u16).min(area.width);
        let boxed = Rect { x: area.x + (area.width - w) / 2, y: area.y + area.height / 3, width: w, height: 1 };
        Clear.render(boxed, buf);
        Paragraph::new(msg)
            .style(bar.add_modifier(Modifier::BOLD))
            .render(boxed, buf);
    }
}

/// Headless: simulate `--secs` seconds (the level's own route with `--autoplay`, else a
/// constant `--push`), render one frame, and paint the terminal cells into a PPM (8x16
/// pixels per cell) exactly as drawn.
fn snapshot(args: &[String], out: &str) -> io::Result<()> {
    let arg = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let (cols, rows) = arg("--size")
        .and_then(|s| s.split_once('x').map(|(a, b)| (a.parse().unwrap_or(160), b.parse().unwrap_or(48))))
        .unwrap_or((160u16, 48u16));
    let push = arg("--push")
        .and_then(|s| s.split_once(',').map(|(a, b)| (a.parse().unwrap_or(0.0), b.parse().unwrap_or(0.0))))
        .unwrap_or((0.0f32, 0.0f32));
    let secs: f32 = arg("--secs").and_then(|s| s.parse().ok()).unwrap_or(0.5);
    let level_no = start_level();
    let mut game = Game::new(levels::load(level_no));
    let mut view = View::new();
    if let Some(z) = arg("--scale").and_then(|s| s.parse().ok()) {
        view.scale = z;
    }
    let mut cam = |g: &Game| view.follow(cam_target(g), STEP, false);
    // --ghost-route R: record the autopilot on route R, then race that recording
    let mut ghost = arg("--ghost-route").and_then(|v| v.parse::<usize>().ok()).map(|r| {
        let mut g = Game::new(levels::load(level_no));
        let mut pilot = Pilot::new(&g, r);
        let mut rec = Vec::new();
        while g.state != State::Won && rec.len() < 240 * 70 {
            let q = ghost::quantize(pilot.push(&g));
            rec.push(q);
            g.step(ghost::dequantize(q));
        }
        Ghost::new(Game::new(levels::load(level_no)), rec)
    });
    if args.iter().any(|a| a == "--autoplay") {
        let which = arg("--route").and_then(|v| v.parse().ok()).unwrap_or(0);
        let hits = game.play_route(which, secs, |g| {
            cam(g);
            if let Some(gh) = &mut ghost {
                gh.step();
            }
        });
        println!("waypoints at {hits:.2?}");
    } else {
        for _ in 0..(secs / STEP) as usize {
            game.step(view.screen_to_world(push));
            view.follow(cam_target(&game), STEP, false);
        }
    }
    // --focus N: centre on gulper N instead of the marble (for art tuning)
    let focus = arg("--focus").and_then(|v| v.parse::<usize>().ok()).and_then(|i| game.eaters.get(i)).map(|e| e.pos);
    view.follow(focus.unwrap_or(cam_target(&game)), 0.0, true);
    let area = Rect::new(0, 0, cols, rows);
    let mut buf = Buffer::empty(area);
    let t0 = Instant::now();
    let reps = 20;
    for _ in 0..reps {
        render::draw(&game, &view, ghost.as_ref().map(|g| &g.game.marble), area, &mut buf);
    }
    eprintln!("draw: {:.2} ms per frame at {cols}x{rows}", t0.elapsed().as_secs_f64() * 1000.0 / reps as f64);
    let input = Input::new(false);
    if args.iter().any(|a| a == "--title") {
        title(&Progress::load(), level_no, area, &mut buf);
    } else {
        hud(&game, &input, level_no, false, area, &mut buf);
    }
    let m = &game.marble;
    println!(
        "state {:?} pos {:.2},{:.2},{:.2} vel {:.2},{:.2},{:.2} grounded {} t {:.2} lost {}",
        game.state, m.pos.x, m.pos.y, m.pos.z, m.vel.x, m.vel.y, m.vel.z, m.grounded, game.elapsed, game.falls
    );
    // the same frame as text, for reading what the HUD and menus say
    let text: String = (0..rows)
        .map(|r| (0..cols).map(|c| buf[(c, r)].symbol().to_string()).collect::<String>() + "\n")
        .collect();
    std::fs::write(format!("{out}.txt"), text)?;
    write_ppm(&buf, out)
}

fn write_ppm(buf: &Buffer, out: &str) -> io::Result<()> {
    let (cw, ch) = (8usize, 16usize);
    let (cols, rows) = (buf.area.width as usize, buf.area.height as usize);
    let (w, h) = (cols * cw, rows * ch);
    let mut img = vec![0u8; w * h * 3];
    let rgb = |c: Color| match c {
        Color::Rgb(r, g, b) => [r, g, b],
        _ => [0, 0, 0],
    };
    for r in 0..rows {
        for c in 0..cols {
            let cell = &buf[(c as u16, r as u16)];
            let (fg, bg) = (rgb(cell.fg), rgb(cell.bg));
            let sym = cell.symbol().chars().next().unwrap_or(' ');
            let code = sym as u32;
            for y in 0..ch {
                for x in 0..cw {
                    let col = if sym == '▀' {
                        if y < ch / 2 { fg } else { bg }
                    } else if sym == '▄' {
                        if y >= ch / 2 { fg } else { bg }
                    } else if (0x2800..0x2900).contains(&code) {
                        let bits = (code - 0x2800) as u8;
                        let (dx, dy) = (x / 4, y / 4);
                        let on = bits & [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]][dx][dy] != 0;
                        // a dot is a round-ish blob inside its 4x4 slot, like a real font
                        let (ix, iy) = (x % 4, y % 4);
                        if on && (1..=2).contains(&ix) && (1..=2).contains(&iy) { fg } else { bg }
                    } else if sym != ' ' {
                        if (2..6).contains(&x) && (4..12).contains(&y) { fg } else { bg }
                    } else {
                        bg
                    };
                    let i = ((r * ch + y) * w + c * cw + x) * 3;
                    img[i..i + 3].copy_from_slice(&col);
                }
            }
        }
    }
    let mut data = format!("P6\n{w} {h}\n255\n").into_bytes();
    data.extend_from_slice(&img);
    std::fs::write(out, data)
}
