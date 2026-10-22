//! Reversed-video proxies (a reversed copy is rendered when you press
//! "Reverse"). Done in short chunks so memory stays low, then concatenated.

use crate::util;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub struct ProxyJob {
    pub media: u64,
    pub progress: AtomicU32,
    pub done: AtomicBool,
    pub result: Mutex<Option<Result<String, String>>>,
}

impl ProxyJob {
    pub fn frac(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed))
    }
}

pub fn reversed_path(path: &str) -> PathBuf {
    let d = util::cache_dir().join("reverse");
    let _ = std::fs::create_dir_all(&d);
    d.join(format!("{:016x}.mp4", util::file_key(path)))
}

pub fn spawn_reverse(media: u64, path: &str, dur_s: f64) -> Arc<ProxyJob> {
    let job =
        Arc::new(ProxyJob { media, progress: AtomicU32::new(0f32.to_bits()), done: AtomicBool::new(false), result: Mutex::new(None) });
    let j = job.clone();
    let path = path.to_string();
    std::thread::spawn(move || {
        let out = reversed_path(&path);
        let res = if out.exists() { Ok(out.to_string_lossy().to_string()) } else { make(&path, dur_s, &out, &j) };
        *j.result.lock().unwrap() = Some(res);
        j.progress.store(1f32.to_bits(), Ordering::Relaxed);
        j.done.store(true, Ordering::Relaxed);
    });
    job
}

fn make(path: &str, dur_s: f64, out: &std::path::Path, job: &ProxyJob) -> Result<String, String> {
    let tmpdir = util::cache_dir().join(format!("reverse-tmp-{:x}", util::new_id()));
    std::fs::create_dir_all(&tmpdir).map_err(|e| e.to_string())?;
    let chunk = 2.0f64;
    let n = (dur_s / chunk).ceil().max(1.0) as usize;
    let mut list = String::new();
    for i in 0..n {
        let f = tmpdir.join(format!("c{:05}.mp4", i));
        let st = util::ffmpeg_job()
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-ss",
                &format!("{:.6}", i as f64 * chunk),
                "-t",
                &format!("{:.6}", chunk),
                "-i",
                path,
            ])
            .args([
                "-map", "0:v:0", "-an", "-vf", "reverse", "-c:v", "libx264", "-preset", "veryfast", "-crf", "15", "-pix_fmt", "yuv420p",
                "-g", "12",
            ])
            .arg(&f)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if !st.success() {
            let _ = std::fs::remove_dir_all(&tmpdir);
            return Err("ffmpeg reverse failed".into());
        }
        job.progress.store((((i + 1) as f32 / n as f32) * 0.95).to_bits(), Ordering::Relaxed);
    }
    for i in (0..n).rev() {
        list.push_str(&format!("file 'c{:05}.mp4'\n", i));
    }
    let lp = tmpdir.join("list.txt");
    std::fs::write(&lp, list).map_err(|e| e.to_string())?;
    let tmp_out = out.with_extension("tmp.mp4");
    let st = util::ffmpeg_job()
        .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-f", "concat", "-safe", "0", "-i"])
        .arg(&lp)
        .args(["-c", "copy"])
        .arg(&tmp_out)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&tmpdir);
    if !st.success() {
        return Err("ffmpeg concat failed".into());
    }
    std::fs::rename(&tmp_out, out).map_err(|e| e.to_string())?;
    Ok(out.to_string_lossy().to_string())
}

// ---------------------------------------------------------------- editing proxies

/// A light copy of heavy media (540p H.264, short GOP) used by the preview so 4K /
/// HEVC / high frame rate footage edits smoothly. Exports always use the originals.
#[derive(Clone, Debug)]
pub struct ProxyInfo {
    pub path: String,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Debug)]
pub enum ProxyState {
    Queued,
    Running(f32),
    Ready(ProxyInfo),
    Failed,
}

struct ProxySpec {
    src: String,
    w: u32,
    h: u32,
    rot: i32,
    dur_s: f64,
}

#[derive(Clone)]
pub struct ProxyManager {
    states: Arc<Mutex<(u64, std::collections::HashMap<String, ProxyState>)>>,
    tx: std::sync::mpsc::Sender<ProxySpec>,
    /// pid of the running ffmpeg and whether it is stopped (SIGSTOP) for playback
    job: Arc<Mutex<(Option<u32>, bool)>>,
}

