// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! A run watched into a video: each tic's picture (the game's, the whole window's, or the
//! game with her corner map) and its sound handed to ffmpeg.

use super::*;

/// the HUD's corner map: zoomed in this much, round her
pub(super) const HUD_ZOOM: f64 = 3.0;

/// What a recording shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordView {
    /// the game's own view
    Game,
    /// the whole watcher window (the game, the map of what she knows, her log)
    Full,
    /// the game's view with what she's at over it (health, ammo, what she's doing, the fight,
    /// a map in the corner)
    Hud,
    /// the stream's 16:9 window (the game and the viewers' column), 1920x1080
    Stream,
}

/// How to record.
pub struct RecordOptions {
    pub out: std::path::PathBuf,
    pub view: RecordView,
    /// the video's size (None: the view's own; the game's, 1280x960)
    pub size: Option<(u32, u32)>,
    /// a frame kept every this many tics (2: twice as fast)
    pub every: u32,
    /// x264's quality (lower: better and bigger; 18 about lossless to the eye) and speed preset
    pub crf: u32,
    pub preset: String,
    pub ffmpeg: std::path::PathBuf,
    /// the stream view's column items not shown (`console::COLUMN_ITEMS`: chat, logo, ...)
    pub hide: Vec<String>,
}

/// ffmpeg: on the path, else where winget put it.
pub fn find_ffmpeg() -> Option<std::path::PathBuf> {
    let ok = |p: &std::path::Path| {
        std::process::Command::new(p)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    };
    if ok(Path::new("ffmpeg")) {
        return Some("ffmpeg".into());
    }
    let local = std::env::var_os("LOCALAPPDATA")?;
    let packages = Path::new(&local)
        .join("Microsoft")
        .join("WinGet")
        .join("Packages");
    for e in std::fs::read_dir(&packages).ok()?.flatten() {
        if !e
            .file_name()
            .to_string_lossy()
            .to_lowercase()
            .contains("ffmpeg")
        {
            continue;
        }
        for b in std::fs::read_dir(e.path()).ok()?.flatten() {
            let exe = b.path().join("bin").join("ffmpeg.exe");
            if ok(&exe) {
                return Some(exe);
            }
        }
    }
    None
}

