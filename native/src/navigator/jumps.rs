// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Run-off jumps: found across the map's gaps once (cached in out/navcache), then checked in
//! the movement simulator; the ones that don't come off are dropped.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use super::{add_height, Jump, Navigator};
use crate::domain::{FRICTION, RUN_THRUST};
use crate::geom::dist;
use crate::jumps::{FinderInput, Jump as LabJump, FINDER_VERSION};
use crate::nav::Heights;

/// A jump that never comes off made a little differently costs this much more (units).
pub const JUMP_DOUBT: f64 = 1024.0;
/// Any run-jump costs this much more than its length (units): a way on foot as short, or
/// all but, is the one taken. A person doesn't leap a gap the stairs beside it cross. Costed
/// as its length alone, a jump she was sure of was a short cut across any corner of a ledge
/// (Doom II MAP24, the shotgun's platform at the end of a spur she can walk, jumped for from
/// the spur's foot; MAP27, the steps to the rocket launcher hopped corner to corner over the
/// nukage; the maintainer watching). Small on purpose: every size tried cost exits on the
/// three-lives test, the more the bigger (docs/design/decisions/05).
pub const JUMP_RISK: f64 = 16.0;
/// A jump that comes off less often than this, made a little differently, is dropped.
const MIN_SURE: f64 = 0.25;

const OK: &str = "ok";
const FAIL: &str = "fail";
const UNVERIFIED: &str = "unverified";
const SOFT: &str = "soft";
const SLID: &str = "slid";

/// How the simulator's check went, jumps by verdict.
#[derive(Clone, Copy, Debug, Default)]
pub struct JumpChecks {
    pub ok: usize,
    pub soft: usize,
    pub slid: usize,
    pub fail: usize,
    pub unverified: usize,
}

