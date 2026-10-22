//! Video decoding through FFmpeg child processes.
//!
//! Each decoder is a worker thread that keeps an `ffmpeg` process piping raw
//! frames on a fixed grid (`rate` frames per source second, starting at the
//! seek point). Small forward jumps are decoded through, backward/large jumps
//! restart the process with an input seek. When VA-API is available the
//! decode and the downscale run on the GPU (NV12 output); files the hardware
//! cannot handle fall back to software decoding automatically.

use crate::engine::{Frame, PixFmt, VideoReq};
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Read;
use std::process::{Child, ChildStdout, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const RENDER_NODE: &str = "/dev/dri/renderD128";

#[derive(Clone, PartialEq, Debug)]
struct Params {
    path: String,
    rate: f64,
    w: u32,
    h: u32,
    /// VA-API decode + scale on the GPU (NV12 output).
    hw: bool,
    /// Clockwise rotation the hardware path applies itself (software decoding autorotates).
    rot: i32,
    /// Cheaper software decode (low preview quality).
    fast: bool,
    /// Skip decoding frames nothing refers to (preview of files with at least 1.5x the
    /// frames we show: those frames would mostly be dropped anyway, decoding gets ~1.7x faster).
    skip: bool,
    /// Decode with transparency (animated GIF / WebP): premultiplied RGBA frames.
    alpha: bool,
}

struct State {
    params: Params,
    gen: u64,
    target: i64,
    playing: bool,
    frames: VecDeque<(i64, Arc<Frame>)>,
    eof: Option<i64>,
    alive: bool,
    failed: bool,
    /// After falling behind playback the decoder restarts a little ahead of the
    /// playhead; until the playhead gets there the gap is expected, not stale.
    lead_start: Option<i64>,
}

struct Shared {
    m: Mutex<State>,
    cv: Condvar,
}

struct Stream {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    last: Instant,
}

impl Drop for Stream {
    fn drop(&mut self) {
        {
            let mut st = self.shared.m.lock().unwrap();
            st.alive = false;
        }
        self.shared.cv.notify_all();
        // Detach: the worker exits (and kills its ffmpeg) on its own, so the UI never waits on a decode.
        drop(self.thread.take());
    }
}

/// Shown in place of media that cannot be decoded (missing or broken file).
pub fn offline_frame() -> Arc<Frame> {
    static F: OnceLock<Arc<Frame>> = OnceLock::new();
    F.get_or_init(|| {
        let (w, h) = (64u32, 36u32);
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                // dark red with diagonal stripes
                let stripe = ((x + y) / 4) % 2 == 0;
                let c = if stripe { [92u8, 22, 30, 255] } else { [64u8, 16, 22, 255] };
                data.extend_from_slice(&c);
            }
        }
        Arc::new(Frame { fmt: PixFmt::Rgba, w, h, data, uid: crate::util::new_id() })
    })
    .clone()
}

// ---------------------------------------------------------------- hardware decoding

static HW_WANTED: AtomicBool = AtomicBool::new(true);

fn hw_bad() -> &'static Mutex<HashSet<String>> {
    static B: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(HashSet::new()))
}

/// VA-API is usable: a render node exists and ffmpeg was built with it (checked once).
pub fn hw_available() -> bool {
    static A: OnceLock<bool> = OnceLock::new();
    *A.get_or_init(|| {
        if cfg!(windows) || !std::path::Path::new(RENDER_NODE).exists() {
            return false;
        }
        crate::util::command("ffmpeg")
            .args(["-hide_banner", "-hwaccels"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().any(|l| l.trim() == "vaapi"))
            .unwrap_or(false)
    })
}

pub fn set_hwaccel(on: bool) {
    HW_WANTED.store(on, Ordering::Relaxed);
}

pub fn hwaccel_active() -> bool {
    HW_WANTED.load(Ordering::Relaxed) && hw_available()
}

fn hw_for(path: &str) -> bool {
    hwaccel_active() && !hw_bad().lock().unwrap().contains(path)
}

/// ffmpeg arguments for a VA-API decode of `path` (shared with the proxy generator).
pub fn hw_input_args(rot: i32) -> Vec<String> {
    let mut v: Vec<String> =
        ["-hwaccel", "vaapi", "-hwaccel_device", RENDER_NODE, "-hwaccel_output_format", "vaapi"].iter().map(|s| s.to_string()).collect();
    if rot != 0 {
        v.push("-noautorotate".into());
    }
    v
}

/// VA-API filter that rotates like ffmpeg's autorotate would.
pub fn hw_transpose(rot: i32) -> &'static str {
    match rot {
        90 => "transpose_vaapi=dir=clock,",
        180 => "transpose_vaapi=dir=reversal,",
        270 => "transpose_vaapi=dir=cclock,",
        _ => "",
    }
}

