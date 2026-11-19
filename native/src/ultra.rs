// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Ultra instinct: the fight searched in the game itself (docs/design/ultra-instinct-plan.md).
//!
//! A prototype of the plan's stages 1 to 3, offline and on one core: the game saved, each
//! thing she could do next played forward in the real engine from that save, what it leaves
//! weighed, the best kept and carried on from (a beam, to a second or two), the first move of
//! the best line made for real. A line she dies on is never chosen; with none she lives on,
//! the game goes back a second or more and it's tried another way.
//!
//! It is the opposite of realism, on purpose: it plays what the monsters will do and what
//! every shot will roll. Never her play by default, never in the score. Where to go is still
//! the explorer's: the way on is a term in what a state is worth, and with nothing after her
//! she plays as she always does.

use crate::dsda::{Mobj, PlayerState, MF_CORPSE, MF_SHOOTABLE};
use crate::engine::{Engine, Mark};
use crate::geom::{dist, Point};
use crate::motor::{heading, norm, Controls};
use crate::play::Agent;
use crate::sweep::RunOptions;
use crate::tactical::bestiary::monster;
use std::time::Instant;

/// What the search goes by. (A prototype's numbers, here till the mode is kept: then
/// `tactical/tuning.rs`, as any judgement number.)
#[derive(Clone, Debug)]
pub struct Tune {
    /// a move is this many tics
    pub move_tics: usize,
    /// moves looked ahead
    pub depth: usize,
    /// lines kept at each
    pub width: usize,
    /// the most she turns in a tic, degrees, and the most that changes from one tic to the next:
    /// her hand's, as when she plays (`aim.hand_turn`, `aim.hand_accel`)
    pub turn: f64,
    pub turn_accel: f64,
    /// a point of her health, and of her armor
    pub health: f64,
    pub armor: f64,
    /// a point of a monster's health left, for one of threat 10 (more for worse)
    pub monster: f64,
    /// a unit of the way on still to go
    pub way: f64,
    /// a missile about to land on her, at the most
    pub missile: f64,
    /// something that claws within its reach
    pub claw: f64,
    /// a key in hand; a sector moved since the look began (a door opened, a lift called)
    pub key: f64,
    pub moved: f64,
    /// a round in hand
    pub ammo: f64,
    /// standing on a floor that hurts that her way on doesn't cross
    pub hot: f64,
    /// stood within `eager_room` of one place, the way on counts for as much again every
    /// `eager_tics`, up to `eager_most` times more: or with something in sight that never
    /// comes and never goes she stands where nothing hits her for good
    pub eager_room: f64,
    /// ...with none of them after her within this, and nothing of theirs in the air within
    /// twice it
    pub eager_clear: f64,
    /// pressed, it counts for more this many times more slowly
    pub eager_pressed: f64,
    pub eager_tics: i64,
    pub eager_most: f64,
    /// a move that was to take her somewhere and took her less than `pushed` units: she was
    /// pressing on a wall, and standing still is the same and looks like nothing's wrong
    pub push: f64,
    pub pushed: f64,
    /// out of the level
    pub exit: f64,
    /// one of them in her sight, or anything of theirs in the air within this: the search has
    /// her; else she plays as she does
    pub near: f64,
    /// ...or one of them after her within this, in sight or not
    pub close: f64,
    /// the game kept this often (tics) to go back to (thinned the further back: `keep`)
    pub keep_every: i64,
    /// gone back to the same one this often, the one before it instead
    pub tries: u32,
    /// how far a line's worth is shaken on a second go, so it isn't the same line again
    pub shake: f64,
    /// hurt this much a second (health and armor together) over `hurt_tics`, the game goes
    /// back to before it and it's tried another way, `hurt_tries` times; hurt as much every
    /// time, the last go is let stand (0: only a death takes it back)
    pub hurt: f64,
    pub hurt_tics: i64,
    pub hurt_tries: u32,
}

impl Default for Tune {
    fn default() -> Self {
        Tune {
            move_tics: 6,
            depth: 8,
            width: 8,
            turn: 18.0,
            turn_accel: 10.0,
            health: 10.0,
            armor: 3.0,
            monster: 0.4,
            way: 0.35,
            missile: 60.0,
            claw: 80.0,
            key: 3000.0,
            moved: 60.0,
            ammo: 0.3,
            hot: 600.0,
            eager_room: 128.0,
            eager_clear: 512.0,
            eager_pressed: 5.0,
            eager_tics: 105,
            eager_most: 12.0,
            push: 40.0,
            pushed: 8.0,
            exit: 1.0e6,
            near: 1500.0,
            close: 256.0,
            keep_every: 35,
            tries: 3,
            shake: 80.0,
            hurt: 8.0,
            hurt_tics: 105,
            hurt_tries: 3,
        }
    }
}

/// Which way she goes in a move.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Go {
    Still,
    /// a compass way, degrees
    Way(f64),
    /// along the way on
    On,
}

/// A move: `Tune::move_tics` of going one way, facing and shooting or not.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Move {
    /// going `go`; `fire`: turned to the nearest of them and shooting, else as she faces
    Plain { go: Go, fire: bool },
    /// what she'd have done herself this tic (a door pressed, a gun changed), then on along the
    /// way
    Hers(Controls),
}

/// What a look ahead is from, kept through it.
struct Root {
    way: Vec<Point>,
    /// the hurting floors her way on goes over (hers to wade: not held against a line)
    wades: Vec<usize>,
    /// how much more the way on counts for her having stood about (1: not)
    eager: f64,
    sectors: Vec<(f64, f64)>,
    keys: i32,
    /// shaken by this (0: not)
    shake: u64,
}

/// A game kept to go back to.
struct Kept {
    slot: i32,
    mark: Mark,
    tries: u32,
    /// gone back to for a hurt (`hurt_back`), this often
    hurt: u32,
}

