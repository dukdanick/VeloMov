//! Animated captions: per-word animation states for each caption style and the
//! rasteriser that paints a caption line word by word (highlight, karaoke sweep,
//! sliding box / underline, pop, bounce, reveal, one word at a time, glow).

use super::raster::{blur_pixmap, fonts_slot, hash_text, paint, rounded_rect, Raster, RasterImg};
use crate::model::TextData;
use crate::util::{hash_bytes, Us};
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Style, Weight};
use std::sync::Arc;
use tiny_skia::{FillRule, LineCap, LineJoin, Mask, Path, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform};

/// How one word looks at a moment of the caption.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WordState {
    pub alpha: f32,
    pub scale: f32,
    /// Vertical offset in em.
    pub dy: f32,
    /// 0 = text colour, 1 = highlight colour.
    pub hl: f32,
    /// Karaoke fill: share of the word (from the left) in the highlight colour; < 0 = off.
    pub sweep: f32,
    /// Weight of this word in the sliding box / underline position.
    pub mark: f32,
    pub glow: f32,
    /// Draw the word in the middle of the caption (one word at a time).
    pub center: bool,
}

const REST: WordState = WordState { alpha: 1.0, scale: 1.0, dy: 0.0, hl: 0.0, sweep: -1.0, mark: 0.0, glow: 0.0, center: false };

fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x).powi(3)
}

fn ease_out_back(x: f32) -> f32 {
    let (c1, c3) = (1.70158, 2.70158);
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

/// Word states of caption style `style` at caption-local time `local` for words spoken at
/// `times`; the second value is the opacity of the box / underline marker.
pub fn states(style: &str, times: &[(Us, Us)], local: Us) -> (Vec<WordState>, f32) {
    let n = times.len();
    let mut v = vec![REST; n];
    // the word being spoken (stays on the last one after it ends)
    let active = times.iter().rposition(|t| t.0 <= local);
    let ramp = |i: usize, ms: f64| ((local - times[i].0) as f64 / (ms * 1000.0)).clamp(0.0, 1.0) as f32;
    let mut mark_alpha = 0.0;
    let mut slide = |v: &mut Vec<WordState>| {
        if let Some(k) = active {
            let p = ease_out(ramp(k, 110.0));
            if k > 0 {
                v[k].mark = p;
                v[k - 1].mark = 1.0 - p;
                mark_alpha = 1.0;
            } else {
                v[k].mark = 1.0;
                mark_alpha = p;
            }
        }
    };
    match style {
        "karaoke" => {
            for (i, t) in times.iter().enumerate() {
                let len = (t.1 - t.0).max(1) as f64;
                v[i].sweep = ((local - t.0) as f64 / len).clamp(0.0, 1.0) as f32;
            }
        }
        "highlight" => {
            if let Some(k) = active {
                let p = ramp(k, 90.0);
                v[k].hl = p;
                if k > 0 {
                    v[k - 1].hl = 1.0 - p;
                }
            }
        }
        "box" | "underline" => slide(&mut v),
        "pop" => {
            if let Some(k) = active {
                v[k].hl = 1.0;
                v[k].scale = 1.0 + 0.2 * ease_out_back(ramp(k, 180.0));
                if k > 0 {
                    let q = ramp(k, 100.0);
                    v[k - 1].scale = 1.0 + 0.2 * (1.0 - q);
                    v[k - 1].hl = 1.0 - q;
                }
            }
        }
        "bounce" => {
            for (i, t) in times.iter().enumerate() {
                if local < t.0 {
                    v[i].alpha = 0.0;
                } else {
                    v[i].alpha = ramp(i, 90.0);
                    v[i].dy = 0.45 * (1.0 - ease_out_back(ramp(i, 260.0)));
                }
            }
            if let Some(k) = active {
                v[k].hl = 1.0;
            }
        }
        "reveal" => {
            for (i, t) in times.iter().enumerate() {
                if local < t.0 {
                    v[i].alpha = 0.0;
                } else {
                    let p = ramp(i, 160.0);
                    v[i].alpha = p;
                    v[i].scale = 0.85 + 0.15 * ease_out(p);
                }
            }
        }
        "single" => {
            for s in v.iter_mut() {
                s.alpha = 0.0;
            }
            if let Some(k) = active {
                v[k].alpha = ramp(k, 60.0);
                v[k].scale = 0.6 + 0.4 * ease_out_back(ramp(k, 200.0));
                v[k].center = true;
            }
        }
        "glow" => {
            if let Some(k) = active {
                let p = ease_out(ramp(k, 120.0));
                v[k].glow = p;
                if k > 0 {
                    v[k - 1].glow = 1.0 - p;
                }
            }
        }
        "dim" => {
            for (i, t) in times.iter().enumerate() {
                if local < t.0 {
                    v[i].alpha = 0.35;
                } else {
                    v[i].alpha = 0.35 + 0.65 * ramp(i, 120.0);
                }
            }
        }
        "bubble" => {
            if let Some(k) = active {
                let p = ramp(k, 160.0);
                v[k].hl = ramp(k, 80.0);
                v[k].scale = 1.0 + 0.1 * ease_out_back(p);
                if k > 0 {
                    let q = ramp(k, 80.0);
                    v[k - 1].hl = 1.0 - q;
                    v[k - 1].scale = 1.0 + 0.1 * (1.0 - q);
                }
            }
        }
        _ => {}
    }
    (v, mark_alpha)
}

pub struct WordShape {
    path: Option<Path>,
    images: Vec<(i32, i32, u32, u32, Vec<u8>)>,
    /// Word box: advance horizontally, line box vertically.
    rect: [f32; 4],
    /// Baseline of the word's line.
    base: f32,
}

/// A caption line shaped once and kept while only the animation changes.
pub struct CapLayout {
    words: Vec<WordShape>,
    bounds: (f32, f32, f32, f32),
    px: f32,
}

fn mix(a: [u8; 4], b: [u8; 4], t: f32, alpha: f32) -> [u8; 4] {
    let t = t.clamp(0.0, 1.0);
    let ch = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t).round().clamp(0.0, 255.0) as u8;
    [ch(0), ch(1), ch(2), (ch(3) as f32 * alpha.clamp(0.0, 1.0)).round() as u8]
}

