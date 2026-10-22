//! Media access: probing, decoding (FFmpeg subprocesses), images, audio caches.

pub mod audio;
pub mod proxy;
pub mod sfx;
pub mod thumbs;
pub mod transcribe;
pub mod video;

use crate::engine::{Frame, FrameProvider, PixFmt, VideoReq};
use crate::model::{MediaAsset, MediaKind};
use crate::util::{self, s_to_us};
use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

fn parse_rate(s: &str) -> Option<f64> {
    let mut it = s.split('/');
    let n: f64 = it.next()?.trim().parse().ok()?;
    let d: f64 = it.next().map(|d| d.trim().parse().unwrap_or(1.0)).unwrap_or(1.0);
    if d == 0.0 || n == 0.0 {
        return None;
    }
    Some(n / d)
}

fn jnum(v: &serde_json::Value) -> Option<f64> {
    match v {
        serde_json::Value::Number(n) => n.as_f64(),
        serde_json::Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Animated WebP (extended "VP8X" header with the animation flag)?
fn is_animated_webp(path: &str) -> bool {
    use std::io::Read;
    let mut b = [0u8; 21];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut b)).is_ok()
        && &b[0..4] == b"RIFF"
        && &b[8..12] == b"WEBP"
        && &b[12..16] == b"VP8X"
        && b[20] & 0x02 != 0
}

/// Animations (GIF, WebP) are decoded with their transparency.
pub fn has_alpha_video(path: &str) -> bool {
    matches!(util::ext_lower(path).as_str(), "gif" | "webp")
}

