# Marble Recovery

Asciiverse isometric physics platformer. Rust + Ratatui + Drawille.
Spiritual successor to Marble Madness (Atari, 1984, designed by Mark Cerny).

## Name

Marble Recovery — not a race, a retrieval. Something went wrong. The marble is
lost, displaced in a strange geometric world. It needs to be brought back.

More compelling premise than the original. Opens narrative possibilities
Marble Madness never explored. Each world feels alien and specific — the
Recovery frame gives permission to go stranger and more varied than the source.

It is also a pun on the original: you recover from madness. Marble Madness becomes
Marble Recovery. Keep both halves of the name; "Marbles" alone was considered on
2026-10-04 and dropped because it loses the pun (and Steam already has ten
"... Marbles" titles, two of them called Lost Marbles).

## Why it works for Asciiverse

Marble Madness is isometric, physics-driven — the ball rolling across geometric
surfaces with momentum and gravity doing all the work. No jump button. No attack.
Pure physics and spatial reasoning.

The Braille sub-pixel rendering for the marble itself — a sphere rolling across
an isometric surface, the shadow beneath it, the momentum visible in how it
approaches edges — is a genuine showcase piece. The isometric geometry in
line-drawing characters with the marble rendered in Braille is exactly the
clever layer fusion that defines the Asciiverse visual language.

The shadow specifically: a soft Braille dot gradient shadow beneath the marble
that stretches and shrinks with height above the surface. Simple, physically
grounded, visually extraordinary in a terminal.

## Rendering approach (proven by the 2026-10-04 spike)

- **Half-block world**: surfaces are rasterized with a depth buffer into `▀` cells
  (fg = top pixel, bg = bottom), giving roughly square, full-colour pixels. This is
  what Marble Madness actually looks like (filled faces: grey checker tops, striped
  blue walls), not line art. Box-drawing for isometric edges was dropped: a true 2:1
  iso edge runs about 4 cells across per cell down, which `╱`/`╲` cannot draw.
- **Checker, not grid lines**: thin grid lines alias into a brick pattern at this
  resolution; a half-tile checker reads as the same grid and cannot alias.
- **Marble**: each Braille dot is ray-tested against the real sphere (lighting,
  specular, depth test against the world, a rolling band from the marble's
  orientation). Cells it fills become half-blocks shaded per half; rim cells with a
  mostly covered half use `▀`/`▄`, and only sparse corner cells stay Braille (David's
  terminal font drew a Braille-heavy rim as grey speckle).
- **Shadow**: darkens the surface pixels under the marble, wider and softer with
  height. A Braille dot shadow would fight the marble for the same cells.
- **Projection**: px = (x - y)·2s, py = (x + y)·s - z·2s, so the view direction is
  (1, 1, 1) and depth is simply x + y + z. Default zoom s = 6.
- **Cost**: drawing is about 0.8 ms per frame at 300x80 on one core. Terminal
  bandwidth is the real cost: about 2.5 MB/s at 160x48 while scrolling (measured in a
  pty). Fine locally, heavy over SSH; colour quantization or a camera dead zone are
  the levers if that matters.

## The Recovery premise

What is being recovered? The marble is lost in a series of increasingly strange
geometric worlds — each level a different surface, different physics rules,
different hazards. The original Marble Madness had this quality already — each
world felt alien and specific. Recovery as a frame deepens it:

- Someone or something scattered the marbles across these worlds
- Each level is a different geometric reality with its own rules
- Recovery is not just navigation — it is understanding each world's logic
- Late game worlds become genuinely strange — physics that shouldn't work, does

## Original design notes (Mark Cerny, 1984)

Marble Madness was Mark Cerny's first major game, designed at age 18.
Now lead architect of PlayStation 4 and 5. The original had:
- Six worlds (Beginner, Intermediate, Aerial, Silly, Crazy, Ultimate)
- Two-player simultaneous (same marble, cooperative/competitive)
- Trackball input in arcade (mapped to keyboard/mouse for home versions)
- Time pressure as core mechanic — running out of time ends the level

Marble Recovery inherits the physics-first philosophy and the alien world
aesthetic. Two-player simultaneous is worth preserving — cooperative marble
recovery is a genuinely interesting multiplayer mode.

## Rights situation

