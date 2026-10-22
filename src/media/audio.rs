//! Audio: decoded PCM caches (48 kHz stereo s16, memory-mapped), derived
//! variants (tempo/reverse/voice effects), waveform peaks and the mixer.

use crate::model::catalog;
use crate::model::keyframe::Prop;
use crate::model::*;
use crate::util::{self, Us};
use memmap2::Mmap;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

pub const RATE: u32 = 48_000;

pub struct Pcm {
    map: Option<Mmap>,
    pub frames: usize,
    /// max |amplitude| per PEAK_BLOCK frames, 0..255
    pub peaks: Vec<u8>,
}

pub const PEAK_BLOCK: usize = 256;

/// FFmpeg filters that reduce steady background noise (hiss, hum, fans, wind) by `k` (0..1):
/// cut rumble below speech, then an FFT denoiser that learns the noise floor as it goes.
pub fn denoise_filter(k: f64) -> String {
    let k = k.clamp(0.0, 1.0);
    let nr = 6.0 + 34.0 * k; // dB of reduction
    let mut f = format!("highpass=f={:.0},afftdn=nr={:.1}:nf=-{:.0}:tn=1", 40.0 + 50.0 * k, nr, 50.0 - 15.0 * k);
    if k > 0.6 {
        // strong: a second spectral pass catches what the first one leaves
        f.push_str(&format!(",afftdn=nr={:.1}:nf=-45:tn=1", 10.0 + 25.0 * (k - 0.6)));
    }
    f
}

/// Silent stretches of a clip, as timeline ranges to cut: stretches quieter than `thresh_db`
/// (dBFS) lasting at least `min_len`, minus a `pad` of air kept on each side.
pub fn silences(pcm: &Pcm, c: &crate::model::Clip, media_dur: Us, thresh_db: f32, min_len: Us, pad: Us) -> Vec<(Us, Us)> {
    let step = 10_000; // 10 ms windows
    let mut quiet: Vec<(Us, bool)> = vec![];
    let mut t = c.start;
    while t < c.end() {
        let te = (t + step).min(c.end());
        let a = c.source_time(t, media_dur);
        let b = c.source_time(te, media_dur);
        let (s0, s1) = if a <= b { (a, b) } else { (b, a) };
        let f0 = (util::us_to_s(s0) * 48_000.0) as usize;
        let f1 = ((util::us_to_s(s1) * 48_000.0) as usize).max(f0 + 1);
        let mut acc = 0.0f64;
        for i in f0..f1 {
            let (l, r) = pcm.frame(i);
            acc += ((l * l + r * r) * 0.5) as f64;
        }
        let rms = (acc / (f1 - f0) as f64).sqrt().max(1e-9);
        quiet.push((t, 20.0 * (rms as f32).log10() < thresh_db));
        t = te;
    }
    let mut out = vec![];
    let mut i = 0;
    while i < quiet.len() {
        if !quiet[i].1 {
            i += 1;
            continue;
        }
        let a = quiet[i].0;
        while i < quiet.len() && quiet[i].1 {
            i += 1;
        }
        let b = if i < quiet.len() { quiet[i].0 } else { c.end() };
        if b - a < min_len {
            continue;
        }
        // keep a little air next to the sound (none at the clip's own edges)
        let a2 = if a <= c.start { c.start } else { a + pad };
        let b2 = if b >= c.end() { c.end() } else { b - pad };
        if b2 - a2 >= 50_000 {
            out.push((a2, b2));
        }
    }
    out
}

impl Pcm {
    fn open(path: &std::path::Path) -> Option<Pcm> {
        let f = std::fs::File::open(path).ok()?;
        let len = f.metadata().ok()?.len() as usize;
        if len < 4 {
            return Some(Pcm { map: None, frames: 0, peaks: vec![] });
        }
        let map = unsafe { Mmap::map(&f).ok()? };
        let frames = len / 4;
        let mut pcm = Pcm { map: Some(map), frames, peaks: vec![] };
        pcm.peaks = pcm.compute_peaks();
        Some(pcm)
    }