fn frame_index(time: f64, rate: f64) -> i64 {
    (time * rate + 1e-4).floor().max(0.0) as i64
}

struct Proc {
    child: Child,
    out: ChildStdout,
    next: i64,
    start: i64,
    gen: u64,
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn(p: &Params, start: i64) -> Option<Child> {
    let t = start as f64 / p.rate;
    let mut cmd = crate::util::command("ffmpeg");
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error"]);
    let vf = if p.alpha {
        format!("fps={:.6},scale={}:{}:flags=bilinear,format=rgba", p.rate, p.w, p.h)
    } else if p.hw {
        cmd.args(hw_input_args(p.rot));
        format!("fps={:.6},{}scale_vaapi=w={}:h={}:format=nv12,hwdownload,format=nv12", p.rate, hw_transpose(p.rot), p.w, p.h)
    } else {
        if p.fast {
            cmd.args(["-skip_loop_filter", "all", "-flags2", "+fast"]);
        }
        // Windows: let ffmpeg decode on the GPU (D3D11VA/DXVA2/…) and hand back normal frames;
        // it falls back to the processor by itself when a file can't be decoded that way
        if cfg!(windows) && HW_WANTED.load(Ordering::Relaxed) {
            cmd.args(["-hwaccel", "auto"]);
        }
        let flags = if p.fast { "fast_bilinear" } else { "bilinear" };
        format!("fps={:.6},scale={}:{}:flags={}:out_color_matrix=bt709:out_range=tv,format=yuv420p", p.rate, p.w, p.h, flags)
    };
    if p.skip {
        cmd.args(["-skip_frame", "noref"]);
    }
    // animated GIF/WebP: their demuxers cannot seek, so decode from the start and drop
    // frames up to `t` (they are small)
    if t > 0.0 && !p.alpha {
        cmd.args(["-ss", &format!("{:.6}", t)]);
    }
    let pix = if p.alpha {
        "rgba"
    } else if p.hw {
        "nv12"
    } else {
        "yuv420p"
    };
    cmd.args(["-i", &p.path]);
    if t > 0.0 && p.alpha {
        cmd.args(["-ss", &format!("{:.6}", t)]);
    }
    cmd.args(["-map", "0:v:0", "-an", "-sn", "-dn", "-vf", &vf, "-f", "rawvideo", "-pix_fmt", pix, "pipe:1"]);
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    match cmd.spawn() {
        Ok(c) => Some(c),
        Err(e) => {
            crate::vlog!("ffmpeg spawn failed: {}", e);
            None
        }
    }
}

/// VELOMOV_DEBUG_DECODE=1: log decoder restarts.
fn debug_decode() -> bool {
    static D: OnceLock<bool> = OnceLock::new();
    *D.get_or_init(|| std::env::var_os("VELOMOV_DEBUG_DECODE").is_some())
}

enum Act {
    /// keep reading the running process; frames before `need` are decoded through and dropped
    Read {
        need: i64,
    },
    Restart {
        at: i64,
    },
}

fn worker(shared: Arc<Shared>, prefetch: i64, keep: usize) {
    let mut proc: Option<Proc> = None;
    let mut fails = 0;
    // newest decoded frame, even one dropped for being late: it is the file's last frame
    // when the stream ends, and holding a clip's end must not cost a new seek
    let mut newest: Option<(i64, Arc<Frame>)> = None;
    loop {
        let (params, gen, act) = {
            let mut st = shared.m.lock().unwrap();
            loop {
                if !st.alive {
                    return;
                }
                let t = match st.eof {
                    Some(e) if st.target >= e && e > 0 => e - 1,
                    _ => st.target,
                };
                // drop frames we won't need any more
                while st.frames.len() > 1 && st.frames.front().map(|f| f.0 < t - 2).unwrap_or(false) {
                    st.frames.pop_front();
                }
                let rate = st.params.rate;
                let lead_gap = st.lead_start.map(|s| t < s && t >= s - (rate * 3.0) as i64).unwrap_or(false);
                if !lead_gap {
                    st.lead_start = None;
                }
                let front = st.frames.front().map(|f| f.0);
                let back = st.frames.back().map(|f| f.0);
                let covered = matches!((front, back), (Some(a), Some(b)) if t >= a && t <= b);
                if !covered && !lead_gap && !st.frames.is_empty() {
                    // the target left the buffer: it is stale
                    st.frames.clear();
                }
                let have_until = if covered {
                    back.unwrap()
                } else if lead_gap {
                    back.unwrap_or(st.lead_start.unwrap() - 1)
                } else {
                    t - 1
                };
                let at_eof = st.eof.map(|e| have_until + 1 >= e).unwrap_or(false);
                let ahead = if st.playing { prefetch * 2 } else { prefetch };
                if st.failed || at_eof || have_until >= t + ahead {
                    let (s, _) = shared.cv.wait_timeout(st, Duration::from_millis(250)).unwrap();
                    st = s;
                    continue;
                }
                let need = have_until + 1;
                let act = match &proc {
                    Some(p) if p.gen == st.gen && need >= p.next && need - p.next <= (rate * 1.5).max(4.0) as i64 => Act::Read { need },
                    Some(p) if p.gen == st.gen && st.playing && need > p.next => {
                        // fell behind playback: restart a little ahead and let the playhead catch up
                        let at = need + (rate * 0.5).round() as i64;
                        st.lead_start = Some(at);
                        st.frames.clear();
                        Act::Restart { at }
                    }
                    _ => Act::Restart { at: need },
                };
                break (st.params.clone(), st.gen, act);
            }
        };
        let need = match act {
            Act::Read { need } => need,
            Act::Restart { at } => {
                if debug_decode() {
                    let next = proc.as_ref().map(|p| p.next);
                    crate::vlog!(
                        "decode {:?} restart {} at {} (decoder was at {:?}) {}x{} hw={}",
                        std::thread::current().id(),
                        crate::util::file_name(&params.path),
                        at,
                        next,
                        params.w,
                        params.h,
                        params.hw
                    );
                }
                proc = None;
                newest = None;
                match spawn(&params, at) {
                    Some(mut child) => {
                        let out = child.stdout.take().unwrap();
                        proc = Some(Proc { child, out, next: at, start: at, gen });
                    }
                    None => {
                        let mut st = shared.m.lock().unwrap();
                        st.failed = true;
                        shared.cv.notify_all();
                        continue;
                    }
                }
                at
            }
        };
        let p = proc.as_mut().unwrap();
        let fsize = if params.alpha {
            (params.w * params.h * 4) as usize
        } else {
            (params.w * params.h + 2 * ((params.w + 1) / 2) * ((params.h + 1) / 2)) as usize
        };
        let mut buf = vec![0u8; fsize];
        match p.out.read_exact(&mut buf) {
            Ok(()) => {
                fails = 0;
                let idx = p.next;
                p.next += 1;
                let fmt = if params.alpha {
                    super::premultiply(&mut buf);
                    PixFmt::Rgba
                } else if params.hw {
                    PixFmt::Nv12
                } else {
                    PixFmt::Yuv420
                };
                let f = Arc::new(Frame { fmt, w: params.w, h: params.h, data: buf, uid: crate::util::new_id() });
                let mut st = shared.m.lock().unwrap();
                if st.gen == gen && idx >= need {
                    if !st.frames.back().map(|b| b.0 + 1 == idx).unwrap_or(true) {
                        st.frames.clear();
                    }
                    st.frames.push_back((idx, f.clone()));
                    while st.frames.len() > keep {
                        st.frames.pop_front();
                    }
                }
                newest = Some((idx, f));
                shared.cv.notify_all();
            }
            Err(_) => {
                let (next, start) = (p.next, p.start);
                if debug_decode() {
                    crate::vlog!("decode {:?} end of stream at {} (started at {})", std::thread::current().id(), next, start);
                }
                proc = None;
                let mut st = shared.m.lock().unwrap();
                if st.gen == gen {
                    if next == start && params.hw {
                        // the hardware could not decode this file: use software from now on
                        crate::vlog!("VA-API cannot decode {}, using software", params.path);
                        hw_bad().lock().unwrap().insert(params.path.clone());
                        st.params.hw = false;
                        st.params.rot = 0;
                        st.gen += 1;
                    } else if next == start {
                        // produced nothing at all from this seek point
                        fails += 1;
                        st.eof = Some(next.min(st.eof.unwrap_or(i64::MAX)));
                        if next == 0 || fails > 6 {
                            st.failed = true;
                        }
                    } else {
                        st.eof = Some(next);
                        if let Some((i, f)) = newest.take() {
                            if i == next - 1 && !st.frames.iter().any(|x| x.0 == i) {
                                if !st.frames.back().map(|b| b.0 + 1 == i).unwrap_or(true) {
                                    st.frames.clear();
                                }
                                st.frames.push_back((i, f));
                            }
                        }
                    }
                }
                shared.cv.notify_all();
            }
        }
    }
}

// ---------------------------------------------------------------- pool

struct Entry {
    stream: Stream,
    /// Clip that used this decoder last (affinity).
    key: u64,
    /// Render frame in which it was last handed out.
    claimed: u64,
}

/// Pool of decoders. Clips prefer "their" decoder, but a decoder whose
/// position is just behind a new request (e.g. the next part of a split clip)
/// is handed over instead of starting a new ffmpeg, so cuts play seamlessly.
pub struct VideoPool {
    entries: Vec<Entry>,
    frame_no: u64,
    /// Last frame shown per file: placeholder while a new decoder seeks.
    last_by_path: HashMap<String, Arc<Frame>>,
    /// Thumbnails per file (time in seconds): last-resort placeholder, never a black frame.
    pub thumbs: HashMap<String, Vec<(f64, Arc<Frame>)>>,
    prefetch: i64,
    keep: usize,
    /// Set by the player: decoders keep more frames ahead and catch up by jumping ahead.
    pub playing: bool,
    /// Cheaper software decoding (low preview quality).
    pub fast: bool,
    /// Exports decode every frame exactly.
    export: bool,
}

impl VideoPool {
    pub fn new(export: bool) -> Self {
        VideoPool {
            entries: vec![],
            frame_no: 1,
            last_by_path: HashMap::new(),
            thumbs: HashMap::new(),
            prefetch: if export { 6 } else { 10 },
            keep: if export { 12 } else { 40 },
            playing: false,
            fast: false,
            export,
        }
    }

