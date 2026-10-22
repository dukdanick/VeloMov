//! CPU rasterisation of text and vector stickers into premultiplied RGBA
//! (cosmic-text for shaping + font fallback, tiny-skia for paths).

use crate::model::catalog;
use crate::model::TextData;
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

pub struct RasterImg {
    pub w: u32,
    pub h: u32,
    /// premultiplied RGBA
    pub data: Vec<u8>,
    pub uid: u64,
}

static FONTS: OnceLock<Mutex<Option<FontSystem>>> = OnceLock::new();

pub(super) fn fonts_slot() -> &'static Mutex<Option<FontSystem>> {
    FONTS.get_or_init(|| Mutex::new(None))
}

/// Load the system fonts in the background (takes a moment with many fonts).
pub fn warm_up_fonts() {
    std::thread::spawn(|| {
        let mut g = fonts_slot().lock().unwrap();
        if g.is_none() {
            *g = Some(FontSystem::new());
        }
    });
}

/// Family names available for the text font picker.
pub fn font_families() -> Vec<String> {
    let mut g = fonts_slot().lock().unwrap();
    if g.is_none() {
        *g = Some(FontSystem::new());
    }
    let fs = g.as_ref().unwrap();
    let mut v: Vec<String> = fs.db().faces().filter_map(|f| f.families.first().map(|(n, _)| n.clone())).collect();
    v.sort();
    v.dedup();
    v.retain(|n| !n.to_lowercase().contains("emoji") && !n.starts_with('.'));
    v
}

pub struct Raster {
    swash: SwashCache,
    pub(super) cache: HashMap<u64, Arc<RasterImg>>,
    order: Vec<u64>,
    pub(super) caption_layouts: HashMap<u64, Arc<super::caption::CapLayout>>,
}

pub(super) fn hash_text(td: &TextData, scale: f32, visible: Option<usize>) -> u64 {
    let s = serde_json::to_string(td).unwrap_or_default();
    let mut h = crate::util::hash_str(&s);
    h = crate::util::hash_bytes(h, &scale.to_bits().to_le_bytes());
    h = crate::util::hash_bytes(h, &(visible.map(|v| v as i64).unwrap_or(-1)).to_le_bytes());
    h
}

pub(super) fn paint(c: [u8; 4]) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c[0], c[1], c[2], c[3]);
    p.anti_alias = true;
    p
}

/// Separable box blur on a premultiplied pixmap (3 passes ~ gaussian).
pub(super) fn blur_pixmap(pm: &mut Pixmap, radius: u32) {
    if radius == 0 {
        return;
    }
    let w = pm.width() as usize;
    let h = pm.height() as usize;
    let data = pm.data_mut();
    let mut tmp = vec![0u8; data.len()];
    for _ in 0..3 {
        // horizontal
        for y in 0..h {
            for c in 0..4 {
                let mut acc: u32 = 0;
                let r = radius as usize;
                let get = |x: isize| -> u32 {
                    let xi = x.clamp(0, w as isize - 1) as usize;
                    data[(y * w + xi) * 4 + c] as u32
                };
                for x in -(r as isize)..=(r as isize) {
                    acc += get(x);
                }
                let win = (2 * r + 1) as u32;
                for x in 0..w {
                    tmp[(y * w + x) * 4 + c] = (acc / win) as u8;
                    acc += get(x as isize + r as isize + 1);
                    acc -= get(x as isize - r as isize);
                }
            }
        }
        // vertical
        for x in 0..w {
            for c in 0..4 {
                let mut acc: u32 = 0;
                let r = radius as usize;
                let get = |y: isize| -> u32 {
                    let yi = y.clamp(0, h as isize - 1) as usize;
                    tmp[(yi * w + x) * 4 + c] as u32
                };
                for y in -(r as isize)..=(r as isize) {
                    acc += get(y);
                }
                let win = (2 * r + 1) as u32;
                for y in 0..h {
                    data[(y * w + x) * 4 + c] = (acc / win) as u8;
                    acc += get(y as isize + r as isize + 1);
                    acc -= get(y as isize - r as isize);
                }
            }
        }
    }
}