pub const PROXY_SHORT_SIDE: u32 = 540;

/// Is this file heavy enough to be worth a proxy?
pub fn wants_proxy(m: &crate::model::MediaAsset) -> bool {
    if m.kind != crate::model::MediaKind::Video || m.path.starts_with("builtin:") {
        return false;
    }
    let px = m.width as f64 * m.height as f64;
    let heavy_codec = matches!(m.codec.as_str(), "hevc" | "h265" | "av1" | "vp9" | "prores" | "dnxhd" | "mjpeg");
    px > 1920.0 * 1088.0 || px * m.fps > 1920.0 * 1080.0 * 31.0 || (heavy_codec && px >= 1280.0 * 720.0)
}

fn proxy_path(src: &str) -> PathBuf {
    let d = util::cache_dir().join("proxies");
    let _ = std::fs::create_dir_all(&d);
    d.join(format!("{:016x}.mp4", util::file_key(src)))
}

fn proxy_size(w: u32, h: u32) -> (u32, u32) {
    let short = w.min(h).max(1) as f64;
    let k = (PROXY_SHORT_SIDE as f64 / short).min(1.0);
    let even = |v: f64| ((v / 2.0).round() as u32 * 2).max(2);
    (even(w as f64 * k), even(h as f64 * k))
}