    /// Start of a new render: decoders handed out from now on belong to this frame.
    pub fn begin_frame(&mut self) {
        self.frame_no += 1;
    }

    fn spawn_stream(&self, params: Params) -> Stream {
        let shared = Arc::new(Shared {
            m: Mutex::new(State {
                params,
                gen: 1,
                target: 0,
                playing: false,
                frames: VecDeque::new(),
                eof: None,
                alive: true,
                failed: false,
                lead_start: None,
            }),
            cv: Condvar::new(),
        });
        let sh = shared.clone();
        let (pf, keep) = (self.prefetch, self.keep);
        let thread = std::thread::Builder::new().name("velomov-decode".into()).spawn(move || worker(sh, pf, keep)).ok();
        Stream { shared, thread, last: Instant::now() }
    }

    fn params_for(&self, req: &VideoReq) -> Params {
        let alpha = super::has_alpha_video(req.path);
        let hw = hw_for(req.path) && !alpha;
        Params {
            path: req.path.to_string(),
            rate: req.rate,
            w: req.w.max(2),
            h: req.h.max(2),
            hw,
            rot: if hw { req.rot } else { 0 },
            fast: self.fast && !hw,
            skip: !self.export && req.src_fps >= req.rate * 1.5 && !alpha,
            alpha,
        }
    }