/// The run watched, as fast as it goes, into an MP4 (ffmpeg encoding the frames piped to it):
/// the level (and with --episode the ones after), as a watch plays it.
pub fn record(
    wad: &str,
    map_name: &str,
    o: &WatchOptions,
    r: &RecordOptions,
) -> Result<(), String> {
    use std::io::Write;
    let (w, mut watch) = first_watch(wad, map_name, o, false)?;
    if r.view == RecordView::Stream {
        watch.stream = Some(stream::Stream::new(
            o.stream.unwrap_or(stream::Side::Right),
            o.fight,
        ));
        if let Some(st) = watch.stream.as_mut() {
            st.hidden = r.hide.clone();
        }
    }
    let (fw, fh) = match r.view {
        RecordView::Full => (W, H),
        RecordView::Stream => (stream::SW, stream::SH),
        RecordView::Hud => (GAME.2, GAME.3),
        RecordView::Game => {
            watch.tic();
            watch
                .frame
                .as_ref()
                .map(|f| (f.0, f.1))
                .ok_or("the game drew nothing")?
        }
    };
    let (ow, oh) = r.size.unwrap_or(match r.view {
        RecordView::Game => (1280, 960),
        _ => (fw as u32, fh as u32),
    });
    // (Doom's pixels are taller than wide: the game's view stretched to 4:3; crisp)
    let scale = format!(
        "scale={ow}:{oh}:flags={}",
        if r.view == RecordView::Game {
            "neighbor"
        } else {
            "bicubic"
        }
    );
    // (somewhere to put it: ffmpeg makes no folders)
    if let Some(dir) = r.out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    // (at x1, the game's sound too: taken a tic at a time beside the frames, put with them at the
    // end)
    let video_out = if o.capture_sound {
        r.out.with_extension("video.mp4")
    } else {
        r.out.clone()
    };
    let audio_out = r.out.with_extension("audio.raw");
    let mut audio = if o.capture_sound {
        Some(std::io::BufWriter::new(
            std::fs::File::create(&audio_out)
                .map_err(|e| format!("{}: {e}", audio_out.display()))?,
        ))
    } else {
        None
    };
    let mut audio_rate = 0;
    let mut ff = std::process::Command::new(&r.ffmpeg)
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "bgr0",
            "-s",
            &format!("{fw}x{fh}"),
            "-r",
            "35",
            "-i",
            "-",
        ])
        .args([
            "-vf",
            &scale,
            "-c:v",
            "libx264",
            "-preset",
            &r.preset,
            "-crf",
            &r.crf.to_string(),
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(&video_out)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("{}: {e}", r.ffmpeg.display()))?;
    let mut pipe = ff.stdin.take().ok_or("no pipe to ffmpeg")?;
    println!(
        "recording {} at {ow}x{oh} (the view {fw}x{fh})",
        r.out.display()
    );
    let mut began = Instant::now();
    let (cw, ch) = if r.view == RecordView::Stream {
        (stream::SW, stream::SH)
    } else {
        (W, H)
    };
    let mut canvas = Canvas {
        px: vec![0; cw * ch],
        width: cw,
        clip: (0, 0, cw, ch),
        text_scale: 1.0,
    };
    if r.view == RecordView::Hud {
        // (the corner map round her, near enough to read)
        (watch.follow, watch.zoom) = (true, HUD_ZOOM);
    }
    let mut bytes: Vec<u8> = Vec::with_capacity(fw * fh * 4);
    let mut start = watch.engine.tic;
    let mut n: u64 = 0;
    // (directed, by Neuro or the GPT stand-in or an orders file: the game at its own pace, real
    // time, whoever directs her thinking on the clock. The video's speed is --speed's)
    let real_time = o.neuro.is_some() || o.directed.is_some();
    loop {
        let (mut paced_from, mut paced_tic) = (Instant::now(), watch.engine.tic);
        while (!watch.engine.finished()
            || (watch.engine.dead()
                && (watch.lives_left > 0 || watch.ultra.is_some() || watch.ultra_unlocked())))
            && ((watch.engine.tic - start) as f64) < o.max_seconds * TICRATE
        {
            // (dead: undone, or the level again with a life less, as the watcher with no
            // window has it (`Watch::died`); it used to be the end of the video whatever
            // lives she had)
            if watch.engine.dead() {
                match watch.died(real_time) {
                    Died::Undone => continue,
                    Died::Over => break,
                    Died::Again(_) => (paced_from, paced_tic) = (Instant::now(), watch.engine.tic),
                }
            }
            if real_time {
                let due = paced_from
                    + std::time::Duration::from_secs_f64(
                        (watch.engine.tic - paced_tic) as f64 / TICRATE,
                    );
                if let Some(wait) = due.checked_duration_since(Instant::now()) {
                    std::thread::sleep(wait);
                }
            }
            watch.tic();
            if let (Some(file), Some((rate, samples))) =
                (audio.as_mut(), watch.engine.grab_tic_sound())
            {
                audio_rate = rate;
                let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
                file.write_all(&bytes)
                    .map_err(|e| format!("{}: {e}", audio_out.display()))?;
            }
            n += 1;
            if !n.is_multiple_of(u64::from(r.every.max(1))) {
                continue;
            }
            bytes.clear();
            match r.view {
                RecordView::Game => {
                    let Some((gw, gh, px)) = &watch.frame else {
                        continue;
                    };
                    if (*gw, *gh) != (fw, fh) {
                        continue;
                    }
                    for &i in px {
                        bytes.extend_from_slice(&watch.palette[usize::from(i)].to_le_bytes());
                    }
                }
                RecordView::Stream => {
                    watch.draw_stream(&mut canvas);
                    for &p in &canvas.px {
                        bytes.extend_from_slice(&p.to_le_bytes());
                    }
                }
                RecordView::Full => {
                    watch.draw(&mut canvas);
                    for &p in &canvas.px {
                        bytes.extend_from_slice(&p.to_le_bytes());
                    }
                }
                RecordView::Hud => {
                    watch.draw_hud(&mut canvas);
                    for y in GAME.1..GAME.1 + GAME.3 {
                        for &p in &canvas.px[y * W + GAME.0..y * W + GAME.0 + GAME.2] {
                            bytes.extend_from_slice(&p.to_le_bytes());
                        }
                    }
                }
            }
            pipe.write_all(&bytes).map_err(|e| format!("ffmpeg: {e}"))?;
            // (how far: the game time recorded, a line written over every game second)
            if (watch.engine.tic - start) % TICRATE as i64 == 0 {
                let secs = (watch.engine.tic - start) as f64 / TICRATE;
                let real = began.elapsed().as_secs_f64().max(1e-3);
                eprint!(
                    "\r  {} {}:{:02} game  ({:.1}x real time)   ",
                    watch.map_name,
                    secs as i64 / 60,
                    secs as i64 % 60,
                    secs / real
                );
                let _ = std::io::stderr().flush();
            }
        }
        eprintln!();
        if watch.engine.dead() {
            watch.tell_death(false);
        }
        let t = watch.engine.tally();
        let how = if watch.engine.exited() {
            "EXITED"
        } else if watch.engine.dead() {
            "DIED"
        } else {
            "TIME"
        };
        println!(
            "{}: {how} items {}/{}, {:.0}s game",
            watch.map_name,
            t[0].0,
            t[0].1,
            (watch.engine.tic - start) as f64 / TICRATE
        );
        if !(o.episode && watch.engine.exited()) {
            break;
        }
        match next_level(watch, &w, o)? {
            Some(next) => watch = next,
            None => break,
        }
        if r.view == RecordView::Hud {
            (watch.follow, watch.zoom) = (true, HUD_ZOOM);
        }
        start = watch.engine.tic;
        began = Instant::now(); // (the rate level by level)
    }
    drop(pipe);
    let status = ff.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("ffmpeg failed ({status})"));
    }
    if let Some(mut file) = audio.take() {
        file.flush().map_err(|e| e.to_string())?;
        drop(file);
        if audio_rate == 0 {
            // (no sound to be had from this library: the video as it is)
            std::fs::rename(&video_out, &r.out).map_err(|e| e.to_string())?;
            println!("(no sound: the engine library has no capture; rebuild it with tools/build-engine.sh)");
        } else {
            let muxed = std::process::Command::new(&r.ffmpeg)
                .args(["-y", "-loglevel", "error", "-i"])
                .arg(&video_out)
                .args([
                    "-f",
                    "s16le",
                    "-ar",
                    &audio_rate.to_string(),
                    "-ac",
                    "2",
                    "-i",
                ])
                .arg(&audio_out)
                .args(["-c:v", "copy", "-c:a", "aac", "-b:a", "160k", "-shortest"])
                .arg(&r.out)
                .status()
                .map_err(|e| e.to_string())?;
            if !muxed.success() {
                return Err(format!("ffmpeg putting the sound with the video failed ({muxed}); the parts are {} and {}", video_out.display(), audio_out.display()));
            }
            let _ = std::fs::remove_file(&video_out);
        }
        let _ = std::fs::remove_file(&audio_out);
    }
    println!("written {}", r.out.display());
    Ok(())
}