struct Laid {
    path: Option<Path>,
    /// color glyph images: (x, y, w, h, rgba premultiplied)
    images: Vec<(i32, i32, u32, u32, Vec<u8>)>,
    underline: Vec<(f32, f32, f32)>,
    bounds: (f32, f32, f32, f32),
}

impl Raster {
    pub fn new() -> Self {
        Raster { swash: SwashCache::new(), cache: HashMap::new(), order: vec![], caption_layouts: HashMap::new() }
    }

    pub(super) fn remember(&mut self, key: u64, img: Arc<RasterImg>) -> Arc<RasterImg> {
        self.cache.insert(key, img.clone());
        self.order.push(key);
        if self.order.len() > 96 {
            let old = self.order.remove(0);
            if !self.order.contains(&old) {
                self.cache.remove(&old);
            }
        }
        img
    }

    fn layout(
        &mut self,
        fs: &mut FontSystem,
        text: &str,
        family: &str,
        px: f32,
        bold: bool,
        italic: bool,
        align: u8,
        letter: f32,
        line: f32,
        visible: Option<usize>,
        underline: bool,
    ) -> Laid {
        let metrics = Metrics::new(px, px * (1.25 + line));
        let mut buf = Buffer::new(fs, metrics);
        buf.set_size(fs, None, None);
        let mut attrs = Attrs::new();
        attrs = if family.is_empty() { attrs.family(Family::SansSerif) } else { attrs.family(Family::Name(family)) };
        if bold {
            attrs = attrs.weight(Weight::BOLD);
        }
        if italic {
            attrs = attrs.style(Style::Italic);
        }
        buf.set_text(fs, text, attrs, Shaping::Advanced);
        buf.shape_until_scroll(fs, false);
        // visible byte limit for typewriter
        let limit_byte = visible.map(|n| text.char_indices().nth(n).map(|(i, _)| i).unwrap_or(text.len()));
        // line widths with letter spacing
        let mut lines: Vec<(f32, f32, usize, f32)> = vec![]; // (width, line_y, glyph_count, line_top)
        let mut max_w: f32 = 1.0;
        for run in buf.layout_runs() {
            let n = run.glyphs.len();
            let w = run.line_w + letter * (n.saturating_sub(1)) as f32;
            max_w = max_w.max(w);
            lines.push((w, run.line_y, n, run.line_top));
        }
        let mut pb = PathBuilder::new();
        let mut images = vec![];
        let mut ul = vec![];
        let (mut minx, mut miny, mut maxx, mut maxy) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let line_h = metrics.line_height;
        for (li, run) in buf.layout_runs().enumerate() {
            let (w, line_y, _n, line_top) = lines[li];
            let off = match align {
                0 => 0.0,
                2 => max_w - w,
                _ => (max_w - w) * 0.5,
            };
            // include the full line box in bounds so empty lines still take space
            minx = minx.min(off);
            maxx = maxx.max(off + w);
            miny = miny.min(line_top);
            maxy = maxy.max(line_top + line_h);
            let mut line_end_x = off;
            for (gi, g) in run.glyphs.iter().enumerate() {
                if let Some(lb) = limit_byte {
                    if g.start >= lb {
                        continue;
                    }
                }
                let gx = off + g.x + g.font_size * g.x_offset + letter * gi as f32;
                let gy = line_y + g.y - g.font_size * g.y_offset;
                line_end_x = line_end_x.max(gx + g.w);
                let mut bb = (minx, miny, maxx, maxy);
                self.emit_glyph(fs, g, gx, gy, off + letter * gi as f32, line_y, &mut pb, &mut images, &mut bb);
                (minx, miny, maxx, maxy) = bb;
            }
            if underline && line_end_x > off {
                ul.push((off, line_end_x, line_y + px * 0.12));
            }
        }
        let path = pb.finish();
        if let Some(p) = &path {
            let b = p.bounds();
            minx = minx.min(b.left());
            miny = miny.min(b.top());
            maxx = maxx.max(b.right());
            maxy = maxy.max(b.bottom());
        }
        if minx > maxx {
            minx = 0.0;
            maxx = px;
            miny = 0.0;
            maxy = px;
        }
        Laid { path, images, underline: ul, bounds: (minx, miny, maxx, maxy) }
    }