Marble Madness is Atari — currently Atari SA (French company that purchased
the Atari name and IP catalog). Complex corporate history but Atari SA has
been relatively open to retro revival conversations recently.

Mark Cerny designed the original but the IP belongs to Atari, not Cerny.
However Cerny's involvement as a historical figure is worth acknowledging
in any outreach — his name attached to the original adds credibility.

Approach Atari SA's licensing department. Frame as a tribute revival on a
curated terminal gaming platform, non-commercial or revenue-share model.

## Open questions / next steps

- [x] Isometric grid calibration for terminal cell aspect ratio (half-blocks, see above)
- [x] Physics engine choice: no Rapier. Sphere vs triangles with closest-point
      push-out (the CGmadness / Neverball / Marble Blast approach) is ~100 lines in
      `src/game.rs`, deterministic, fixed 240 Hz step. Determinism makes replays,
      ghosts and network 2P cheap later.
- [x] Shadow rendering
- [x] Input: keyboard (Entity's kitty-protocol / repeat-timing tiers), mouse virtual
      trackball (hold button, push toward pointer), gamepad via `gilrs` (analog
      stick; SDL-mapped devices only, since gilrs also reports drawing-tablet key pads
      and 3D mice as gamepads; `MARBLE_ANY_PAD=1` overrides). Played with a real
      pad by David 2026-10-04: "It just works so extremely well with the gamepad!"
      The analog stick is the closest thing to the arcade trackball.
- [ ] Two-player simultaneous: same-keyboard needs the kitty protocol (legacy
      terminals only repeat the last held key); mouse + keys or pad + keys works
      anywhere; networked 2P over iroh like Tetrix
- [ ] Level design language: how strange does Recovery go beyond the original?
      Note from the spike: a half-unit cross-slope per cell is about as strong as
      full push, too steep for a bridge; a quarter-unit step is probably needed.
- [x] Level format: text files in `levels/`, compiled in (see "Levels and hazards")
- [x] First hazards: slime and gulpers
- [x] Tidal slime, bullies
- [x] Moving parts: pistons
- [ ] More hazards: tubes, catapults, sideways-moving platforms
- [ ] Time limits are generous (60 to 75 s against 8 to 12 s autopilot runs); tune
      once people have played them
- [ ] Rights: optional, not a blocker for an original-IP successor; stay clear of
      the name, the original level layouts and the enemy designs

## Story arc

The pun carries the story: to lose your marbles is to go mad, and you recover
them. Twelve marbles are lost; each level recovers one. Three acts, each with its
own palette (`theme:` in the level file) and its own relationship with physics:

- **Act I: Losing It** (levels 1 to 4, `classic`): the arcade original's grey tiles
  and blue cliffs. Normal rules.
- **Act II: The Madness** (5 to 10, `madness`): violet. The rules bend: ice, springs,
  low gravity ("physics that shouldn't work, does").
- **Act III: Recovery** (11 to 12, `dawn`): warm light, the way home.

A story card types itself out before each level when you arrive from the title or
from the previous win (`n`/`p` skip straight past), with the act heading on the
first level of each act, and the last level's `ending:` lines play after it. The
voice is wry and warm and plays on the idiom, never clinical. All the text lives in
the level files (`act:`, `story:`, `ending:`), so it can be rewritten without code.
"Recovered N of 12" (levels with a best time) shows on the title and in the HUD.

## Levels and hazards

**Every shipped level is an original design** (authored 2026-10-04 onward), not a
reproduction or adaptation of any Marble Madness course, and not taken from the GPL
references (CGmadness ships 24 levels; none are used). They can ship under whatever
license Marble Recovery picks. Keep it that way: new levels are designed fresh.

Format (`levels/*.lvl`, documented in `src/level.rs`): `name:`, `time:`, any number of
`eater:` patrols (`eater: 8,1 12,1 speed 1.2 chase 1.4`) and `bully:` homes
(`bully: 11,1 chase 3`), an optional `tide: period wet lag`, one or more `route:`
lines (cells to roll through, `w2.5` = wait 2.5 s), then `grid:` with one token per
cell (height in half units 0-9 A-Z, then modifiers: `> < v ^` slopes, `S` start, `G`
goal, `~` slime, `%` tidal slime, `|` piston, `*` ice, `!` spring, `.` void),
with optional `piston: period rise lag`, `gravity:` (default 18), `spring:` (launch
speed, default 9.5), and the story headers above.

**Level ids are the file name without its order number** (`08-piston-steps.lvl` is
`piston-steps`), so reordering levels never orphans best times or ghosts. Saved
data from before that is migrated on load (it was keyed with the number, and the
2026-10-04 reshuffle into acts briefly orphaned David's Piston Steps and Spire
times until this was fixed).

**The route lab** designs routes: `MR_LEVEL=7 MR_ROUTE="2,2 w? 10,2 ..." cargo test
--release route_lab -- --ignored --nocapture` plays a route, searches every `w?`
wait, and reports the outcome plus hazard pressure (soak, closest gulper and bully,
hunting time). Also test the careless line (full push, `--push` in a snapshot): a
level whose careful route passes is only interesting if careless play fails.

The level grids were first laid out with a small generator script (aligned columns);
the `.lvl` files are the source of truth now, edit them directly.

**Every `route:` is a solvability proof.** `cargo test` plays each one with a steering
autopilot, all hazards live, and fails unless it reaches the goal without losing the
marble once. Add a route for each intended line (Gulper Bog has three). Snapshot any
of them: `--snapshot out.ppm --level 4 --autoplay --route 2 --secs 5` (`--focus N`
centres on gulper N, for art work).

Hazards:
- **Slime** (`~`): heavy drag, 60% push, and a marble left in it 1.5 s dissolves
  (meter in the HUD, drains again on clean ground). A quick crossing is survivable.
- **Gulpers** (our marble-eaters; deliberately not the original's Marble Munchers):
  walk a patrol path, hunt a marble inside their `chase` range, never step off their
  own level (a ledge is a safe wall), chew for 2 s after a meal. Freshly respawned
  marbles get 1.5 s of grace.
- **Tidal slime** (`%`): slime for part of each tide, dry olive silt otherwise, and
  it bubbles for 0.9 s before it floods. Cells lag each other along x + y, so the
  tide rolls across the level as a wave (negative lag rolls it toward the start).
- **Bullies** (original; iron marbles 2.5x your mass): roll straight at you when you
  come within `chase`, glowing red, and knock you with real momentum exchange
  (restitution 0.7). They share the marble's physics exactly (`roll()` in
  `game.rs`), slime slows them too, and they ignore edges: lure them off, and they
  return home 3 s later.
- **Pistons** (`|`): blocks that rise `rise` units above their written height and
  back, eased, once per `period`, lagged along x + y. The level rebuilds just their
  faces and their neighbours' walls each step (`Level::set_clock`). On their own
  they are safe (push against a lift's wall and it comes level), so a piston level
  needs another pressure; Piston Steps puts a gulper on the last landing.
- **Ice** (`*`): 30% push and almost no drag; you coast and steering bites late.
- **Springs** (`!`): throw anything that lands on them (bullies too) up at the
  level's `spring:` speed, once per landing. Hold toward the next terrace in the air.
- **Gravity** (`gravity:`): per level. At 6, falls are long and soft and you steer
  across gaps in the air; a careless full push still sails past and shatters.
- **Hard landings** shatter at 10.5 u/s normal impact (a drop of about 3 units at
  normal gravity).

**Respawn points are never inside a hazard's reach.** A cell within a gulper's
patrol or a bully's home range (plus both radii and a margin) is never chosen, so a
loss sends you back a little rather than into the same jaws (`can_respawn_at`,
`menace_map` in `game.rs`). A test sits on every possible respawn point on every
level for 6 s at three moments in the hazards' cycles; it found 38 bad points across
5 levels when first written. Another test plays 2 minutes of random input on every
level, checking nothing goes non-finite, every loss recovers, nothing piles up.

The marble gets an x-ray outline wherever the world hides it (a risen piston in
front of it hid it completely without one).

**Check hazards really engage, not just that routes pass.** Two levels passed their
routes first time without the hazard ever touching the marble (Low Tide's wave rolled
with the player at the player's speed). Probe peak slime soak, gulper and bully
closest approach, and hunting/anger time along each route; a hazard with zero
pressure is decoration. Low Tide's waits were then found by searching, not guessed.

Level slate (file order is play order):
1. First Descent (Act I): ramp, tilted bridge, safe drop
2. Slime Terraces: three terraces, one clean winding line across each
3. The Gulper Yard: an open yard, a patrolling gulper, a hunter, a sentinel
4. Low Tide: a tidal flat with the wave rolling at you; a straight dash dissolves,
   wait on the start (about 1 s) and two islands (about 4 s and 6 s)
5. Gulper Bog (Act II): two contested dry causeways, or unguarded stepping stones
   that only meet at their corners (guarded lines: near misses at 0.6 to 0.9 units)
6. Bully Ridge: a bully charging along a two-wide ridge, two more in a plaza with a
   hole to lure them into (contact made, two bullies fell off chasing)
7. Thin Ice: ice runs between grippy corner pads, void all round
8. Piston Steps: the socket is 4.5 units up, three phased lifts are the only way;
   wait for the last lift in the far corner, or the gulper gets you after about 3 s
9. Spring Fever: springs up three terraces; the careless line loses a marble
10. Weightless: gravity 6, leap the gaps and steer in the air; full push sails past
    and shatters
11. Switchback Spire (Act III): long descent, a guard punishing overshoot, slime
12. The Long Way Home: slime zig-zag, an ice slide onto a gulper's landing (first
    draft: the slide was too fast to stop before the gulper; shortened, pad
    deepened), a tidal strip (peak soak 0.99 of 1.5), a spring to the last socket

## Reference sources

In `references/Madness/`: CGmadness (GPL-2.0+; block grid level model, `ball/Ball.cpp`
physics), Trackballs (GPL-2), marble-madness-3d (no license, read only), Wireframe #34
(CC BY-NC-SA, NonCommercial, nothing to take).

Better sources found 2026-10-04 (licenses checked against the repos):
- Marble Love, github.com/magno73/marble-love (MIT for its code, no Atari data):
  TypeScript reimplementation of the 1984 arcade code from the disassembly;
  `docs/level-header-format.md`, `packages/engine/src/` (`trackball-apply.ts`,
  `move-velocity.ts`, `position-update.ts`). The real game's logic.
- Neverball, github.com/Neverball/neverball (GPL-2.0+): `share/solid_sim_sol.c`,
  a compact swept-sphere solver.
- Marble Blast Web, github.com/vanilagy/MarbleBlast (MIT): `src/ts/marble.ts`,
  `src/ts/physics/`. The well-loved Marble Blast feel.
- Mark Cerny, Classic Game Postmortem: Marble Madness, GDC 2011 (free on GDC Vault).
- IsometricBlocks, github.com/shaunlebron/IsometricBlocks (ISC): draw-order sorting.

## Catalog position

Marble Recovery sits alongside Aurora (the ski game) as the contemplative
physics experiences in the catalog. Not twitchy, not strategic — pure spatial
reasoning and momentum feel. Different emotional register from Entity or
Bob-sleigh. The player who loves Marble Recovery is not necessarily the same
player who loves Tetrix.

## Status

Playable (2026-10-04): title screen with level select and best times (saved to
`$XDG_DATA_HOME/marble-recovery/best-times.txt`, default `~/.local/share/...`), a
ghost of your best run on each level to race (`.../marble-recovery/ghosts/`, inputs
per physics step, quantized so replay is bit-exact; bump `FORMAT` in `ghost.rs`
whenever physics changes, or old ghosts drift), an attract mode playing the
highlighted level's proof route behind the menu, pause (Space), twelve
original levels in a three-act story with story cards and an ending, slime, tidal
slime, pistons, ice, springs, per-level gravity, gulpers, bullies, shatter, timer, camera that
looks ahead along your motion, session logging per the Asciiverse convention. 23
tests: every level route completes with zero losses, each hazard mechanic, the
best-times file, and bit-exact ghost replay on every level. Own git repo, private
at github.com/theasciiverse/marble-recovery.

```bash
cargo run --release                       # title screen
cargo run --release -- --level 4          # straight into a level
cargo test --release                      # levels completable + hazard mechanics
RUST_LOG=debug cargo run --release 2>marble.log
# a frame as an image plus out.ppm.txt with the frame's text (add --title for the menu,
# --ghost-route R to race a recording of route R, --focus N to centre on gulper N)
cargo run --release -- --snapshot out.ppm --level 4 --autoplay --secs 6 --size 160x48
```

Conceived 2026-07-02, scout camp.
