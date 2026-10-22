//! VeloMov - a lightweight video editor for Linux and Windows.
// Windows: a GUI program (no console window); the command-line modes attach to the
// console they were started from.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod demo;
mod engine;
mod export;
mod media;
mod model;
mod playback;
mod preview;
mod util;

use anyhow::{anyhow, Result};
use std::path::PathBuf;

const HELP: &str = "VeloMov - lightweight video editor for Linux and Windows

USAGE:
  velomov [PROJECT.vcut]                 open the editor (optionally a project)
  velomov --make-demo DIR                generate demo media + DIR/demo.vcut
  velomov --render-frame PROJECT T OUT.png [--height H]
                                         render one frame at T seconds
  velomov --export PROJECT OUT [--res 1080] [--fps 30] [--codec h264|hevc|av1|vp9|prores|gif|h264-vaapi|hevc-vaapi]
                                         [--quality lower|recommended|higher] [--no-audio]
  velomov --captions PROJECT [--lang auto|pt|en…] [--model base] [--style karaoke] [--chars 0]
                                         add auto captions (speech recognition with Whisper)
  velomov --help | --version
";

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

fn cli_render_frame(args: &[String]) -> Result<()> {
    if args.len() < 3 {
        return Err(anyhow!("usage: --render-frame PROJECT T OUT.png"));
    }
    let p = model::Project::load(&PathBuf::from(&args[0]))?;
    let t: f64 = args[1].parse()?;
    let h: u32 = arg_value(args, "--height").and_then(|v| v.parse().ok()).unwrap_or(p.height);
    let w = ((p.width as f64 * h as f64 / p.height as f64 / 2.0).round() as u32 * 2).max(2);
    let mut eng = engine::Engine::headless()?;
    let mut frames = media::Frames::new(true);
    let data = eng.render_rgba(&p, util::s_to_us(t), &mut frames, w, h);
    image::save_buffer(&args[2], &data, w, h, image::ExtendedColorType::Rgba8)?;
    println!("wrote {} ({}x{})", args[2], w, h);
    Ok(())
}

/// `--new-project OUT.vcut files…`: a project with the files on the timeline (videos/photos on
/// the main track in order, audio on an audio track).
fn cli_new_project(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(anyhow!("usage: --new-project OUT.vcut FILES..."));
    }
    let mut p = model::Project::new_named(&util::file_stem_of(&args[0]));
    for f in &args[1..] {
        let path = std::fs::canonicalize(f)?;
        let m = media::probe(&path.to_string_lossy())?;
        let (id, kind) = (m.id, m.kind);
        p.media.push(m);
        let place = if kind == model::MediaKind::Audio { model::edit::Target::Overlay(0) } else { model::edit::Target::Main(i64::MAX) };
        p.insert_media(id, place, 5 * util::SEC);
    }
    p.save(&PathBuf::from(&args[0]))?;
    println!("wrote {} ({:.1}s)", args[0], util::us_to_s(p.duration()));
    Ok(())
}