fn faded(c: [u8; 4], alpha: f32) -> [u8; 4] {
    [c[0], c[1], c[2], (c[3] as f32 * alpha.clamp(0.0, 1.0)).round() as u8]
}

impl Raster {
    /// Rasterise an animated caption at caption-local time `local`. `span` is the time range
    /// words are spread over when the text has no (matching) word timings.
    pub fn caption(&mut self, td: &TextData, scale: f32, local: Us, span: (Us, Us)) -> Arc<RasterImg> {
        let spans = td.word_spans(span);
        let times: Vec<(Us, Us)> = spans.iter().map(|s| (s.2, s.3)).collect();
        let (states, mark_alpha) = states(&td.caption, &times, local);
        let base = hash_text(td, scale, None);
        let q = |f: f32| ((f * 200.0).round() as i32).to_le_bytes();
        let mut key = hash_bytes(base, b"caption");
        for s in &states {
            for f in [s.alpha, s.scale, s.dy, s.hl, s.sweep, s.mark, s.glow] {
                key = hash_bytes(key, &q(f));
            }
            key = hash_bytes(key, &[s.center as u8]);
        }
        key = hash_bytes(key, &q(mark_alpha));
        if let Some(i) = self.cache.get(&key) {
            return i.clone();
        }
        let lay = match self.caption_layouts.get(&base) {
            Some(l) => l.clone(),
            None => {
                let l = Arc::new(self.caption_layout(td, scale, &spans));
                if self.caption_layouts.len() > 48 {
                    self.caption_layouts.clear();
                }
                self.caption_layouts.insert(base, l.clone());
                l
            }
        };
        let img = Arc::new(paint_caption(td, &lay, &states, mark_alpha));
        self.remember(key, img)
    }