    fn pick(&mut self, req: &VideoReq) -> usize {
        let params = self.params_for(req);
        let now = self.frame_no;
        let want = frame_index(req.time, req.rate);
        // 1. the decoder this clip used before (or already got in this frame)
        let mut idx = self.entries.iter().position(|e| e.key == req.key);
        // 2. a free decoder of the same file whose position is just behind (continuation)
        if idx.is_none() {
            let mut best: Option<(usize, i64)> = None;
            for (i, e) in self.entries.iter().enumerate() {
                if e.claimed == now {
                    continue;
                }
                let st = e.stream.shared.m.lock().unwrap();
                if st.params != params || st.failed {
                    continue;
                }
                let gap = want - st.target;
                if (0..=(req.rate * 1.5) as i64).contains(&gap) && best.map(|b| gap < b.1).unwrap_or(true) {
                    best = Some((i, gap));
                }
            }
            idx = best.map(|b| b.0);
        }
        // 3. any decoder of the same file idle for a few frames
        if idx.is_none() {
            idx = self.entries.iter().position(|e| e.claimed + 3 < now && e.stream.shared.m.lock().unwrap().params.path == params.path);
        }
        let i = match idx {
            Some(i) => i,
            None => {
                let stream = self.spawn_stream(params.clone());
                self.entries.push(Entry { stream, key: req.key, claimed: now });
                self.entries.len() - 1
            }
        };
        let playing = self.playing;
        let e = &mut self.entries[i];
        e.key = req.key;
        e.claimed = now;
        e.stream.last = Instant::now();
        {
            let mut st = e.stream.shared.m.lock().unwrap();
            if st.params != params {
                st.params = params;
                st.gen += 1;
                st.frames.clear();
                st.eof = None;
                st.failed = false;
                st.lead_start = None;
            }
            st.playing = playing;
        }
        i
    }