/// How a run went.
#[derive(Debug, Default)]
pub struct Tally {
    pub decisions: u64,
    pub played: u64,
    pub restores: u64,
    pub saves: u64,
    pub back: u32,
    pub searched_secs: f64,
    /// the longest look, in seconds
    pub worst_look: f64,
    pub ultra_tics: i64,
    /// moves made for real, and those of them that were to take her somewhere and didn't
    pub moves: u64,
    pub pressed: u64,
    /// gone back for a hurt, and hurts let stand after every go at them
    pub hurt_back: u32,
    pub hurt_stood: u32,
}

fn alive(m: &Mobj) -> bool {
    m.flags & MF_SHOOTABLE != 0 && m.flags & MF_CORPSE == 0 && m.health > 0
}

/// The thing's kind, if it's a monster.
fn monster_of(m: &Mobj) -> Option<crate::tactical::bestiary::Monster> {
    crate::dsda::type_name(m.kind).and_then(monster)
}

/// A missile of theirs (not her own: the player's kind is 0).
fn hostile_missile(m: &Mobj) -> bool {
    m.owner_type > 0 && m.momx.hypot(m.momy) > 1.0
}

/// Is she in a fight this tic: one of them in her sight (`seen`), anything of theirs in the
/// air within `near`, or one of them after her right by her (round a corner, at her back)?
/// One after her somewhere out of sight is no fight: with the last of them a room away the
/// search held her standing still for 21 seconds, her way on a straight line through a wall,
/// till her own route gave up as stuck (the maintainer, watching E2M9 seed 1).
fn threatened(p: &PlayerState, things: &[Mobj], seen: bool, near: f64, close: f64) -> bool {
    let pos = (p.x, p.y);
    seen || things.iter().any(|m| {
        let d = dist((m.x, m.y), pos);
        (hostile_missile(m) && d < near)
            || (alive(m) && monster_of(m).is_some() && m.after_her != 0 && d < close)
    })
}

/// How far the way on still goes from `pos`: to the nearest point of it, and on along it.
fn way_left(way: &[Point], pos: Point) -> f64 {
    if way.len() < 2 {
        return way.first().map_or(0.0, |&g| dist(g, pos));
    }
    let mut rest: Vec<f64> = vec![0.0; way.len()];
    for i in (0..way.len() - 1).rev() {
        rest[i] = rest[i + 1] + dist(way[i], way[i + 1]);
    }
    let mut best = f64::INFINITY;
    for i in 0..way.len() - 1 {
        let (a, b) = (way[i], way[i + 1]);
        let len = dist(a, b).max(1e-9);
        let t = (((pos.0 - a.0) * (b.0 - a.0) + (pos.1 - a.1) * (b.1 - a.1)) / (len * len))
            .clamp(0.0, 1.0);
        let q = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
        best = best.min(dist(q, pos) + (1.0 - t) * len + rest[i + 1]);
    }
    best
}

/// The point of the way on to make for from `pos`: the first a little way off.
fn way_point(way: &[Point], pos: Point) -> Option<Point> {
    way.iter()
        .copied()
        .skip(1)
        .find(|&q| dist(q, pos) > 24.0)
        .or(way.last().copied())
}

/// A number from three, for shaking (splitmix64).
fn mix(a: u64, b: u64, c: u64) -> f64 {
    let mut z = a
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(b.wrapping_mul(0xBF58_476D_1CE4_E5B9))
        .wrapping_add(c.wrapping_mul(0x94D0_49BB_1331_11EB));
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    ((z ^ (z >> 31)) % 10_000) as f64 / 10_000.0
}

pub struct Ultra {
    pub tune: Tune,
    pub tally: Tally,
    /// the move under way, and its tics left
    doing: Option<(Move, usize)>,
    /// the move just played went nowhere, told to go
    pushed: bool,
    /// the level's floors (which hurt)
    pub hot: Option<std::rc::Rc<crate::navigator::Navigator>>,
    /// where she's stood about, and since when
    stood: (Point, i64),
    /// how long she's stood about, in `eager_tics` (slower while pressed)
    idle: f64,
    /// her own controls till this tic (the search had nothing better than standing)
    let_be: i64,
    /// her turn the tic before, in whichever game is being played (the hand's limit is from it)
    was: f64,
    /// where the move under way began
    began: Option<Point>,
    kept: Vec<Kept>,
    kept_at: i64,
    /// shaking, till this tic (after going back)
    shaken: (u64, i64),
    sectors: Vec<(f64, f64)>,
    pub log: Vec<String>,
}

/// slots: 0 the look's root; 1.. the beam's lines, two rows of them; from `KEPT0` the games
/// kept to go back to; `START_SLOT` the game as the run began
const START_SLOT: i32 = 500;
const ROW: i32 = 64;
const KEPT0: i32 = 200;
const KEPT_SLOTS: i32 = 128;

impl Ultra {
    pub fn new(tune: Tune) -> Ultra {
        Ultra {
            tune,
            tally: Tally::default(),
            doing: None,
            pushed: false,
            hot: None,
            stood: ((f64::MAX, f64::MAX), 0),
            let_be: i64::MIN,
            idle: 0.0,
            was: 0.0,
            began: None,
            kept: Vec::new(),
            kept_at: i64::MIN / 2,
            shaken: (0, i64::MIN / 2),
            sectors: Vec::new(),
            log: Vec::new(),
        }
    }