    fn samples(&self) -> &[i16] {
        match &self.map {
            Some(m) => {
                let bytes: &[u8] = m;
                // s16le, 2-byte aligned mmap
                let n = bytes.len() / 2;
                unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const i16, n) }
            }
            None => &[],
        }
    }

    fn compute_peaks(&self) -> Vec<u8> {
        let s = self.samples();
        let blocks = self.frames.div_ceil(PEAK_BLOCK);
        let mut out = Vec::with_capacity(blocks);
        for b in 0..blocks {
            let a = b * PEAK_BLOCK * 2;
            let e = ((b + 1) * PEAK_BLOCK * 2).min(s.len());
            let mut m: i32 = 0;
            for v in &s[a..e] {
                m = m.max((*v as i32).abs());
            }
            out.push(((m as f32 / 32768.0).sqrt() * 255.0).min(255.0) as u8);
        }
        out
    }

    #[inline]
    pub fn frame(&self, i: usize) -> (f32, f32) {
        let s = self.samples();
        if i >= self.frames {
            return (0.0, 0.0);
        }
        (s[i * 2] as f32 / 32768.0, s[i * 2 + 1] as f32 / 32768.0)
    }

    #[inline]
    pub fn interp(&self, pos: f64) -> (f32, f32) {
        if pos < 0.0 {
            return (0.0, 0.0);
        }
        let i = pos.floor() as usize;
        let f = (pos - i as f64) as f32;
        let (a0, a1) = self.frame(i);
        if f == 0.0 {
            return (a0, a1);
        }
        let (b0, b1) = self.frame(i + 1);
        (a0 + (b0 - a0) * f, a1 + (b1 - a1) * f)
    }
}

enum Job {
    Decode { key: String, src: String, out: PathBuf },
    Derive { key: String, base: PathBuf, filter: String, out: PathBuf },
    Reverse { key: String, base: PathBuf, out: PathBuf },
}

struct Inner {
    ready: HashMap<String, Arc<Pcm>>,
    pending: HashSet<String>,
    failed: HashSet<String>,
}

/// Lazily decoded PCM library, shared by the UI, the player and the exporter.
#[derive(Clone)]
pub struct AudioLib {
    inner: Arc<Mutex<Inner>>,
    tx: Sender<Job>,
}