fn cli_export(args: &[String]) -> Result<()> {
    if args.len() < 2 {
        return Err(anyhow!("usage: --export PROJECT OUT"));
    }
    let p = model::Project::load(&PathBuf::from(&args[0]))?;
    let mut s = export::Settings { path: PathBuf::from(&args[1]), fps: p.fps, ..Default::default() };
    if let Some(v) = arg_value(args, "--res") {
        s.res = v.parse()?;
    }
    if let Some(v) = arg_value(args, "--fps") {
        s.fps = v.parse()?;
    }
    if let Some(v) = arg_value(args, "--codec") {
        s.codec = match v.as_str() {
            "h264" => export::Codec::H264,
            "hevc" | "h265" => export::Codec::Hevc,
            "av1" => export::Codec::Av1,
            "vp9" => export::Codec::Vp9,
            "prores" => export::Codec::ProRes,
            "gif" => export::Codec::Gif,
            "h264-vaapi" => export::Codec::H264Vaapi,
            "hevc-vaapi" => export::Codec::HevcVaapi,
            o => return Err(anyhow!("unknown codec {}", o)),
        };
    }
    if let Some(v) = arg_value(args, "--quality") {
        s.quality = match v.as_str() {
            "lower" => export::Quality::Lower,
            "higher" => export::Quality::Higher,
            _ => export::Quality::Recommended,
        };
    }
    if args.iter().any(|a| a == "--no-audio") {
        s.audio = false;
    }
    let prog = export::Progress::default();
    *prog.started.lock().unwrap() = Some(std::time::Instant::now());
    let t0 = std::time::Instant::now();
    let r = std::thread::scope(|sc| {
        let h = sc.spawn(|| export::run(&p, &s, None, &prog));
        let mut last = -1i32;
        while !h.is_finished() {
            let pct = (prog.frac() * 100.0) as i32;
            if pct != last {
                eprint!("\r[velomov] {} {:3}%  ", prog.stage.lock().unwrap(), pct);
                last = pct;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        h.join().unwrap()
    });
    eprintln!();
    r?;
    println!("exported {} in {:.1}s", s.path.display(), t0.elapsed().as_secs_f64());
    Ok(())
}

fn cli_captions(args: &[String]) -> Result<()> {
    use media::transcribe;
    let path = PathBuf::from(args.first().ok_or_else(|| anyhow!("usage: --captions PROJECT"))?);
    let mut p = model::Project::load(&path)?;
    let range = (0, p.duration());
    let req = transcribe::Request {
        project: p.clone(),
        range,
        only: None,
        lang: arg_value(args, "--lang").unwrap_or_else(|| "auto".into()),
        model: arg_value(args, "--model").unwrap_or_else(|| transcribe::DEFAULT_MODEL.into()),
        threads: arg_value(args, "--threads").and_then(|v| v.parse().ok()).unwrap_or(0),
        gpu: !args.iter().any(|a| a == "--no-gpu"),
    };
    let t0 = std::time::Instant::now();
    let job = transcribe::spawn(req);
    while !job.done.load(std::sync::atomic::Ordering::Relaxed) {
        eprint!("\r[velomov] {} {:3.0}%  ", job.stage(), job.frac() * 100.0);
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    eprintln!();
    if let Some(e) = job.error.lock().unwrap().take() {
        return Err(anyhow!(e));
    }
    let (words, lang) = job.result.lock().unwrap().take().ok_or_else(|| anyhow!("cancelled"))?;
    let style = arg_value(args, "--style").unwrap_or_else(|| "karaoke".into());
    let chars = arg_value(args, "--chars").and_then(|v| v.parse().ok()).unwrap_or(0);
    let (n, _) = transcribe::insert_captions(&mut p, &words, range, &style, chars, true);
    p.save(&path)?;
    println!("{} words, {} captions ({}) in {:.1}s", words.len(), n, lang, t0.elapsed().as_secs_f64());
    Ok(())
}

/// Developer helper: render every transition / effect / filter / animation into contact sheets.
fn cli_catalog_sheet(args: &[String]) -> Result<()> {
    use model::catalog;
    let dir = PathBuf::from(args.first().cloned().unwrap_or_else(|| "sheets".into()));
    std::fs::create_dir_all(&dir)?;
    let mut eng = engine::Engine::headless()?;
    let mut sf = preview::SampleFrames::new(640, 360);
    let (tw, th) = (256u32, 144u32);
    let label = |eng: &mut engine::Engine, text: &str| {
        let td = model::TextData { text: text.into(), size: 3.2, bold: true, ..Default::default() };
        eng.raster.text(&td, 1.0, None)
    };
    let sheet = |name: &str, rows: Vec<(String, Vec<Vec<u8>>)>, eng: &mut engine::Engine| -> Result<()> {
        let cols = rows.iter().map(|r| r.1.len()).max().unwrap_or(1) as u32;
        let lw = 170u32;
        let per_col = 2u32;
        let rows_n = rows.len() as u32;
        let rows_per = rows_n.div_ceil(per_col);
        let cell_w = lw + cols * (tw + 4);
        let mut img = image::RgbaImage::from_pixel(cell_w * per_col, rows_per * (th + 4), image::Rgba([16, 16, 18, 255]));
        for (i, (name, tiles)) in rows.iter().enumerate() {
            let col = i as u32 / rows_per;
            let row = i as u32 % rows_per;
            let ox = col * cell_w;
            let oy = row * (th + 4);
            for (k, t) in tiles.iter().enumerate() {
                let x0 = ox + lw + k as u32 * (tw + 4);
                for y in 0..th {
                    for x in 0..tw {
                        let i = ((y * tw + x) * 4) as usize;
                        img.put_pixel(x0 + x, oy + y, image::Rgba([t[i], t[i + 1], t[i + 2], 255]));
                    }
                }
            }
            let l = label(eng, name);
            for y in 0..l.h.min(th) {
                for x in 0..l.w.min(lw) {
                    let i = ((y * l.w + x) * 4) as usize;
                    let a = l.data[i + 3] as u32;
                    if a > 0 {
                        let px = img.get_pixel_mut(ox + 4 + x, oy + 4 + y);
                        for c in 0..3 {
                            px.0[c] = ((l.data[i + c] as u32) + (px.0[c] as u32) * (255 - a) / 255).min(255) as u8;
                        }
                    }
                }
            }
        }
        let path = dir.join(format!("{}.png", name));
        img.save(&path)?;
        println!("wrote {}", path.display());
        Ok(())
    };
    let mut rows = vec![];
    for d in catalog::TRANSITIONS {
        let (p, s, e) = preview::transition_project(d.id);
        let tiles = [0.2, 0.4, 0.6, 0.8].iter().map(|f| eng.render_rgba(&p, s + ((e - s) as f64 * f) as i64, &mut sf, tw, th)).collect();
        rows.push((d.id.to_string(), tiles));
    }
    sheet("transitions", rows, &mut eng)?;
    let mut rows = vec![];
    for d in catalog::EFFECTS {
        let p = preview::effect_project(d.id);
        let tiles = [0.4, 1.1, 2.3].iter().map(|t| eng.render_rgba(&p, util::s_to_us(*t), &mut sf, tw, th)).collect();
        rows.push((d.id.to_string(), tiles));
    }
    sheet("effects", rows, &mut eng)?;
    let mut rows = vec![];
    for d in catalog::FILTERS {
        let p = preview::filter_project(d.id);
        rows.push((d.id.to_string(), vec![eng.render_rgba(&p, util::SEC, &mut sf, tw, th)]));
    }
    sheet("filters", rows, &mut eng)?;
    let mut rows = vec![];
    for d in catalog::ANIMATIONS {
        let (p, total) = preview::anim_project(d.id, d.kind);
        let fr: Vec<f64> = match d.kind {
            catalog::AnimKind::In => vec![0.1, 0.3, 0.5],
            catalog::AnimKind::Out => vec![0.6, 0.8, 0.95],
            catalog::AnimKind::Combo => vec![0.2, 0.45, 0.7],
        };
        let tiles = fr.iter().map(|f| eng.render_rgba(&p, (total as f64 * f) as i64, &mut sf, tw, th)).collect();
        rows.push((format!("{:?} {}", d.kind, d.id), tiles));
    }
    sheet("animations", rows, &mut eng)?;
    let mut rows = vec![];
    for d in catalog::STICKERS {
        let p = preview::sticker_project(d.id);
        rows.push((d.id.to_string(), vec![eng.render_rgba(&p, util::SEC / 2, &mut sf, tw, th)]));
    }
    for d in catalog::TEXT_PRESETS {
        let p = preview::text_project(d);
        rows.push((d.id.to_string(), vec![eng.render_rgba(&p, util::SEC / 2, &mut sf, tw, th)]));
    }
    sheet("stickers_text", rows, &mut eng)?;
    Ok(())
}

#[cfg(windows)]
fn attach_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    // SAFETY: plain Win32 call without pointers; failing (no parent console) is harmless.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if args.first().is_some_and(|a| a.starts_with("--") && a != "--wayland") {
        attach_console();
    }
    let res = match args.first().map(|s| s.as_str()) {
        Some("--help") | Some("-h") => {
            print!("{}", HELP);
            Ok(())
        }
        Some("--version") => {
            println!("velomov {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Some("--make-demo") => {
            let dir = PathBuf::from(args.get(1).cloned().unwrap_or_else(|| "velomov-demo".into()));
            demo::make_demo(&dir).and_then(|mut p| {
                let out = dir.join("demo.vcut");
                p.save(&out)?;
                println!("demo project: {}", out.display());
                Ok(())
            })
        }
        Some("--render-frame") => cli_render_frame(&args[1..]),
        Some("--new-project") => cli_new_project(&args[1..]),
        Some("--export") => cli_export(&args[1..]),
        Some("--captions") => cli_captions(&args[1..]),
        Some("--catalog-sheet") => cli_catalog_sheet(&args[1..]),
        _ => app::run(args),
    };
    if let Err(e) = res {
        eprintln!("error: {:#}", e);
        std::process::exit(1);
    }
}