/// Number of frames of the first video stream (counted by decoding: small files only).
fn count_frames(path: &str) -> Option<u64> {
    let out = crate::util::command("ffprobe")
        .args(["-v", "error", "-count_frames", "-select_streams", "v:0", "-show_entries", "stream=nb_read_frames", "-of", "csv=p=0", path])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Probe a media file (ffprobe for audio/video, the image crate for stills).
pub fn probe(path: &str) -> Result<MediaAsset> {
    let ext = util::ext_lower(path);
    let name = util::file_name(path);
    if util::IMAGE_EXTS.contains(&ext.as_str()) && !(ext == "webp" && is_animated_webp(path)) {
        let (w, h) = image_dims(path)?;
        return Ok(MediaAsset {
            id: util::new_id(),
            path: path.to_string(),
            name,
            kind: MediaKind::Image,
            duration: 0,
            width: w,
            height: h,
            fps: 30.0,
            has_video: true,
            has_audio: false,
            ..Default::default()
        });
    }
    let out = crate::util::command("ffprobe")
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams", path])
        .output()
        .context("ffprobe not found - install ffmpeg")?;
    if !out.status.success() {
        return Err(anyhow!("ffprobe failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout)?;
    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    let mut m = MediaAsset { id: util::new_id(), path: path.to_string(), name, ..Default::default() };
    let mut vdur = None;
    let mut adur = None;
    let mut frames: Option<u64> = None;
    for s in &streams {
        let ct = s["codec_type"].as_str().unwrap_or("");
        if ct == "video" && s["disposition"]["attached_pic"].as_i64().unwrap_or(0) == 0 && !m.has_video {
            m.has_video = true;
            m.width = s["width"].as_u64().unwrap_or(0) as u32;
            m.height = s["height"].as_u64().unwrap_or(0) as u32;
            let fr = s["avg_frame_rate"]
                .as_str()
                .and_then(parse_rate)
                .or_else(|| s["r_frame_rate"].as_str().and_then(parse_rate))
                .unwrap_or(30.0);
            m.fps = if (1.0..=240.0).contains(&fr) { fr } else { 30.0 };
            // clockwise turn ffmpeg applies: the old "rotate" tag is clockwise,
            // display-matrix angles are counter-clockwise
            let mut cw = 0.0;
            if let Some(r) = s["tags"]["rotate"].as_str().and_then(|r| r.parse::<f64>().ok()) {
                cw = r;
            }
            if let Some(list) = s["side_data_list"].as_array() {
                for sd in list {
                    if let Some(r) = jnum(&sd["rotation"]) {
                        cw = -r;
                    }
                }
            }
            let r = (cw.round() as i64).rem_euclid(360);
            if r == 90 || r == 270 {
                std::mem::swap(&mut m.width, &mut m.height);
            }
            m.rotation = r as i32;
            m.codec = s["codec_name"].as_str().unwrap_or("").to_string();
            vdur = jnum(&s["duration"]);
            frames = jnum(&s["nb_frames"]).map(|f| f as u64);
            if m.codec == "webp_anim" {
                // the WebP demuxer knows neither duration nor frame count: count them
                frames = count_frames(path);
                if let Some(n) = frames {
                    vdur = Some(n as f64 / m.fps.max(1.0));
                }
            }
        }
        if ct == "audio" && !m.has_audio {
            m.has_audio = true;
            adur = jnum(&s["duration"]);
        }
    }
    let fdur = jnum(&v["format"]["duration"]);
    let dur = vdur.or(fdur).or(adur).unwrap_or(0.0);
    m.duration = s_to_us(dur);
    if m.has_video {
        // single-frame things (a GIF/WebP with one frame, a cover picture) behave like images
        m.kind =
            if dur < 0.05 || (frames == Some(1) && matches!(ext.as_str(), "gif" | "webp")) { MediaKind::Image } else { MediaKind::Video };
    } else if m.has_audio {
        m.kind = MediaKind::Audio;
        m.duration = s_to_us(adur.or(fdur).unwrap_or(0.0));
    } else {
        return Err(anyhow!("no audio or video streams"));
    }
    if m.kind == MediaKind::Image {
        m.duration = 0;
        m.has_audio = false;
    }
    if m.kind == MediaKind::Video && (m.width == 0 || m.height == 0) {
        return Err(anyhow!("unsupported video stream"));
    }
    Ok(m)
}

fn image_dims(path: &str) -> Result<(u32, u32)> {
    let img = load_image_rgba(path, 1 << 14)?;
    Ok((img.width(), img.height()))
}

/// Decode an image applying EXIF orientation, limited to `max_dim`.
pub fn load_image_rgba(path: &str, max_dim: u32) -> Result<image::RgbaImage> {
    use image::ImageDecoder;
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder)?;
    img.apply_orientation(orientation);
    if img.width() > max_dim || img.height() > max_dim {
        img = img.resize(max_dim, max_dim, image::imageops::FilterType::Triangle);
    }
    Ok(img.to_rgba8())
}

pub(crate) fn premultiply(data: &mut [u8]) {
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u32;
        if a < 255 {
            px[0] = (px[0] as u32 * a / 255) as u8;
            px[1] = (px[1] as u32 * a / 255) as u8;
            px[2] = (px[2] as u32 * a / 255) as u8;
        }
    }
}

/// Background-loaded still images.
pub struct ImageCache {
    /// Finished loads (`None` = the file could not be decoded).
    map: HashMap<String, Option<Arc<Frame>>>,
    inflight: std::collections::HashSet<String>,
    tx: Sender<(String, Option<Arc<Frame>>)>,
    rx: Receiver<(String, Option<Arc<Frame>>)>,
    pub blocking: bool,
}

impl Default for ImageCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageCache {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        ImageCache { map: HashMap::new(), inflight: Default::default(), tx, rx, blocking: false }
    }

    fn load(path: &str) -> Option<Arc<Frame>> {
        match load_image_rgba(path, 4096) {
            Ok(img) => {
                let (w, h) = (img.width(), img.height());
                let mut data = img.into_raw();
                premultiply(&mut data);
                Some(Arc::new(Frame { fmt: PixFmt::Rgba, w, h, data, uid: util::new_id() }))
            }
            Err(e) => {
                crate::vlog!("image {}: {}", path, e);
                None
            }
        }
    }

    pub fn get(&mut self, path: &str) -> Option<Arc<Frame>> {
        while let Ok((p, f)) = self.rx.try_recv() {
            self.inflight.remove(&p);
            self.map.insert(p, f);
        }
        if let Some(v) = self.map.get(path) {
            return Some(v.clone().unwrap_or_else(video::offline_frame));
        }
        if self.blocking {
            let f = Self::load(path);
            self.map.insert(path.to_string(), f.clone());
            return Some(f.unwrap_or_else(video::offline_frame));
        }
        if self.inflight.insert(path.to_string()) {
            let tx = self.tx.clone();
            let p = path.to_string();
            std::thread::spawn(move || {
                let f = Self::load(&p);
                let _ = tx.send((p, f));
            });
        }
        None
    }

    pub fn has_pending(&self) -> bool {
        !self.inflight.is_empty()
    }

    pub fn forget(&mut self, path: &str) {
        self.map.remove(path);
    }
}

/// FrameProvider used by the preview and the exporter.
pub struct Frames {
    pub video: video::VideoPool,
    pub images: ImageCache,
    pub blocking: bool,
    /// Decode the light proxies instead of the originals (preview at medium/low quality).
    pub use_proxies: bool,
    /// Ready proxies by original path.
    pub proxies: HashMap<String, proxy::ProxyInfo>,
}

impl Frames {
    pub fn new(blocking: bool) -> Self {
        let mut images = ImageCache::new();
        images.blocking = blocking;
        Frames { video: video::VideoPool::new(blocking), images, blocking, use_proxies: false, proxies: HashMap::new() }
    }
}

impl FrameProvider for Frames {
    fn begin_frame(&mut self) {
        self.video.begin_frame();
    }
    fn video(&mut self, req: &VideoReq) -> Option<(Arc<Frame>, bool)> {
        if self.blocking {
            return self.video.request_blocking(req, std::time::Duration::from_secs(20)).map(|f| (f, true));
        }
        if self.use_proxies {
            if let Some(p) = self.proxies.get(req.path) {
                // never ask the proxy for more pixels than it has
                let k = (p.w as f64 / req.w.max(1) as f64).min(p.h as f64 / req.h.max(1) as f64).min(1.0);
                let even = |v: f64| ((v / 2.0).round() as u32 * 2).max(2);
                let preq = VideoReq { path: &p.path, w: even(req.w as f64 * k), h: even(req.h as f64 * k), rot: 0, ..*req };
                return self.video.request(&preq).or_else(|| self.video.placeholder(req.path, req.time).map(|f| (f, false)));
            }
        }
        self.video.request(req)
    }
    fn image(&mut self, path: &str) -> Option<Arc<Frame>> {
        self.images.get(path)
    }
}