    fn caption_layout(&mut self, td: &TextData, scale: f32, spans: &[(usize, usize, Us, Us)]) -> CapLayout {
        let mut guard = fonts_slot().lock().unwrap();
        if guard.is_none() {
            *guard = Some(FontSystem::new());
        }
        let fs = guard.as_mut().unwrap();
        let px = (td.size as f32 * 4.5 * scale).clamp(2.0, 2000.0);
        let letter = td.letter_spacing as f32 * px * 0.1;
        let metrics = Metrics::new(px, px * (1.25 + td.line_spacing as f32 * 0.25));
        let mut buf = Buffer::new(fs, metrics);
        buf.set_size(fs, None, None);
        let mut attrs = Attrs::new();
        attrs = if td.font.is_empty() { attrs.family(Family::SansSerif) } else { attrs.family(Family::Name(&td.font)) };
        if td.bold {
            attrs = attrs.weight(Weight::BOLD);
        }
        if td.italic {
            attrs = attrs.style(Style::Italic);
        }
        let text = if td.text.is_empty() { " " } else { td.text.as_str() };
        buf.set_text(fs, text, attrs, Shaping::Advanced);
        buf.shape_until_scroll(fs, false);
        let max_w = buf.layout_runs().map(|r| r.line_w + letter * r.glyphs.len().saturating_sub(1) as f32).fold(1.0f32, f32::max);
        let line_h = metrics.line_height;
        let n = spans.len();
        let mut pbs: Vec<PathBuilder> = (0..n).map(|_| PathBuilder::new()).collect();
        let mut imgs: Vec<Vec<(i32, i32, u32, u32, Vec<u8>)>> = (0..n).map(|_| vec![]).collect();
        let mut rects = vec![[f32::MAX, f32::MAX, f32::MIN, f32::MIN]; n];
        let mut bases = vec![0.0f32; n];
        let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for run in buf.layout_runs() {
            let w = run.line_w + letter * run.glyphs.len().saturating_sub(1) as f32;
            let off = match td.align {
                0 => 0.0,
                2 => max_w - w,
                _ => (max_w - w) * 0.5,
            };
            b.0 = b.0.min(off);
            b.2 = b.2.max(off + w);
            b.1 = b.1.min(run.line_top);
            b.3 = b.3.max(run.line_top + line_h);
            for (gi, g) in run.glyphs.iter().enumerate() {
                let Some(wi) = spans.iter().position(|s| g.start >= s.0 && g.start < s.1) else { continue };
                let gx = off + g.x + g.font_size * g.x_offset + letter * gi as f32;
                let gy = run.line_y + g.y - g.font_size * g.y_offset;
                let r = &mut rects[wi];
                r[0] = r[0].min(gx);
                r[2] = r[2].max(gx + g.w);
                r[1] = r[1].min(run.line_top);
                r[3] = r[3].max(run.line_top + line_h);
                bases[wi] = run.line_y;
                self.emit_glyph(fs, g, gx, gy, off + letter * gi as f32, run.line_y, &mut pbs[wi], &mut imgs[wi], &mut b);
            }
        }
        let words: Vec<WordShape> = pbs
            .into_iter()
            .zip(imgs)
            .zip(rects)
            .zip(bases)
            .map(|(((pb, images), r), base)| {
                let path = pb.finish();
                if let Some(p) = &path {
                    let pb = p.bounds();
                    b = (b.0.min(pb.left()), b.1.min(pb.top()), b.2.max(pb.right()), b.3.max(pb.bottom()));
                }
                WordShape { path, images, rect: if r[0] > r[2] { [0.0; 4] } else { r }, base }
            })
            .collect();
        if b.0 > b.2 {
            b = (0.0, 0.0, px, px);
        }
        CapLayout { words, bounds: b, px }
    }
}