fn lab_jump(j: &Jump) -> LabJump {
    LabJump {
        src_polys: j.src_polys.iter().copied().collect(),
        dest_poly: j.dest_poly,
        dest: j.dest,
        runup: j.runup,
        takeoff: j.takeoff,
        speed: j.speed,
        requires: j.requires.clone(),
        strafe: j.strafe,
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Cached {
    #[serde(default)]
    rising: Option<crate::nav::RisingTakeoff>,
    src: Vec<usize>,
    dest_poly: usize,
    dest: (f64, f64),
    runup: (f64, f64),
    takeoff: (f64, f64),
    speed: f64,
    requires: Vec<(usize, (f64, f64))>,
    #[serde(default)]
    strafe: bool,
}

/// What the finder depends on, hashed: the map, the mesh, the heights sectors can take, which
/// portals the box fits through, the static walls, and the finder's version.
fn cache_key(nav: &Navigator, options: &[Vec<Heights>]) -> String {
    let mut s = String::new();
    let m = &nav.map;
    for v in &m.vertexes {
        let _ = write!(s, "{},{};", v.x, v.y);
    }
    for l in &m.linedefs {
        let _ = write!(
            s,
            "{},{},{},{},{},{},{};",
            l.v1, l.v2, l.flags, l.special, l.tag, l.front, l.back
        );
    }
    for d in &m.sidedefs {
        let _ = write!(
            s,
            "{},{},{},{},{},{};",
            d.x_offset, d.y_offset, d.upper, d.lower, d.middle, d.sector
        );
    }
    for sec in &m.sectors {
        let _ = write!(
            s,
            "{},{},{},{},{},{},{};",
            sec.floor, sec.ceiling, sec.floor_tex, sec.ceiling_tex, sec.light, sec.special, sec.tag
        );
    }
    for t in &m.things {
        let _ = write!(s, "{},{},{},{},{};", t.x, t.y, t.angle, t.kind, t.flags);
    }
    for (i, p) in nav.mesh.live_polys() {
        let _ = write!(s, "{i}:{}:{:?};", p.sector, p.vertices);
    }
    for p in &nav.mesh.portals {
        let _ = write!(s, "{},{},{:?},{:?},{:?};", p.a, p.b, p.p1, p.p2, p.linedef);
    }
    let _ = write!(s, "{:?}{:?}", nav.portal_fits, options);
    let mut cells: Vec<_> = nav.static_wall_grid.iter().collect();
    cells.sort_by_key(|(c, _)| **c);
    let _ = write!(s, "{cells:?}{FINDER_VERSION}");
    let digest = Sha256::digest(s.as_bytes());
    digest.iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// The jumps across the map's gaps (the finder: JumpLab::find_jumps). Sectors a trigger can
/// move may take any of their heights; the moved ones a jump needs are its `requires`.
pub(super) fn find_jumps_cached(nav: &Navigator) -> Vec<Jump> {
    let mut options: Vec<Vec<Heights>> = nav.movers.initial.iter().map(|&h| vec![h]).collect();
    for t in &nav.triggers {
        for (s, hs) in &t.effects {
            for &h in hs {
                add_height(&mut options[*s], h);
            }
        }
    }
    let path = crate::config::out_dir()
        .join("navcache")
        .join(format!("jumps_rs_{}.json", cache_key(nav, &options)));
    let from = |c: Cached| Jump {
        rising: c.rising,
        src_polys: c.src.into_iter().collect(),
        dest_poly: c.dest_poly,
        dest: c.dest,
        runup: c.runup,
        takeoff: c.takeoff,
        speed: c.speed,
        requires: c
            .requires
            .into_iter()
            .map(|(s, (floor, ceiling))| (s, Heights { floor, ceiling }))
            .collect(),
        soft: false,
        release: 0.0,
        through: false,
        sure: 1.0,
        strafe: c.strafe,
    };
    if let Some(cached) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Cached>>(&t).ok())
    {
        return cached.into_iter().map(from).collect();
    }
    let moving: crate::det::HashSet<usize> = nav
        .triggers
        .iter()
        .flat_map(|t| t.effects.iter().map(|(s, _)| *s))
        .collect();
    let input = FinderInput {
        options,
        moving: moving.clone(),
        portal_fits: nav.portal_fits.clone(),
    };
    let mut found = nav.jump_lab.find_jumps(&input);
    let fits_from: Vec<[bool; 2]> = nav
        .mesh
        .portals
        .iter()
        .enumerate()
        .map(|(pi, p)| [p.a, p.b].map(|frm| nav.fits_at_start(pi, frm)))
        .collect();
    found.extend(nav.jump_lab.find_climbs(&moving, &fits_from));
    let ordinary = found.len();
    let mut rising = vec![Vec::new(); input.options.len()];
    // The floors that go up under her: a lift's, a floor's that a line raises.
    let can_rise: BTreeSet<_> = nav
        .triggers
        .iter()
        .filter(|t| t.info.raises_floor())
        .flat_map(|t| t.effects.iter().map(|(s, _)| *s))
        .collect();
    // Sample the elevations of actual nearby landings, not every tic of a lift.
    for (_, poly) in nav.mesh.live_polys() {
        let s = poly.sector;
        let heights = &input.options[s];
        if heights.len() < 2 || !can_rise.contains(&s) {
            continue;
        }
        let base = nav.movers.real[s];
        for (_, other) in nav.mesh.live_polys() {
            let landing = nav.movers.real[other.sector].floor;
            if input.moving.contains(&other.sector)
                || !heights
                    .iter()
                    .any(|h| h.floor < landing - crate::geom::MAX_STEP)
                || !heights.iter().any(|h| h.floor > landing)
                || base.ceiling - landing < crate::geom::PLAYER_HEIGHT
                || !poly
                    .vertices
                    .iter()
                    .any(|&p| other.vertices.iter().any(|&q| dist(p, q) < 256.0))
            {
                continue;
            }
            add_height(
                &mut rising[s],
                Heights {
                    floor: landing,
                    ceiling: base.ceiling,
                },
            );
        }
    }
    found.extend(nav.jump_lab.find_from(&input, Some(&rising)));
    let out: Vec<Cached> = found
        .into_iter()
        .enumerate()
        .map(|(i, j)| Cached {
            rising: (i >= ordinary).then(|| {
                let sector = nav.sector_of_poly(j.src_polys[0]);
                let landing = nav.movers.real[nav.sector_of_poly(j.dest_poly)].floor;
                let low = input.options[sector]
                    .iter()
                    .map(|h| h.floor)
                    .fold(f64::INFINITY, f64::min);
                // Leave time to turn and get to the run-up. A floor still at its
                // bottom stop alone cannot establish this opportunity.
                crate::nav::RisingTakeoff {
                    sector,
                    lineup_floor: (low + 1.0)
                        .max(landing - crate::nav::RisingTakeoff::WAIT_TICS as f64),
                }
            }),
            src: j.src_polys,
            dest_poly: j.dest_poly,
            dest: j.dest,
            runup: j.runup,
            takeoff: j.takeoff,
            speed: j.speed,
            requires: j.requires.iter().map(|(s, h)| (*s, h.tuple())).collect(),
            strafe: j.strafe,
        })
        .collect();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // (the jumps as the file has them, not as they were worked out: a number read back from
    // its text can be the next one along from the number written, and a run on jumps found
    // this time then went another way than the same run on jumps kept from last time, by a
    // jump's last bit. Every run reads them now, the first as the rest)
    let Ok(text) = serde_json::to_string(&out) else {
        return out.into_iter().map(from).collect();
    };
    let _ = std::fs::write(&path, &text);
    serde_json::from_str::<Vec<Cached>>(&text)
        .unwrap_or(out)
        .into_iter()
        .map(from)
        .collect()
}

/// The speed a run from standing has after `length`, pushed forward and `side` sideways (the
/// game's thrust and friction, as the finder reckons a run-up's).
fn run_speed(length: f64, side: f64) -> f64 {
    let thrust = RUN_THRUST * (1.0 + (side / 50.0).powi(2)).sqrt();
    let (mut v, mut d) = (0.0, 0.0);
    while d < length {
        v = v * FRICTION + thrust;
        d += v;
    }
    v
}

impl Navigator {
    /// Check the jumps in the movement simulator and keep only those that come off (made a
    /// little off their line too: nobody lines up to a fraction of a unit), and how sure each
    /// is. One that comes in too fast to stop on its landing is tried from shorter run-ups
    /// ("ok~x,y": it sticks from there); "soft+N": coasting from N past the takeoff.
    pub fn keep_verified(&mut self) -> JumpChecks {
        let lab: Vec<LabJump> = self.jumps.iter().map(lab_jump).collect();
        let verdicts = self.jump_lab.check_jumps(&lab);
        let mut results: Vec<String> = Vec::with_capacity(verdicts.len());
        let mut jumps: Vec<Jump> = Vec::with_capacity(verdicts.len());
        for (j, r) in self.jumps.iter().zip(verdicts) {
            // The launch is timed against the floor motion, so retain the
            // simulated landing only if the player can stop there.
            if j.rising.is_some() && !(r == OK || r.starts_with("ok~") || r.starts_with(SOFT)) {
                continue;
            }
            let mut j = j.clone();
            let mut r = r;
            if let Some((v, at)) = r
                .split_once('~')
                .map(|(a, b)| (a.to_string(), b.to_string()))
            {
                let xy: Vec<f64> = at.split(',').filter_map(|x| x.parse().ok()).collect();
                if let [x, y] = xy[..] {
                    j.runup = (x, y);
                    // (and the speed that run gives, not the longer one's: the follower calls
                    // a sprint off that's well short of the speed it should have)
                    j.speed = run_speed(dist(j.runup, j.takeoff), j.sidestep());
                }
                r = v;
            }
            if let Some(rest) = r.strip_prefix("soft+").map(str::to_string) {
                j.release = rest.parse().unwrap_or(0.0);
                r = SOFT.to_string();
            }
            results.push(r);
            jumps.push(j);
        }
        // One that slides off its landing is only kept when there's no better way across (one
        // that stops there, or one that runs straight across it).
        let sector_of = |p: usize| self.mesh.poly(p).sector;
        let sticks: BTreeSet<(Vec<usize>, usize)> = jumps
            .iter()
            .zip(&results)
            .filter(|(_, r)| *r == OK || *r == SOFT || r.contains('@'))
            .map(|(j, _)| {
                (
                    j.src_polys.iter().copied().collect(),
                    sector_of(j.dest_poly),
                )
            })
            .collect();
        let mut keep: Vec<(Jump, String)> = Vec::new();
        for (j, r) in jumps.iter().zip(&results) {
            if let Some((_, at)) = r.split_once('@') {
                // run straight across the strip it lands on, to where the simulator stopped
                let xy: Vec<f64> = at.split(',').filter_map(|x| x.parse().ok()).collect();
                if let [x, y] = xy[..]
                    && let Some(end) = self.mesh.poly_at((x, y))
                {
                    let mut t = j.clone();
                    t.dest_poly = end;
                    t.dest = (x, y);
                    t.through = true;
                    keep.push((t, r.clone()));
                }
            }
            let v = r.split('@').next().unwrap_or("");
            // (one come down short is the jump to where it stops, kept above, and no jump to
            // where it was meant for)
            if v == FAIL
                || v == "short"
                || v == SLID
                    && sticks.contains(&(
                        j.src_polys.iter().copied().collect(),
                        sector_of(j.dest_poly),
                    ))
            {
                continue;
            }
            let mut j = j.clone();
            j.soft = v == SOFT;
            keep.push((j, r.clone()));
        }
        // how sure each is: of it made a little differently (shifted, turned, a longer or
        // shorter run-up), the share that still comes off (the follower's jumps come down
        // some 6-12 off); unverified ones (needing moved floors) at half
        let tried: Vec<usize> = (0..keep.len())
            .filter(|&i| keep[i].1 != UNVERIFIED)
            .collect();
        let scores = self.jump_lab.robustness(
            &tried
                .iter()
                .map(|&i| lab_jump(&keep[i].0))
                .collect::<Vec<_>>(),
        );
        let mut sure = vec![0.5; keep.len()];
        for (&i, s) in tried.iter().zip(scores) {
            sure[i] = s;
        }
        let kept: Vec<Jump> = keep
            .into_iter()
            .zip(sure)
            .filter_map(|((mut j, _), s)| {
                j.sure = s;
                (s >= MIN_SURE).then_some(j)
            })
            .collect();
        self.set_jumps(kept);
        let mut counts = JumpChecks::default();
        for r in &results {
            match r.split('@').next().unwrap_or("") {
                OK => counts.ok += 1,
                SOFT => counts.soft += 1,
                SLID => counts.slid += 1,
                FAIL => counts.fail += 1,
                _ => counts.unverified += 1,
            }
        }
        counts
    }
}