    /// Add one shaped glyph: its outline to `pb`, or (colour emoji / bitmap fonts) an image.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn emit_glyph(
        &mut self,
        fs: &mut FontSystem,
        g: &cosmic_text::LayoutGlyph,
        gx: f32,
        gy: f32,
        img_x: f32,
        line_y: f32,
        pb: &mut PathBuilder,
        images: &mut Vec<(i32, i32, u32, u32, Vec<u8>)>,
        b: &mut (f32, f32, f32, f32),
    ) {
        let phys = g.physical((0.0, 0.0), 1.0);
        let cmds = self.swash.get_outline_commands(fs, phys.cache_key).map(|c| c.to_vec());
        match cmds {
            Some(cmds) if !cmds.is_empty() => {
                use cosmic_text::Command;
                for c in cmds {
                    match c {
                        Command::MoveTo(p) => pb.move_to(gx + p.x, gy - p.y),
                        Command::LineTo(p) => pb.line_to(gx + p.x, gy - p.y),
                        Command::QuadTo(c1, p) => pb.quad_to(gx + c1.x, gy - c1.y, gx + p.x, gy - p.y),
                        Command::CurveTo(c1, c2, p) => pb.cubic_to(gx + c1.x, gy - c1.y, gx + c2.x, gy - c2.y, gx + p.x, gy - p.y),
                        Command::Close => pb.close(),
                    }
                }
            }
            _ => {
                if let Some(img) = self.swash.get_image(fs, phys.cache_key).as_ref() {
                    let pw = img.placement.width;
                    let ph = img.placement.height;
                    if pw > 0 && ph > 0 {
                        let mut rgba = vec![0u8; (pw * ph * 4) as usize];
                        match img.content {
                            cosmic_text::SwashContent::Color => {
                                for i in 0..(pw * ph) as usize {
                                    let a = img.data[i * 4 + 3] as u32;
                                    rgba[i * 4] = (img.data[i * 4] as u32 * a / 255) as u8;
                                    rgba[i * 4 + 1] = (img.data[i * 4 + 1] as u32 * a / 255) as u8;
                                    rgba[i * 4 + 2] = (img.data[i * 4 + 2] as u32 * a / 255) as u8;
                                    rgba[i * 4 + 3] = a as u8;
                                }
                            }
                            _ => {
                                for i in 0..(pw * ph) as usize {
                                    let a = img.data.get(i).copied().unwrap_or(0);
                                    rgba[i * 4] = a;
                                    rgba[i * 4 + 1] = a;
                                    rgba[i * 4 + 2] = a;
                                    rgba[i * 4 + 3] = a;
                                }
                            }
                        }
                        let x = img_x as i32 + phys.x + img.placement.left;
                        let y = line_y as i32 + phys.y - img.placement.top;
                        b.0 = b.0.min(x as f32);
                        b.1 = b.1.min(y as f32);
                        b.2 = b.2.max((x + pw as i32) as f32);
                        b.3 = b.3.max((y + ph as i32) as f32);
                        images.push((x, y, pw, ph, rgba));
                    }
                }
            }
        }
    }

    /// Rasterise a text clip. `scale` converts text size units into pixels.
    pub fn text(&mut self, td: &TextData, scale: f32, visible: Option<usize>) -> Arc<RasterImg> {
        let key = hash_text(td, scale, visible);
        if let Some(i) = self.cache.get(&key) {
            return i.clone();
        }
        let mut guard = fonts_slot().lock().unwrap();
        if guard.is_none() {
            *guard = Some(FontSystem::new());
        }
        let fs = guard.as_mut().unwrap();
        let px = (td.size as f32 * 4.5 * scale).clamp(2.0, 2000.0);
        let text = if td.text.is_empty() { " " } else { td.text.as_str() };
        let laid = self.layout(
            fs,
            text,
            &td.font,
            px,
            td.bold,
            td.italic,
            td.align,
            td.letter_spacing as f32 * px * 0.1,
            td.line_spacing as f32 * 0.25,
            visible,
            td.underline,
        );
        drop(guard);
        let img = Arc::new(self.paint_text(td, px, &laid));
        self.remember(key, img)
    }

    fn paint_text(&self, td: &TextData, px: f32, laid: &Laid) -> RasterImg {
        let (minx, miny, maxx, maxy) = laid.bounds;
        let stroke_w = if td.stroke { (td.stroke_width as f32 * px * 0.22).max(1.0) } else { 0.0 };
        let bg_pad = if td.background { px * 0.28 } else { 0.0 };
        let shadow_blur = if td.shadow { (td.shadow_blur as f32 * px * 0.25).max(1.0) } else { 0.0 };
        let shadow_dist = if td.shadow { td.shadow_distance as f32 * px * 0.25 } else { 0.0 };
        let ang = (td.shadow_angle as f32).to_radians();
        let (sdx, sdy) = (shadow_dist * ang.cos(), -shadow_dist * ang.sin());
        let pad = stroke_w + bg_pad + shadow_blur * 2.0 + shadow_dist + 4.0;
        let w = ((maxx - minx) + pad * 2.0).ceil().clamp(1.0, 8192.0) as u32;
        let h = ((maxy - miny) + pad * 2.0).ceil().clamp(1.0, 8192.0) as u32;
        let Some(mut pm) = Pixmap::new(w, h) else {
            return RasterImg { w: 1, h: 1, data: vec![0; 4], uid: crate::util::new_id() };
        };
        let tr = Transform::from_translate(pad - minx, pad - miny);
        // background box
        if td.background {
            let r = (td.bg_round as f32 * px * 0.6).max(0.0);
            let x0 = pad - bg_pad;
            let y0 = pad - bg_pad;
            let bw = (maxx - minx) + bg_pad * 2.0;
            let bh = (maxy - miny) + bg_pad * 2.0;
            if let Some(rr) = rounded_rect(x0, y0, bw, bh, r) {
                pm.fill_path(&rr, &paint(td.bg_color), FillRule::Winding, Transform::identity(), None);
            }
        }
        let stroke = Stroke { width: stroke_w * 2.0, line_join: LineJoin::Round, line_cap: LineCap::Round, ..Default::default() };
        // shadow
        if td.shadow {
            if let Some(mut sp) = Pixmap::new(w, h) {
                let sc = td.shadow_color;
                let st = tr.post_translate(sdx, sdy);
                if let Some(path) = &laid.path {
                    if td.stroke {
                        sp.stroke_path(path, &paint(sc), &stroke, st, None);
                    }
                    sp.fill_path(path, &paint(sc), FillRule::Winding, st, None);
                }
                for (x0, x1, y) in &laid.underline {
                    if let Some(r) = tiny_skia::Rect::from_xywh(*x0, *y, (x1 - x0).max(1.0), px * 0.07) {
                        sp.fill_rect(r, &paint(sc), st, None);
                    }
                }
                blur_pixmap(&mut sp, (shadow_blur * 0.6) as u32);
                pm.draw_pixmap(0, 0, sp.as_ref(), &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
            }
        }
        if let Some(path) = &laid.path {
            if td.stroke {
                pm.stroke_path(path, &paint(td.stroke_color), &stroke, tr, None);
            }
            pm.fill_path(path, &paint(td.color), FillRule::Winding, tr, None);
        }
        for (x0, x1, y) in &laid.underline {
            if let Some(r) = tiny_skia::Rect::from_xywh(*x0, *y, (x1 - x0).max(1.0), px * 0.07) {
                pm.fill_rect(r, &paint(td.color), tr, None);
            }
        }
        // colour glyphs (emoji)
        for (x, y, iw, ih, rgba) in &laid.images {
            if let Some(src) = tiny_skia::PixmapRef::from_bytes(rgba, *iw, *ih) {
                let dx = (*x as f32 + pad - minx) as i32;
                let dy = (*y as f32 + pad - miny) as i32;
                pm.draw_pixmap(dx, dy, src, &tiny_skia::PixmapPaint::default(), Transform::identity(), None);
            }
        }
        RasterImg { w, h, data: pm.take(), uid: crate::util::new_id() }
    }

    /// Rasterise a built-in sticker at `size` pixels.
    pub fn sticker(&mut self, id: &str, color: [u8; 4], size: u32) -> Arc<RasterImg> {
        let size = size.clamp(8, 4096);
        let mut key = crate::util::hash_str(id);
        key = crate::util::hash_bytes(key, &color);
        key = crate::util::hash_bytes(key, &size.to_le_bytes());
        if let Some(i) = self.cache.get(&key) {
            return i.clone();
        }
        let def = catalog::sticker(id);
        if let Some(e) = def.and_then(|d| d.emoji) {
            let mut guard = fonts_slot().lock().unwrap();
            if guard.is_none() {
                *guard = Some(FontSystem::new());
            }
            let fs = guard.as_mut().unwrap();
            let laid = self.layout(fs, e, "Noto Color Emoji", size as f32 * 0.8, false, false, 1, 0.0, 0.0, None, false);
            drop(guard);
            let td = TextData { color: [255, 255, 255, 255], ..Default::default() };
            let img = Arc::new(self.paint_text(&td, size as f32 * 0.8, &laid));
            return self.remember(key, img);
        }
        let img = Arc::new(draw_shape(id, color, size, self));
        self.remember(key, img)
    }

    /// Render a short label with the UI font (used by "Subscribe"/"Like" stickers).
    fn label_path(&mut self, text: &str, px: f32) -> Laid {
        let mut guard = fonts_slot().lock().unwrap();
        if guard.is_none() {
            *guard = Some(FontSystem::new());
        }
        let fs = guard.as_mut().unwrap();
        self.layout(fs, text, "", px, true, false, 1, px * 0.04, 0.0, None, false)
    }
}

