//! Tiny synthetic projects used to preview library items (transitions,
//! effects, filters, animations, text styles, stickers) on two built-in
//! sample pictures (animated library tiles).

use crate::engine::{Frame, FrameProvider, PixFmt, VideoReq};
use crate::model::catalog::{self, AnimKind};
use crate::model::*;
use crate::util::SEC;
use std::sync::Arc;

pub const A: &str = "builtin:sample-a";
pub const B: &str = "builtin:sample-b";

pub struct SampleFrames {
    a: Arc<Frame>,
    b: Arc<Frame>,
}

impl SampleFrames {
    pub fn new(w: u32, h: u32) -> Self {
        let mk = |k| {
            let img = crate::demo::sample_image(k, w, h);
            Arc::new(Frame { fmt: PixFmt::Rgba, w, h, data: img.into_raw(), uid: crate::util::new_id() })
        };
        SampleFrames { a: mk(0), b: mk(1) }
    }
    pub fn get(&self, path: &str) -> Option<Arc<Frame>> {
        match path {
            A => Some(self.a.clone()),
            B => Some(self.b.clone()),
            _ => None,
        }
    }
}

impl FrameProvider for SampleFrames {
    fn video(&mut self, _req: &VideoReq) -> Option<(Arc<Frame>, bool)> {
        None
    }
    fn image(&mut self, path: &str) -> Option<Arc<Frame>> {
        self.get(path)
    }
}

fn base(w: u32, h: u32) -> Project {
    let mut p = Project::new_named("preview");
    p.width = w;
    p.height = h;
    p.ratio = Ratio::R16x9;
    p.magnet = true;
    p.media.push(MediaAsset {
        id: 1,
        path: A.into(),
        name: "A".into(),
        kind: MediaKind::Image,
        width: 640,
        height: 360,
        has_video: true,
        ..Default::default()
    });
    p.media.push(MediaAsset {
        id: 2,
        path: B.into(),
        name: "B".into(),
        kind: MediaKind::Image,
        width: 640,
        height: 360,
        has_video: true,
        ..Default::default()
    });
    p
}

fn clip(media: u64, dur: i64) -> Clip {
    Clip::new(ClipKind::Media { media }, 0, dur)
}

/// A -> B with the transition; returns the transition region.
pub fn transition_project(id: &str) -> (Project, i64, i64) {
    let mut p = base(1280, 720);
    let mi = p.main_track_index();
    let def = catalog::transition(id);
    let dur = def.map(|d| d.dur).unwrap_or(SEC / 2).max(SEC / 2);
    let mut a = clip(1, SEC + dur);
    a.transition = Some(TransitionUse { id: id.into(), dur, blur: def.map(|d| d.blur).unwrap_or(0.5) });
    let b = clip(2, SEC + dur);
    p.tracks[mi].clips.push(a);
    p.tracks[mi].clips.push(b);
    p.normalize();
    let (_, from, to) = p.transition_regions()[0];
    (p, from, to)
}

pub fn effect_project(id: &str) -> Project {
    let mut p = base(1280, 720);
    let mi = p.main_track_index();
    p.tracks[mi].clips.push(clip(1, 4 * SEC));
    let def = catalog::effect(id);
    let mut params = [0.5; 4];
    if let Some(d) = def {
        for (i, pd) in d.params.iter().enumerate().take(4) {
            params[i] = pd.default;
        }
    }
    let fx = Clip::new(ClipKind::Effect(EffectUse { id: id.into(), params }), 0, 4 * SEC);
    p.add_clip(fx, 0);
    p
}

pub fn filter_project(id: &str) -> Project {
    let mut p = base(1280, 720);
    let mi = p.main_track_index();
    let mut c = clip(1, 4 * SEC);
    c.filter = Some(FilterUse { id: id.into(), intensity: 1.0 });
    p.tracks[mi].clips.push(c);
    p.normalize();
    p
}

/// Animation preview: an image card on a dark background.
pub fn anim_project(id: &str, kind: AnimKind) -> (Project, i64) {
    let mut p = base(1280, 720);
    p.bg_color = [24, 24, 28, 255];
    let def = catalog::animation(id);
    let dur = def.map(|d| d.dur).unwrap_or(SEC / 2);
    let total = (dur + SEC / 2).max(SEC);
    let mut c = if def.map(|d| d.text_only).unwrap_or(false) {
        Clip::new(ClipKind::Text(TextData { text: "Hello!".into(), size: 30.0, bold: true, ..Default::default() }), 0, total)
    } else {
        let mut c = clip(1, total);
        c.tf.scale = 0.62;
        c
    };
    let au = AnimUse { id: id.into(), dur };
    match kind {
        AnimKind::In => c.anim_in = Some(au),
        AnimKind::Out => c.anim_out = Some(au),
        AnimKind::Combo => c.anim_combo = Some(au),
    }
    let mi = p.main_track_index();
    if matches!(c.kind, ClipKind::Text(_)) {
        p.tracks[mi].clips.push(clip(1, total));
        p.add_clip(c, 0);
    } else {
        p.add_clip(c, 0);
    }
    p.normalize();
    (p, total)
}

pub fn text_project(preset: &catalog::TextPreset) -> Project {
    let mut p = base(1280, 720);
    p.bg_color = [40, 40, 46, 255];
    let td = text_from_preset(preset, "Text");
    let c = Clip::new(ClipKind::Text(TextData { size: 34.0, ..td }), 0, 2 * SEC);
    p.add_clip(c, 0);
    p
}

pub fn text_from_preset(preset: &catalog::TextPreset, text: &str) -> TextData {
    let mut td = TextData {
        text: text.into(),
        color: preset.color,
        bold: preset.bold,
        italic: preset.italic,
        preset: preset.id.into(),
        ..Default::default()
    };
    if let Some((c, w)) = preset.stroke {
        td.stroke = true;
        td.stroke_color = c;
        td.stroke_width = w;
    }
    if let Some(bg) = preset.bg {
        td.background = true;
        td.bg_color = bg;
    }
    td.shadow = preset.shadow;
    td
}

pub fn sticker_project(id: &str) -> Project {
    let mut p = base(1280, 720);
    p.bg_color = [40, 40, 46, 255];
    let col = catalog::sticker(id).map(|s| s.default_color).unwrap_or([255, 255, 255, 255]);
    let mut c = Clip::new(ClipKind::Sticker(StickerData { id: id.into(), color: col }), 0, 2 * SEC);
    c.tf.scale = 1.6;
    p.add_clip(c, 0);
    p
}
