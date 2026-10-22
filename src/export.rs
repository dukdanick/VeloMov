//! Export: renders every frame on the GPU (same compositor as the preview),
//! reads it back and pipes it into an FFmpeg encoder together with the mixed
//! audio track.

use crate::engine::{gpu::Readback, Engine};
use crate::media::{self, audio};
use crate::model::Project;
use crate::util::{self, Us, SEC};
use anyhow::{anyhow, Result};
use eframe::wgpu;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Codec {
    H264,
    Hevc,
    Av1,
    Vp9,
    ProRes,
    Gif,
    H264Vaapi,
    HevcVaapi,
}

impl Codec {
    pub const ALL: [Codec; 8] =
        [Codec::H264, Codec::Hevc, Codec::Av1, Codec::Vp9, Codec::ProRes, Codec::Gif, Codec::H264Vaapi, Codec::HevcVaapi];
    pub fn label(self) -> &'static str {
        match self {
            Codec::H264 => "H.264",
            Codec::Hevc => "HEVC (H.265)",
            Codec::Av1 => "AV1",
            Codec::Vp9 => "VP9 (WebM)",
            Codec::ProRes => "ProRes 422",
            Codec::Gif => "GIF",
            Codec::H264Vaapi => "H.264 (GPU, VA-API)",
            Codec::HevcVaapi => "HEVC (GPU, VA-API)",
        }
    }
    pub fn ext(self) -> &'static str {
        match self {
            Codec::Vp9 => "webm",
            Codec::ProRes => "mov",
            Codec::Gif => "gif",
            _ => "mp4",
        }
    }
    pub fn available(self) -> bool {
        let enc = match self {
            Codec::H264 => "libx264",
            Codec::Hevc => "libx265",
            Codec::Av1 => "libsvtav1",
            Codec::Vp9 => "libvpx-vp9",
            Codec::ProRes => "prores_ks",
            Codec::Gif => "gif",
            Codec::H264Vaapi => "h264_vaapi",
            Codec::HevcVaapi => "hevc_vaapi",
        };
        encoders().contains(&enc.to_string())
            && (!matches!(self, Codec::H264Vaapi | Codec::HevcVaapi) || std::path::Path::new("/dev/dri/renderD128").exists())
    }
}