fn run_ffmpeg_progress(mut cmd: Command, dur_s: f64, job: &Mutex<(Option<u32>, bool)>, on_progress: &dyn Fn(f32)) -> bool {
    use std::io::BufRead;
    cmd.args(["-progress", "pipe:1", "-nostats"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let Ok(mut child) = cmd.spawn() else { return false };
    {
        let mut j = job.lock().unwrap();
        j.0 = Some(child.id());
        if j.1 {
            signal(child.id(), true);
        }
    }
    if let Some(out) = child.stdout.take() {
        for line in std::io::BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(v) = line.strip_prefix("out_time_us=") {
                if let Ok(us) = v.trim().parse::<f64>() {
                    on_progress(((us / 1e6) / dur_s.max(0.001)).clamp(0.0, 1.0) as f32);
                }
            }
        }
    }
    job.lock().unwrap().0 = None;
    child.wait().map(|s| s.success()).unwrap_or(false)
}

/// Pause (SIGSTOP) or resume (SIGCONT) an encoder. Windows has no such signal: there the
/// proxy simply keeps going while the preview plays.
#[allow(unused_variables)]
fn signal(pid: u32, pause: bool) {
    #[cfg(unix)]
    // SAFETY: plain kill(2) on a child we spawned.
    unsafe {
        libc::kill(pid as i32, if pause { libc::SIGSTOP } else { libc::SIGCONT });
    }
}

fn make_proxy(spec: &ProxySpec, out: &std::path::Path, job: &Mutex<(Option<u32>, bool)>, on_progress: &dyn Fn(f32)) -> bool {
    let tmp = out.with_extension("tmp.mp4");
    let base = |cmd: &mut Command| {
        cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y"]);
    };
    let mut ok = false;
    if super::video::hwaccel_active() {
        // decode, scale and encode on the GPU
        let mut cmd = util::ffmpeg_job();
        base(&mut cmd);
        cmd.args(super::video::hw_input_args(spec.rot));
        cmd.args(["-i", &spec.src, "-map", "0:v:0", "-an", "-sn", "-dn"]);
        cmd.args(["-vf", &format!("{}scale_vaapi=w={}:h={}:format=nv12", super::video::hw_transpose(spec.rot), spec.w, spec.h)]);
        cmd.args(["-c:v", "h264_vaapi", "-qp", "24", "-g", "10", "-bf", "0"]);
        cmd.arg(&tmp);
        ok = run_ffmpeg_progress(cmd, spec.dur_s, job, on_progress);
    }
    if !ok {
        let mut cmd = util::ffmpeg_job();
        base(&mut cmd);
        cmd.args(["-i", &spec.src, "-map", "0:v:0", "-an", "-sn", "-dn"]);
        cmd.args(["-vf", &format!("scale={}:{}:flags=bilinear,format=yuv420p", spec.w, spec.h)]);
        cmd.args(["-c:v", "libx264", "-preset", "veryfast", "-crf", "24", "-g", "10", "-bf", "0"]);
        cmd.arg(&tmp);
        ok = run_ffmpeg_progress(cmd, spec.dur_s, job, on_progress);
    }
    ok && std::fs::rename(&tmp, out).is_ok()
}

impl ProxyManager {
    pub fn new() -> Self {
        let states = Arc::new(Mutex::new((1u64, std::collections::HashMap::new())));
        let (tx, rx) = std::sync::mpsc::channel::<ProxySpec>();
        let st = states.clone();
        let job = Arc::new(Mutex::new((None, false)));
        let jb = job.clone();
        std::thread::Builder::new()
            .name("velomov-proxy".into())
            .spawn(move || {
                while let Ok(spec) = rx.recv() {
                    let out = proxy_path(&spec.src);
                    let set = |s: ProxyState| {
                        let mut g = st.lock().unwrap();
                        g.0 += 1;
                        g.1.insert(spec.src.clone(), s);
                    };
                    set(ProxyState::Running(0.0));
                    let progress = |f: f32| {
                        let mut g = st.lock().unwrap();
                        g.1.insert(spec.src.clone(), ProxyState::Running(f));
                    };
                    if make_proxy(&spec, &out, &jb, &progress) {
                        set(ProxyState::Ready(ProxyInfo { path: out.to_string_lossy().to_string(), w: spec.w, h: spec.h }));
                    } else {
                        crate::vlog!("proxy failed for {}", spec.src);
                        set(ProxyState::Failed);
                    }
                }
            })
            .expect("proxy worker");
        ProxyManager { states, tx, job }
    }

    /// Hold proxy encoding while the preview plays: the video decoder of the GPU is
    /// shared, and playback matters more. Cheap to call every frame.
    pub fn set_paused(&self, paused: bool) {
        let mut j = self.job.lock().unwrap();
        if j.1 != paused {
            j.1 = paused;
            if let Some(pid) = j.0 {
                signal(pid, paused);
            }
        }
    }

    /// Make sure a proxy exists (or is being made) for this media.
    pub fn request(&self, m: &crate::model::MediaAsset) {
        if !wants_proxy(m) || !std::path::Path::new(&m.path).exists() {
            return;
        }
        let mut g = self.states.lock().unwrap();
        if matches!(g.1.get(&m.path), Some(ProxyState::Ready(_)) | Some(ProxyState::Queued) | Some(ProxyState::Running(_))) {
            return;
        }
        let (w, h) = proxy_size(m.width, m.height);
        let out = proxy_path(&m.path);
        g.0 += 1;
        if out.exists() {
            util::touch(&out);
            g.1.insert(m.path.clone(), ProxyState::Ready(ProxyInfo { path: out.to_string_lossy().to_string(), w, h }));
            return;
        }
        g.1.insert(m.path.clone(), ProxyState::Queued);
        let _ = self.tx.send(ProxySpec { src: m.path.clone(), w, h, rot: m.rotation, dur_s: crate::util::us_to_s(m.duration) });
    }

    pub fn state(&self, src: &str) -> Option<ProxyState> {
        self.states.lock().unwrap().1.get(src).cloned()
    }

    /// Changes whenever a proxy starts, finishes or fails.
    pub fn version(&self) -> u64 {
        self.states.lock().unwrap().0
    }

    pub fn ready(&self) -> std::collections::HashMap<String, ProxyInfo> {
        let g = self.states.lock().unwrap();
        g.1.iter().filter_map(|(k, v)| if let ProxyState::Ready(i) = v { Some((k.clone(), i.clone())) } else { None }).collect()
    }

    /// Drop finished proxies (their files were deleted); running ones are kept.
    pub fn forget_finished(&self) {
        let mut g = self.states.lock().unwrap();
        g.1.retain(|_, s| matches!(s, ProxyState::Queued | ProxyState::Running(_)));
        g.0 += 1;
    }

    pub fn busy(&self) -> bool {
        self.states.lock().unwrap().1.values().any(|s| matches!(s, ProxyState::Queued | ProxyState::Running(_)))
    }
}

impl Default for ProxyManager {
    fn default() -> Self {
        Self::new()
    }
}