fn dir() -> PathBuf {
    let d = util::cache_dir().join("audio");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn base_key(path: &str) -> String {
    format!("{:016x}", util::file_key(path))
}

fn run_job(job: &Job) -> bool {
    match job {
        Job::Decode { src, out, .. } => {
            let tmp = out.with_extension("tmp");
            let st = crate::util::command("ffmpeg")
                .args([
                    "-nostdin",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-y",
                    "-i",
                    src,
                    "-vn",
                    "-sn",
                    "-ac",
                    "2",
                    "-ar",
                    "48000",
                    "-f",
                    "s16le",
                ])
                .arg(&tmp)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            st.map(|s| s.success()).unwrap_or(false) && std::fs::rename(&tmp, out).is_ok()
        }
        Job::Derive { base, filter, out, .. } => {
            let tmp = out.with_extension("tmp");
            let st = crate::util::command("ffmpeg")
                .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-f", "s16le", "-ar", "48000", "-ac", "2", "-i"])
                .arg(base)
                .args(["-af", filter, "-ac", "2", "-ar", "48000", "-f", "s16le"])
                .arg(&tmp)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
            st.map(|s| s.success()).unwrap_or(false) && std::fs::rename(&tmp, out).is_ok()
        }
        Job::Reverse { base, out, .. } => {
            let Ok(data) = std::fs::read(base) else { return false };
            let n = data.len() / 4;
            let mut rev = vec![0u8; n * 4];
            for i in 0..n {
                rev[i * 4..i * 4 + 4].copy_from_slice(&data[(n - 1 - i) * 4..(n - i) * 4]);
            }
            let tmp = out.with_extension("tmp");
            std::fs::write(&tmp, rev).is_ok() && std::fs::rename(&tmp, out).is_ok()
        }
    }
}

impl AudioLib {
    pub fn new() -> Self {
        let inner = Arc::new(Mutex::new(Inner { ready: HashMap::new(), pending: HashSet::new(), failed: HashSet::new() }));
        let (tx, rx) = channel::<Job>();
        let inner2 = inner.clone();
        std::thread::Builder::new()
            .name("velomov-audio".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    let (key, out) = match &job {
                        Job::Decode { key, out, .. } | Job::Derive { key, out, .. } | Job::Reverse { key, out, .. } => {
                            (key.clone(), out.clone())
                        }
                    };
                    let cached = out.exists();
                    if cached {
                        crate::util::touch(&out);
                    }
                    let ok = cached || run_job(&job);
                    let pcm = if ok { Pcm::open(&out) } else { None };
                    let mut g = inner2.lock().unwrap();
                    g.pending.remove(&key);
                    match pcm {
                        Some(p) => {
                            g.ready.insert(key, Arc::new(p));
                        }
                        None => {
                            g.failed.insert(key);
                        }
                    }
                }
            })
            .expect("audio worker");
        AudioLib { inner, tx }
    }

    fn lookup_or(&self, key: &str, make: impl FnOnce() -> Option<Job>) -> Option<Arc<Pcm>> {
        let mut g = self.inner.lock().unwrap();
        if let Some(p) = g.ready.get(key) {
            return Some(p.clone());
        }
        if g.pending.contains(key) || g.failed.contains(key) {
            return None;
        }
        if let Some(job) = make() {
            g.pending.insert(key.to_string());
            drop(g);
            let _ = self.tx.send(job);
        }
        None
    }

    /// Base PCM for a media file (decodes it in the background on first use).
    pub fn base(&self, path: &str) -> Option<Arc<Pcm>> {
        let key = base_key(path);
        let out = dir().join(format!("{}.pcm", key));
        let src = path.to_string();
        let k2 = key.clone();
        self.lookup_or(&key, move || Some(Job::Decode { key: k2, src, out }))
    }

    /// Variant with tempo change (pitch preserved), reversal, noise reduction (0..1)
    /// and/or a voice effect.
    pub fn variant(&self, path: &str, tempo: f64, reverse: bool, voice: Option<&str>, denoise: f64) -> Option<Arc<Pcm>> {
        let tempo_on = (tempo - 1.0).abs() > 1e-3;
        let denoise_on = denoise > 0.005;
        if !tempo_on && !reverse && voice.is_none() && !denoise_on {
            return self.base(path);
        }
        let bkey = base_key(path);
        // reversal first (pure Rust), filters on top
        let src_key = if reverse { format!("{}-rev", bkey) } else { bkey.clone() };
        if reverse {
            let base = self.base(path)?;
            let _ = base;
            let out = dir().join(format!("{}.pcm", src_key));
            let k2 = src_key.clone();
            let bpath = dir().join(format!("{}.pcm", bkey));
            let r = self.lookup_or(&src_key, move || Some(Job::Reverse { key: k2, base: bpath, out }));
            if !tempo_on && voice.is_none() && !denoise_on {
                return r;
            }
            r?;
        } else {
            self.base(path)?;
        }
        let mut filters: Vec<String> = vec![];
        if denoise_on {
            filters.push(denoise_filter(denoise));
        }
        if tempo_on {
            let mut s = tempo;
            while s < 0.5 {
                filters.push("atempo=0.5".into());
                s /= 0.5;
            }
            while s > 100.0 {
                filters.push("atempo=100".into());
                s /= 100.0;
            }
            filters.push(format!("atempo={:.5}", s));
        }
        if let Some(v) = voice.and_then(catalog::voice) {
            filters.push(v.filter.to_string());
        }
        let filter = filters.join(",");
        let key = format!("{}-{:016x}", src_key, util::hash_str(&filter));
        let out = dir().join(format!("{}.pcm", key));
        let base = dir().join(format!("{}.pcm", src_key));
        let k2 = key.clone();
        self.lookup_or(&key, move || Some(Job::Derive { key: k2, base, filter, out }))
    }

    pub fn busy(&self) -> bool {
        !self.inner.lock().unwrap().pending.is_empty()
    }

    pub fn failed(&self, path: &str) -> bool {
        self.inner.lock().unwrap().failed.contains(&base_key(path))
    }
}

