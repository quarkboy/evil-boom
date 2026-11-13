// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! How steadily the window shows the game (`EVIL_DEBUG_FRAMES=S`: S seconds of it, then the window
//! shuts): each frame's gap from the one before, the tics it moved on, and where its time went
//! (the game and her, the picture, putting it on the screen). Written to out/watch/frames.tsv,
//! summed up on stdout.

use std::time::Instant;

use crate::config::out_dir;

pub(super) struct Frames {
    /// (gap since the last frame, tics moved on, stepping, painting, presenting), in ms
    rows: Vec<(f64, i64, f64, f64, f64)>,
    last: Option<Instant>,
    /// the last picture of the game (a hash), and of the frames that played no tic, how many
    /// showed a picture changed all the same (drawn between tics), and how many there were
    picture: u64,
    between: (usize, usize),
    /// when it began, and for how long
    began: Instant,
    seconds: f64,
}

impl Frames {
    /// The frames counted, if `EVIL_DEBUG_FRAMES` asks.
    pub(super) fn from_env() -> Option<Frames> {
        let seconds = std::env::var("EVIL_DEBUG_FRAMES").ok()?.parse().ok()?;
        Some(Frames {
            rows: Vec::new(),
            last: None,
            picture: 0,
            between: (0, 0),
            began: Instant::now(),
            seconds,
        })
    }

    /// Its time up: the window to shut.
    pub(super) fn done(&self) -> bool {
        self.began.elapsed().as_secs_f64() >= self.seconds
    }

    /// A frame: begun at `began` (before the tics were stepped), `painted` and `shown` when
    /// those were done, `tics` moved on, the game's `picture`.
    pub(super) fn frame(
        &mut self,
        began: Instant,
        stepped: Instant,
        painted: Instant,
        tics: i64,
        picture: &[u8],
    ) {
        use std::hash::{Hash, Hasher};
        let shown = Instant::now();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        picture.hash(&mut h);
        let picture = h.finish();
        if tics == 0 && self.rows.len() >= 60 {
            self.between.0 += usize::from(picture != self.picture);
            self.between.1 += 1;
        }
        self.picture = picture;
        let ms = |a: Instant, b: Instant| (b - a).as_secs_f64() * 1000.0;
        let gap = self.last.map_or(0.0, |l| ms(l, shown));
        if self.last.is_some() {
            self.rows.push((
                gap,
                tics,
                ms(began, stepped),
                ms(stepped, painted),
                ms(painted, shown),
            ));
        }
        self.last = Some(shown);
    }
}

impl Drop for Frames {
    fn drop(&mut self) {
        use std::io::Write;
        // (the first second left out: the window opening, the level loading)
        let rows: Vec<_> = self.rows.iter().skip(60).copied().collect();
        if rows.is_empty() {
            return;
        }
        let path = out_dir().join("watch").join("frames.tsv");
        if let Ok(mut f) = std::fs::File::create(&path) {
            let _ = writeln!(f, "gap_ms\ttics\tstep_ms\tpaint_ms\tpresent_ms");
            for r in &rows {
                let _ = writeln!(f, "{:.2}\t{}\t{:.2}\t{:.2}\t{:.2}", r.0, r.1, r.2, r.3, r.4);
            }
        }
        let pct = |mut v: Vec<f64>, p: f64| {
            v.sort_by(f64::total_cmp);
            v[((v.len() - 1) as f64 * p).round() as usize]
        };
        let col = |f: fn(&(f64, i64, f64, f64, f64)) -> f64| rows.iter().map(f).collect::<Vec<_>>();
        let gaps = col(|r| r.0);
        let total: f64 = gaps.iter().sum();
        let median = pct(gaps.clone(), 0.5);
        let late = gaps.iter().filter(|&&g| g > median * 1.5).count();
        let mut per = std::collections::BTreeMap::new();
        for r in &rows {
            *per.entry(r.1).or_insert(0usize) += 1;
        }
        println!(
            "frames: {} in {:.1}s, {:.1}/s; gap ms p50 {:.1} p95 {:.1} p99 {:.1} max {:.1}; \
             late (>1.5x median) {late} ({:.1}%)",
            rows.len(),
            total / 1000.0,
            rows.len() as f64 * 1000.0 / total,
            median,
            pct(gaps.clone(), 0.95),
            pct(gaps.clone(), 0.99),
            pct(gaps, 1.0),
            100.0 * late as f64 / rows.len() as f64,
        );
        for (name, v) in [
            ("step", col(|r| r.2)),
            ("paint", col(|r| r.3)),
            ("present", col(|r| r.4)),
        ] {
            println!(
                "  {name} ms p50 {:.1} p95 {:.1} p99 {:.1} max {:.1}",
                pct(v.clone(), 0.5),
                pct(v.clone(), 0.95),
                pct(v.clone(), 0.99),
                pct(v, 1.0)
            );
        }
        let per: Vec<String> = per
            .iter()
            .map(|(t, n)| format!("{t}: {:.1}%", 100.0 * *n as f64 / rows.len() as f64))
            .collect();
        println!("  tics a frame: {}; {}", per.join(", "), path.display());
        println!(
            "  frames with no tic played: {}, the picture changed in {} ({:.1}%)",
            self.between.1,
            self.between.0,
            100.0 * self.between.0 as f64 / self.between.1.max(1) as f64
        );
    }
}