impl Default for Raster {
    fn default() -> Self {
        Self::new()
    }
}

pub(super) fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    let mut pb = PathBuilder::new();
    let k = 0.5523 * r;
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn star_path(cx: f32, cy: f32, ro: f32, ri: f32, n: usize, rot: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for i in 0..n * 2 {
        let r = if i % 2 == 0 { ro } else { ri };
        let a = rot + i as f32 * std::f32::consts::PI / n as f32;
        let (x, y) = (cx + r * a.sin(), cy - r * a.cos());
        if i == 0 {
            pb.move_to(x, y)
        } else {
            pb.line_to(x, y)
        }
    }
    pb.close();
    pb.finish()
}

fn draw_shape(id: &str, color: [u8; 4], size: u32, raster: &mut Raster) -> RasterImg {
    let s = size as f32;
    let (w, h) = match id {
        "subscribe" | "like" => ((s * 1.9) as u32, (s * 0.55) as u32),
        "underline" => (s as u32, (s * 0.3) as u32),
        "arrow" => (s as u32, (s * 0.6) as u32),
        _ => (size, size),
    };
    let mut pm = Pixmap::new(w.max(1), h.max(1)).unwrap();
    let (wf, hf) = (w as f32, h as f32);
    let fill = paint(color);
    let white = paint([255, 255, 255, 255]);
    let outline = Stroke { width: s * 0.035, line_join: LineJoin::Round, line_cap: LineCap::Round, ..Default::default() };
    let id_t = Transform::identity();
    match id {
        "heart" => {
            let mut pb = PathBuilder::new();
            pb.move_to(s * 0.5, s * 0.88);
            pb.cubic_to(s * 0.1, s * 0.6, s * 0.02, s * 0.38, s * 0.12, s * 0.22);
            pb.cubic_to(s * 0.24, s * 0.04, s * 0.45, s * 0.08, s * 0.5, s * 0.26);
            pb.cubic_to(s * 0.55, s * 0.08, s * 0.76, s * 0.04, s * 0.88, s * 0.22);
            pb.cubic_to(s * 0.98, s * 0.38, s * 0.9, s * 0.6, s * 0.5, s * 0.88);
            pb.close();
            if let Some(p) = pb.finish() {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
                pm.stroke_path(&p, &white, &outline, id_t, None);
            }
        }
        "star" => {
            if let Some(p) = star_path(s * 0.5, s * 0.54, s * 0.45, s * 0.19, 5, 0.0) {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
                pm.stroke_path(&p, &white, &outline, id_t, None);
            }
        }
        "sparkle" => {
            if let Some(p) = star_path(s * 0.5, s * 0.5, s * 0.48, s * 0.09, 4, 0.0) {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
            }
            if let Some(p) = star_path(s * 0.8, s * 0.2, s * 0.14, s * 0.03, 4, 0.0) {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
            }
        }
        "circle" => {
            if let Some(p) = PathBuilder::from_circle(s * 0.5, s * 0.5, s * 0.45) {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
            }
        }
        "ring" => {
            if let Some(p) = PathBuilder::from_circle(s * 0.5, s * 0.5, s * 0.4) {
                let st = Stroke { width: s * 0.08, ..Default::default() };
                pm.stroke_path(&p, &fill, &st, id_t, None);
            }
        }
        "arrow" => {
            let mut pb = PathBuilder::new();
            pb.move_to(wf * 0.05, hf * 0.35);
            pb.line_to(wf * 0.6, hf * 0.35);
            pb.line_to(wf * 0.6, hf * 0.08);
            pb.line_to(wf * 0.97, hf * 0.5);
            pb.line_to(wf * 0.6, hf * 0.92);
            pb.line_to(wf * 0.6, hf * 0.65);
            pb.line_to(wf * 0.05, hf * 0.65);
            pb.close();
            if let Some(p) = pb.finish() {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
                pm.stroke_path(&p, &paint([0, 0, 0, 200]), &outline, id_t, None);
            }
        }
        "arrow_curve" => {
            let mut pb = PathBuilder::new();
            pb.move_to(s * 0.15, s * 0.8);
            pb.quad_to(s * 0.2, s * 0.25, s * 0.72, s * 0.28);
            if let Some(p) = pb.finish() {
                let st = Stroke { width: s * 0.09, line_cap: LineCap::Round, ..Default::default() };
                pm.stroke_path(&p, &fill, &st, id_t, None);
            }
            let mut pb = PathBuilder::new();
            pb.move_to(s * 0.62, s * 0.1);
            pb.line_to(s * 0.92, s * 0.29);
            pb.line_to(s * 0.62, s * 0.46);
            pb.close();
            if let Some(p) = pb.finish() {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
            }
        }
        "bubble" | "thought" => {
            if id == "bubble" {
                if let Some(p) = rounded_rect(s * 0.05, s * 0.1, s * 0.9, s * 0.6, s * 0.18) {
                    pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
                    pm.stroke_path(&p, &paint([20, 20, 20, 255]), &outline, id_t, None);
                }
                let mut pb = PathBuilder::new();
                pb.move_to(s * 0.25, s * 0.68);
                pb.line_to(s * 0.2, s * 0.92);
                pb.line_to(s * 0.45, s * 0.68);
                pb.close();
                if let Some(p) = pb.finish() {
                    pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
                }
            } else {
                let mut pb = PathBuilder::new();
                pb.push_oval(tiny_skia::Rect::from_xywh(s * 0.05, s * 0.08, s * 0.9, s * 0.6).unwrap());
                if let Some(p) = pb.finish() {
                    pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
                    pm.stroke_path(&p, &paint([20, 20, 20, 255]), &outline, id_t, None);
                }
                for (cx, cy, r) in [(0.28, 0.78, 0.07), (0.18, 0.9, 0.04)] {
                    if let Some(p) = PathBuilder::from_circle(s * cx, s * cy, s * r) {
                        pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
                        pm.stroke_path(&p, &paint([20, 20, 20, 255]), &outline, id_t, None);
                    }
                }
            }
        }
        "bolt" => {
            let mut pb = PathBuilder::new();
            pb.move_to(s * 0.58, s * 0.04);
            pb.line_to(s * 0.2, s * 0.56);
            pb.line_to(s * 0.46, s * 0.56);
            pb.line_to(s * 0.38, s * 0.96);
            pb.line_to(s * 0.8, s * 0.4);
            pb.line_to(s * 0.53, s * 0.4);
            pb.close();
            if let Some(p) = pb.finish() {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
                pm.stroke_path(&p, &paint([40, 30, 0, 255]), &outline, id_t, None);
            }
        }
        "check" | "cross" => {
            if let Some(p) = PathBuilder::from_circle(s * 0.5, s * 0.5, s * 0.46) {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
            }
            let mut pb = PathBuilder::new();
            if id == "check" {
                pb.move_to(s * 0.27, s * 0.52);
                pb.line_to(s * 0.44, s * 0.68);
                pb.line_to(s * 0.74, s * 0.34);
            } else {
                pb.move_to(s * 0.32, s * 0.32);
                pb.line_to(s * 0.68, s * 0.68);
                pb.move_to(s * 0.68, s * 0.32);
                pb.line_to(s * 0.32, s * 0.68);
            }
            if let Some(p) = pb.finish() {
                let st = Stroke { width: s * 0.09, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
                pm.stroke_path(&p, &white, &st, id_t, None);
            }
        }
        "burst" => {
            if let Some(p) = star_path(s * 0.5, s * 0.5, s * 0.48, s * 0.3, 12, 0.0) {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
                pm.stroke_path(&p, &paint([255, 255, 255, 255]), &outline, id_t, None);
            }
        }
        "location" => {
            let mut pb = PathBuilder::new();
            pb.move_to(s * 0.5, s * 0.95);
            pb.cubic_to(s * 0.2, s * 0.6, s * 0.15, s * 0.45, s * 0.15, s * 0.38);
            pb.cubic_to(s * 0.15, s * 0.15, s * 0.32, s * 0.03, s * 0.5, s * 0.03);
            pb.cubic_to(s * 0.68, s * 0.03, s * 0.85, s * 0.15, s * 0.85, s * 0.38);
            pb.cubic_to(s * 0.85, s * 0.45, s * 0.8, s * 0.6, s * 0.5, s * 0.95);
            pb.close();
            if let Some(p) = pb.finish() {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
            }
            if let Some(p) = PathBuilder::from_circle(s * 0.5, s * 0.37, s * 0.13) {
                pm.fill_path(&p, &white, FillRule::Winding, id_t, None);
            }
        }
        "crown" => {
            let mut pb = PathBuilder::new();
            pb.move_to(s * 0.1, s * 0.78);
            pb.line_to(s * 0.06, s * 0.3);
            pb.line_to(s * 0.3, s * 0.5);
            pb.line_to(s * 0.5, s * 0.18);
            pb.line_to(s * 0.7, s * 0.5);
            pb.line_to(s * 0.94, s * 0.3);
            pb.line_to(s * 0.9, s * 0.78);
            pb.close();
            if let Some(p) = pb.finish() {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
                pm.stroke_path(&p, &paint([120, 70, 0, 255]), &outline, id_t, None);
            }
        }
        "underline" => {
            let mut pb = PathBuilder::new();
            pb.move_to(wf * 0.04, hf * 0.62);
            pb.cubic_to(wf * 0.3, hf * 0.2, wf * 0.7, hf * 0.75, wf * 0.96, hf * 0.35);
            if let Some(p) = pb.finish() {
                let st = Stroke { width: hf * 0.32, line_cap: LineCap::Round, ..Default::default() };
                pm.stroke_path(&p, &fill, &st, id_t, None);
            }
        }
        "subscribe" | "like" => {
            if let Some(p) = rounded_rect(0.0, 0.0, wf, hf, hf * 0.22) {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
            }
            let label = if id == "subscribe" { "SUBSCRIBE" } else { "LIKE" };
            let laid = raster.label_path(label, hf * 0.42);
            if let Some(path) = &laid.path {
                let (minx, miny, maxx, maxy) = laid.bounds;
                let tx = (wf - (maxx - minx)) * 0.5 - minx;
                let ty = (hf - (maxy - miny)) * 0.5 - miny;
                pm.fill_path(path, &white, FillRule::Winding, Transform::from_translate(tx, ty), None);
            }
        }
        _ => {
            if let Some(p) = PathBuilder::from_circle(s * 0.5, s * 0.5, s * 0.45) {
                pm.fill_path(&p, &fill, FillRule::Winding, id_t, None);
            }
        }
    }
    RasterImg { w, h, data: pm.take(), uid: crate::util::new_id() }
}
