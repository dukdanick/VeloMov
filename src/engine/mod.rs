//! The compositor: turns a project + time into pixels on the GPU.
//!
//! Layer order: the main track is the bottom layer, tracks
//! above it are drawn on top, effect/filter/adjustment clips process
//! everything below them. Movement (keyframes, animations, transitions) is
//! evaluated at three instants of a virtual shutter so the composite shader
//! can integrate real motion blur.

pub mod caption;
pub mod color;
pub mod gpu;
pub mod motion;
pub mod raster;

use crate::model::catalog::{self, AnimKind};
use crate::model::keyframe::Prop;
use crate::model::*;
use crate::util::{us_to_s, Us};
use eframe::wgpu;
use gpu::{Gpu, Pipe, Tex, U};
use motion::{TPlan, XD};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PixFmt {
    /// planar Y, U, V (BT.709 limited range)
    Yuv420,
    /// Y plane + interleaved UV (hardware decoders)
    Nv12,
    /// premultiplied RGBA
    Rgba,
}

/// A decoded picture. Yuv420 = planar Y, U, V (BT.709 limited range); Rgba = premultiplied.
pub struct Frame {
    pub fmt: PixFmt,
    pub w: u32,
    pub h: u32,
    pub data: Vec<u8>,
    pub uid: u64,
}

#[derive(Clone, Copy)]
pub struct VideoReq<'a> {
    /// Decoder stream key (clip id).
    pub key: u64,
    pub path: &'a str,
    /// Seconds in the file's own timeline.
    pub time: f64,
    /// Frame grid of the decode stream (frames per source second).
    pub rate: f64,
    pub w: u32,
    pub h: u32,
    /// Clockwise display rotation of the file (0/90/180/270).
    pub rot: i32,
    /// Frame rate of the file itself.
    pub src_fps: f64,
}

pub trait FrameProvider {
    /// Called once at the start of every rendered frame.
    fn begin_frame(&mut self) {}
    /// Returns a frame and whether it is the exact one requested.
    fn video(&mut self, req: &VideoReq) -> Option<(Arc<Frame>, bool)>;
    fn image(&mut self, path: &str) -> Option<Arc<Frame>>;
}

#[derive(Default, Debug, Clone, Copy)]
pub struct RenderInfo {
    /// Some source was not ready yet: render again soon.
    pub pending: bool,
}

struct Slot {
    rgba: Tex,
    fmt: PixFmt,
    planes: Vec<Tex>,
    uid: u64,
    used: u64,
}

struct Ctx<'a> {
    p: &'a Project,
    t: Us,
    w: u32,
    h: u32,
    export: bool,
    fd: f64,
}

impl<'a> Ctx<'a> {
    fn pw(&self) -> f64 {
        self.p.width as f64
    }
    fn ph(&self) -> f64 {
        self.p.height as f64
    }
    /// output px per project px
    fn rs(&self) -> f64 {
        self.h as f64 / self.p.height.max(1) as f64
    }
}

type M3 = [[f64; 3]; 3];

fn mm(a: &M3, b: &M3) -> M3 {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    r
}
fn mt(x: f64, y: f64) -> M3 {
    [[1.0, 0.0, x], [0.0, 1.0, y], [0.0, 0.0, 1.0]]
}
fn ms(x: f64, y: f64) -> M3 {
    [[x, 0.0, 0.0], [0.0, y, 0.0], [0.0, 0.0, 1.0]]
}
fn mr(a: f64) -> M3 {
    let (s, c) = a.sin_cos();
    [[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]
}
fn minv(m: &M3) -> M3 {
    let (a, b, c, d, e, f) = (m[0][0], m[0][1], m[0][2], m[1][0], m[1][1], m[1][2]);
    let det = a * e - b * d;
    let det = if det.abs() < 1e-12 { 1e-12_f64.copysign(det + 1e-30) } else { det };
    let ia = e / det;
    let ib = -b / det;
    let id = -d / det;
    let ie = a / det;
    [[ia, ib, -(ia * c + ib * f)], [id, ie, -(id * c + ie * f)], [0.0, 0.0, 1.0]]
}
fn apply(m: &M3, x: f64, y: f64) -> (f64, f64) {
    (m[0][0] * x + m[0][1] * y + m[0][2], m[1][0] * x + m[1][1] * y + m[1][2])
}

fn rgba_f(c: [u8; 4]) -> [f32; 4] {
    let a = c[3] as f32 / 255.0;
    [c[0] as f32 / 255.0 * a, c[1] as f32 / 255.0 * a, c[2] as f32 / 255.0 * a, a]
}

/// Combined In/Out/Combo animation at time t; second value = motion blur strength.
pub fn clip_anim(c: &Clip, t: Us) -> (XD, f64) {
    let local = t - c.start;
    let dur = c.duration.max(1);
    let mut xd = XD::ID;
    let mut blur: f64 = 0.0;
    if let Some(a) = &c.anim_in {
        let d = a.dur.min(dur).max(1);
        if local < d {
            let p = (local as f64 / d as f64).clamp(0.0, 1.0);
            xd = xd.then(motion::anim_xd(&a.id, AnimKind::In, p, c.id));
            blur = blur.max(catalog::animation(&a.id).map(|d| d.blur).unwrap_or(0.0));
        }
    }
    if let Some(a) = &c.anim_out {
        let d = a.dur.min(dur).max(1);
        if local > dur - d {
            let p = ((local - (dur - d)) as f64 / d as f64).clamp(0.0, 1.0);
            xd = xd.then(motion::anim_xd(&a.id, AnimKind::Out, p, c.id));
            blur = blur.max(catalog::animation(&a.id).map(|d| d.blur).unwrap_or(0.0));
        }
    }
    if let Some(a) = &c.anim_combo {
        let period = a.dur.max(100_000);
        let p = (local.max(0) % period) as f64 / period as f64;
        xd = xd.then(motion::anim_xd(&a.id, AnimKind::Combo, p, c.id));
        blur = blur.max(catalog::animation(&a.id).map(|d| d.blur).unwrap_or(0.0));
    }
    (xd, blur)
}

/// Number of characters visible for typewriter animations.
fn visible_chars(c: &Clip, td: &TextData, t: Us) -> Option<usize> {
    let n = td.text.chars().count();
    let local = t - c.start;
    if let Some(a) = &c.anim_in {
        if a.id == "typewriter" {
            let d = a.dur.min(c.duration).max(1);
            if local < d {
                return Some(((local as f64 / d as f64) * n as f64).floor() as usize);
            }
        }
    }
    if let Some(a) = &c.anim_out {
        if a.id == "typewriter_out" {
            let d = a.dur.min(c.duration).max(1);
            if local > c.duration - d {
                let p = (local - (c.duration - d)) as f64 / d as f64;
                return Some(n - ((p * n as f64).floor() as usize).min(n));
            }
        }
    }
    None
}

/// The picture of a text clip at `t`: animated captions or ordinary text.
fn text_image(raster: &mut raster::Raster, c: &Clip, td: &TextData, scale: f32, t: Us) -> Arc<raster::RasterImg> {
    if td.caption.is_empty() {
        return raster.text(td, scale, visible_chars(c, td, t));
    }
    // caption-local time: trims and splits move src_in along with the start
    let local = c.src_in + ((t - c.start) as f64 * c.speed) as Us;
    raster.caption(td, scale, local, (0, c.src_in + c.duration))
}

/// Decoder request for a video clip at timeline time `t` (shared by rendering and pre-roll).
fn video_req<'a>(ctx: &Ctx, c: &Clip, m: &'a MediaAsset, t: Us, natural: (f64, f64)) -> VideoReq<'a> {
    let src_t = c.source_time_ext(t, m.duration);
    let (path, ft) = match (&m.reversed, c.reverse) {
        (Some(rp), true) => (rp.as_str(), (m.duration - src_t).max(0)),
        _ => (m.path.as_str(), src_t),
    };
    let speed = c.speed_at(t).abs();
    let fps = ctx.p.fps.max(1.0);
    let rate = if c.freeze.is_some() { m.fps.max(1.0) } else { (fps / speed.max(0.05)).min(m.fps.max(fps)).max(1.0) };
    // Decode size: the smallest of full/half/quarter resolution that still covers the
    // largest on-screen size of the clip (keyframed zooms included), so it never changes mid-clip.
    let (mw, mh) = (m.width.max(1) as f64, m.height.max(1) as f64);
    let mut max_scale = c.tf.scale.abs();
    if let Some(tr) = c.kf.track(Prop::Scale) {
        for k in &tr.keys {
            max_scale = max_scale.max(k.v.abs());
        }
    }
    let disp = natural.0.max(natural.1) * max_scale.max(0.05) * ctx.rs();
    let full = mw.max(mh);
    let mut level = 1.0;
    for l in [0.25, 0.5] {
        if full * l >= disp * 0.95 && full * l >= 96.0 {
            level = l;
            break;
        }
    }
    let even = |v: f64| ((v / 2.0).round() as u32 * 2).max(2);
    VideoReq { key: c.id, path, time: us_to_s(ft), rate, w: even(mw * level), h: even(mh * level), rot: m.rotation, src_fps: m.fps }
}