    /// The controls of one tic of `mv`, as the game stands.
    fn controls(
        &self,
        mv: &Move,
        first: bool,
        p: &PlayerState,
        root: &Root,
        hers: Option<&Controls>,
        seen: Option<Point>,
    ) -> Controls {
        let pos = (p.x, p.y);
        let (go, fire) = match *mv {
            Move::Hers(c) if first => return c,
            Move::Hers(_) => (Go::On, false),
            Move::Plain { go, fire } => (go, fire),
        };
        let mut c = Controls {
            run: true,
            ..Controls::default()
        };
        let mut facing = p.angle;
        // Not shooting, the turn is her own, and the press and the gun she'd change to: where
        // she goes is by the compass, so which way she faces changes nothing a look ahead
        // counted on, and it's facing a door and pressing it that opens it. (Played for real
        // only: a tic looked ahead has no her to ask.)
        if let Some(h) = hers {
            c.use_ = h.use_;
            c.weapon = h.weapon;
            c.exact = h.exact;
            if !fire {
                c.turn = h.turn.clamp(-self.tune.turn, self.tune.turn).clamp(
                    self.was - self.tune.turn_accel,
                    self.was + self.tune.turn_accel,
                );
                facing = p.angle - c.turn;
            }
        }
        if fire {
            // (the nearest of them she has a line of sight to, by the game's own check: the
            // turn's worked out, not searched. The nearest whatever was between had her
            // shooting rockets into the wall it stood behind)
            let target = seen;
            if let Some(t) = target {
                // (her hand's limits, as when she plays: so fast and no faster, and no jerk
                // from the tic before)
                let off = norm(heading(pos, t) - p.angle);
                let (most, accel) = (self.tune.turn, self.tune.turn_accel);
                c.turn = (-off)
                    .clamp(-most, most)
                    .clamp(self.was - accel, self.was + accel);
                facing = p.angle - c.turn;
                c.attack = (off + c.turn).abs() < 4.0;
            }
        }
        let way = match go {
            Go::Still => None,
            Go::Way(w) => Some(w),
            Go::On => way_point(&root.way, pos).map(|q| heading(pos, q)),
        };
        if let Some(w) = way {
            let rel = norm(w - facing).to_radians();
            c.forward = 50.0 * rel.cos();
            c.strafe = -40.0 * rel.sin();
        }
        c
    }

    /// `mv` played from where the game stands. False: dead in it.
    fn play(&mut self, e: &mut Engine, mv: &Move, root: &Root) -> bool {
        let fires = matches!(mv, Move::Plain { fire: true, .. });
        let goes = !matches!(mv, Move::Plain { go: Go::Still, .. });
        let mut from: Option<Point> = None;
        self.pushed = false;
        for i in 0..self.tune.move_tics {
            let c = {
                let p = e.game.player();
                if p.playerstate != 0 || p.health <= 0 {
                    return false;
                }
                from.get_or_insert((p.x, p.y));
                let seen = if fires { Self::seen(e) } else { None };
                self.controls(mv, i == 0, &p, root, None, seen)
            };
            e.step(&c);
            self.was = c.turn;
            self.tally.played += 1;
            if e.exited() {
                return true;
            }
        }
        let p = e.game.player();
        // (told to go and gone nowhere: she's been pressing on a wall)
        self.pushed = goes && from.is_some_and(|f| dist(f, (p.x, p.y)) < self.tune.pushed);
        p.playerstate == 0 && p.health > 0
    }

    /// Where the nearest of them she has a line of sight to is.
    fn seen(e: &Engine) -> Option<Point> {
        e.game.nearest_seen().map(|(_, at)| (at.0, at.1))
    }

    /// What the game as it stands is worth.
    fn worth(&mut self, e: &mut Engine, root: &Root) -> f64 {
        let t = self.tune.clone();
        if e.exited() {
            return t.exit;
        }
        e.game.sectors(&mut self.sectors);
        let moved = self
            .sectors
            .iter()
            .zip(&root.sectors)
            .filter(|(a, b)| (a.0 - b.0).abs() > 0.5 || (a.1 - b.1).abs() > 0.5)
            .count()
            .min(4);
        let (p, things) = e.peek();
        let pos = (p.x, p.y);
        let mut w = f64::from(p.health) * t.health + f64::from(p.armor) * t.armor;
        w += f64::from(p.ammo.iter().sum::<i32>()) * t.ammo;
        w += f64::from(p.cards.iter().filter(|&&c| c != 0).count() as i32 - root.keys) * t.key;
        w += moved as f64 * t.moved;
        w -= way_left(&root.way, pos) * t.way * root.eager;
        // On a floor that hurts that her way on doesn't cross: it costs nothing the tic she steps
        // onto it and the look is a second and a half, so a lava river read as the way to take,
        // and every way out of it was further back than there were games kept (the maintainer,
        // watching E4M6).
        if let Some(sec) = self.hot.as_ref().and_then(|nav| {
            let sec = nav.sector_of_poly(nav.poly_at(pos)?);
            (nav.damage.get(sec).is_some_and(|&d| d != 0)).then_some(sec)
        }) {
            if !root.wades.contains(&sec) {
                w -= t.hot;
            }
        }
        for m in things {
            if hostile_missile(m) {
                // (when it's nearest her as it flies, and how near)
                let (rx, ry) = (pos.0 - m.x, pos.1 - m.y);
                let v2 = m.momx * m.momx + m.momy * m.momy;
                let when = (rx * m.momx + ry * m.momy) / v2;
                if when > 0.0 && when < 35.0 {
                    let miss = (m.x + m.momx * when - pos.0).hypot(m.y + m.momy * when - pos.1);
                    if miss < 48.0 {
                        w -= t.missile * (1.0 - when / 35.0) * (1.0 - miss / 48.0);
                    }
                }
                continue;
            }
            if !alive(m) {
                continue;
            }
            let Some(kind) = monster_of(m) else { continue };
            w -= f64::from(m.health) * t.monster * (kind.threat / 10.0).max(0.3);
            if kind.melee && dist((m.x, m.y), pos) < 72.0 + kind.radius {
                w -= t.claw;
            }
        }
        w
    }

    /// The moves to try from a line (`first`: the look's first, where what she'd have done
    /// herself is one of them).
    fn moves(hers: Option<Controls>) -> Vec<Move> {
        let mut out = Vec::new();
        for fire in [false, true] {
            out.push(Move::Plain {
                go: Go::Still,
                fire,
            });
            out.push(Move::Plain { go: Go::On, fire });
            for k in 0..8 {
                out.push(Move::Plain {
                    go: Go::Way(f64::from(k) * 45.0),
                    fire,
                });
            }
        }
        out.extend(hers.map(Move::Hers));
        out
    }

