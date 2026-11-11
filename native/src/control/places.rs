//! The level as places a commander can talk about: areas (rooms, halls, corridors) made of sectors.
//!
//! Doom maps have no room names, and a sector is often just a step or a patch of light. An area is
//! the sectors that run into each other at walkable heights across openings that are much of their
//! edge: a room with its steps, alcoves and pools stays one area (they share most of their outline
//! with it); a doorway or an archway (a small part of either side's outline), a door, a lift is
//! where one area ends and the next begins. Built once from the map (the same for everyone); what she knows
//! of each comes from what she's seen.

use std::collections::BTreeMap;

use crate::geom::{dist, Point};
use crate::map::MapData;
use crate::navigator::{Navigator, MAX_STEP, PLAYER_HEIGHT};
use crate::specials::{classify_line, Category};

/// two sectors are one area where the openings between them are this share of the smaller one's
/// outline (a step or an alcove: most of it; an archway between rooms: a small part)...
const SHARED: f64 = 0.25;
/// ...and at least this wide
const WIDE: f64 = 64.0;

/// ...an area smaller than this (map units squared) isn't one to talk about (a door, a step)
pub const SMALL: f64 = 128.0 * 128.0;

#[derive(Clone, Debug)]
pub struct Area {
    /// "area-N", N its lowest sector
    pub id: String,
    pub sectors: Vec<usize>,
    /// the middle of its biggest poly (a spot inside it)
    pub center: Point,
    /// its floor's area (map units squared)
    pub size: f64,
    /// some of its floor hurts (nukage, lava, blood)
    pub hurts: bool,
    /// open to the sky
    pub outdoors: bool,
}

pub struct Places {
    pub areas: Vec<Area>,
    /// sector -> index into `areas`
    area_of: Vec<usize>,
}

impl Places {
    pub fn build(nav: &Navigator) -> Places {
        let m: &MapData = &nav.map;
        let n = m.sectors.len();
        let movers = moving_sectors(m);
        // (the openings between each pair of sectors, walkable at the map's heights)
        let mut shared: BTreeMap<(usize, usize), f64> = BTreeMap::new();
        let mut outline = vec![0.0; n];
        for li in 0..m.linedefs.len() {
            let (p, q) = m.line_seg(li);
            let (f, b) = m.sides(li);
            for s in [f, b].into_iter().flatten() {
                outline[s] += dist(p, q);
            }
        }
        for li in 0..m.linedefs.len() {
            let (Some(a), Some(b)) = m.sides(li) else {
                continue;
            };
            if a == b || movers[a] || movers[b] {
                continue;
            }
            let (sa, sb) = (&m.sectors[a], &m.sectors[b]);
            let open = |s: &crate::map::Sector| s.ceiling - s.floor >= PLAYER_HEIGHT;
            if (sa.floor - sb.floor).abs() > MAX_STEP
                || !open(sa)
                || !open(sb)
                || sa.ceiling.min(sb.ceiling) - sa.floor.max(sb.floor) < PLAYER_HEIGHT
            {
                continue;
            }
            let (p, q) = m.line_seg(li);
            *shared.entry((a.min(b), a.max(b))).or_insert(0.0) += dist(p, q);
        }
        let mut parent: Vec<usize> = (0..n).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        for (&(a, b), &w) in &shared {
            if w >= WIDE && w >= SHARED * outline[a].min(outline[b]) {
                let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                if ra != rb {
                    parent[ra.max(rb)] = ra.min(rb);
                }
            }
        }
        // (the polys of each sector: its floor's size, a spot in it)
        let mut size = vec![0.0; n];
        let mut biggest: Vec<Option<(f64, Point)>> = vec![None; n];
        for poly in nav.mesh.polys.iter().flatten() {
            let a = shoelace(&poly.vertices);
            size[poly.sector] += a;
            if biggest[poly.sector].is_none_or(|(b, _)| a > b) {
                biggest[poly.sector] = Some((a, poly.center));
            }
        }
        let mut index: BTreeMap<usize, usize> = BTreeMap::new();
        let mut areas: Vec<Area> = Vec::new();
        let mut area_of = vec![0; n];
        for s in 0..n {
            let r = root(&mut parent, s);
            let i = *index.entry(r).or_insert_with(|| {
                areas.push(Area {
                    id: format!("area-{r}"),
                    sectors: Vec::new(),
                    center: (0.0, 0.0),
                    size: 0.0,
                    hurts: false,
                    outdoors: false,
                });
                areas.len() - 1
            });
            area_of[s] = i;
            let a = &mut areas[i];
            a.sectors.push(s);
            a.size += size[s];
            a.hurts |= nav.damage[s] != 0;
            a.outdoors |= m.sectors[s].ceiling_tex.starts_with("F_SKY");
        }
        for a in &mut areas {
            if let Some((_, c)) = a
                .sectors
                .iter()
                .filter_map(|&s| biggest[s])
                .max_by(|x, y| x.0.total_cmp(&y.0))
            {
                a.center = c;
            }
        }
        Places { areas, area_of }
    }

    /// The area a sector is in.
    pub fn area_of(&self, sector: usize) -> &Area {
        &self.areas[self.area_of[sector]]
    }

    pub fn by_id(&self, id: &str) -> Option<&Area> {
        self.areas.iter().find(|a| a.id == id)
    }

    /// A few words for an area: how big, what's notable.
    pub fn sketch(a: &Area) -> String {
        // (short: Evil reads it, and her context is precious, the user's)
        let size = match a.size {
            s if s < 256.0 * 256.0 => "small ",
            s if s < 640.0 * 640.0 => "",
            s if s < 1280.0 * 1280.0 => "large ",
            _ => "huge ",
        };
        let what = if a.outdoors { "outdoors" } else { "room" };
        let hurts = if a.hurts { ", floor hurts" } else { "" };
        format!("{size}{what}{hurts}")
    }
}

/// Sectors that move: what a special line targets by tag, and the doors opened by hand (the
/// sector behind a door line with no tag).
fn moving_sectors(m: &MapData) -> Vec<bool> {
    let mut moving = vec![false; m.sectors.len()];
    for l in &m.linedefs {
        let Some(info) = classify_line(l.special) else {
            continue;
        };
        if matches!(
            info.category,
            Category::Light
                | Category::Exit
                | Category::SecretExit
                | Category::Teleport
                | Category::Other
        ) {
            continue;
        }
        if l.tag != 0 {
            for (s, sec) in m.sectors.iter().enumerate() {
                if sec.tag == l.tag {
                    moving[s] = true;
                }
            }
        } else if matches!(info.category, Category::Door | Category::LockedDoor) {
            if let Some(b) = m.back_sector(l) {
                moving[b] = true;
            }
        }
    }
    moving
}

fn shoelace(v: &[Point]) -> f64 {
    let n = v.len();
    (0..n)
        .map(|i| v[i].0 * v[(i + 1) % n].1 - v[(i + 1) % n].0 * v[i].1)
        .sum::<f64>()
        .abs()
        / 2.0
}