fn natural_media_size(ctx: &Ctx, c: &Clip, m: &MediaAsset) -> (f64, f64) {
    let (mw, mh) = (m.width.max(1) as f64, m.height.max(1) as f64);
    let vw = mw * (1.0 - c.crop.left - c.crop.right).max(0.01);
    let vh = mh * (1.0 - c.crop.top - c.crop.bottom).max(0.01);
    let k = (ctx.pw() / vw).min(ctx.ph() / vh);
    (vw * k, vh * k)
}

pub struct Engine {
    pub gpu: Gpu,
    pub raster: raster::Raster,
    slots: HashMap<u64, Slot>,
    frame_no: u64,
    /// 0 = high, 1 = medium, 2 = low (preview only).
    pub preview_quality: u8,
    /// The preview is playing (paused frames get the smoothest motion blur).
    pub playing: bool,
}

pub struct Target<'a> {
    pub view: &'a wgpu::TextureView,
    pub w: u32,
    pub h: u32,
}

impl Engine {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Engine {
            gpu: Gpu::new(device, queue),
            raster: raster::Raster::new(),
            slots: HashMap::new(),
            frame_no: 0,
            preview_quality: 0,
            playing: false,
        }
    }

    pub fn headless() -> anyhow::Result<Self> {
        let (d, q) = gpu::headless_device()?;
        Ok(Self::new(d, q))
    }

    /// Render `project` at time `t` into `target`.
    pub fn render(&mut self, project: &Project, t: Us, frames: &mut dyn FrameProvider, target: &Target, export: bool) -> RenderInfo {
        self.frame_no += 1;
        frames.begin_frame();
        self.gpu.begin_frame();
        let mut enc = self.gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("velomov-frame") });
        let mut info = RenderInfo::default();
        let ctx = Ctx { p: project, t, w: target.w.max(2), h: target.h.max(2), export, fd: 1_000_000.0 / project.fps.max(1.0) };
        let bg = rgba_f(project.bg_color);
        let mut canvas = self.gpu.tex(ctx.w, ctx.h);
        self.gpu.clear(&mut enc, &canvas.view, bg);
        for ti in (0..project.tracks.len()).rev() {
            let tr = &project.tracks[ti];
            if tr.hidden || tr.kind == TrackKind::Audio {
                continue;
            }
            match tr.kind {
                TrackKind::Video | TrackKind::Text | TrackKind::Sticker => {
                    if tr.main {
                        canvas = self.draw_main(&mut enc, &ctx, tr, canvas, frames, &mut info);
                    } else {
                        for c in tr.clips.iter().filter(|c| c.contains(t) && !c.disabled) {
                            canvas = self.draw_clip(&mut enc, &ctx, c, canvas, frames, &mut info, [XD::ID; 3], 0.0);
                        }
                    }
                }
                TrackKind::Effect => {
                    for c in tr.clips.iter().filter(|c| c.contains(t) && !c.disabled) {
                        if let ClipKind::Effect(e) = &c.kind {
                            canvas = self.apply_effect(&mut enc, &ctx, c, e, canvas);
                        }
                    }
                }
                TrackKind::Filter => {
                    for c in tr.clips.iter().filter(|c| c.contains(t) && !c.disabled) {
                        if let ClipKind::Filter(f) = &c.kind {
                            if let Some(def) = catalog::filter(&f.id) {
                                let mut u = U::with_canvas(ctx.w, ctx.h);
                                u.gf = color::block(&color::GradeInput::from(&def.look));
                                u.gmix = [c.prop(Prop::EffectAmount, t) as f32, 0.0, 0.0, 0.0];
                                u.cfg[3] = 2.0;
                                u.time = [us_to_s(t - c.start) as f32, 0.0, (self.frame_no % 977) as f32, 0.0];
                                canvas = self.full_pass(&mut enc, Pipe::Grade, &canvas, None, &u, true);
                            }
                        }
                    }
                }
                TrackKind::Adjust => {
                    for c in tr.clips.iter().filter(|c| c.contains(t) && !c.disabled) {
                        let a = c.adjust_at(t);
                        let gi = color::GradeInput::from(&a);
                        if !color::is_identity(&gi) {
                            let mut u = U::with_canvas(ctx.w, ctx.h);
                            u.ga = color::block(&gi);
                            u.cfg[3] = if color::has_spatial(&gi) { 5.0 } else { 1.0 };
                            u.time = [0.0, 0.0, (self.frame_no % 977) as f32, 0.0];
                            canvas = self.full_pass(&mut enc, Pipe::Grade, &canvas, None, &u, true);
                        }
                    }
                }
                TrackKind::Audio => {}
            }
        }
        let mut u = U::with_canvas(ctx.w, ctx.h);
        u.p[0] = [0.0, 0.0, 1.0, 1.0];
        u.p[1] = [1.0, 0.0, 0.0, 0.0];
        self.gpu.pass(&mut enc, Pipe::Copy, target.view, [Some(&canvas.view), None, None], &u);
        self.gpu.queue.submit(Some(enc.finish()));
        self.gpu.release(canvas);
        self.gc();
        info
    }

    /// Render into a pooled texture and return it (caller must release it).
    pub fn render_tex(
        &mut self,
        project: &Project,
        t: Us,
        frames: &mut dyn FrameProvider,
        w: u32,
        h: u32,
        export: bool,
    ) -> (Tex, RenderInfo) {
        let tex = self.gpu.tex(w, h);
        let info = {
            let target = Target { view: &tex.view, w, h };
            self.render(project, t, frames, &target, export)
        };
        (tex, info)
    }

    /// Convenience: render to tightly packed RGBA bytes.
    pub fn render_rgba(&mut self, project: &Project, t: Us, frames: &mut dyn FrameProvider, w: u32, h: u32) -> Vec<u8> {
        let (tex, _) = self.render_tex(project, t, frames, w, h, true);
        let data = self.gpu.read_rgba(&tex);
        self.gpu.release(tex);
        data
    }

    /// Pre-roll: start decoders for video clips that become visible within `lookahead`,
    /// so playback does not stall at cuts and transitions. Call after `render` for the frame.
    pub fn prefetch(&mut self, project: &Project, t: Us, lookahead: Us, frames: &mut dyn FrameProvider, w: u32, h: u32) {
        let ctx = Ctx { p: project, t, w: w.max(2), h: h.max(2), export: false, fd: 1_000_000.0 / project.fps.max(1.0) };
        // a clip that follows a transition becomes visible half a transition before its start
        let mi = project.main_track_index();
        let mut early: HashMap<u64, Us> = HashMap::new();
        for (i, from, _) in project.transition_regions() {
            early.insert(project.tracks[mi].clips[i + 1].id, from);
        }
        let mut next = vec![];
        for tr in &project.tracks {
            if tr.hidden || tr.kind != TrackKind::Video {
                continue;
            }
            for c in &tr.clips {
                let vis = early.get(&c.id).copied().unwrap_or(c.start);
                if c.disabled || vis <= t || vis > t + lookahead {
                    continue;
                }
                let Some(m) = c.media_id().and_then(|id| project.media(id)) else { continue };
                if m.kind != MediaKind::Video {
                    continue;
                }
                let natural = natural_media_size(&ctx, c, m);
                next.push((vis, video_req(&ctx, c, m, vis, natural)));
            }
        }
        // only warm up the next couple of clips: every warm-up is a decoder
        next.sort_by_key(|n| n.0);
        for (_, req) in next.iter().take(2) {
            let _ = frames.video(req);
        }
    }

    fn gc(&mut self) {
        let now = self.frame_no;
        let dead: Vec<u64> = self.slots.iter().filter(|(_, s)| now.saturating_sub(s.used) > 90).map(|(k, _)| *k).collect();
        for k in dead {
            if let Some(s) = self.slots.remove(&k) {
                self.gpu.release(s.rgba);
            }
        }
    }

    pub fn clear_cache(&mut self) {
        let keys: Vec<u64> = self.slots.keys().copied().collect();
        for k in keys {
            if let Some(s) = self.slots.remove(&k) {
                self.gpu.release(s.rgba);
            }
        }
        self.gpu.trim_pool();
    }

    /// Run a full-screen pass reading `src` (and optionally `aux`) into a new texture.
    fn full_pass(&mut self, enc: &mut wgpu::CommandEncoder, pipe: Pipe, src: &Tex, aux: Option<&Tex>, u: &U, release_src: bool) -> Tex {
        let _ = release_src;
        let out = self.gpu.tex(src.w, src.h);
        self.gpu.pass(enc, pipe, &out.view, [Some(&src.view), aux.map(|a| &a.view), None], u);
        out
    }

    fn copy_scaled(&mut self, enc: &mut wgpu::CommandEncoder, src: &wgpu::TextureView, w: u32, h: u32) -> Tex {
        let out = self.gpu.tex(w, h);
        let mut u = U::with_canvas(w, h);
        u.p[0] = [0.0, 0.0, 1.0, 1.0];
        self.gpu.pass(enc, Pipe::Copy, &out.view, [Some(src), None, None], &u);
        out
    }

    /// Gaussian blur with a downsample pyramid. `sigma` in pixels of `src`.
    fn blur(&mut self, enc: &mut wgpu::CommandEncoder, src_view: &wgpu::TextureView, sw: u32, sh: u32, sigma: f32) -> Tex {
        let mut cur: Option<Tex> = None;
        let (mut w, mut h) = (sw, sh);
        let mut s = sigma;
        while s > 3.0 && w > 8 && h > 8 {
            let nw = (w / 2).max(1);
            let nh = (h / 2).max(1);
            let next = {
                let v = cur.as_ref().map(|t| &t.view).unwrap_or(src_view);
                // borrow dance: copy into a new texture
                let out = self.gpu.tex(nw, nh);
                let mut u = U::with_canvas(nw, nh);
                u.p[0] = [0.0, 0.0, 1.0, 1.0];
                self.gpu.pass(enc, Pipe::Copy, &out.view, [Some(v), None, None], &u);
                out
            };
            if let Some(old) = cur.take() {
                self.gpu.release(old);
            }
            cur = Some(next);
            w = nw;
            h = nh;
            s *= 0.5;
        }
        if s > 0.25 {
            for dir in [(1.0f32, 0.0f32), (0.0, 1.0)] {
                let out = self.gpu.tex(w, h);
                let mut u = U::with_canvas(w, h);
                u.p[0] = [dir.0 / w as f32, dir.1 / h as f32, s, 0.0];
                {
                    let v = cur.as_ref().map(|t| &t.view).unwrap_or(src_view);
                    self.gpu.pass(enc, Pipe::Blur, &out.view, [Some(v), None, None], &u);
                }
                if let Some(old) = cur.take() {
                    self.gpu.release(old);
                }
                cur = Some(out);
            }
        }
        let result = {
            let v = cur.as_ref().map(|t| &t.view).unwrap_or(src_view);
            self.copy_scaled(enc, &v.clone(), sw, sh)
        };
        if let Some(old) = cur.take() {
            self.gpu.release(old);
        }
        result
    }

    // ------------------------------------------------------------------ sources

    fn slot_for(&mut self, key: u64, w: u32, h: u32, fmt: PixFmt) -> &mut Slot {
        let recreate = match self.slots.get(&key) {
            Some(s) => s.rgba.w != w || s.rgba.h != h || s.fmt != fmt,
            None => true,
        };
        if recreate {
            if let Some(old) = self.slots.remove(&key) {
                self.gpu.release(old.rgba);
            }
            let rgba = self.gpu.tex(w, h);
            let (cw, ch) = ((w + 1) / 2, (h + 1) / 2);
            let planes = match fmt {
                PixFmt::Yuv420 => vec![self.gpu.plane_tex(w, h), self.gpu.plane_tex(cw, ch), self.gpu.plane_tex(cw, ch)],
                PixFmt::Nv12 => vec![self.gpu.plane_tex(w, h), self.gpu.plane_tex_rg(cw, ch)],
                PixFmt::Rgba => vec![],
            };
            self.slots.insert(key, Slot { rgba, fmt, planes, uid: 0, used: 0 });
        }
        let s = self.slots.get_mut(&key).unwrap();
        s.used = self.frame_no;
        s
    }

    fn upload_frame(&mut self, enc: &mut wgpu::CommandEncoder, key: u64, f: &Frame) {
        let uid = self.slot_for(key, f.w, f.h, f.fmt).uid;
        if uid == f.uid {
            return;
        }
        let slot = self.slots.get(&key).unwrap();
        let ysz = (f.w * f.h) as usize;
        let (cw, ch) = ((f.w + 1) / 2, (f.h + 1) / 2);
        let csz = (cw * ch) as usize;
        match f.fmt {
            PixFmt::Yuv420 if f.data.len() >= ysz + 2 * csz => {
                self.gpu.upload(&slot.planes[0], &f.data[..ysz], 1);
                self.gpu.upload(&slot.planes[1], &f.data[ysz..ysz + csz], 1);
                self.gpu.upload(&slot.planes[2], &f.data[ysz + csz..ysz + 2 * csz], 1);
                let u = U::with_canvas(f.w, f.h);
                let (p0, p1, p2, target) =
                    (slot.planes[0].view.clone(), slot.planes[1].view.clone(), slot.planes[2].view.clone(), slot.rgba.view.clone());
                self.gpu.pass(enc, Pipe::Yuv, &target, [Some(&p0), Some(&p1), Some(&p2)], &u);
            }
            PixFmt::Nv12 if f.data.len() >= ysz + 2 * csz => {
                self.gpu.upload(&slot.planes[0], &f.data[..ysz], 1);
                self.gpu.upload(&slot.planes[1], &f.data[ysz..ysz + 2 * csz], 2);
                let u = U::with_canvas(f.w, f.h);
                let (p0, p1, target) = (slot.planes[0].view.clone(), slot.planes[1].view.clone(), slot.rgba.view.clone());
                self.gpu.pass(enc, Pipe::Nv12, &target, [Some(&p0), Some(&p1), None], &u);
            }
            PixFmt::Rgba => self.gpu.upload(&slot.rgba, &f.data, 4),
            _ => return,
        }
        self.slots.get_mut(&key).unwrap().uid = f.uid;
    }

    fn upload_raster(&mut self, key: u64, img: &raster::RasterImg) {
        let uid = self.slot_for(key, img.w, img.h, PixFmt::Rgba).uid;
        if uid != img.uid {
            let slot = self.slots.get(&key).unwrap();
            self.gpu.upload(&slot.rgba, &img.data, 4);
            self.slots.get_mut(&key).unwrap().uid = img.uid;
        }
    }

    /// Make the clip's picture available in its slot; returns (slot key, natural size in project px).
    fn prepare_source(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        ctx: &Ctx,
        c: &Clip,
        frames: &mut dyn FrameProvider,
        info: &mut RenderInfo,
        t: Us,
    ) -> Option<(u64, (f64, f64))> {
        let (pw, ph) = (ctx.pw(), ctx.ph());
        let rs = ctx.rs();
        match &c.kind {
            ClipKind::Media { media } => {
                let m = ctx.p.media(*media)?;
                if m.kind == MediaKind::Audio {
                    return None;
                }
                let (mw, mh) = (m.width.max(1) as f64, m.height.max(1) as f64);
                let vw = mw * (1.0 - c.crop.left - c.crop.right).max(0.01);
                let vh = mh * (1.0 - c.crop.top - c.crop.bottom).max(0.01);
                let k = (pw / vw).min(ph / vh);
                let natural = (vw * k, vh * k);
                let frame = if m.kind == MediaKind::Image {
                    frames.image(&m.path)
                } else {
                    let req = video_req(ctx, c, m, t, natural);
                    match frames.video(&req) {
                        Some((f, exact)) => {
                            if !exact {
                                info.pending = true;
                            }
                            Some(f)
                        }
                        None => {
                            info.pending = true;
                            None
                        }
                    }
                };
                let f = frame?;
                self.upload_frame(enc, c.id, &f);
                Some((c.id, natural))
            }
            ClipKind::Text(td) => {
                let unit = pw.min(ph) / 1080.0;
                let scale = (unit * rs) as f32;
                let img = text_image(&mut self.raster, c, td, scale, t);
                self.upload_raster(c.id, &img);
                Some((c.id, (img.w as f64 / rs, img.h as f64 / rs)))
            }
            ClipKind::Sticker(sd) => {
                let unit = pw.min(ph) / 1080.0;
                let size = (300.0 * unit * rs * c.tf.scale.abs().clamp(0.25, 4.0)).round() as u32;
                let img = self.raster.sticker(&sd.id, sd.color, size.max(16));
                self.upload_raster(c.id, &img);
                let k = c.tf.scale.abs().clamp(0.25, 4.0);
                Some((c.id, (img.w as f64 / rs / k, img.h as f64 / rs / k)))
            }
            _ => None,
        }
    }

    /// Natural (scale = 1) size of a visual clip in project pixels.
    fn natural_size(&mut self, ctx: &Ctx, c: &Clip, t: Us) -> Option<(f64, f64)> {
        let (pw, ph) = (ctx.pw(), ctx.ph());
        let rs = ctx.rs();
        match &c.kind {
            ClipKind::Media { media } => {
                let m = ctx.p.media(*media)?;
                if m.kind == MediaKind::Audio {
                    return None;
                }
                let (mw, mh) = (m.width.max(1) as f64, m.height.max(1) as f64);
                let vw = mw * (1.0 - c.crop.left - c.crop.right).max(0.01);
                let vh = mh * (1.0 - c.crop.top - c.crop.bottom).max(0.01);
                let k = (pw / vw).min(ph / vh);
                Some((vw * k, vh * k))
            }
            ClipKind::Text(td) => {
                let unit = pw.min(ph) / 1080.0;
                let img = text_image(&mut self.raster, c, td, (unit * rs) as f32, t);
                Some((img.w as f64 / rs, img.h as f64 / rs))
            }
            ClipKind::Sticker(sd) => {
                let unit = pw.min(ph) / 1080.0;
                let k = c.tf.scale.abs().clamp(0.25, 4.0);
                let size = (300.0 * unit * rs * k).round() as u32;
                let img = self.raster.sticker(&sd.id, sd.color, size.max(16));
                Some((img.w as f64 / rs / k, img.h as f64 / rs / k))
            }
            _ => None,
        }
    }

    /// Corners of a clip's frame in output pixels (for the on-canvas gizmo); ignores animations.
    pub fn clip_quad(&mut self, project: &Project, c: &Clip, t: Us, w: u32, h: u32) -> Option<[(f64, f64); 4]> {
        let ctx = Ctx { p: project, t, w: w.max(2), h: h.max(2), export: false, fd: 1_000_000.0 / project.fps.max(1.0) };
        let natural = self.natural_size(&ctx, c, t)?;
        let m = self.layer_matrix(&ctx, c, t, natural, XD::ID, XD::ID);
        Some([apply(&m, 0.0, 0.0), apply(&m, 1.0, 0.0), apply(&m, 1.0, 1.0), apply(&m, 0.0, 1.0)])
    }

    // ------------------------------------------------------------------ layers

    fn layer_matrix(&self, ctx: &Ctx, c: &Clip, t: Us, natural: (f64, f64), anim: XD, cam: XD) -> M3 {
        let (pw, ph) = (ctx.pw(), ctx.ph());
        let s = c.prop(Prop::Scale, t);
        let (sx, sy) = if c.tf.uniform { (1.0, 1.0) } else { (c.prop(Prop::ScaleX, t), c.prop(Prop::ScaleY, t)) };
        let fx = if c.tf.flip_h { -1.0 } else { 1.0 };
        let fy = if c.tf.flip_v { -1.0 } else { 1.0 };
        let w = natural.0 * s * sx * anim.sx * fx;
        let h = natural.1 * s * sy * anim.sy * fy;
        let cx = pw / 2.0 + c.prop(Prop::PosX, t) + anim.tx * pw;
        let cy = ph / 2.0 - c.prop(Prop::PosY, t) + anim.ty * ph;
        let r = (c.prop(Prop::Rotation, t) + anim.rot).to_radians();
        let layer = mm(&mm(&mm(&mt(cx, cy), &mr(r)), &ms(w, h)), &mt(-0.5, -0.5));
        let camm = cam_matrix(ctx, cam);
        let out = ms(ctx.w as f64 / pw, ctx.h as f64 / ph);
        mm(&mm(&out, &camm), &layer)
    }

    #[allow(clippy::too_many_arguments)]
    fn composite(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        ctx: &Ctx,
        canvas: Tex,
        src_view: &wgpu::TextureView,
        src_size: (u32, u32),
        mats: [M3; 3],
        crop: Crop,
        opacity: f64,
        blend: Blend,
        extra: impl FnOnce(&mut U),
    ) -> Tex {
        // texture uv = crop offset + layer uv * crop size
        let cw = (1.0 - crop.left - crop.right).max(0.001);
        let ch = (1.0 - crop.top - crop.bottom).max(0.001);
        let cm: M3 = [[cw, 0.0, crop.left], [0.0, ch, crop.top], [0.0, 0.0, 1.0]];
        let mut u = U::with_canvas(ctx.w, ctx.h);
        u.src = [src_size.0 as f32, src_size.1 as f32, 1.0 / src_size.0.max(1) as f32, 1.0 / src_size.1.max(1) as f32];
        for (k, m) in mats.iter().enumerate() {
            let inv = mm(&cm, &minv(m));
            u.m[k * 2] = [inv[0][0] as f32, inv[0][1] as f32, inv[0][2] as f32, 0.0];
            u.m[k * 2 + 1] = [inv[1][0] as f32, inv[1][1] as f32, inv[1][2] as f32, 0.0];
        }
        // samples from on-screen displacement over the shutter
        let mut disp: f64 = 0.0;
        for (x, y) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0), (0.5, 0.5)] {
            let a = apply(&mats[0], x, y);
            let b = apply(&mats[2], x, y);
            let m = apply(&mats[1], x, y);
            disp = disp.max(((a.0 - m.0).hypot(a.1 - m.1)) + ((m.0 - b.0).hypot(m.1 - b.1)));
        }
        // motion blur samples: full quality on export, fewer at lower preview qualities
        // a paused frame is rendered once: it can afford smooth blur at any quality
        let max_n = if ctx.export {
            64.0
        } else if !self.playing {
            32.0
        } else {
            [24.0, 12.0, 6.0][self.preview_quality.min(2) as usize]
        };
        let n = (disp / 1.6).ceil().clamp(1.0, max_n);
        u.cfg = [n as f32, opacity.clamp(0.0, 1.0) as f32, blend.index() as f32, 0.0];
        u.crop = [crop.left as f32, crop.top as f32, (1.0 - crop.right) as f32, (1.0 - crop.bottom) as f32];
        u.time = [0.0, 0.0, (self.frame_no % 977) as f32, 0.0];
        extra(&mut u);
        let out = self.gpu.tex(ctx.w, ctx.h);
        self.gpu.pass(enc, Pipe::Composite, &out.view, [Some(src_view), Some(&canvas.view), None], &u);
        self.gpu.release(canvas);
        out
    }

    /// Draw one visual clip on top of `canvas`. `cam` = extra camera delta at the 3 shutter instants.
    #[allow(clippy::too_many_arguments)]
    fn draw_clip(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        ctx: &Ctx,
        c: &Clip,
        canvas: Tex,
        frames: &mut dyn FrameProvider,
        info: &mut RenderInfo,
        cam: [XD; 3],
        cam_blur: f64,
    ) -> Tex {
        let t = ctx.t;
        let Some((key, natural)) = self.prepare_source(enc, ctx, c, frames, info, t) else { return canvas };
        let (anim_m, anim_blur) = clip_anim(c, t);
        let mb = c.motion_blur.max(anim_blur).max(cam_blur);
        let sh = if mb > 0.001 { mb * 1.5 * ctx.fd } else { 0.0 };
        let times = [t - (sh * 0.5) as Us, t, t + (sh * 0.5) as Us];
        let mut mats = [[[0.0; 3]; 3]; 3];
        for k in 0..3 {
            let a = if k == 1 { anim_m } else { clip_anim(c, times[k]).0 };
            mats[k] = self.layer_matrix(ctx, c, times[k], natural, a, cam[k]);
        }
        let opacity = c.prop(Prop::Opacity, t) * anim_m.alpha * cam[1].alpha;
        if opacity <= 0.001 {
            return canvas;
        }
        // grading
        let adj = c.adjust_at(t);
        let gi = color::GradeInput::from(&adj);
        let mut flags = 0u32;
        let mut ga = None;
        if !color::is_identity(&gi) {
            ga = Some(color::block(&gi));
            flags |= 1;
            if color::has_spatial(&gi) {
                flags |= 4;
            }
        }
        let mut gf = None;
        let mut fint = 0.0;
        if let Some(f) = &c.filter {
            if let Some(def) = catalog::filter(&f.id) {
                gf = Some(color::block(&color::GradeInput::from(&def.look)));
                fint = c.prop(Prop::FilterIntensity, t);
                if fint > 0.0 {
                    flags |= 2;
                }
            }
        }
        let chroma = c.chroma;
        if chroma.enabled && matches!(c.kind, ClipKind::Media { .. }) {
            flags |= 8;
        }
        let mask = c.mask.map(|m| {
            let mut m = m;
            m.cx = c.prop(Prop::MaskX, t);
            m.cy = c.prop(Prop::MaskY, t);
            m.w = c.prop(Prop::MaskW, t);
            m.h = c.prop(Prop::MaskH, t);
            m.rotation = c.prop(Prop::MaskRotation, t);
            m.feather = c.prop(Prop::MaskFeather, t);
            m
        });
        if mask.is_some() {
            flags |= 16;
        }
        let crop = if matches!(c.kind, ClipKind::Media { .. }) { c.crop } else { Crop::default() };
        let (sw, shh) = {
            let s = self.slots.get(&key).unwrap();
            (s.rgba.w, s.rgba.h)
        };
        // optional pre-blur (blur in/out animations, blur transitions)
        let blur_frac = anim_m.blur.max(cam[1].blur);
        let blurred = if blur_frac > 0.0005 {
            let disp_scale = (natural.1 * c.prop(Prop::Scale, t).abs().max(0.01) * ctx.rs()) / shh.max(1) as f64;
            let sigma = (blur_frac * ctx.h as f64 / disp_scale.max(0.01)) as f32;
            let view = self.slots.get(&key).unwrap().rgba.view.clone();
            Some(self.blur(enc, &view, sw, shh, sigma.min(400.0)))
        } else {
            None
        };
        let src_view = match &blurred {
            Some(b) => b.view.clone(),
            None => self.slots.get(&key).unwrap().rgba.view.clone(),
        };
        let aspect = (sw as f64 * (1.0 - crop.left - crop.right)) / (shh as f64 * (1.0 - crop.top - crop.bottom)).max(0.001);
        let local_s = us_to_s(t - c.start) as f32;
        let out = self.composite(enc, ctx, canvas, &src_view, (sw, shh), mats, crop, opacity, c.blend, |u| {
            u.cfg[3] = flags as f32;
            if let Some(g) = ga {
                u.ga = g;
            }
            if let Some(g) = gf {
                u.gf = g;
            }
            u.gmix = [fint as f32, 0.0, 0.0, 0.0];
            u.time[0] = local_s;
            if let Some(m) = mask {
                u.mask = [
                    [m.kind.index() as f32, m.cx as f32, m.cy as f32, (m.rotation as f32).to_radians()],
                    [m.w as f32, m.h as f32, m.feather as f32, m.round as f32],
                    [if m.invert { 1.0 } else { 0.0 }, aspect as f32, 0.0, 0.0],
                ];
            }
            if chroma.enabled {
                u.chroma = [
                    [
                        chroma.color[0] as f32 / 255.0,
                        chroma.color[1] as f32 / 255.0,
                        chroma.color[2] as f32 / 255.0,
                        chroma.intensity as f32,
                    ],
                    [chroma.shadow as f32, 1.0, 0.0, 0.0],
                ];
            }
        });
        if let Some(b) = blurred {
            self.gpu.release(b);
        }
        out
    }

    /// The main track at time t (handles transitions and the canvas fill).
    fn draw_main(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        ctx: &Ctx,
        tr: &Track,
        canvas: Tex,
        frames: &mut dyn FrameProvider,
        info: &mut RenderInfo,
    ) -> Tex {
        let t = ctx.t;
        let clips = &tr.clips;
        for (i, from, to) in ctx.p.transition_regions() {
            if t >= from && t < to {
                let (a, b) = (&clips[i], &clips[i + 1]);
                if let Some(trn) = &a.transition {
                    return self.draw_transition(enc, ctx, a, b, trn, (from, to), canvas, frames, info);
                }
            }
        }
        if let Some(c) = clips.iter().find(|c| c.contains(t)) {
            if c.disabled {
                return canvas;
            }
            let canvas = self.draw_fill(enc, ctx, c, canvas, frames, info);
            return self.draw_clip(enc, ctx, c, canvas, frames, info, [XD::ID; 3], 0.0);
        }
        canvas
    }

    /// "Canvas": colour or blurred copy behind a main-track clip.
    fn draw_fill(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        ctx: &Ctx,
        c: &Clip,
        canvas: Tex,
        frames: &mut dyn FrameProvider,
        info: &mut RenderInfo,
    ) -> Tex {
        match c.canvas {
            CanvasFill::None => canvas,
            CanvasFill::Color(col) => {
                self.gpu.clear(enc, &canvas.view, rgba_f(col));
                canvas
            }
            CanvasFill::Blur(level) => {
                let Some((key, _)) = self.prepare_source(enc, ctx, c, frames, info, ctx.t) else { return canvas };
                let (sw, sh) = {
                    let s = self.slots.get(&key).unwrap();
                    (s.rgba.w, s.rgba.h)
                };
                let (pw, ph) = (ctx.pw(), ctx.ph());
                let vw = sw as f64 * (1.0 - c.crop.left - c.crop.right).max(0.01);
                let vh = sh as f64 * (1.0 - c.crop.top - c.crop.bottom).max(0.01);
                let kc = (pw / vw).max(ph / vh);
                let cover = (vw * kc, vh * kc);
                let sigma_out = (0.012 + level.clamp(0.0, 1.0) * 0.05) * ctx.h as f64;
                let sigma_src = sigma_out / (kc * ctx.rs()).max(0.001);
                let view = self.slots.get(&key).unwrap().rgba.view.clone();
                let blurred = self.blur(enc, &view, sw, sh, sigma_src as f32);
                let m = mm(
                    &ms(ctx.w as f64 / pw, ctx.h as f64 / ph),
                    &mm(&mm(&mt(pw / 2.0, ph / 2.0), &ms(cover.0 * 1.04, cover.1 * 1.04)), &mt(-0.5, -0.5)),
                );
                let bv = blurred.view.clone();
                let out = self.composite(enc, ctx, canvas, &bv, (sw, sh), [m, m, m], c.crop, 1.0, Blend::Normal, |_| {});
                self.gpu.release(blurred);
                out
            }
        }
    }

    /// Render one side of a main-track transition as a full canvas.
    fn render_side(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        ctx: &Ctx,
        c: &Clip,
        frames: &mut dyn FrameProvider,
        info: &mut RenderInfo,
    ) -> Tex {
        let tex = self.gpu.tex(ctx.w, ctx.h);
        self.gpu.clear(enc, &tex.view, rgba_f(ctx.p.bg_color));
        if c.disabled {
            return tex;
        }
        let tex = self.draw_fill(enc, ctx, c, tex, frames, info);
        self.draw_clip(enc, ctx, c, tex, frames, info, [XD::ID; 3], 0.0)
    }

    /// Draw a full-canvas texture as a layer moved by `cam` (with motion blur).
    fn draw_full_layer(&mut self, enc: &mut wgpu::CommandEncoder, ctx: &Ctx, canvas: Tex, layer: &Tex, cam: [XD; 3]) -> Tex {
        let (pw, ph) = (ctx.pw(), ctx.ph());
        let mut mats = [[[0.0; 3]; 3]; 3];
        for k in 0..3 {
            let lm = mm(&mm(&mt(pw / 2.0, ph / 2.0), &ms(pw, ph)), &mt(-0.5, -0.5));
            mats[k] = mm(&mm(&ms(ctx.w as f64 / pw, ctx.h as f64 / ph), &cam_matrix(ctx, cam[k])), &lm);
        }
        let lv = layer.view.clone();
        self.composite(enc, ctx, canvas, &lv, (layer.w, layer.h), mats, Crop::default(), cam[1].alpha, Blend::Normal, |_| {})
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_transition(
        &mut self,
        enc: &mut wgpu::CommandEncoder,
        ctx: &Ctx,
        a: &Clip,
        b: &Clip,
        trn: &TransitionUse,
        region: (Us, Us),
        canvas: Tex,
        frames: &mut dyn FrameProvider,
        info: &mut RenderInfo,
    ) -> Tex {
        let t = ctx.t;
        let span = (region.1 - region.0).max(1) as f64;
        let prog = |tt: f64| ((tt - region.0 as f64) / span).clamp(0.0, 1.0);
        let sh = trn.blur.clamp(0.0, 1.0) * 1.5 * ctx.fd;
        let times = [t as f64 - sh * 0.5, t as f64, t as f64 + sh * 0.5];
        let seed = a.id ^ b.id;
        let plans: [TPlan; 3] = [
            motion::transition_plan(&trn.id, prog(times[0]), seed),
            motion::transition_plan(&trn.id, prog(times[1]), seed),
            motion::transition_plan(&trn.id, prog(times[2]), seed),
        ];
        let pm = plans[1];
        let ta = self.render_side(enc, ctx, a, frames, info);
        let tb = self.render_side(enc, ctx, b, frames, info);
        let bg = rgba_f(ctx.p.bg_color);
        match pm.mix {
            None => {
                self.gpu.clear(enc, &canvas.view, bg);
                let (first, second, fx, sx) = if pm.b_on_top {
                    (&ta, &tb, [plans[0].a, plans[1].a, plans[2].a], [plans[0].b, plans[1].b, plans[2].b])
                } else {
                    (&tb, &ta, [plans[0].b, plans[1].b, plans[2].b], [plans[0].a, plans[1].a, plans[2].a])
                };
                let canvas = self.draw_full_layer(enc, ctx, canvas, first, fx);
                let canvas = self.draw_full_layer(enc, ctx, canvas, second, sx);
                self.gpu.release(ta);
                self.gpu.release(tb);
                canvas
            }
            Some((kind, params)) => {
                // apply per-side camera moves first (zoom/spin based mixes)
                let side = |eng: &mut Engine, enc: &mut wgpu::CommandEncoder, tex: Tex, xd: [XD; 3]| -> Tex {
                    let mut tex = tex;
                    if !(xd[0].is_identity() && xd[1].is_identity() && xd[2].is_identity()) {
                        let mut moved_xd = xd;
                        for x in moved_xd.iter_mut() {
                            x.blur = 0.0;
                        }
                        if !(moved_xd
                            .iter()
                            .all(|x| x.tx == 0.0 && x.ty == 0.0 && x.sx == 1.0 && x.sy == 1.0 && x.rot == 0.0 && x.alpha == 1.0))
                        {
                            let blank = eng.gpu.tex(ctx.w, ctx.h);
                            eng.gpu.clear(enc, &blank.view, bg);
                            let moved = eng.draw_full_layer(enc, ctx, blank, &tex, moved_xd);
                            eng.gpu.release(tex);
                            tex = moved;
                        }
                        if xd[1].blur > 0.0005 {
                            let v = tex.view.clone();
                            let b = eng.blur(enc, &v, tex.w, tex.h, (xd[1].blur * ctx.h as f64) as f32);
                            eng.gpu.release(tex);
                            tex = b;
                        }
                    }
                    tex
                };
                let ta = side(self, enc, ta, [plans[0].a, plans[1].a, plans[2].a]);
                let tb = side(self, enc, tb, [plans[0].b, plans[1].b, plans[2].b]);
                let mut u = U::with_canvas(ctx.w, ctx.h);
                u.p[0] = params;
                u.time = [0.0, prog(times[1]) as f32, (seed % 997) as f32, kind as f32];
                let mixed = self.gpu.tex(ctx.w, ctx.h);
                self.gpu.pass(enc, Pipe::Transition, &mixed.view, [Some(&ta.view), Some(&tb.view), None], &u);
                self.gpu.release(ta);
                self.gpu.release(tb);
                // place over the canvas (premultiplied over)
                let mut u2 = U::with_canvas(ctx.w, ctx.h);
                u2.time = [0.0, 0.0, 0.0, motion::tk::OVER as f32];
                let out = self.gpu.tex(ctx.w, ctx.h);
                self.gpu.pass(enc, Pipe::Transition, &out.view, [Some(&canvas.view), Some(&mixed.view), None], &u2);
                self.gpu.release(mixed);
                self.gpu.release(canvas);
                out
            }
        }
    }

    // ------------------------------------------------------------------ effects

    fn apply_effect(&mut self, enc: &mut wgpu::CommandEncoder, ctx: &Ctx, c: &Clip, e: &EffectUse, canvas: Tex) -> Tex {
        let t = ctx.t;
        let local = us_to_s(t - c.start);
        let dur = us_to_s(c.duration);
        let amt = c.prop(Prop::EffectAmount, t);
        let speed = e.params[1];
        let h = ctx.h as f32;
        let seed = (self.frame_no % 977) as f32;
        let id = e.id.as_str();
        // camera-like effects (warp the whole canvas with motion blur)
        if motion::warp_effect(id, local, dur, amt, speed, c.id).is_some() {
            let sh = 0.8 * ctx.fd / 1_000_000.0;
            let xd = [
                motion::warp_effect(id, local - sh * 0.5, dur, amt, speed, c.id).unwrap(),
                motion::warp_effect(id, local, dur, amt, speed, c.id).unwrap(),
                motion::warp_effect(id, local + sh * 0.5, dur, amt, speed, c.id).unwrap(),
            ];
            let blank = self.gpu.tex(ctx.w, ctx.h);
            self.gpu.clear(enc, &blank.view, rgba_f(ctx.p.bg_color));
            let out = self.draw_full_layer(enc, ctx, blank, &canvas, xd);
            self.gpu.release(canvas);
            return out;
        }
        match id {
            "blur" => {
                let v = canvas.view.clone();
                let b = self.blur(enc, &v, canvas.w, canvas.h, (amt as f32) * 0.03 * h);
                self.gpu.release(canvas);
                b
            }
            "bloom" | "dreamy" | "tilt_shift" => {
                let sigma = match id {
                    "bloom" => 0.022 * h,
                    "dreamy" => 0.012 * h,
                    _ => (0.004 + amt as f32 * 0.016) * h,
                };
                let v = canvas.view.clone();
                let b = self.blur(enc, &v, canvas.w, canvas.h, sigma);
                let mut u = U::with_canvas(ctx.w, ctx.h);
                u.p[0] = [amt as f32, e.params[1] as f32, e.params[2] as f32, 1.0];
                u.time = [local as f32, 0.0, seed, motion::effect_kind(id).unwrap() as f32];
                let out = self.full_pass(enc, Pipe::Effect, &canvas, Some(&b), &u, true);
                self.gpu.release(b);
                self.gpu.release(canvas);
                out
            }
            _ => {
                let Some(kind) = motion::effect_kind(id) else { return canvas };
                let mut u = U::with_canvas(ctx.w, ctx.h);
                let mut p0 = [amt as f32, speed as f32, e.params[2] as f32, 1.0];
                match id {
                    "letterbox" => p0[0] = 0.04 + e.params[0] as f32 * 0.12,
                    "opening" => {
                        let open = motion::ease_in_out_cubic(local / (1.8 - speed * 1.2).max(0.3));
                        p0[0] = (0.5 + (0.11 - 0.5) * open) as f32;
                    }
                    "directional_blur" => p0[1] = e.params[1] as f32,
                    "halftone" => p0[1] = e.params[1] as f32,
                    _ => {}
                }
                u.p[0] = p0;
                u.p[1] = match id {
                    "split2" => [1.0, 2.0, 0.0, 0.0],
                    "split3" => [1.0, 3.0, 0.0, 0.0],
                    "split4" => [2.0, 2.0, 0.0, 0.0],
                    "split9" => [3.0, 3.0, 0.0, 0.0],
                    _ => [1.0, 1.0, 0.0, 0.0],
                };
                u.time = [local as f32, (local / dur.max(0.001)) as f32, seed, kind as f32];
                let out = self.full_pass(enc, Pipe::Effect, &canvas, None, &u, true);
                self.gpu.release(canvas);
                out
            }
        }
    }
}

fn cam_matrix(ctx: &Ctx, cam: XD) -> M3 {
    let (pw, ph) = (ctx.pw(), ctx.ph());
    if cam.is_identity() || (cam.tx == 0.0 && cam.ty == 0.0 && cam.sx == 1.0 && cam.sy == 1.0 && cam.rot == 0.0) {
        return mt(0.0, 0.0);
    }
    mm(
        &mm(&mm(&mt(pw / 2.0 + cam.tx * pw, ph / 2.0 + cam.ty * ph), &mr(cam.rot.to_radians())), &ms(cam.sx, cam.sy)),
        &mt(-pw / 2.0, -ph / 2.0),
    )
}