impl Default for AudioLib {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------- mixer

pub struct MixClip {
    pub pcm: Arc<Pcm>,
    /// timeline start (output frames)
    pub start: i64,
    pub len: i64,
    /// pcm position (frames) at clip start
    pub src0: f64,
    /// pcm frames per output frame
    pub step: f64,
    pub gain: f32,
    pub fade_in: i64,
    pub fade_out: i64,
    /// (local frame, gain) keyframes
    pub vol_keys: Vec<(i64, f32)>,
    /// speed curve support: (curve, clip len) -> integral mapping
    pub curve: Option<(SpeedCurve, f64)>,
}

#[derive(Default)]
pub struct MixPlan {
    pub clips: Vec<MixClip>,
}

fn us_to_frames(t: Us) -> i64 {
    (t as f64 * RATE as f64 / 1e6).round() as i64
}

/// Build the mix plan for a project. Returns (plan, complete) where complete=false
/// means some audio is still being decoded.
pub fn build_plan(p: &Project, lib: &AudioLib) -> (MixPlan, bool) {
    let mut plan = MixPlan::default();
    let mut complete = true;
    // Transitions crossfade the audio of the two clips over the transition region:
    // the outgoing clip continues for half a transition, the incoming one starts half early.
    let mut head: HashMap<u64, Us> = HashMap::new();
    let mut tail: HashMap<u64, Us> = HashMap::new();
    let mi = p.main_track_index();
    for (i, from, to) in p.transition_regions() {
        let half = (to - from) / 2;
        tail.insert(p.tracks[mi].clips[i].id, half);
        head.insert(p.tracks[mi].clips[i + 1].id, half);
    }
    for tr in &p.tracks {
        if tr.muted || !(tr.kind == TrackKind::Audio || tr.kind == TrackKind::Video) {
            continue;
        }
        for c in &tr.clips {
            if c.disabled || c.freeze.is_some() {
                continue;
            }
            let Some(mid) = c.media_id() else { continue };
            let Some(m) = p.media(mid) else { continue };
            if !m.has_audio {
                continue;
            }
            if tr.kind == TrackKind::Video && c.audio_detached {
                continue;
            }
            if c.volume <= 0.0 && !c.kf.has(Prop::Volume) {
                continue;
            }
            let speed = c.speed.max(0.01);
            let curve = c.curve.clone();
            let tempo = if c.change_pitch || curve.is_some() { 1.0 } else { speed };
            let voice = c.voice.as_ref().map(|v| v.id.as_str()).filter(|v| !v.is_empty());
            let pcm = match lib.variant(&m.path, tempo, c.reverse, voice, c.denoise) {
                Some(p) => p,
                None => {
                    if !lib.failed(&m.path) {
                        complete = false;
                    }
                    continue;
                }
            };
            let dur = m.duration;
            let src_len = c.src_len();
            let (src0_us, step) = if c.reverse {
                let rev_start = (dur - c.src_in - src_len).max(0) as f64;
                if tempo != 1.0 {
                    (rev_start / tempo, 1.0)
                } else {
                    (rev_start, speed)
                }
            } else if tempo != 1.0 {
                (c.src_in as f64 / tempo, 1.0)
            } else if curve.is_some() {
                (c.src_in as f64, 1.0)
            } else {
                (c.src_in as f64, speed)
            };
            let (h, tl) =
                if curve.is_some() { (0, 0) } else { (head.get(&c.id).copied().unwrap_or(0), tail.get(&c.id).copied().unwrap_or(0)) };
            let hf = us_to_frames(h);
            let vol_keys =
                c.kf.track(Prop::Volume).map(|t| t.keys.iter().map(|k| (us_to_frames(k.t) + hf, k.v as f32)).collect()).unwrap_or_default();
            plan.clips.push(MixClip {
                pcm,
                start: us_to_frames(c.start) - hf,
                len: us_to_frames(c.duration + h + tl),
                // before the in-point the PCM position is negative = silence when there is no handle
                src0: src0_us * RATE as f64 / 1e6 - hf as f64 * step,
                step,
                gain: c.volume as f32,
                fade_in: if h > 0 { us_to_frames((c.fade_in + h).max(2 * h)) } else { us_to_frames(c.fade_in) },
                fade_out: if tl > 0 { us_to_frames((c.fade_out + tl).max(2 * tl)) } else { us_to_frames(c.fade_out) },
                vol_keys,
                curve: curve.map(|cv| (cv, us_to_frames(c.duration) as f64)),
            });
        }
    }
    (plan, complete)
}

fn key_gain(keys: &[(i64, f32)], t: i64, base: f32) -> f32 {
    if keys.is_empty() {
        return base;
    }
    if t <= keys[0].0 {
        return keys[0].1;
    }
    let last = keys[keys.len() - 1];
    if t >= last.0 {
        return last.1;
    }
    for w in keys.windows(2) {
        if t >= w[0].0 && t < w[1].0 {
            let f = (t - w[0].0) as f32 / (w[1].0 - w[0].0).max(1) as f32;
            return w[0].1 + (w[1].1 - w[0].1) * f;
        }
    }
    base
}

/// Mix `out.len()/2` stereo frames starting at timeline frame `pos`.
pub fn mix(plan: &MixPlan, pos: i64, out: &mut [f32]) {
    for v in out.iter_mut() {
        *v = 0.0;
    }
    let n = (out.len() / 2) as i64;
    for c in &plan.clips {
        let a = pos.max(c.start);
        let b = (pos + n).min(c.start + c.len);
        if a >= b {
            continue;
        }
        for t in a..b {
            let local = t - c.start;
            let src = match &c.curve {
                Some((cv, len)) => c.src0 + curve_integral(cv, local as f64 / len.max(1.0)) * len,
                None => c.src0 + local as f64 * c.step,
            };
            let (l, r) = c.pcm.interp(src);
            let mut g = key_gain(&c.vol_keys, local, c.gain);
            if c.fade_in > 0 && local < c.fade_in {
                g *= local as f32 / c.fade_in as f32;
            }
            if c.fade_out > 0 && local > c.len - c.fade_out {
                g *= ((c.len - local) as f32 / c.fade_out as f32).max(0.0);
            }
            let i = ((t - pos) * 2) as usize;
            out[i] += l * g;
            out[i + 1] += r * g;
        }
    }
    for v in out.iter_mut() {
        // soft knee limiter
        let x = *v;
        let ax = x.abs();
        *v = if ax <= 0.9 { x } else { x.signum() * (0.9 + 0.1 * ((ax - 0.9) / 0.1).tanh()) };
    }
}

/// Render the whole timeline mix to a WAV file (exporter).
pub fn render_wav(plan: &MixPlan, from: Us, to: Us, path: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;
    let start = us_to_frames(from);
    let frames = (us_to_frames(to) - start).max(0) as u32;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let data_len = frames * 4;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&RATE.to_le_bytes())?;
    f.write_all(&(RATE * 4).to_le_bytes())?;
    f.write_all(&4u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    let chunk = 4800i64;
    let mut buf = vec![0f32; chunk as usize * 2];
    let mut pos = start;
    let end = start + frames as i64;
    while pos < end {
        let n = chunk.min(end - pos) as usize;
        mix(plan, pos, &mut buf[..n * 2]);
        let mut bytes = Vec::with_capacity(n * 4);
        for v in &buf[..n * 2] {
            bytes.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        f.write_all(&bytes)?;
        pos += n as i64;
    }
    f.flush()
}