    /// The look ahead from where the game stands: the first move of the best line, and what
    /// the line's worth. None: she dies on every one.
    fn look(&mut self, e: &mut Engine, root: &Root, hers: Controls) -> Option<(Move, f64)> {
        let t0 = Instant::now();
        let mark0 = e.save(0)?;
        self.tally.saves += 1;
        self.tally.decisions += 1;
        let was0 = self.was;
        let mut lines: Vec<(i32, Mark, Option<Move>, f64)> = vec![(0, mark0.clone(), None, was0)];
        let mut best: Option<(Move, f64)> = None;
        for depth in 0..self.tune.depth {
            // (every move from every line kept: played, weighed)
            let mut tried: Vec<(f64, usize, Move)> = Vec::new();
            for (li, (slot, mark, _, was)) in lines.iter().enumerate() {
                for mv in Self::moves((depth == 0).then_some(hers)) {
                    e.restore(*slot, mark);
                    self.tally.restores += 1;
                    self.was = *was;
                    if !self.play(e, &mv, root) {
                        continue;
                    }
                    let pushed = self.pushed;
                    let mut w = self.worth(e, root);
                    if pushed {
                        w -= self.tune.push;
                    }
                    if root.shake != 0 {
                        w += self.tune.shake
                            * mix(
                                root.shake,
                                mark.tic as u64,
                                tried.len() as u64 + li as u64 * 97,
                            );
                    }
                    tried.push((w, li, mv));
                }
            }
            if tried.is_empty() {
                break; // (dead whatever she does from here: what's found so far, if anything)
            }
            tried.sort_by(|a, b| b.0.total_cmp(&a.0));
            tried.truncate(self.tune.width);
            // (the best of them played again and kept, to go on from)
            let row = 1 + ROW * (depth as i32 % 2);
            let mut next: Vec<(i32, Mark, Option<Move>, f64)> = Vec::new();
            for (k, (w, li, mv)) in tried.iter().enumerate() {
                let (slot, mark, first, was) = &lines[*li];
                let first = first.unwrap_or(*mv);
                if k == 0 {
                    best = Some((first, *w));
                }
                if depth + 1 == self.tune.depth {
                    break; // (the last: nothing goes on from it)
                }
                e.restore(*slot, mark);
                self.tally.restores += 1;
                self.was = *was;
                if !self.play(e, mv, root) || e.exited() {
                    continue;
                }
                let s = row + k as i32;
                let Some(m) = e.save(s) else { continue };
                self.tally.saves += 1;
                next.push((s, m, Some(first), self.was));
            }
            if next.is_empty() {
                break;
            }
            lines = next;
        }
        e.restore(0, &mark0);
        self.was = was0;
        self.tally.restores += 1;
        let took = t0.elapsed().as_secs_f64();
        self.tally.searched_secs += took;
        self.tally.worst_look = self.tally.worst_look.max(took);
        best
    }

    /// The game kept to go back to: one every `keep_every`, and of those kept before, fewer the
    /// further back (the maintainer's: a second ago, two, four, eight, sixteen: about two for
    /// each doubling of how long ago, the oldest and the newest of it). So a few dozen reach
    /// back over the whole run, and a mistake made minutes before can be gone back past; a flat
    /// minute of them ran out 100 seconds into E4M6 with her in the lava.
    fn keep(&mut self, e: &mut Engine) {
        if e.tic - self.kept_at < self.tune.keep_every {
            return;
        }
        let Some(slot) =
            (KEPT0..KEPT0 + KEPT_SLOTS).find(|s| self.kept.iter().all(|k| k.slot != *s))
        else {
            return;
        };
        let Some(mark) = e.save(slot) else { return };
        self.tally.saves += 1;
        self.kept.push(Kept {
            slot,
            mark,
            tries: 0,
            hurt: 0,
        });
        self.kept_at = e.tic;
        // (thinned: by how many doublings ago each is, the first and the last of each kept)
        let (now, every) = (e.tic, self.tune.keep_every.max(1));
        let ago = |k: &Kept| (((now - k.mark.tic) / every).max(1) as u64).ilog2();
        let n = self.kept.len();
        let keep: Vec<bool> = (0..n)
            .map(|i| {
                let b = ago(&self.kept[i]);
                i == 0 || i + 1 == n || ago(&self.kept[i - 1]) != b || ago(&self.kept[i + 1]) != b
            })
            .collect();
        let mut i = 0;
        self.kept.retain(|k| {
            let stays = keep[i];
            i += 1;
            if !stays {
                e.game.forget(k.slot);
            }
            stays
        });
    }

    /// Back to the last game kept (the one before, gone back to too often). The tics gone
    /// back, or None: nothing kept to go back to.
    fn go_back(&mut self, e: &mut Engine) -> Option<i64> {
        let now = e.tic;
        while self.kept.last().is_some_and(|k| k.tries >= self.tune.tries) {
            self.kept.pop();
        }
        let k = self.kept.last_mut()?;
        k.tries += 1;
        let (slot, mark, tries) = (k.slot, k.mark.clone(), k.tries);
        if !e.restore(slot, &mark) {
            return None;
        }
        self.tally.restores += 1;
        self.tally.back += 1;
        self.doing = None;
        self.let_be = i64::MIN;
        self.was = 0.0;
        self.kept_at = e.tic;
        // (another way this time, till past where it went wrong)
        self.shaken = (
            u64::from(self.tally.back) * 7919 + u64::from(tries),
            now + 70,
        );
        self.log.push(format!(
            "tic {now}: no line she lives on: back {} tics to tic {} (go {tries} from there)",
            now - e.tic,
            e.tic
        ));
        Some(now - e.tic)
    }