fn paint_caption(td: &TextData, lay: &CapLayout, states: &[WordState], mark_alpha: f32) -> RasterImg {
    let px = lay.px;
    let (minx, miny, maxx, maxy) = lay.bounds;
    let stroke_w = if td.stroke { (td.stroke_width as f32 * px * 0.22).max(1.0) } else { 0.0 };
    let bg_pad = if td.background { px * 0.28 } else { 0.0 };
    let shadow_blur = if td.shadow { (td.shadow_blur as f32 * px * 0.25).max(1.0) } else { 0.0 };
    let shadow_dist = if td.shadow { td.shadow_distance as f32 * px * 0.25 } else { 0.0 };
    let ang = (td.shadow_angle as f32).to_radians();
    let (sdx, sdy) = (shadow_dist * ang.cos(), -shadow_dist * ang.sin());
    // room for words that grow, jump and glow (the size never changes while it animates)
    let pad = stroke_w + bg_pad + shadow_blur * 2.0 + shadow_dist + px * 0.5 + 4.0;
    let w = ((maxx - minx) + pad * 2.0).ceil().clamp(1.0, 8192.0) as u32;
    let h = ((maxy - miny) + pad * 2.0).ceil().clamp(1.0, 8192.0) as u32;
    let Some(mut pm) = Pixmap::new(w, h) else {
        return RasterImg { w: 1, h: 1, data: vec![0; 4], uid: crate::util::new_id() };
    };
    let base = Transform::from_translate(pad - minx, pad - miny);
    let (ccx, ccy) = ((minx + maxx) * 0.5, (miny + maxy) * 0.5);
    // a word that grows pushes the other words of its line aside
    let mut push = vec![0.0f32; lay.words.len()];
    for (i, s) in states.iter().enumerate() {
        if s.scale > 1.001 && !s.center {
            let r = lay.words[i].rect;
            let half = (s.scale - 1.0) * (r[2] - r[0]) * 0.5;
            for (j, wj) in lay.words.iter().enumerate() {
                if j != i && (wj.rect[1] - r[1]).abs() < 1.0 {
                    push[j] += if j < i { -half } else { half };
                }
            }
        }
    }
    let word_tf = |i: usize| -> Transform {
        let st = states[i];
        let r = lay.words[i].rect;
        let (cx, cy) = ((r[0] + r[2]) * 0.5, (r[1] + r[3]) * 0.5);
        let (tx, ty) = if st.center { (ccx - cx, ccy - cy) } else { (push[i], 0.0) };
        Transform::from_translate(-cx, -cy)
            .post_scale(st.scale, st.scale)
            .post_translate(cx + tx, cy + ty + st.dy * px)
            .post_concat(base)
    };
    let visible = |i: usize| states.get(i).is_some_and(|s| s.alpha > 0.004) && lay.words[i].path.is_some();
    let stroke = Stroke { width: stroke_w * 2.0, line_join: LineJoin::Round, line_cap: LineCap::Round, ..Default::default() };
    // background box behind the whole line
    if td.background {
        let r = (td.bg_round as f32 * px * 0.6).max(0.0);
        if let Some(rr) = rounded_rect(pad - bg_pad, pad - bg_pad, (maxx - minx) + bg_pad * 2.0, (maxy - miny) + bg_pad * 2.0, r) {
            pm.fill_path(&rr, &paint(td.bg_color), FillRule::Winding, Transform::identity(), None);
        }
    }
    if td.shadow {
        if let Some(mut sp) = Pixmap::new(w, h) {
            for (i, wd) in lay.words.iter().enumerate() {
                let Some(path) = wd.path.as_ref().filter(|_| visible(i)) else { continue };
                let tf = word_tf(i).post_translate(sdx, sdy);
                let c = faded(td.shadow_color, states[i].alpha);
                if td.stroke {
                    sp.stroke_path(path, &paint(c), &stroke, tf, None);
                }
                sp.fill_path(path, &paint(c), FillRule::Winding, tf, None);
            }
            blur_pixmap(&mut sp, (shadow_blur * 0.6) as u32);
            pm.draw_pixmap(0, 0, sp.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
    }
    // glow around the spoken word
    if states.iter().any(|s| s.glow > 0.01) {
        if let Some(mut gp) = Pixmap::new(w, h) {
            let gs = Stroke { width: px * 0.16, line_join: LineJoin::Round, ..Default::default() };
            for (i, wd) in lay.words.iter().enumerate() {
                let Some(path) = wd.path.as_ref().filter(|_| states[i].glow > 0.01) else { continue };
                let c = faded(td.hl_color, states[i].glow * states[i].alpha);
                gp.stroke_path(path, &paint(c), &gs, word_tf(i), None);
                gp.fill_path(path, &paint(c), FillRule::Winding, word_tf(i), None);
            }
            blur_pixmap(&mut gp, (px * 0.14).max(2.0) as u32);
            for _ in 0..2 {
                pm.draw_pixmap(0, 0, gp.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
            }
        }
    }
    draw_mark(&mut pm, td, lay, states, mark_alpha, base, false);
    for (i, wd) in lay.words.iter().enumerate() {
        if !states.get(i).is_some_and(|s| s.alpha > 0.004) {
            continue;
        }
        let st = states[i];
        let tf = word_tf(i);
        if let Some(path) = &wd.path {
            if td.stroke {
                pm.stroke_path(path, &paint(faded(td.stroke_color, st.alpha)), &stroke, tf, None);
            }
            pm.fill_path(path, &paint(mix(td.color, td.hl_color, st.hl, st.alpha)), FillRule::Winding, tf, None);
            if st.sweep > 0.0 {
                let r = wd.rect;
                let x1 = r[0] + (r[2] - r[0]) * st.sweep;
                let pad = stroke_w + px * 0.1;
                if let (Some(mut mask), Some(clip)) =
                    (Mask::new(w, h), tiny_skia::Rect::from_ltrb(r[0] - pad, r[1] - px, x1, r[3] + px).map(PathBuilder::from_rect))
                {
                    mask.fill_path(&clip, FillRule::Winding, true, tf);
                    pm.fill_path(path, &paint(faded(td.hl_color, st.alpha)), FillRule::Winding, tf, Some(&mask));
                }
            }
        }
        for (x, y, iw, ih, rgba) in &wd.images {
            if let Some(src) = tiny_skia::PixmapRef::from_bytes(rgba, *iw, *ih) {
                let paint = PixmapPaint { opacity: st.alpha, ..Default::default() };
                pm.draw_pixmap(0, 0, src, &paint, Transform::from_translate(*x as f32, *y as f32).post_concat(tf), None);
            }
        }
    }
    draw_mark(&mut pm, td, lay, states, mark_alpha, base, true);
    RasterImg { w, h, data: pm.take(), uid: crate::util::new_id() }
}

/// Sliding box (under the words) or underline (over them): weighted position between the
/// previous and the current word.
fn draw_mark(pm: &mut Pixmap, td: &TextData, lay: &CapLayout, states: &[WordState], mark_alpha: f32, base: Transform, over: bool) {
    let underline = td.caption == "underline";
    if underline != over {
        return;
    }
    let px = lay.px;
    let wsum: f32 = states.iter().map(|s| s.mark).sum();
    if wsum <= 0.001 || mark_alpha <= 0.001 {
        return;
    }
    let top = states.iter().enumerate().max_by(|a, b| a.1.mark.total_cmp(&b.1.mark)).map(|x| x.0).unwrap_or(0);
    let same_line =
        states.iter().enumerate().filter(|(_, s)| s.mark > 0.001).all(|(i, _)| (lay.words[i].rect[1] - lay.words[top].rect[1]).abs() < 1.0);
    let (mut r, mut baseline) = ([0.0f32; 4], 0.0f32);
    if same_line {
        for (i, s) in states.iter().enumerate() {
            for k in 0..4 {
                r[k] += lay.words[i].rect[k] * s.mark / wsum;
            }
            baseline += lay.words[i].base * s.mark / wsum;
        }
    } else {
        r = lay.words[top].rect;
        baseline = lay.words[top].base;
    }
    let c = faded(td.hl_color, mark_alpha);
    let shape = if underline {
        let bh = px * 0.13;
        rounded_rect(r[0], baseline + px * 0.14, r[2] - r[0], bh, bh * 0.5)
    } else {
        let (mx, my) = (px * 0.14, px * 0.04);
        rounded_rect(r[0] - mx, r[1] + my, r[2] - r[0] + mx * 2.0, r[3] - r[1] - my * 2.0, px * 0.18)
    };
    if let Some(p) = shape {
        pm.fill_path(&p, &paint(c), FillRule::Winding, base, None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::SEC;

    #[test]
    fn karaoke_sweeps_the_spoken_word() {
        let times = [(0, SEC), (SEC, 2 * SEC)];
        let (s, _) = states("karaoke", &times, SEC + SEC / 2);
        assert_eq!(s[0].sweep, 1.0);
        assert!((s[1].sweep - 0.5).abs() < 1e-3);
    }

    #[test]
    fn one_word_at_a_time_shows_only_the_spoken_word() {
        let times = [(0, SEC), (SEC, 2 * SEC), (2 * SEC, 3 * SEC)];
        let (s, _) = states("single", &times, SEC + SEC / 2);
        assert_eq!(s.iter().filter(|w| w.alpha > 0.0).count(), 1);
        assert!(s[1].alpha > 0.0 && s[1].center);
    }

    #[test]
    fn words_follow_edited_text() {
        let mut td = TextData { text: "hello big world".into(), ..Default::default() };
        td.words = ["hello", "big", "world"]
            .iter()
            .enumerate()
            .map(|(i, w)| crate::model::Word { text: w.to_string(), start: i as Us * SEC, end: (i as Us + 1) * SEC })
            .collect();
        // a fixed word keeps its timing
        td.text = "hello bug world".into();
        let s = td.word_spans((0, 0));
        assert_eq!(s[1], (6, 9, SEC, 2 * SEC));
        // a different number of words is spread over the same time
        td.text = "hello world".into();
        let s = td.word_spans((0, 0));
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].2, 0);
        assert_eq!(s[1].3, 3 * SEC);
    }
}