    /// Placeholder when a decoder has nothing yet: the file's last frame, else its nearest thumbnail.
    pub fn placeholder(&self, path: &str, time: f64) -> Option<Arc<Frame>> {
        if let Some(f) = self.last_by_path.get(path) {
            return Some(f.clone());
        }
        let list = self.thumbs.get(path)?;
        list.iter().min_by(|a, b| (a.0 - time).abs().total_cmp(&(b.0 - time).abs())).map(|t| t.1.clone())
    }

    /// Non-blocking: best available frame and whether it is exact.
    pub fn request(&mut self, req: &VideoReq) -> Option<(Arc<Frame>, bool)> {
        let i = self.pick(req);
        let shared = self.entries[i].stream.shared.clone();
        let n = frame_index(req.time, req.rate);
        let mut st = shared.m.lock().unwrap();
        let target = match st.eof {
            Some(e) if n >= e && e > 0 => e - 1,
            _ => n,
        };
        if st.target != target {
            st.target = target;
            shared.cv.notify_all();
        }
        if let Some((_, f)) = st.frames.iter().find(|f| f.0 == target) {
            let f = f.clone();
            drop(st);
            self.last_by_path.insert(req.path.to_string(), f.clone());
            return Some((f, true));
        }
        if st.failed {
            return Some(st.frames.back().map(|f| f.1.clone()).unwrap_or_else(offline_frame)).map(|f| (f, true));
        }
        // closest frame for display while seeking, else a placeholder for the file
        let best = st.frames.iter().min_by_key(|f| (f.0 - target).abs()).map(|f| f.1.clone());
        drop(st);
        best.or_else(|| self.placeholder(req.path, req.time)).map(|f| (f, false))
    }

    /// Blocking variant for export / still renders.
    pub fn request_blocking(&mut self, req: &VideoReq, timeout: Duration) -> Option<Arc<Frame>> {
        let i = self.pick(req);
        let shared = self.entries[i].stream.shared.clone();
        let n = frame_index(req.time, req.rate);
        let deadline = Instant::now() + timeout;
        let mut st = shared.m.lock().unwrap();
        loop {
            let target = match st.eof {
                Some(e) if n >= e && e > 0 => e - 1,
                _ => n,
            };
            if st.target != target {
                st.target = target;
                shared.cv.notify_all();
            }
            if let Some((_, f)) = st.frames.iter().find(|f| f.0 == target) {
                return Some(f.clone());
            }
            if st.failed {
                return Some(st.frames.back().map(|f| f.1.clone()).unwrap_or_else(offline_frame));
            }
            let now = Instant::now();
            if now >= deadline {
                return st.frames.back().map(|f| f.1.clone());
            }
            let (s, _) = shared.cv.wait_timeout(st, deadline - now).unwrap();
            st = s;
        }
    }

    /// Stop decoders that have not been used recently.
    pub fn gc(&mut self, idle: Duration) {
        let now = Instant::now();
        self.entries.retain(|e| now.duration_since(e.stream.last) < idle);
        if self.entries.is_empty() {
            self.last_by_path.clear();
        }
    }

    pub fn stop_all(&mut self) {
        self.entries.clear();
        self.last_by_path.clear();
    }

    pub fn active(&self) -> usize {
        self.entries.len()
    }
}