    /// Hurt too fast (`Tune::hurt`): back to the last game kept from before `since` (the tic
    /// the stretch she was hurt in began), to try it another way. The tics gone back; None: let
    /// stand (every go at it hurt as much, or nothing's kept from before it).
    fn hurt_back(&mut self, e: &mut Engine, since: i64, lost: f64) -> Option<i64> {
        let now = e.tic;
        let i = self.kept.iter().rposition(|k| k.mark.tic <= since)?;
        if self.kept[i].hurt >= self.tune.hurt_tries {
            self.tally.hurt_stood += 1;
            self.log.push(format!(
                "tic {now}: {lost:.0} lost since tic {since} whatever she tried from tic {}: let stand",
                self.kept[i].mark.tic
            ));
            return None;
        }
        self.kept[i].hurt += 1;
        let (slot, mark, go) = (
            self.kept[i].slot,
            self.kept[i].mark.clone(),
            self.kept[i].hurt,
        );
        if !e.restore(slot, &mark) {
            return None;
        }
        // (what was kept after it is of a game that didn't happen)
        self.kept.truncate(i + 1);
        self.tally.restores += 1;
        self.tally.hurt_back += 1;
        self.doing = None;
        self.let_be = i64::MIN;
        self.was = 0.0;
        self.kept_at = e.tic;
        self.shaken = (
            u64::from(self.tally.hurt_back) * 104_729 + u64::from(go),
            now + 35,
        );
        self.log.push(format!(
            "tic {now}: {lost:.0} lost since tic {since}: back {} tics to tic {} (go {go} at it)",
            now - e.tic,
            e.tic
        ));
        Some(now - e.tic)
    }

    /// The sector at `q`, if its floor hurts.
    fn hot_at(&self, agent: &Agent, q: Point) -> Option<usize> {
        let nav = &agent.x().nav;
        let sec = nav.sector_of_poly(nav.poly_at(q)?);
        nav.damage.get(sec).is_some_and(|&d| d != 0).then_some(sec)
    }

    /// This tic's controls: the search's, something after her; else `hers`. None: dead
    /// whatever she does, and the game's gone back (`back` tics): ask again.
    pub fn act(&mut self, e: &mut Engine, agent: &Agent, hers: Controls) -> Result<Controls, i64> {
        let (p, things) = e.peek();
        let things: Vec<Mobj> = things.to_vec();
        let seen = Self::seen(e).is_some();
        if !threatened(&p, &things, seen, self.tune.near, self.tune.close) {
            self.doing = None;
            self.was = hers.turn;
            self.keep(e);
            return Ok(hers);
        }
        self.tally.ultra_tics += 1;
        let pos = (p.x, p.y);
        let mut sectors = Vec::new();
        e.game.sectors(&mut sectors);
        // (stood about: within a little way of one place since when)
        // (...with nothing pressing her: with something flying at her or one of them coming, where
        // she stands is the fight's business, and hurrying her on then had her into walls and
        // into their fire on E2M9)
        let pressed = things.iter().any(|m| {
            let d = dist((m.x, m.y), pos);
            (hostile_missile(m) && d < self.tune.eager_clear * 2.0)
                || (alive(m)
                    && monster_of(m).is_some()
                    && m.after_her != 0
                    && d < self.tune.eager_clear)
        });
        if dist(self.stood.0, pos) > self.tune.eager_room {
            self.stood = (pos, e.tic);
            self.idle = 0.0;
        } else {
            // (pressed, far more slowly: but not never, or a corner nothing can hit her in,
            // with them after her and unable to reach her, is where she stays: E4M6's start)
            let slow = if pressed {
                self.tune.eager_pressed
            } else {
                1.0
            };
            self.idle += 1.0 / (self.tune.eager_tics as f64 * slow);
        }
        let way = agent.player().map_or(Vec::new(), |pl| pl.way_on(pos));
        let root = Root {
            wades: way.iter().filter_map(|&q| self.hot_at(agent, q)).collect(),
            way,
            eager: 1.0 + self.idle.min(self.tune.eager_most),
            sectors,
            keys: p.cards.iter().filter(|&&c| c != 0).count() as i32,
            shake: if e.tic < self.shaken.1 {
                self.shaken.0
            } else {
                0
            },
        };
        if e.tic < self.let_be {
            return Ok(hers);
        }
        if self.doing.is_none_or(|(_, left)| left == 0) {
            self.keep(e);
            match self.look(e, &root, hers) {
                Some((mv, w)) => {
                    if self.tally.decisions % 20 == 1 {
                        self.log.push(format!(
                            "tic {}: {mv:?} (worth {w:.0}; health {}, {} of them alive)",
                            e.tic,
                            p.health,
                            things
                                .iter()
                                .filter(|m| alive(m) && monster_of(m).is_some())
                                .count()
                        ));
                    }
                    // (the move before, for the count: did it take her anywhere?)
                    if let (Some((was, _)), Some(from)) = (self.doing, self.began) {
                        self.tally.moves += 1;
                        if !matches!(was, Move::Plain { go: Go::Still, .. })
                            && dist(from, pos) < self.tune.pushed
                        {
                            self.tally.pressed += 1;
                        }
                    }
                    // Nothing better than standing still, and nothing pressing her: her own legs
                    // for the move, then. She knows the way round what's in front of her, which a
                    // compass move doesn't; stood still by the search with monsters in sight
                    // across the lava, her route called itself stuck for half a minute (E4M6's
                    // start, the maintainer watching).
                    if !pressed && matches!(mv, Move::Plain { go: Go::Still, .. }) {
                        self.doing = None;
                        self.let_be = e.tic + self.tune.move_tics as i64;
                        self.was = hers.turn;
                        return Ok(hers);
                    }
                    self.began = Some(pos);
                    self.doing = Some((mv, self.tune.move_tics));
                }
                None => return Err(self.go_back(e).unwrap_or(0)),
            }
        }
        let (mv, left) = self.doing.expect("a move was chosen");
        let seen = if matches!(mv, Move::Plain { fire: true, .. }) {
            Self::seen(e)
        } else {
            None
        };
        let c = self.controls(
            &mv,
            left == self.tune.move_tics,
            &p,
            &root,
            Some(&hers),
            seen,
        );
        self.doing = Some((mv, left - 1));
        self.was = c.turn;
        Ok(c)
    }
}

