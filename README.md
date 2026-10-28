# Marble Recovery

**THEY SAY YOU'VE LOST YOUR MARBLES.**

They're right. Twelve of them, rolled off into places that should not exist.
Time to go and get them back, one at a time.

No jump button. No fire button. Just a marble, gravity and your nerve. Ease it
down the slopes, coax it along the ledges and keep it rolling, because one hard
landing and it shatters. Slime that waits for you to stop, Gulpers who can't
tell a marble from a snack, a tide that keeps its own time, iron bully marbles
who come straight at you. Then the madness sets in: ice, rising floors, springs
and a world where falling takes its time. Physics that shouldn't work, does!

Twelve hand-built levels in three acts (Losing It, The Madness, Recovery), a
ghost of your best run to race on every level, and a gamepad stick that feels
like an arcade trackball. After Marble Madness (Mark Cerny, Atari, 1984); every
level here is a new design.

## Play

With [av](https://asciiverse.sh), the Asciiverse client:

```
av install marble-recovery
av play marble-recovery
```

To build it yourself: `cargo run --release` (`--level 4` goes straight into a
level).

## Keys

| Key | |
|---|---|
| arrows, W A S D, H J K L | roll |
| mouse | hold a button and push toward the pointer, like a trackball |
| Enter, Space | go on; Space pauses during play |
| R | restart the level |
| N / P | next / previous level |
| + / - | zoom |
| Esc | back |
| Q | quit |

Gamepad: the stick rolls.

Best times and ghosts are saved in `~/.local/share/marble-recovery/`. Session
log for bug reports: `RUST_LOG=debug marble-recovery 2>marble.log`

## Licence

GNU General Public License, version 3 or (at your option) any later
version; see [LICENSE](LICENSE).
