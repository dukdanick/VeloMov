//! Built-in sample pictures (used for library previews) and the demo project
//! generator (`velomov --make-demo DIR`), which exercises most features.

use crate::media;
use crate::model::catalog;
use crate::model::edit::Target;
use crate::model::keyframe::{Ease, Prop};
use crate::model::*;
use crate::util::{self, SEC};
use anyhow::{anyhow, Result};
use std::path::Path;
use std::process::Stdio;

fn mixc(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn hash(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(374761393) ^ y.wrapping_mul(668265263);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    (h ^ (h >> 16)) as f32 / u32::MAX as f32
}

/// Procedural sample pictures: 0 = sunset over mountains, 1 = neon city at night.
pub fn sample_image(kind: u32, w: u32, h: u32) -> image::RgbaImage {
    let mut img = image::RgbaImage::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let u = x as f32 / w as f32;
            let v = y as f32 / h as f32;
            let c = if kind == 0 {
                let horizon = 0.62;
                if v < horizon {
                    let t = v / horizon;
                    let mut sky = mixc([0.28, 0.16, 0.45], [1.0, 0.55, 0.25], t.powf(1.3));
                    // sun
                    let d = ((u - 0.68) * (w as f32 / h as f32)).hypot(v - 0.5);
                    let sun = (1.0 - ((d - 0.09) / 0.01).clamp(0.0, 1.0)) + (0.25 / (1.0 + d * 30.0));
                    sky = mixc(sky, [1.0, 0.92, 0.6], sun.min(1.0));
                    // mountains
                    let m1 = 0.47 + 0.07 * (u * 9.0).sin() + 0.04 * (u * 23.0 + 1.0).sin();
                    let m2 = 0.54 + 0.05 * (u * 6.0 + 2.0).sin() + 0.02 * (u * 31.0).sin();
                    if v > m2 {
                        sky = mixc([0.12, 0.08, 0.2], [0.2, 0.1, 0.25], (v - m2) * 4.0);
                    } else if v > m1 {
                        sky = mixc(sky, [0.35, 0.2, 0.42], 0.75);
                    }
                    sky
                } else {
                    let t = (v - horizon) / (1.0 - horizon);
                    let refl = 0.25 * (1.0 - t) * (((u - 0.68).abs() * 14.0).min(1.0) - 1.0).abs();
                    let ripple = 0.04 * ((v * 220.0 + (u * 13.0).sin() * 3.0).sin());
                    let base = mixc([0.55, 0.3, 0.35], [0.08, 0.05, 0.15], t);
                    [base[0] + refl + ripple, base[1] + refl * 0.8 + ripple, base[2] + refl * 0.4 + ripple]
                }
            } else {
                let mut c = mixc([0.02, 0.02, 0.08], [0.1, 0.03, 0.2], v);
                // bokeh
                for i in 0..14u32 {
                    let cx = hash(i, 1);
                    let cy = hash(i, 2) * 0.6;
                    let r = 0.02 + hash(i, 3) * 0.05;
                    let d = ((u - cx) * (w as f32 / h as f32)).hypot(v - cy);
                    let col = match i % 4 {
                        0 => [1.0, 0.2, 0.6],
                        1 => [0.1, 0.8, 1.0],
                        2 => [1.0, 0.7, 0.2],
                        _ => [0.6, 0.3, 1.0],
                    };
                    let a = (1.0 - ((d - r) / 0.006).clamp(0.0, 1.0)) * 0.35;
                    c = mixc(c, col, a);
                }
                // skyline
                let bw = 0.06;
                let bi = (u / bw).floor() as u32;
                let bh = 0.45 + hash(bi, 7) * 0.35;
                if v > bh {
                    c = [0.03, 0.03, 0.06];
                    let wx = ((u / bw).fract() * 6.0).floor() as u32;
                    let wy = ((v - bh) * 60.0).floor() as u32;
                    let lit = hash(bi * 13 + wx, wy) > 0.55
                        && (u / bw).fract() > 0.12
                        && (u / bw).fract() < 0.88
                        && ((v - bh) * 60.0).fract() < 0.6;
                    if lit {
                        c = if hash(bi, wy) > 0.5 { [1.0, 0.8, 0.4] } else { [0.4, 0.9, 1.0] };
                    }
                }
                c
            };
            img.put_pixel(
                x,
                y,
                image::Rgba([
                    (c[0].clamp(0.0, 1.0) * 255.0) as u8,
                    (c[1].clamp(0.0, 1.0) * 255.0) as u8,
                    (c[2].clamp(0.0, 1.0) * 255.0) as u8,
                    255,
                ]),
            );
        }
    }
    img
}