/// Live, the game runs at this much of its speed while the search has her (the plan's section
/// 4: what a look has to fit in), and a session not kept on is over when the search hasn't had
/// her for `LIVE_OVER` tics: the fight's over.
pub const LIVE_SPEED: f64 = 0.5;
pub const LIVE_OVER: i64 = 5 * 35;

/// What a tic of a session came to.
pub enum Went {
    /// the controls to play this tic
    On(Controls),
    /// the game was put back this many tics: nothing to play, ask again
    Back(i64),
    /// no line she lives on and no game left to go back to
    NoWay,
}

/// The ultra instinct on, from the tic it's begun till it's dropped: the search, the games kept
/// to go back to, and going back for a death or a hurt. Begun at any moment of a level (it's for
/// getting her out of a pickle, not for playing a level through), it goes back no further than
/// where it began. The offline run and the watcher's window both play through this.
pub struct Session {
    pub ultra: Ultra,
    /// the tics gone back over: her own clock only goes on
    pub gone_back: i64,
    /// her health and armor together, by the tic, a few seconds of it
    hps: std::collections::VecDeque<(i64, f64)>,
    /// the tic up to which what she's lost is let stand
    stood: i64,
    /// its controls as they came out in the end, by the game's tics
    pub tape: Vec<(i64, Controls)>,
    pub worst: i32,
    /// the search had her this tic (one of them in sight or near), and the tic it last did
    pub searching: bool,
    pub searched_at: i64,
}

impl Session {
    pub fn new(tune: Tune, agent: &Agent, tic: i64) -> Session {
        let mut ultra = Ultra::new(tune);
        ultra.hot = Some(std::rc::Rc::clone(&agent.x().nav));
        Session {
            ultra,
            gone_back: 0,
            hps: std::collections::VecDeque::new(),
            stood: i64::MIN,
            tape: Vec::new(),
            worst: i32::MAX,
            searching: false,
            searched_at: tic,
        }
    }

    /// What she's shown: her clock with the tics gone back added (it only goes on).
    pub fn see(&mut self, ob: &mut crate::motor::Observation) {
        ob.tic += self.gone_back;
        ob.hit_tic = ob.hit_tic.map(|t| t + self.gone_back);
        self.worst = self.worst.min(ob.health.map_or(self.worst, |h| h as i32));
    }

    fn went_back(&mut self, e: &Engine, back: i64) {
        self.gone_back += back;
        self.tape.retain(|&(t, _)| t < e.tic);
        self.hps.retain(|&(t, _)| t <= e.tic);
    }

    /// Dead for real (a death no look saw coming): back to a game kept. None: there's none.
    pub fn died(&mut self, e: &mut Engine) -> Option<i64> {
        let back = self.ultra.go_back(e)?;
        self.went_back(e, back);
        Some(back)
    }

    /// This tic's controls, hers given (`Ultra::act`), or the game put back.
    pub fn decide(&mut self, e: &mut Engine, agent: &Agent, hers: Controls) -> Went {
        let before = self.ultra.tally.ultra_tics;
        let went = match self.ultra.act(e, agent, hers) {
            Ok(c) => {
                self.tape.push((e.tic, c));
                Went::On(c)
            }
            Err(0) if self.ultra.kept.is_empty() => Went::NoWay,
            Err(back) => {
                self.went_back(e, back);
                Went::Back(back)
            }
        };
        self.searching = self.ultra.tally.ultra_tics > before;
        if self.searching {
            self.searched_at = e.tic;
        }
        went
    }

    /// After the tic's played. Hurt too fast: back, and another way (not past what's been let
    /// stand). Some: the game was put back this many tics.
    pub fn stepped(&mut self, e: &mut Engine) -> Option<i64> {
        let p = e.game.player();
        let hp = f64::from(p.health + p.armor);
        self.hps.retain(|&(t, _)| t < e.tic); // (gone back since for a death: those tics didn't happen)
        self.hps.push_back((e.tic, hp));
        while self
            .hps
            .front()
            .is_some_and(|&(t, _)| e.tic - t > self.ultra.tune.hurt_tics)
        {
            self.hps.pop_front();
        }
        if self.ultra.tune.hurt <= 0.0 || p.health <= 0 {
            return None;
        }
        let limit = self.ultra.tune.hurt * self.ultra.tune.hurt_tics as f64 / 35.0;
        // (from the most she had in the stretch, after what's let stand)
        let stood = self.stood;
        let (since, most) = self
            .hps
            .iter()
            .filter(|&&(t, _)| t >= stood)
            .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
            .copied()
            .filter(|&(_, most)| most - hp >= limit)?;
        match self.ultra.hurt_back(e, since, most - hp) {
            Some(back) => {
                self.went_back(e, back);
                Some(back)
            }
            None => {
                self.stood = e.tic;
                None
            }
        }
    }

    /// Dropped: the games kept let go.
    pub fn end(&mut self, e: &Engine) {
        for k in self.ultra.kept.drain(..) {
            e.game.forget(k.slot);
        }
    }
}