fn encoders() -> &'static Vec<String> {
    static E: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    E.get_or_init(|| {
        let out = crate::util::command("ffmpeg").args(["-hide_banner", "-encoders"]).output();
        match out {
            Ok(o) => {
                String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.split_whitespace().nth(1).map(|s| s.to_string())).collect()
            }
            Err(_) => vec![],
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Lower,
    Recommended,
    Higher,
}

impl Quality {
    pub fn label(self) -> &'static str {
        match self {
            Quality::Lower => "Lower",
            Quality::Recommended => "Recommended",
            Quality::Higher => "Higher",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioFmt {
    Mp3,
    Aac,
    Wav,
    Flac,
}

impl AudioFmt {
    pub fn ext(self) -> &'static str {
        match self {
            AudioFmt::Mp3 => "mp3",
            AudioFmt::Aac => "m4a",
            AudioFmt::Wav => "wav",
            AudioFmt::Flac => "flac",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub path: PathBuf,
    /// Resolution as the short side (480/720/1080/1440/2160).
    pub res: u32,
    pub fps: f64,
    pub codec: Codec,
    pub quality: Quality,
    pub video: bool,
    pub audio: bool,
    pub audio_fmt: AudioFmt,
    pub range: Option<(Us, Us)>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            path: util::videos_dir().join("velomov-export.mp4"),
            res: 1080,
            fps: 30.0,
            codec: Codec::H264,
            quality: Quality::Recommended,
            video: true,
            audio: true,
            audio_fmt: AudioFmt::Mp3,
            range: None,
        }
    }
}

pub fn output_size(p: &Project, res: u32) -> (u32, u32) {
    let short = p.width.min(p.height).max(1) as f64;
    let k = res as f64 / short;
    let even = |v: f64| ((v / 2.0).round() as u32 * 2).max(2);
    (even(p.width as f64 * k), even(p.height as f64 * k))
}

#[derive(Default)]
pub struct Progress {
    pub frac: AtomicU32,
    pub done: AtomicBool,
    pub cancel: AtomicBool,
    pub error: Mutex<Option<String>>,
    pub stage: Mutex<String>,
    pub fps: AtomicU32,
    pub output: Mutex<Option<PathBuf>>,
    pub started: Mutex<Option<Instant>>,
}

impl Progress {
    pub fn frac(&self) -> f32 {
        f32::from_bits(self.frac.load(Ordering::Relaxed))
    }
    fn set(&self, f: f32) {
        self.frac.store(f.to_bits(), Ordering::Relaxed);
    }
    fn stage(&self, s: &str) {
        *self.stage.lock().unwrap() = s.to_string();
    }
    pub fn eta_s(&self) -> Option<f64> {
        let st = (*self.started.lock().unwrap())?;
        let f = self.frac() as f64;
        if f < 0.02 {
            return None;
        }
        let el = st.elapsed().as_secs_f64();
        Some(el / f - el)
    }
}

pub fn spawn(project: Project, settings: Settings, device: Option<(wgpu::Device, wgpu::Queue)>) -> Arc<Progress> {
    let prog = Arc::new(Progress::default());
    *prog.started.lock().unwrap() = Some(Instant::now());
    let p2 = prog.clone();
    std::thread::Builder::new()
        .name("velomov-export".into())
        .spawn(move || {
            let r = run(&project, &settings, device, &p2);
            if let Err(e) = r {
                crate::vlog!("export failed: {:#}", e);
                *p2.error.lock().unwrap() = Some(format!("{:#}", e));
                let _ = std::fs::remove_file(&settings.path);
            } else if !p2.cancel.load(Ordering::Relaxed) {
                *p2.output.lock().unwrap() = Some(settings.path.clone());
                p2.set(1.0);
            }
            p2.done.store(true, Ordering::Relaxed);
        })
        .expect("export thread");
    prog
}

fn mix_audio(project: &Project, from: Us, to: Us, prog: &Progress) -> Result<PathBuf> {
    prog.stage("Mixing audio");
    let lib = audio::AudioLib::new();
    let start = Instant::now();
    let plan = loop {
        let (plan, complete) = audio::build_plan(project, &lib);
        if complete {
            break plan;
        }
        if start.elapsed().as_secs() > 600 || prog.cancel.load(Ordering::Relaxed) {
            break plan;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    let wav = util::cache_dir().join(format!("export-{:x}.wav", util::new_id()));
    audio::render_wav(&plan, from, to, &wav)?;
    Ok(wav)
}

pub fn run(project: &Project, s: &Settings, device: Option<(wgpu::Device, wgpu::Queue)>, prog: &Progress) -> Result<()> {
    let (from, to) = s.range.unwrap_or((0, project.duration()));
    if to <= from {
        return Err(anyhow!("the timeline is empty"));
    }
    if let Some(dir) = s.path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // audio only
    if !s.video {
        let wav = mix_audio(project, from, to, prog)?;
        prog.stage("Encoding audio");
        let mut cmd = crate::util::command("ffmpeg");
        cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"]).arg(&wav);
        match s.audio_fmt {
            AudioFmt::Mp3 => cmd.args(["-c:a", "libmp3lame", "-b:a", "256k"]),
            AudioFmt::Aac => cmd.args(["-c:a", "aac", "-b:a", "256k"]),
            AudioFmt::Wav => cmd.args(["-c:a", "pcm_s16le"]),
            AudioFmt::Flac => cmd.args(["-c:a", "flac"]),
        };
        let st = cmd.arg(&s.path).status()?;
        let _ = std::fs::remove_file(&wav);
        if !st.success() {
            return Err(anyhow!("ffmpeg audio encode failed"));
        }
        return Ok(());
    }
    let (w, h) = output_size(project, s.res);
    let audio_wav = if s.audio && s.codec != Codec::Gif { Some(mix_audio(project, from, to, prog)?) } else { None };
    prog.stage("Rendering");
    let mut engine = match device {
        Some((d, q)) => Engine::new(d, q),
        None => Engine::headless()?,
    };
    let mut frames = media::Frames::new(true);
    let fps = s.fps.max(1.0);
    let total = (((to - from) as f64 / SEC as f64) * fps).round().max(1.0) as u64;

    // encoder
    let mut cmd = crate::util::command("ffmpeg");
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y"]);
    let vaapi = matches!(s.codec, Codec::H264Vaapi | Codec::HevcVaapi);
    if vaapi {
        cmd.args(["-vaapi_device", "/dev/dri/renderD128"]);
    }
    cmd.args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{}x{}", w, h), "-r", &format!("{}", fps), "-i", "pipe:0"]);
    if let Some(wav) = &audio_wav {
        cmd.arg("-i").arg(wav);
    }
    let crf = |l: u32, r: u32, hq: u32| -> String {
        match s.quality {
            Quality::Lower => l,
            Quality::Recommended => r,
            Quality::Higher => hq,
        }
        .to_string()
    };
    let colour = ["-colorspace", "bt709", "-color_primaries", "bt709", "-color_trc", "bt709", "-color_range", "tv"];
    let vf_709 = "scale=out_color_matrix=bt709:out_range=tv,format=yuv420p";
    match s.codec {
        Codec::H264 => {
            cmd.args(["-vf", vf_709, "-c:v", "libx264", "-preset", "medium", "-crf", &crf(26, 20, 16), "-pix_fmt", "yuv420p"]).args(colour);
        }
        Codec::Hevc => {
            cmd.args([
                "-vf",
                vf_709,
                "-c:v",
                "libx265",
                "-preset",
                "medium",
                "-crf",
                &crf(28, 23, 19),
                "-pix_fmt",
                "yuv420p",
                "-tag:v",
                "hvc1",
                "-x265-params",
                "log-level=error",
            ])
            .args(colour);
        }
        Codec::Av1 => {
            cmd.args(["-vf", vf_709, "-c:v", "libsvtav1", "-preset", "8", "-crf", &crf(38, 32, 26), "-pix_fmt", "yuv420p"]).args(colour);
        }
        Codec::Vp9 => {
            cmd.args([
                "-vf",
                vf_709,
                "-c:v",
                "libvpx-vp9",
                "-crf",
                &crf(36, 31, 24),
                "-b:v",
                "0",
                "-row-mt",
                "1",
                "-deadline",
                "good",
                "-cpu-used",
                "4",
                "-pix_fmt",
                "yuv420p",
            ])
            .args(colour);
        }
        Codec::ProRes => {
            cmd.args([
                "-vf",
                "scale=out_color_matrix=bt709:out_range=tv,format=yuv422p10le",
                "-c:v",
                "prores_ks",
                "-profile:v",
                "2",
                "-pix_fmt",
                "yuv422p10le",
            ])
            .args(colour);
        }
        Codec::Gif => {
            let gw = w.min(720);
            cmd.args(["-vf", &format!("fps={},scale={}:-2:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4", fps.min(20.0), gw), "-loop", "0"]);
        }
        Codec::H264Vaapi => {
            cmd.args(["-vf", "format=nv12,hwupload", "-c:v", "h264_vaapi", "-qp", &crf(28, 22, 18)]);
        }
        Codec::HevcVaapi => {
            cmd.args(["-vf", "format=nv12,hwupload", "-c:v", "hevc_vaapi", "-qp", &crf(30, 24, 20)]);
        }
    }
    if audio_wav.is_some() {
        cmd.args(["-map", "0:v:0", "-map", "1:a:0"]);
        match s.codec {
            Codec::Vp9 => {
                cmd.args(["-c:a", "libopus", "-b:a", "192k"]);
            }
            Codec::ProRes => {
                cmd.args(["-c:a", "pcm_s16le"]);
            }
            _ => {
                cmd.args(["-c:a", "aac", "-b:a", "192k"]);
            }
        }
        cmd.arg("-shortest");
    }
    if s.codec.ext() == "mp4" {
        cmd.args(["-movflags", "+faststart"]);
    }
    cmd.arg(&s.path);
    cmd.stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    crate::vlog!("export {}x{} @{} -> {}", w, h, fps, s.path.display());
    let mut child = cmd.spawn().map_err(|e| anyhow!("cannot start ffmpeg: {}", e))?;
    let stdin = child.stdin.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let err_text = Arc::new(Mutex::new(String::new()));
    let et = err_text.clone();
    let err_thread = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut stderr, &mut s);
        *et.lock().unwrap() = s;
    });
    // writer thread so rendering overlaps with encoding
    let (wtx, wrx) = std::sync::mpsc::sync_channel::<Vec<u8>>(3);
    let writer = std::thread::spawn(move || -> std::io::Result<()> {
        let mut stdin = stdin;
        while let Ok(buf) = wrx.recv() {
            stdin.write_all(&buf)?;
        }
        stdin.flush()
    });
    let mut rb = Readback::new(&engine.gpu.device, w, h, 3);
    let t0 = Instant::now();
    let write_err = std::cell::Cell::new(false);
    let mut sink = |_: u64, data: &[u8]| {
        if wtx.send(data.to_vec()).is_err() {
            write_err.set(true);
        }
    };
    for n in 0..total {
        if prog.cancel.load(Ordering::Relaxed) {
            break;
        }
        let t = from + ((n as f64) * SEC as f64 / fps).round() as Us;
        let (tex, _) = engine.render_tex(project, t, &mut frames, w, h, true);
        if rb.pending_in_next().is_some() {
            rb.collect_oldest(&engine.gpu.device, &mut sink);
        }
        let mut enc = engine.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("export-copy") });
        rb.copy(&mut enc, &tex, n);
        engine.gpu.queue.submit(Some(enc.finish()));
        engine.gpu.release(tex);
        prog.set((n + 1) as f32 / total as f32 * 0.99);
        let el = t0.elapsed().as_secs_f32().max(0.001);
        prog.fps.store(((n + 1) as f32 / el).to_bits(), Ordering::Relaxed);
        if n % 30 == 0 {
            frames.video.gc(std::time::Duration::from_secs(5));
        }
        if write_err.get() {
            break;
        }
    }
    while rb.collect_oldest(&engine.gpu.device, &mut sink) {}
    drop(wtx);
    let wres = writer.join();
    let cancelled = prog.cancel.load(Ordering::Relaxed);
    if cancelled {
        let _ = child.kill();
    }
    prog.stage("Finishing");
    let status = child.wait()?;
    let _ = err_thread.join();
    frames.video.stop_all();
    if let Some(wav) = &audio_wav {
        let _ = std::fs::remove_file(wav);
    }
    if cancelled {
        let _ = std::fs::remove_file(&s.path);
        return Ok(());
    }
    if !status.success() || matches!(wres, Ok(Err(_))) {
        let e = err_text.lock().unwrap().clone();
        return Err(anyhow!("ffmpeg failed: {}", e.trim()));
    }
    Ok(())
}
