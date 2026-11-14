# Going round in circles: a catalogue

Where two of her rules, each right by itself, hand her to and fro. Found by
`tools/dev/cycles.py` over one run of every level on both skills, 2026-11-13. It's a list of
what to trace, not of what's wrong: of the entries below only the first two were read in the
logs, and none has been traced to its rule.

## How it was made

- **The runs:** all 68 levels (DOOM E1-E4, DOOM II), Hurt Me Plenty and Ultra-Violence, a pistol
  start, one life, one seed each (1003 and 1004): 136 runs, 63,608 s of game time. The build is
  de419dd (the lost-sight cap in). The logs are in `out/cycles/`, the stretches found in
  `out/cycles/stretches.tsv`.
- **A change of state** is a new move of the fight's, a new objective of the explorer's, or the
  route follower's "stuck", "off course", "moved unexpectedly". Monsters' and things' names are
  left out, so the same to-and-fro at an imp and at a baron counts as one kind.
- **A stretch** is ten changes or more, none more than 150 tics after the last, among at most
  four states, two of which come round three times or more. Stretches of the same two states
  within 350 tics of each other are one.
- **What it can't tell:** a hard fight is also many changes among few states (backing off,
  dodging, standing). So it finds 1,108 stretches, 10,414 s, in 130 of the 136 runs, and most of
  that is fighting. The two sortings below are what pick the cycles out: long with nothing
  killed, and turning several times a second.

To do it again: `python3 tools/dev/cycles.py out/cycles/*.txt` (`--list` for every stretch,
`--tsv FILE` for the table).

## Long loops: 30 s or more

25 stretches, 3,686 s. With what she killed and was hurt meanwhile: nothing killed over minutes
is a loop, not a fight.

| what goes round | stretches, runs | seconds | kills | hurt | the longest |
|---|---|---|---|---|---|
| out of ammo: away from them ↔ out of ammo: getting past | 8 in 1 | 2,675 | 0 | 5 | doom2 MAP06 UV, tic 57482 |
| shooting on the way ↔ stepping for a shot (its way blocked) | 1 in 1 | 232 | 0 | 0 | doom2 MAP14 HMP, tic 23296 |
| standing and fighting ↔ stepping for a shot | 4 in 4 | 190 | 9 | 38 | doom2 MAP22 HMP, tic 5490 |
| out of ammo: away from them ↔ route: moved unexpectedly | 1 in 1 | 151 | 0 | 30 | doom E4M4 UV, tic 7734 |
| dodging ↔ outmatched: keeping clear | 3 in 3 | 118 | 4 | 16 | doom E4M2 UV, tic 582 |
| spotted one ↔ route: stuck | 1 in 1 | 59 | 0 | 0 | doom E4M9 HMP |
| closing in (out of reach) ↔ dodging | 1 in 1 | 52 | 3 | 4 | doom E2M4 UV |
| out of ammo, none to get to ↔ try the wall | 1 in 1 | 46 | 0 | 0 | doom2 MAP06 UV |
| try the wall ↔ walk across a sector | 1 in 1 | 36 | 1 | 0 | doom E3M3 HMP |
| backing off ↔ stepping for a shot | 1 in 1 | 35 | 0 | 20 | doom E3M3 HMP |
| outmatched: keeping clear (ammo kept) ↔ stepping for a shot | 1 in 1 | 31 | 0 | 121 | doom E2M8 HMP |
| lost sight: keeping at it ↔ route: moved unexpectedly | 1 in 1 | 31 | 0 | 0 | doom2 MAP14 HMP |
| backing off ↔ shooting | 1 in 1 | 30 | 0 | 0 | doom2 MAP08 HMP |

The two read in the logs:

- **Out of ammo, MAP06 UV.** Out of everything with imps about and no ammo she knows of, she
  goes between "away from them" and "getting past" one every second or two, for most of an
  hour of game time (the run ends at the 3,600 s limit, 24 of 112 killed). 394 times. Out of
  ammo shows in three more rows (E4M4 UV 151 s, MAP06's walls, and MAP14 UV's "away from them ↔
  pick up to heal", 19 short stretches, 169 s). The largest thing here by far, and one run.
- **Its way blocked, MAP14 HMP.** An arachnotron 700 off, plasma up: "holding fire: plasma
  wouldn't get there", "stepping for a shot (its way blocked)", "shooting on the way", four
  times a second, then "lost sight: keeping at it" and round again, 232 s.

## Flickers: three changes a second or more

Two moves at the same target passing her legs or her aim back and forth several times a second.
Short each time, but in half the runs. 2,582 s in all, 4% of the game time.

| what goes round | stretches, runs | seconds | a second |
|---|---|---|---|
| standing and fighting ↔ stepping for a shot | 151 in 68 | 839 | 4.5 |
| shooting on the way ↔ standing and fighting | 123 in 52 | 510 | 4.9 |
| shooting on the way ↔ stepping for a shot | 86 in 51 | 367 | 5.4 |
| shooting on the way ↔ stepping for a shot (its way blocked) | 6 in 6 | 261 | 4.4 |
| outmatched: keeping clear ↔ stepping for a shot | 23 in 16 | 159 | 3.4 |
| outmatched: keeping clear ↔ the same, on along the route | 32 in 23 | 123 | 4.1 |
| backing off ↔ shooting on the way | 16 in 12 | 107 | 3.7 |
| out of reach: going on ↔ outmatched: keeping clear | 19 in 13 | 102 | 3.3 |
| outmatched: keeping clear (ammo kept) ↔ stepping for a shot | 7 in 5 | 67 | 3.8 |
| away from a barrel ↔ shooting on the way | 10 in 8 | 49 | 3.0 |

Seven of the ten have "stepping for a shot" or "shooting on the way" on one side: a shot that's
there one tic and not the next (the step taken gets it, the move changes, the shot's lost). She
does kill through them (the first three rows: 298 kills meanwhile), so they aren't dead time;
what they cost is unmeasured.

## Fights, not cycles

The rest of the 10,414 s: backing off ↔ dodging (370 s, 88 kills, hurt 157 a minute), dodging ↔
keeping clear, backing off ↔ standing, closing in ↔ standing, and the explorer going from one
look to the next. Many changes among few states because that's what a fight or a room is.
Left alone.

## The one fixed, for scale

"Lost sight: keeping at it ↔ shooting on the way" was 133 times round on E1M7 UV before the cap
(de419dd, [decisions](decisions/05-stream-and-directed-play.md)). After it, in these runs: 13
stretches in 10 runs, 133 s, the longest 29 s.

## An order to take them in

1. **Out of ammo: away ↔ getting past.** One run, but 2,675 s of it, and the same shape as
   the lost-sight loop: the same monsters, no shot, round again. The cap's ledger
   (`patience.rs`) is the likely tool; what she should do instead, out of ammo with none known,
   is the question to settle first.
2. **Stepping for a shot ↔ standing / shooting on the way.** In half the runs. One rule's
   answer flips the other's every few tics: a little holding to the step once taken (as the
   fight's darts are held) may be all it wants. Measure what it costs before touching it.
3. **Its way blocked** (plasma that wouldn't get there, MAP14): one long one; read with 2.
4. The single stretches under a minute: as they come up on stream.

One seed a level-skill is a thin sample for the rare long loops: MAP06's is one run. More seeds
before deciding any of them is common.