/// Stage 1's check: the game saved, played on, put back and played the same again comes out
/// the same; and what a save and a restore cost.
pub fn check(wad: &str, map: &str, o: &RunOptions) -> Result<String, String> {
    let crate::sweep::Setup {
        mut e, mut agent, ..
    } = crate::sweep::setup(wad, map, o, false, None)?;
    let mut last: Option<Controls> = None;
    let step = |e: &mut Engine, agent: &mut Agent, last: &mut Option<Controls>| {
        let ob = e.observe();
        let c = agent.act(&ob, last.as_ref());
        e.step(&c);
        *last = Some(c);
        c
    };
    for _ in 0..500 {
        step(&mut e, &mut agent, &mut last);
    }
    let state = |e: &mut Engine| -> String {
        let (p, things) = e.peek();
        let mut s = format!(
            "{:.4},{:.4},{:.4},{:.4},{},{},{:?}",
            p.x, p.y, p.z, p.angle, p.health, p.armor, p.ammo
        );
        for m in things {
            s += &format!(
                ";{},{},{:.4},{:.4},{:.4},{},{}",
                m.id, m.kind, m.x, m.y, m.z, m.health, m.state
            );
        }
        s
    };
    let t = Instant::now();
    let mark = e
        .save(0)
        .ok_or("the engine library can't save (rebuild it)")?;
    let save_ms = t.elapsed().as_secs_f64() * 1000.0;
    let bytes = e.game.save(1).unwrap_or(0);
    let mut cmds = Vec::new();
    let t = Instant::now();
    for _ in 0..500 {
        cmds.push(step(&mut e, &mut agent, &mut last));
    }
    let with_her = t.elapsed().as_secs_f64() * 1000.0 / 500.0;
    let first = state(&mut e);
    let t = Instant::now();
    e.restore(0, &mark);
    let restore_ms = t.elapsed().as_secs_f64() * 1000.0;
    let t = Instant::now();
    for c in &cmds {
        e.step(c);
    }
    let tic_ms = t.elapsed().as_secs_f64() * 1000.0 / 500.0;
    let again = state(&mut e);
    // (and the cost of each a hundred times over)
    let t = Instant::now();
    for _ in 0..100 {
        e.restore(0, &mark);
    }
    let restore_100 = t.elapsed().as_secs_f64() * 10.0;
    let t = Instant::now();
    for _ in 0..100 {
        e.save(2);
    }
    let save_100 = t.elapsed().as_secs_f64() * 10.0;
    Ok(format!(
        "{wad} {map}: {} after 500 tics, put back and played again; saved game {bytes} bytes; save {save_ms:.3} ms (mean of 100: {save_100:.3}), restore {restore_ms:.3} ms (mean of 100: {restore_100:.3}); a tic of the engine alone {tic_ms:.4} ms, with her thinking {with_her:.4} ms",
        if first == again { "THE SAME" } else { "DIFFERENT" }
    ))
}

/// A way of playing a tape: its state at every tic as a number, how it ended, and the whole
/// state at the tic asked for.
type Played = (Vec<(i64, u64)>, String, String);

/// `ultra-check --tape FILE`: the run's controls played three ways, and where they part. As
/// they are; with the game saved and put back before every tic; and with it saved, played
/// `AHEAD` tics on, and put back before every tic (as a look does). All three should be one
/// game: a tape that plays out otherwise than its run means a restore isn't the game it saved.
pub fn check_tape(
    wad: &str,
    map: &str,
    o: &RunOptions,
    tape: &std::path::Path,
) -> Result<String, String> {
    use std::hash::{Hash, Hasher};
    const AHEAD: i64 = 30;
    let tape: std::collections::BTreeMap<i64, Controls> = crate::replay::Recording::read(tape)?
        .entries
        .into_iter()
        .filter_map(|en| match en {
            crate::replay::Entry::Tic(t, c) => Some((t, c)),
            _ => None,
        })
        .collect();
    let end = tape.keys().next_back().copied().unwrap_or(0);
    let state = |e: &mut Engine| -> String {
        let (mut p, things) = e.peek();
        // (what she was last told isn't the game's state)
        p.message_tic = 0;
        p.message.fill(0);
        let mut s = format!("{p:?}");
        for m in things {
            s += &format!("\n{m:?}");
        }
        s
    };
    // (one game a process: every way played starts from the level's start, put back; hers before
    // the tape begins are kept from the first)
    let crate::sweep::Setup {
        mut e, mut agent, ..
    } = crate::sweep::setup(wad, map, o, false, None)?;
    let start = e.save(3).ok_or("the engine library can't save")?;
    let mut tape = tape;
    let mut first = true;
    let mut play = |mode: u8, full: Option<i64>| -> Result<Played, String> {
        if !first {
            e.restore(3, &start);
        }
        let mut last: Option<Controls> = None;
        let mut seen = Vec::new();
        let mut whole = String::new();
        let mut how = "the tape ran out".to_string();
        while e.tic <= end {
            if e.exited() {
                how = format!("out at tic {}", e.tic);
                break;
            }
            if e.dead() {
                how = format!("dead at tic {}", e.tic);
                break;
            }
            if mode > 0 && tape.contains_key(&e.tic) {
                let mark = e.save(0).ok_or("the engine library can't save")?;
                if mode == 2 {
                    for t in e.tic..e.tic + AHEAD {
                        let Some(c) = tape.get(&t) else { break };
                        e.step(c);
                    }
                }
                e.restore(0, &mark);
            }
            let c = match tape.get(&e.tic) {
                Some(c) => *c,
                None => {
                    let ob = e.observe();
                    let c = agent.act(&ob, last.as_ref());
                    tape.insert(e.tic, c);
                    c
                }
            };
            e.step(&c);
            last = Some(c);
            let s = state(&mut e);
            let mut h = std::collections::hash_map::DefaultHasher::new();
            s.hash(&mut h);
            seen.push((e.tic, h.finish()));
            if full == Some(e.tic) {
                whole = s;
                break;
            }
        }
        first = false;
        Ok((seen, how, whole))
    };
    // (EVIL_TAPE_DUMP=MODE:TIC: that way played from a fresh game, its whole state at the tic)
    if let Some((mode, tic)) = std::env::var("EVIL_TAPE_DUMP").ok().and_then(|v| {
        let (m, t) = v.split_once(':')?;
        Some((m.parse::<u8>().ok()?, t.parse::<i64>().ok()?))
    }) {
        return Ok(play(mode, Some(tic))?.2);
    }
    let (plain, how, _) = play(0, None)?;
    let mut out = format!("{wad} {map}: the tape as it is: {how}");
    for (mode, name) in [
        (1u8, "saved and put back every tic"),
        (2, "saved, played on and put back every tic"),
    ] {
        let (other, how, _) = play(mode, None)?;
        let parts = plain
            .iter()
            .zip(&other)
            .find(|(a, b)| a != b)
            .map(|(a, _)| a.0);
        out += &format!("\n{name}: {how}; ");
        match parts {
            None => out += "the same game throughout",
            Some(t) => {
                out += &format!("PARTS at tic {t}:");
                let a = play(0, Some(t))?.2;
                let b = play(mode, Some(t))?.2;
                let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
                if la.len() != lb.len() {
                    out += &format!(" {} things for {}", la.len() - 1, lb.len().max(1) - 1);
                }
                for (x, y) in la.iter().zip(&lb).filter(|(x, y)| x != y).take(12) {
                    out += &format!("\n  {x}\n  {y}");
                }
            }
        }
    }
    Ok(out)
}