fn ffmpeg_gen(out: &Path, args: &[&str]) -> Result<()> {
    if out.exists() {
        return Ok(());
    }
    let st = crate::util::command("ffmpeg")
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .arg(out)
        .stdin(Stdio::null())
        .status()?;
    if !st.success() {
        return Err(anyhow!("ffmpeg failed generating {}", out.display()));
    }
    Ok(())
}

fn add_media(p: &mut Project, path: &Path) -> Result<u64> {
    let m = media::probe(&path.to_string_lossy())?;
    let id = m.id;
    p.media.push(m);
    Ok(id)
}

/// Generate demo media in `dir` and a project showing transitions, keyframes, effects...
pub fn make_demo(dir: &Path) -> Result<Project> {
    std::fs::create_dir_all(dir)?;
    let c1 = dir.join("demo-testsrc.mp4");
    let c2 = dir.join("demo-mandelbrot.mp4");
    let c3 = dir.join("demo-gradients.mp4");
    let c4 = dir.join("demo-bars.mp4");
    let im = dir.join("demo-sunset.png");
    let im2 = dir.join("demo-city.png");
    ffmpeg_gen(
        &c1,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=1280x720:rate=30:duration=4",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=330:duration=4",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ],
    )?;
    ffmpeg_gen(
        &c2,
        &[
            "-f",
            "lavfi",
            "-i",
            "mandelbrot=size=1280x720:rate=30",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=495:duration=4",
            "-t",
            "4",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ],
    )?;
    ffmpeg_gen(
        &c3,
        &["-f", "lavfi", "-i", "gradients=size=1280x720:rate=30:speed=0.02:duration=4", "-c:v", "libx264", "-pix_fmt", "yuv420p"],
    )?;
    ffmpeg_gen(&c4, &["-f", "lavfi", "-i", "smptehdbars=size=1280x720:rate=30:duration=4", "-c:v", "libx264", "-pix_fmt", "yuv420p"])?;
    if !im.exists() {
        sample_image(0, 1600, 900).save(&im)?;
    }
    if !im2.exists() {
        sample_image(1, 1600, 900).save(&im2)?;
    }
    let mut p = Project::new_named("VeloMov demo");
    let m1 = add_media(&mut p, &c1)?;
    let m2 = add_media(&mut p, &c2)?;
    let m3 = add_media(&mut p, &c3)?;
    let m4 = add_media(&mut p, &c4)?;
    let mi = add_media(&mut p, &im)?;
    let mi2 = add_media(&mut p, &im2)?;
    // main track
    let a = p.insert_media(mi, Target::Main(i64::MAX), 3 * SEC).unwrap();
    let b = p.insert_media(m1, Target::Main(i64::MAX), 3 * SEC).unwrap();
    let c = p.insert_media(m2, Target::Main(i64::MAX), 3 * SEC).unwrap();
    let d = p.insert_media(mi2, Target::Main(i64::MAX), 3 * SEC).unwrap();
    let e = p.insert_media(m3, Target::Main(i64::MAX), 3 * SEC).unwrap();
    p.apply_ratio(Ratio::R16x9);
    let tr = |id: &str| TransitionUse {
        id: id.into(),
        dur: catalog::transition(id).map(|t| t.dur).unwrap_or(SEC / 2),
        blur: catalog::transition(id).map(|t| t.blur).unwrap_or(0.5),
    };
    p.set_transition(a, Some(tr("slide_left")));
    p.set_transition(b, Some(tr("pull_in")));
    p.set_transition(c, Some(tr("whip_right")));
    p.set_transition(d, Some(tr("cube_left")));
    // keyframes: ken-burns on the first picture
    if let Some(ca) = p.clip_mut(a) {
        ca.kf.set(Prop::Scale, 0, 1.0);
        ca.kf.set(Prop::Scale, 3 * SEC, 1.25);
        ca.kf.set_ease(Prop::Scale, 0, Ease::EaseInOut);
        ca.anim_in = Some(AnimUse { id: "fade_in".into(), dur: SEC / 2 });
    }
    if let Some(cc) = p.clip_mut(c) {
        cc.filter = Some(FilterUse { id: "cinematic".into(), intensity: 1.0 });
        cc.canvas = CanvasFill::Blur(0.6);
        cc.tf.scale = 0.8;
    }
    if let Some(cd) = p.clip_mut(d) {
        cd.adjust.saturation = 0.3;
        cd.adjust.vignette = 0.5;
    }
    // overlay with keyframed slide + motion blur and a circle mask
    let ov = p.insert_media(m4, Target::Overlay(SEC * 3 / 2), 3 * SEC).unwrap();
    if let Some(co) = p.clip_mut(ov) {
        co.duration = 2 * SEC;
        co.tf.scale = 0.35;
        co.motion_blur = 0.8;
        co.kf.set(Prop::PosX, 0, -1200.0);
        co.kf.set(Prop::PosX, SEC / 2, 520.0);
        co.kf.set_ease(Prop::PosX, 0, Ease::EaseOut);
        co.kf.set(Prop::PosY, 0, 260.0);
        co.mask = Some(Mask { kind: MaskKind::Circle, w: 0.5, h: 0.89, feather: 0.1, ..Default::default() });
        co.anim_out = Some(AnimUse { id: "slide_up_out".into(), dur: SEC * 4 / 10 });
    }
    // title
    let mut title = Clip::new(
        ClipKind::Text(TextData {
            text: "VeloMov".into(),
            size: 22.0,
            bold: true,
            stroke: true,
            stroke_color: [0, 0, 0, 255],
            stroke_width: 0.45,
            shadow: true,
            ..Default::default()
        }),
        0,
        0,
    );
    title.duration = 2 * SEC + SEC / 2;
    title.tf.y = 250.0;
    title.anim_in = Some(AnimUse { id: "pop_in".into(), dur: SEC / 2 });
    title.anim_out = Some(AnimUse { id: "slide_left_out".into(), dur: SEC * 4 / 10 });
    p.add_clip(title, SEC / 4);
    let mut sub = Clip::new(
        ClipKind::Text(TextData { text: "slide • pull in • whip • cube".into(), size: 9.0, background: true, ..Default::default() }),
        0,
        0,
    );
    sub.duration = 4 * SEC;
    sub.tf.y = -380.0;
    sub.anim_in = Some(AnimUse { id: "typewriter".into(), dur: SEC });
    p.add_clip(sub, 3 * SEC);
    // sticker
    let mut st = Clip::new(ClipKind::Sticker(StickerData { id: "heart".into(), color: [255, 64, 96, 255] }), 0, 0);
    st.duration = 2 * SEC;
    st.tf.x = 650.0;
    st.tf.y = 300.0;
    st.tf.scale = 0.7;
    st.anim_combo = Some(AnimUse { id: "combo_heartbeat".into(), dur: SEC });
    p.add_clip(st, 6 * SEC);
    // effects
    let mut fx = Clip::new(ClipKind::Effect(EffectUse { id: "glitch".into(), params: [0.7, 0.6, 0.5, 0.5] }), 0, 0);
    fx.duration = SEC;
    p.add_clip(fx, 9 * SEC);
    let mut fx2 = Clip::new(ClipKind::Effect(EffectUse { id: "letterbox".into(), params: [0.5, 0.5, 0.5, 0.5] }), 0, 0);
    fx2.duration = 3 * SEC;
    p.add_clip(fx2, 11 * SEC);
    // sound effects on the transitions
    for (i, id) in ["whoosh", "whoosh", "whip"].iter().enumerate() {
        if let Some(path) = media::sfx::path_for(id) {
            if let Ok(mut m) = media::probe(&path.to_string_lossy()) {
                m.builtin = true;
                m.name = catalog::SFX.iter().find(|s| s.id == *id).map(|s| s.name.to_string()).unwrap_or_default();
                let mid = m.id;
                p.media.push(m);
                let cut = p.tracks[p.main_track_index()].clips[i + 1].start;
                p.insert_media(mid, Target::Overlay(cut - SEC / 4), 0);
            }
        }
    }
    let _ = (e, util::new_id());
    p.normalize();
    Ok(p)
}
