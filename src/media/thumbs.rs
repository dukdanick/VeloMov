//! Timeline filmstrip / library thumbnails, extracted in the background.

use crate::model::MediaKind;
use std::collections::HashSet;
use std::io::Read;
use std::process::Stdio;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

pub const THUMB_H: u32 = 96;

#[derive(Clone)]
pub struct ThumbImg {
    pub w: u32,
    pub h: u32,
    /// straight RGBA
    pub rgba: Vec<u8>,
}

struct Job {
    media: u64,
    idx: u32,
    path: String,
    kind: MediaKind,
    time: f64,
    w: u32,
    h: u32,
}

pub struct ThumbDone {
    pub media: u64,
    pub idx: u32,
    /// source time of the thumbnail (seconds)
    pub time: f64,
    pub path: String,
    pub img: Option<ThumbImg>,
}

pub struct Thumbs {
    tx: Sender<Job>,
    rx: Receiver<ThumbDone>,
    requested: HashSet<(u64, u32)>,
    queue_len: Arc<Mutex<usize>>,
}

/// Thumbnails are cached on disk (keyed by file identity), so reopening a project is instant.
fn cache_path(job: &Job) -> std::path::PathBuf {
    let dir = crate::util::cache_dir().join("thumbs");
    let _ = std::fs::create_dir_all(&dir);
    let idx = if job.idx == u32::MAX { "cover".to_string() } else { format!("{}", job.idx) };
    dir.join(format!("{:016x}-{}-{:.3}-{}x{}.png", crate::util::file_key(&job.path), idx, job.time, job.w, job.h))
}

fn extract_cached(job: &Job) -> Option<ThumbImg> {
    let cp = cache_path(job);
    if let Ok(img) = image::open(&cp) {
        crate::util::touch(&cp);
        let img = img.to_rgba8();
        return Some(ThumbImg { w: img.width(), h: img.height(), rgba: img.into_raw() });
    }
    let t = extract(job)?;
    if let Some(img) = image::RgbaImage::from_raw(t.w, t.h, t.rgba.clone()) {
        let _ = img.save(&cp);
    }
    Some(t)
}

fn extract(job: &Job) -> Option<ThumbImg> {
    match job.kind {
        MediaKind::Image => {
            let img = super::load_image_rgba(&job.path, 2048).ok()?;
            let r = image::imageops::resize(&img, job.w, job.h, image::imageops::FilterType::Triangle);
            Some(ThumbImg { w: job.w, h: job.h, rgba: r.into_raw() })
        }
        MediaKind::Video => {
            let mut cmd = crate::util::command("ffmpeg");
            cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-noaccurate_seek"]);
            if job.time > 0.0 {
                cmd.args(["-ss", &format!("{:.3}", job.time)]);
            }
            cmd.args([
                "-i",
                &job.path,
                "-map",
                "0:v:0",
                "-frames:v",
                "1",
                "-vf",
                &format!("scale={}:{}:flags=bilinear", job.w, job.h),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "pipe:1",
            ]);
            cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
            let mut child = cmd.spawn().ok()?;
            let mut out = child.stdout.take()?;
            let mut buf = vec![0u8; (job.w * job.h * 4) as usize];
            let ok = out.read_exact(&mut buf).is_ok();
            let _ = child.kill();
            let _ = child.wait();
            if ok {
                Some(ThumbImg { w: job.w, h: job.h, rgba: buf })
            } else {
                None
            }
        }
        MediaKind::Audio => None,
    }
}

impl Thumbs {
    pub fn new(workers: usize) -> Self {
        let (tx, jrx) = channel::<Job>();
        let (dtx, rx) = channel::<ThumbDone>();
        let jrx = Arc::new(Mutex::new(jrx));
        let queue_len = Arc::new(Mutex::new(0usize));
        for i in 0..workers.max(1) {
            let jrx = jrx.clone();
            let dtx = dtx.clone();
            let ql = queue_len.clone();
            let _ = std::thread::Builder::new().name(format!("velomov-thumb{}", i)).spawn(move || loop {
                let job = {
                    let g = jrx.lock().unwrap();
                    match g.recv() {
                        Ok(j) => j,
                        Err(_) => return,
                    }
                };
                let img = extract_cached(&job);
                {
                    let mut q = ql.lock().unwrap();
                    *q = q.saturating_sub(1);
                }
                if dtx.send(ThumbDone { media: job.media, idx: job.idx, time: job.time, path: job.path.clone(), img }).is_err() {
                    return;
                }
            });
        }
        Thumbs { tx, rx, requested: HashSet::new(), queue_len }
    }

    /// Number of filmstrip thumbnails generated for a media of `dur` seconds.
    pub fn count_for(dur_s: f64) -> u32 {
        ((dur_s / 1.5).ceil() as u32).clamp(1, 80)
    }

    pub fn time_of(idx: u32, count: u32, dur_s: f64) -> f64 {
        ((idx as f64 + 0.5) / count as f64 * dur_s).max(0.0)
    }

    pub fn request(&mut self, media: u64, idx: u32, path: &str, kind: MediaKind, time: f64, aspect: f64) {
        if !self.requested.insert((media, idx)) {
            return;
        }
        let h = THUMB_H;
        let w = (((h as f64 * aspect) / 2.0).round() as u32 * 2).clamp(16, 400);
        *self.queue_len.lock().unwrap() += 1;
        let _ = self.tx.send(Job { media, idx, path: path.to_string(), kind, time, w, h });
    }

    pub fn poll(&mut self) -> Vec<ThumbDone> {
        let mut v = vec![];
        while let Ok(d) = self.rx.try_recv() {
            v.push(d);
        }
        v
    }

    pub fn busy(&self) -> bool {
        *self.queue_len.lock().unwrap() > 0
    }

    pub fn forget_media(&mut self, media: u64) {
        self.requested.retain(|(m, _)| *m != media);
    }
}