/// A level played with the ultra instinct on, offline. A line of how it went.
pub fn run(wad: &str, map: &str, o: &RunOptions, tune: Tune) -> Result<String, String> {
    let crate::sweep::Setup {
        mut e,
        mut agent,
        map_name,
        ..
    } = crate::sweep::setup(wad, map, o, false, None)?;
    let mut session = Session::new(tune, &agent, e.tic);
    // (the game as it starts, for playing the tape over at the end)
    let start = e.save(START_SLOT);
    let t0 = Instant::now();
    let tic0 = e.tic;
    let mut last: Option<Controls> = None;
    let cap = (o.max_seconds * 35.0) as i64;
    let mut played = 0i64;
    let mut outcome = "TIME";
    // (nothing more collected, learned or tried for `--stall` seconds: the run's over. Her own
    // exploring is hers here too, and with nothing left she'd try at 3 health she stood idle
    // for half an hour of an E4M6 run till the time was up)
    let mut best = crate::sweep::progress(&e, agent.x(), o);
    let mut best_at = 0i64;
    while played < cap {
        if e.exited() {
            outcome = "EXITED";
            break;
        }
        if played % 35 == 0 {
            let now = crate::sweep::progress(&e, agent.x(), o);
            if now.iter().zip(&best).any(|(n, b)| n > b) {
                best = std::array::from_fn(|i| now[i].max(best[i]));
                best_at = played;
            } else if (played - best_at) as f64 > o.stall_seconds * 35.0 {
                outcome = "STALLED";
                break;
            }
        }
        if e.dead() {
            // (a death the look didn't see coming: back, as from a look with none to live on)
            if session.died(&mut e).is_some() {
                last = None;
                continue;
            }
            outcome = "DIED";
            break;
        }
        let mut ob = e.observe();
        session.see(&mut ob);
        let hers = agent.act(&ob, last.as_ref());
        let c = match session.decide(&mut e, &agent, hers) {
            Went::On(c) => c,
            Went::Back(_) => {
                last = None;
                continue;
            }
            Went::NoWay => {
                outcome = "NO WAY";
                break;
            }
        };
        e.step(&c);
        last = Some(c);
        played += 1;
        if session.stepped(&mut e).is_some() {
            last = None;
        }
    }
    let Session {
        ultra,
        gone_back,
        tape,
        worst,
        ..
    } = session;
    // The tape played over from the start with nothing saved or put back: it must come out as
    // the run did, or a restore somewhere wasn't the game it saved (2026-11-19: it wasn't, with
    // a revenant's missile in the air, and the tape of an exit played out as a death).
    let state = |e: &Engine| {
        let p = e.game.player();
        format!(
            "tic {}, health {} armor {} at ({:.2}, {:.2})",
            e.tic, p.health, p.armor, p.x, p.y
        )
    };
    let taped_same = start.map(|start| {
        let was = state(&e);
        e.restore(START_SLOT, &start);
        for (t, c) in &tape {
            if e.tic == *t {
                e.step(c);
            }
        }
        let now = state(&e);
        if now == was {
            "the tape plays out the same".to_string()
        } else {
            format!("THE TAPE PLAYS OUT OTHERWISE (the run: {was}; the tape: {now})")
        }
    });
    let [items, kills, secrets] = e.tally();
    let p = e.game.player();
    let dir = crate::config::out_dir().join("ultra");
    let _ = std::fs::create_dir_all(&dir);
    // (a file a skill and a seed: runs side by side each keep their own)
    let file = dir.join(format!(
        "{}_{map_name}.k{}.s{}.txt",
        crate::config::wad_tag(&e.iwad),
        o.fight,
        o.seed.unwrap_or(0)
    ));
    let mut text = ultra.log.join("\n");
    text.push('\n');
    text += &agent.x().log.join("\n");
    let _ = std::fs::write(&file, text);
    let taped = file.with_extension("run");
    crate::replay::Recording {
        entries: tape
            .into_iter()
            .map(|(t, c)| crate::replay::Entry::Tic(t, c))
            .collect(),
    }
    .write(&taped)?;
    let t = &ultra.tally;
    Ok(format!(
        "ULTRA {outcome} {map_name}: {:.0}s game ({} tics, {} of them searched), health {} armor {} (lowest health {worst}), kills {}/{}, items {}/{}, secrets {}/{}; gone back {} times for a death and {} for a hurt ({gone_back} tics), {} hurts let stand; {} of {} moves pressed on a wall; {} looks, {} tics played ahead, {} restores, {} saves, {:.1}s searching of {:.1}s real; {}; log {}; to watch it: evil-boom watch {wad} {map_name} --fight {} --seed {} --mortal --tape {}",
        (e.tic - tic0) as f64 / 35.0,
        e.tic - tic0,
        t.ultra_tics,
        p.health,
        p.armor,
        kills.0,
        kills.1,
        items.0,
        items.1,
        secrets.0,
        secrets.1,
        t.back,
        t.hurt_back,
        t.hurt_stood,
        t.pressed,
        t.moves,
        t.decisions,
        t.played,
        t.restores,
        t.saves,
        t.searched_secs,
        t0.elapsed().as_secs_f64(),
        taped_same.as_deref().unwrap_or("the tape not checked"),
        file.display(),
        o.fight,
        o.seed.unwrap_or(0),
        taped.display()
    ))
}
