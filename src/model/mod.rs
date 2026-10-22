//! Project data model.
//!
//! A project is a list of
//! materials (media) and a list of tracks made of segments (clips). Times are
//! integer microseconds, positions are canvas pixels relative to the centre
//! with Y pointing up, scale 1.0 means "fit to canvas", rotation is degrees.

pub mod catalog;
pub mod edit;
pub mod keyframe;

use crate::util::{self, Us, SEC};
use keyframe::{Keyframes, Prop};
use serde::{Deserialize, Serialize};

/// 2: transitions are centred on the cut (version 1 overlapped the two clips).
pub const PROJECT_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Ratio {
    Original,
    R16x9,
    R9x16,
    R1x1,
    R4x3,
    R3x4,
    R4x5,
    R21x9,
    R235x1,
    R2x1,
    R185x1,
    /// Any width × height typed by the user.
    Custom,
}

impl Ratio {
    pub const ALL: [Ratio; 11] = [
        Ratio::Original,
        Ratio::R16x9,
        Ratio::R9x16,
        Ratio::R1x1,
        Ratio::R4x3,
        Ratio::R3x4,
        Ratio::R4x5,
        Ratio::R21x9,
        Ratio::R235x1,
        Ratio::R2x1,
        Ratio::R185x1,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Ratio::Original => "Original",
            Ratio::R16x9 => "16:9",
            Ratio::R9x16 => "9:16",
            Ratio::R1x1 => "1:1",
            Ratio::R4x3 => "4:3",
            Ratio::R3x4 => "3:4",
            Ratio::R4x5 => "4:5",
            Ratio::R21x9 => "21:9",
            Ratio::R235x1 => "2.35:1",
            Ratio::R2x1 => "2:1",
            Ratio::R185x1 => "1.85:1",
            Ratio::Custom => "Custom",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Ratio::R16x9 => "YouTube",
            Ratio::R9x16 => "TikTok / Reels / Shorts",
            Ratio::R1x1 => "Instagram",
            Ratio::R4x5 => "Instagram feed",
            Ratio::R21x9 | Ratio::R235x1 => "Cinema",
            _ => "",
        }
    }
    pub fn aspect(self) -> Option<f64> {
        Some(match self {
            Ratio::Original | Ratio::Custom => return None,
            Ratio::R16x9 => 16.0 / 9.0,
            Ratio::R9x16 => 9.0 / 16.0,
            Ratio::R1x1 => 1.0,
            Ratio::R4x3 => 4.0 / 3.0,
            Ratio::R3x4 => 3.0 / 4.0,
            Ratio::R4x5 => 4.0 / 5.0,
            Ratio::R21x9 => 21.0 / 9.0,
            Ratio::R235x1 => 2.35,
            Ratio::R2x1 => 2.0,
            Ratio::R185x1 => 1.85,
        })
    }
}

/// Canvas size for an aspect with the short side at `short` pixels (even numbers).
pub fn canvas_for_aspect(aspect: f64, short: u32) -> (u32, u32) {
    let s = short as f64;
    let (w, h) = if aspect >= 1.0 { (s * aspect, s) } else { (s, s / aspect) };
    let even = |v: f64| (((v / 2.0).round() as u32) * 2).max(2);
    (even(w), even(h))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    Video,
    Image,
    Audio,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaAsset {
    pub id: u64,
    pub path: String,
    pub name: String,
    pub kind: MediaKind,
    pub duration: Us,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub has_video: bool,
    pub has_audio: bool,
    /// Path of a reversed proxy (generated on demand by "Reverse").
    pub reversed: Option<String>,
    /// Built-in generated asset (sound effects etc.)
    pub builtin: bool,
    /// Clockwise rotation from the file's display matrix (0/90/180/270), already applied to width/height.
    pub rotation: i32,
    /// Video codec name (ffprobe), used to decide on proxies.
    pub codec: String,
}

impl Default for MediaAsset {
    fn default() -> Self {
        Self {
            id: 0,
            path: String::new(),
            name: String::new(),
            kind: MediaKind::Video,
            duration: 0,
            width: 0,
            height: 0,
            fps: 30.0,
            has_video: false,
            has_audio: false,
            reversed: None,
            builtin: false,
            rotation: 0,
            codec: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum TrackKind {
    Video,
    Audio,
    Text,
    Sticker,
    Effect,
    Filter,
    Adjust,
}

impl TrackKind {
    pub fn is_visual_layer(self) -> bool {
        matches!(self, TrackKind::Video | TrackKind::Text | TrackKind::Sticker)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    pub id: u64,
    pub kind: TrackKind,
    pub main: bool,
    pub clips: Vec<Clip>,
    pub hidden: bool,
    pub muted: bool,
    pub locked: bool,
}

impl Default for Track {
    fn default() -> Self {
        Self { id: 0, kind: TrackKind::Video, main: false, clips: vec![], hidden: false, muted: false, locked: false }
    }
}

impl Track {
    pub fn new(kind: TrackKind) -> Self {
        Self { id: util::new_id(), kind, ..Default::default() }
    }
    pub fn end(&self) -> Us {
        self.clips.iter().map(|c| c.end()).max().unwrap_or(0)
    }
    pub fn sort(&mut self) {
        self.clips.sort_by_key(|c| (c.start, c.id));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Transform {
    /// Canvas pixels from centre, +x right.
    pub x: f64,
    /// Canvas pixels from centre, +y up.
    pub y: f64,
    /// 1.0 = fit inside canvas.
    pub scale: f64,
    pub scale_x: f64,
    pub scale_y: f64,
    pub uniform: bool,
    /// Degrees, clockwise.
    pub rotation: f64,
    pub opacity: f64,
    pub flip_h: bool,
    pub flip_v: bool,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            scale: 1.0,
            scale_x: 1.0,
            scale_y: 1.0,
            uniform: true,
            rotation: 0.0,
            opacity: 1.0,
            flip_h: false,
            flip_v: false,
        }
    }
}

/// "Adjust" panel. Values are slider/100 (most are -1..1, some 0..1).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Adjust {
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
    pub exposure: f64,
    pub temperature: f64,
    pub tint: f64,
    pub highlights: f64,
    pub shadows: f64,
    pub whites: f64,
    pub blacks: f64,
    pub sharpen: f64,
    pub clarity: f64,
    pub fade: f64,
    pub vignette: f64,
    pub grain: f64,
    pub hue: f64,
}

impl Default for Adjust {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            exposure: 0.0,
            temperature: 0.0,
            tint: 0.0,
            highlights: 0.0,
            shadows: 0.0,
            whites: 0.0,
            blacks: 0.0,
            sharpen: 0.0,
            clarity: 0.0,
            fade: 0.0,
            vignette: 0.0,
            grain: 0.0,
            hue: 0.0,
        }
    }
}

impl Adjust {
    pub fn get(&self, p: Prop) -> Option<f64> {
        Some(match p {
            Prop::Brightness => self.brightness,
            Prop::Contrast => self.contrast,
            Prop::Saturation => self.saturation,
            Prop::Exposure => self.exposure,
            Prop::Temperature => self.temperature,
            Prop::Tint => self.tint,
            Prop::Highlights => self.highlights,
            Prop::Shadows => self.shadows,
            Prop::Whites => self.whites,
            Prop::Blacks => self.blacks,
            Prop::Sharpen => self.sharpen,
            Prop::Clarity => self.clarity,
            Prop::Fade => self.fade,
            Prop::Vignette => self.vignette,
            Prop::Grain => self.grain,
            Prop::Hue => self.hue,
            _ => return None,
        })
    }
    pub fn set(&mut self, p: Prop, v: f64) {
        match p {
            Prop::Brightness => self.brightness = v,
            Prop::Contrast => self.contrast = v,
            Prop::Saturation => self.saturation = v,
            Prop::Exposure => self.exposure = v,
            Prop::Temperature => self.temperature = v,
            Prop::Tint => self.tint = v,
            Prop::Highlights => self.highlights = v,
            Prop::Shadows => self.shadows = v,
            Prop::Whites => self.whites = v,
            Prop::Blacks => self.blacks = v,
            Prop::Sharpen => self.sharpen = v,
            Prop::Clarity => self.clarity = v,
            Prop::Fade => self.fade = v,
            Prop::Vignette => self.vignette = v,
            Prop::Grain => self.grain = v,
            Prop::Hue => self.hue = v,
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Blend {
    #[default]
    Normal,
    Darken,
    Multiply,
    LinearBurn,
    ColorBurn,
    Lighten,
    Screen,
    Add,
    ColorDodge,
    Overlay,
    SoftLight,
    HardLight,
    Difference,
    Exclusion,
}

impl Blend {
    pub const ALL: [Blend; 14] = [
        Blend::Normal,
        Blend::Darken,
        Blend::Multiply,
        Blend::LinearBurn,
        Blend::ColorBurn,
        Blend::Lighten,
        Blend::Screen,
        Blend::Add,
        Blend::ColorDodge,
        Blend::Overlay,
        Blend::SoftLight,
        Blend::HardLight,
        Blend::Difference,
        Blend::Exclusion,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Blend::Normal => "Normal",
            Blend::Darken => "Darken",
            Blend::Multiply => "Multiply",
            Blend::LinearBurn => "Linear burn",
            Blend::ColorBurn => "Color burn",
            Blend::Lighten => "Lighten",
            Blend::Screen => "Screen",
            Blend::Add => "Add",
            Blend::ColorDodge => "Color dodge",
            Blend::Overlay => "Overlay",
            Blend::SoftLight => "Soft light",
            Blend::HardLight => "Hard light",
            Blend::Difference => "Difference",
            Blend::Exclusion => "Exclusion",
        }
    }
    pub fn index(self) -> u32 {
        Blend::ALL.iter().position(|b| *b == self).unwrap_or(0) as u32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MaskKind {
    #[default]
    Linear,
    Mirror,
    Circle,
    Rectangle,
    Heart,
    Star,
}

impl MaskKind {
    pub const ALL: [MaskKind; 6] =
        [MaskKind::Linear, MaskKind::Mirror, MaskKind::Circle, MaskKind::Rectangle, MaskKind::Heart, MaskKind::Star];
    pub fn index(self) -> u32 {
        MaskKind::ALL.iter().position(|b| *b == self).unwrap_or(0) as u32 + 1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Mask {
    pub kind: MaskKind,
    /// Centre offset in fractions of the clip size (-0.5..0.5), +y up.
    pub cx: f64,
    pub cy: f64,
    /// Size in fractions of the clip size.
    pub w: f64,
    pub h: f64,
    pub rotation: f64,
    /// 0..1
    pub feather: f64,
    /// 0..1 (rectangle corner roundness)
    pub round: f64,
    pub invert: bool,
}

impl Default for Mask {
    fn default() -> Self {
        Self { kind: MaskKind::Circle, cx: 0.0, cy: 0.0, w: 0.5, h: 0.5, rotation: 0.0, feather: 0.05, round: 0.0, invert: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChromaKey {
    pub enabled: bool,
    pub color: [u8; 3],
    /// 0..1
    pub intensity: f64,
    /// 0..1 edge darkening / spill
    pub shadow: f64,
}

impl Default for ChromaKey {
    fn default() -> Self {
        Self { enabled: false, color: [0, 255, 0], intensity: 0.35, shadow: 0.2 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Crop {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Default for Crop {
    fn default() -> Self {
        Self { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 }
    }
}

impl Crop {
    pub fn is_identity(&self) -> bool {
        self.left == 0.0 && self.top == 0.0 && self.right == 0.0 && self.bottom == 0.0
    }
}

/// Main-track background fill ("Canvas").
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum CanvasFill {
    #[default]
    None,
    /// Blurred, scaled copy of the clip behind it. 0..1 blur level.
    Blur(f64),
    Color([u8; 4]),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AnimUse {
    pub id: String,
    pub dur: Us,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TransitionUse {
    pub id: String,
    pub dur: Us,
    /// Motion blur strength 0..1 for transform-based transitions.
    pub blur: f64,
}

impl Default for TransitionUse {
    fn default() -> Self {
        Self { id: "mix".into(), dur: SEC / 2, blur: 0.6 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterUse {
    pub id: String,
    /// 0..1
    pub intensity: f64,
}

impl Default for FilterUse {
    fn default() -> Self {
        Self { id: String::new(), intensity: 1.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EffectUse {
    pub id: String,
    /// Generic parameters: [amount, speed, extra1, extra2], meaning defined by the catalog.
    pub params: [f64; 4],
}

impl Default for EffectUse {
    fn default() -> Self {
        Self { id: String::new(), params: [0.5, 0.5, 0.5, 0.5] }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextData {
    pub text: String,
    pub font: String,
    /// Size in text units (default 15).
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: [u8; 4],
    /// 0 left, 1 centre, 2 right
    pub align: u8,
    pub letter_spacing: f64,
    pub line_spacing: f64,
    pub stroke: bool,
    pub stroke_color: [u8; 4],
    /// 0..1 relative width
    pub stroke_width: f64,
    pub background: bool,
    pub bg_color: [u8; 4],
    pub bg_round: f64,
    pub shadow: bool,
    pub shadow_color: [u8; 4],
    pub shadow_blur: f64,
    pub shadow_distance: f64,
    pub shadow_angle: f64,
    /// Preset id for the text library (informational).
    pub preset: String,
    /// Animated caption style (`catalog::CAPTION_STYLES`); empty = plain text.
    pub caption: String,
    /// Colour of the spoken word for animated captions.
    pub hl_color: [u8; 4],
    /// Word timings of an auto caption (see `Word`); empty for typed text.
    pub words: Vec<Word>,
}

/// One spoken word of a caption. Times are caption-local: 0 is the moment the caption
/// started when it was made, so `src_in` (moved by trims and splits) keeps them in sync.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub text: String,
    pub start: Us,
    pub end: Us,
}

impl TextData {
    pub fn is_caption(&self) -> bool {
        !self.caption.is_empty() || !self.words.is_empty()
    }

    /// The words of the text (split on whitespace) as (byte range, start, end) in caption-local
    /// time. Timings come from `words` while the text still has the same number of words (so
    /// fixing a misheard word keeps them); otherwise they are spread over `span` by length.
    pub fn word_spans(&self, span: (Us, Us)) -> Vec<(usize, usize, Us, Us)> {
        let mut ranges = vec![];
        let mut start = None;
        for (i, ch) in self.text.char_indices() {
            if ch.is_whitespace() {
                if let Some(s) = start.take() {
                    ranges.push((s, i));
                }
            } else if start.is_none() {
                start = Some(i);
            }
        }
        if let Some(s) = start {
            ranges.push((s, self.text.len()));
        }
        if ranges.len() == self.words.len() {
            return ranges.iter().zip(&self.words).map(|(r, w)| (r.0, r.1, w.start, w.end)).collect();
        }
        let (a, b) = match (self.words.first(), self.words.last()) {
            (Some(f), Some(l)) => (f.start, l.end),
            _ => span,
        };
        let total: usize = ranges.iter().map(|r| self.text[r.0..r.1].chars().count() + 1).sum();
        let mut acc = 0usize;
        ranges
            .iter()
            .map(|r| {
                let n = self.text[r.0..r.1].chars().count() + 1;
                let s = a + ((b - a) as f64 * acc as f64 / total.max(1) as f64) as Us;
                acc += n;
                let e = a + ((b - a) as f64 * acc as f64 / total.max(1) as f64) as Us;
                (r.0, r.1, s, e)
            })
            .collect()
    }
}

impl Default for TextData {
    fn default() -> Self {
        Self {
            text: "Default text".into(),
            font: String::new(),
            size: 15.0,
            bold: false,
            italic: false,
            underline: false,
            color: [255, 255, 255, 255],
            align: 1,
            letter_spacing: 0.0,
            line_spacing: 0.0,
            stroke: false,
            stroke_color: [0, 0, 0, 255],
            stroke_width: 0.4,
            background: false,
            bg_color: [0, 0, 0, 160],
            bg_round: 0.3,
            shadow: false,
            shadow_color: [0, 0, 0, 200],
            shadow_blur: 0.3,
            shadow_distance: 0.3,
            shadow_angle: -45.0,
            preset: String::new(),
            caption: String::new(),
            hl_color: [255, 221, 0, 255],
            words: vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StickerData {
    pub id: String,
    pub color: [u8; 4],
}

impl Default for StickerData {
    fn default() -> Self {
        Self { id: "heart".into(), color: [255, 64, 96, 255] }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClipKind {
    Media { media: u64 },
    Text(TextData),
    Sticker(StickerData),
    Effect(EffectUse),
    Filter(FilterUse),
    Adjust,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoiceFx {
    pub id: String,
}

impl Default for VoiceFx {
    fn default() -> Self {
        Self { id: String::new() }
    }
}

/// A speed curve: list of (position 0..1 in clip, speed multiplier) points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpeedCurve {
    pub name: String,
    pub points: Vec<(f64, f64)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clip {
    pub id: u64,
    /// Timeline start.
    pub start: Us,
    /// Timeline duration.
    pub duration: Us,
    pub kind: ClipKind,
    /// Source in-point (source time, before speed).
    pub src_in: Us,
    pub speed: f64,
    pub curve: Option<SpeedCurve>,
    pub reverse: bool,
    /// "Change pitch" toggle (off = keep pitch when speeding up).
    pub change_pitch: bool,
    /// Freeze-frame clip: shows this source time for its whole duration.
    pub freeze: Option<Us>,
    pub tf: Transform,
    pub blend: Blend,
    pub adjust: Adjust,
    pub filter: Option<FilterUse>,
    pub anim_in: Option<AnimUse>,
    pub anim_out: Option<AnimUse>,
    pub anim_combo: Option<AnimUse>,
    /// Transition into the next clip on the same track.
    pub transition: Option<TransitionUse>,
    pub mask: Option<Mask>,
    pub chroma: ChromaKey,
    pub crop: Crop,
    pub canvas: CanvasFill,
    pub kf: Keyframes,
    /// Motion blur applied to keyframed / animated movement (0..1).
    pub motion_blur: f64,
    /// Linear gain (1.0 = 0 dB).
    pub volume: f64,
    pub fade_in: Us,
    pub fade_out: Us,
    pub voice: Option<VoiceFx>,
    /// Noise reduction strength 0..1 (0 = off).
    pub denoise: f64,
    /// Audio was separated into its own clip ("Extract audio").
    pub audio_detached: bool,
    pub disabled: bool,
    pub group: u64,
    pub link: u64,
    pub label: String,
}

impl Default for Clip {
    fn default() -> Self {
        Self {
            id: 0,
            start: 0,
            duration: 3 * SEC,
            kind: ClipKind::Adjust,
            src_in: 0,
            speed: 1.0,
            curve: None,
            reverse: false,
            change_pitch: false,
            freeze: None,
            tf: Transform::default(),
            blend: Blend::Normal,
            adjust: Adjust::default(),
            filter: None,
            anim_in: None,
            anim_out: None,
            anim_combo: None,
            transition: None,
            mask: None,
            chroma: ChromaKey::default(),
            crop: Crop::default(),
            canvas: CanvasFill::None,
            kf: Keyframes::default(),
            motion_blur: 0.0,
            volume: 1.0,
            fade_in: 0,
            fade_out: 0,
            voice: None,
            denoise: 0.0,
            audio_detached: false,
            disabled: false,
            group: 0,
            link: 0,
            label: String::new(),
        }
    }
}

impl Clip {
    pub fn new(kind: ClipKind, start: Us, duration: Us) -> Self {
        Self { id: util::new_id(), start, duration, kind, ..Default::default() }
    }
    #[inline]
    pub fn end(&self) -> Us {
        self.start + self.duration
    }
    pub fn media_id(&self) -> Option<u64> {
        match self.kind {
            ClipKind::Media { media } => Some(media),
            _ => None,
        }
    }
    pub fn contains(&self, t: Us) -> bool {
        t >= self.start && t < self.end()
    }
    /// Source duration consumed by this clip (constant speed).
    pub fn src_len(&self) -> Us {
        if self.freeze.is_some() {
            return 0;
        }
        match &self.curve {
            Some(c) => (self.duration as f64 * curve_mean_speed(c)).round() as Us,
            None => (self.duration as f64 * self.speed).round() as Us,
        }
    }
    /// Map a timeline time to the source time (microseconds, unreversed source).
    pub fn source_time(&self, t: Us, media_dur: Us) -> Us {
        if let Some(f) = self.freeze {
            return f;
        }
        let local = (t - self.start).clamp(0, self.duration);
        let consumed = match &self.curve {
            Some(c) => {
                let p = local as f64 / self.duration.max(1) as f64;
                (curve_integral(c, p) * self.duration as f64).round() as Us
            }
            None => (local as f64 * self.speed).round() as Us,
        };
        let s = if self.reverse { self.src_in + self.src_len() - consumed } else { self.src_in + consumed };
        if media_dur > 0 {
            s.clamp(0, media_dur)
        } else {
            s.max(0)
        }
    }
    /// Like `source_time`, but continues past the clip edges (used by transitions, which
    /// show a little of the media before the in-point and after the out-point; the result
    /// is clamped to the media, which holds the first/last frame when there is no handle).
    pub fn source_time_ext(&self, t: Us, media_dur: Us) -> Us {
        if let Some(f) = self.freeze {
            return f;
        }
        let local = t - self.start;
        let edge = local.clamp(0, self.duration);
        let base = self.source_time(self.start + edge, media_dur);
        let extra = local - edge;
        if extra == 0 {
            return base;
        }
        let speed = self.speed_at(self.start + edge).max(0.01);
        let dir = if self.reverse { -1.0 } else { 1.0 };
        let s = base + (extra as f64 * speed * dir).round() as Us;
        if media_dur > 0 {
            s.clamp(0, media_dur)
        } else {
            s.max(0)
        }
    }
    /// Effective speed at timeline time t (for decode-rate decisions).
    pub fn speed_at(&self, t: Us) -> f64 {
        if self.freeze.is_some() {
            return 0.0;
        }
        match &self.curve {
            Some(c) => curve_speed(c, ((t - self.start) as f64 / self.duration.max(1) as f64).clamp(0.0, 1.0)),
            None => self.speed,
        }
    }
    pub fn display_name(&self, project: &Project) -> String {
        if !self.label.is_empty() {
            return self.label.clone();
        }
        match &self.kind {
            ClipKind::Media { media } => project.media(*media).map(|m| m.name.clone()).unwrap_or_else(|| "Media".into()),
            ClipKind::Text(t) => t.text.lines().next().unwrap_or("").to_string(),
            ClipKind::Sticker(s) => catalog::sticker_name(&s.id).to_string(),
            ClipKind::Effect(e) => catalog::effect(&e.id).map(|d| d.name.to_string()).unwrap_or_else(|| e.id.clone()),
            ClipKind::Filter(f) => catalog::filter(&f.id).map(|d| d.name.to_string()).unwrap_or_else(|| f.id.clone()),
            ClipKind::Adjust => "Adjustment".into(),
        }
    }
    pub fn is_visual(&self) -> bool {
        matches!(self.kind, ClipKind::Media { .. } | ClipKind::Text(_) | ClipKind::Sticker(_))
    }
    /// Value of a keyframable property at timeline time `t`.
    pub fn prop(&self, p: Prop, t: Us) -> f64 {
        let base = self.base_prop(p);
        self.kf.value(p, t - self.start, base)
    }
    pub fn base_prop(&self, p: Prop) -> f64 {
        match p {
            Prop::PosX => self.tf.x,
            Prop::PosY => self.tf.y,
            Prop::Scale => self.tf.scale,
            Prop::ScaleX => self.tf.scale_x,
            Prop::ScaleY => self.tf.scale_y,
            Prop::Rotation => self.tf.rotation,
            Prop::Opacity => self.tf.opacity,
            Prop::Volume => self.volume,
            Prop::FilterIntensity => self.filter.as_ref().map(|f| f.intensity).unwrap_or(1.0),
            Prop::EffectAmount => match &self.kind {
                ClipKind::Effect(e) => e.params[0],
                ClipKind::Filter(f) => f.intensity,
                _ => 0.0,
            },
            Prop::MaskX => self.mask.map(|m| m.cx).unwrap_or(0.0),
            Prop::MaskY => self.mask.map(|m| m.cy).unwrap_or(0.0),
            Prop::MaskW => self.mask.map(|m| m.w).unwrap_or(0.5),
            Prop::MaskH => self.mask.map(|m| m.h).unwrap_or(0.5),
            Prop::MaskRotation => self.mask.map(|m| m.rotation).unwrap_or(0.0),
            Prop::MaskFeather => self.mask.map(|m| m.feather).unwrap_or(0.0),
            p => self.adjust.get(p).unwrap_or(0.0),
        }
    }
    pub fn set_base_prop(&mut self, p: Prop, v: f64) {
        match p {
            Prop::PosX => self.tf.x = v,
            Prop::PosY => self.tf.y = v,
            Prop::Scale => self.tf.scale = v,
            Prop::ScaleX => self.tf.scale_x = v,
            Prop::ScaleY => self.tf.scale_y = v,
            Prop::Rotation => self.tf.rotation = v,
            Prop::Opacity => self.tf.opacity = v,
            Prop::Volume => self.volume = v,
            Prop::FilterIntensity => {
                if let Some(f) = self.filter.as_mut() {
                    f.intensity = v
                }
            }
            Prop::EffectAmount => match &mut self.kind {
                ClipKind::Effect(e) => e.params[0] = v,
                ClipKind::Filter(f) => f.intensity = v,
                _ => {}
            },
            Prop::MaskX => {
                if let Some(m) = self.mask.as_mut() {
                    m.cx = v
                }
            }
            Prop::MaskY => {
                if let Some(m) = self.mask.as_mut() {
                    m.cy = v
                }
            }
            Prop::MaskW => {
                if let Some(m) = self.mask.as_mut() {
                    m.w = v
                }
            }
            Prop::MaskH => {
                if let Some(m) = self.mask.as_mut() {
                    m.h = v
                }
            }
            Prop::MaskRotation => {
                if let Some(m) = self.mask.as_mut() {
                    m.rotation = v
                }
            }
            Prop::MaskFeather => {
                if let Some(m) = self.mask.as_mut() {
                    m.feather = v
                }
            }
            p => self.adjust.set(p, v),
        }
    }
    /// Set a property: if the property is keyframed, add/update a
    /// keyframe at `t`; otherwise change the static value.
    pub fn set_prop_at(&mut self, p: Prop, t: Us, v: f64) {
        if self.kf.has(p) {
            let local = (t - self.start).clamp(0, self.duration);
            self.kf.set(p, local, v);
        } else {
            self.set_base_prop(p, v);
        }
    }
    /// Adjust values (with keyframes applied) at time t.
    pub fn adjust_at(&self, t: Us) -> Adjust {
        let mut a = self.adjust;
        for p in Prop::ADJUST {
            if self.kf.has(p) {
                a.set(p, self.prop(p, t));
            }
        }
        a
    }
    pub fn has_audio(&self, project: &Project) -> bool {
        match self.kind {
            ClipKind::Media { media } => project.media(media).map(|m| m.has_audio).unwrap_or(false) && !self.audio_detached,
            _ => false,
        }
    }
}

pub fn curve_speed(c: &SpeedCurve, p: f64) -> f64 {
    let pts = &c.points;
    if pts.is_empty() {
        return 1.0;
    }
    if p <= pts[0].0 {
        return pts[0].1;
    }
    for w in pts.windows(2) {
        if p <= w[1].0 {
            let f = (p - w[0].0) / (w[1].0 - w[0].0).max(1e-9);
            let f = f * f * (3.0 - 2.0 * f);
            return w[0].1 + (w[1].1 - w[0].1) * f;
        }
    }
    pts[pts.len() - 1].1
}

/// Integral of the speed curve over [0, p] (in clip-duration units).
pub fn curve_integral(c: &SpeedCurve, p: f64) -> f64 {
    let n = 64;
    let mut acc = 0.0;
    let step = p / n as f64;
    for i in 0..n {
        let x = (i as f64 + 0.5) * step;
        acc += curve_speed(c, x) * step;
    }
    acc
}

pub fn curve_mean_speed(c: &SpeedCurve) -> f64 {
    curve_integral(c, 1.0)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Project {
    /// Format version (0 when missing: very old files).
    #[serde(default)]
    pub version: u32,
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub ratio: Ratio,
    pub fps: f64,
    pub bg_color: [u8; 4],
    pub media: Vec<MediaAsset>,
    pub tracks: Vec<Track>,
    /// Main track magnet (P)
    pub magnet: bool,
    /// Auto snapping (N)
    pub snapping: bool,
    /// Audio/video linkage (~)
    pub linkage: bool,
    /// Preview axis (S): scrubbing preview follows the mouse
    pub preview_axis: bool,
    pub markers: Vec<Us>,
    pub created: i64,
    pub modified: i64,
    pub cover_time: Us,
}

impl Default for Project {
    fn default() -> Self {
        let mut main = Track::new(TrackKind::Video);
        main.main = true;
        Self {
            version: PROJECT_VERSION,
            id: format!("{:x}", util::new_id()),
            name: "Untitled".into(),
            width: 1920,
            height: 1080,
            ratio: Ratio::Original,
            fps: 30.0,
            bg_color: [0, 0, 0, 255],
            media: vec![],
            tracks: vec![main],
            magnet: true,
            snapping: true,
            linkage: true,
            preview_axis: false,
            markers: vec![],
            created: util::now_ms(),
            modified: util::now_ms(),
            cover_time: 0,
        }
    }
}

impl Project {
    pub fn new_named(name: &str) -> Self {
        Self { name: name.to_string(), ..Default::default() }
    }
    pub fn media(&self, id: u64) -> Option<&MediaAsset> {
        self.media.iter().find(|m| m.id == id)
    }
    pub fn media_mut(&mut self, id: u64) -> Option<&mut MediaAsset> {
        self.media.iter_mut().find(|m| m.id == id)
    }
    pub fn duration(&self) -> Us {
        self.tracks.iter().map(|t| t.end()).max().unwrap_or(0)
    }
    pub fn main_track_index(&self) -> usize {
        match self.tracks.iter().position(|t| t.main) {
            Some(i) => i,
            None => 0,
        }
    }
    pub fn ensure_main_track(&mut self) {
        if !self.tracks.iter().any(|t| t.main) {
            let mut main = Track::new(TrackKind::Video);
            main.main = true;
            // Put it after the last non-audio track.
            let pos = self.tracks.iter().position(|t| t.kind == TrackKind::Audio).unwrap_or(self.tracks.len());
            self.tracks.insert(pos, main);
        }
    }
    pub fn frame_dur(&self) -> Us {
        util::frame_us(self.fps)
    }
    /// Transitions on the main track: (left clip index, start, end) of each transition region.
    /// A transition is centred on the cut between two touching clips.
    pub fn transition_regions(&self) -> Vec<(usize, Us, Us)> {
        let tr = &self.tracks[self.main_track_index()];
        let mut v = vec![];
        for (i, w) in tr.clips.windows(2).enumerate() {
            if let Some(t) = &w[0].transition {
                let cut = w[0].end();
                let half = t.dur / 2;
                v.push((i, cut - half, cut - half + t.dur));
            }
        }
        v
    }
    pub fn find_clip(&self, id: u64) -> Option<(usize, usize)> {
        for (ti, t) in self.tracks.iter().enumerate() {
            if let Some(ci) = t.clips.iter().position(|c| c.id == id) {
                return Some((ti, ci));
            }
        }
        None
    }
    pub fn clip(&self, id: u64) -> Option<&Clip> {
        self.find_clip(id).map(|(t, c)| &self.tracks[t].clips[c])
    }
    pub fn clip_mut(&mut self, id: u64) -> Option<&mut Clip> {
        match self.find_clip(id) {
            Some((t, c)) => Some(&mut self.tracks[t].clips[c]),
            None => None,
        }
    }
    pub fn track_of(&self, clip_id: u64) -> Option<&Track> {
        self.find_clip(clip_id).map(|(t, _)| &self.tracks[t])
    }
    pub fn max_id(&self) -> u64 {
        let mut m = 0;
        for md in &self.media {
            m = m.max(md.id);
        }
        for t in &self.tracks {
            m = m.max(t.id);
            for c in &t.clips {
                m = m.max(c.id).max(c.group).max(c.link);
            }
        }
        m
    }
    /// Media duration helper (0 for stills).
    pub fn media_dur(&self, clip: &Clip) -> Us {
        clip.media_id().and_then(|m| self.media(m)).map(|m| m.duration).unwrap_or(0)
    }
    /// The canvas size for the current ratio given the first main-track clip.
    /// Change the aspect ratio, keeping the resolution (the short side) of the canvas.
    pub fn apply_ratio(&mut self, ratio: Ratio) {
        self.ratio = ratio;
        if ratio == Ratio::Custom {
            return;
        }
        let short = self.short_side();
        let aspect = match ratio.aspect() {
            Some(a) => a,
            None => {
                let main = &self.tracks[self.main_track_index()];
                let first = main.clips.iter().filter_map(|c| c.media_id()).filter_map(|m| self.media(m)).find(|m| m.width > 0);
                match first {
                    Some(m) => m.width as f64 / m.height.max(1) as f64,
                    None => 16.0 / 9.0,
                }
            }
        };
        let (w, h) = canvas_for_aspect(aspect, short);
        self.width = w;
        self.height = h;
    }

    /// Resolution as the length of the short side (1080 for 1920×1080 and 1080×1920).
    pub fn short_side(&self) -> u32 {
        self.width.min(self.height).max(2)
    }

    /// Change the resolution, keeping the aspect ratio.
    pub fn set_short_side(&mut self, short: u32) {
        let aspect = self.width.max(1) as f64 / self.height.max(1) as f64;
        let (w, h) = canvas_for_aspect(aspect, short.clamp(16, 8192));
        self.width = w;
        self.height = h;
    }

    /// Any canvas size (even numbers, 16..8192).
    pub fn set_custom_size(&mut self, w: u32, h: u32) {
        let even = |v: u32| (v.clamp(16, 8192) / 2) * 2;
        self.ratio = Ratio::Custom;
        self.width = even(w);
        self.height = even(h);
    }
    pub fn save(&mut self, path: &std::path::Path) -> anyhow::Result<()> {
        self.modified = util::now_ms();
        let json = serde_json::to_string_pretty(self)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
    pub fn load(path: &std::path::Path) -> anyhow::Result<Project> {
        let s = std::fs::read_to_string(path)?;
        let mut p: Project = serde_json::from_str(&s)?;
        p.ensure_main_track();
        util::bump_ids(p.max_id());
        p.migrate();
        Ok(p)
    }

    /// Bring an older project up to the current format.
    pub fn migrate(&mut self) {
        if self.version < 2 {
            self.split_old_overlaps();
        }
        self.version = PROJECT_VERSION;
    }

    /// Version 1 overlapped the two clips of a transition (the second one started a transition
    /// earlier, so every transition shortened the video). Now clips meet at a cut and the
    /// transition is centred on it: cut each old overlap in the middle, which keeps the picture,
    /// the length and every other track exactly where they were.
    fn split_old_overlaps(&mut self) {
        let mi = self.main_track_index();
        let clips = &mut self.tracks[mi].clips;
        clips.sort_by_key(|c| c.start);
        for i in 0..clips.len().saturating_sub(1) {
            let ov = clips[i].end() - clips[i + 1].start;
            if ov <= 0 {
                continue;
            }
            let (ta, tb) = (ov / 2, ov - ov / 2);
            if clips[i].duration - ta < 1000 || clips[i + 1].duration - tb < 1000 {
                continue;
            }
            // first clip: its tail becomes the transition handle
            let a = &mut clips[i];
            let snapshot = a.clone();
            a.kf.trim_end(a.duration - ta, &move |p: Prop| snapshot.base_prop(p));
            if a.freeze.is_none() && a.reverse {
                a.src_in += (ta as f64 * a.speed).round() as Us;
            }
            a.duration -= ta;
            // second clip: its head becomes the handle
            let b = &mut clips[i + 1];
            let snapshot = b.clone();
            b.kf.trim_start(tb, &move |p: Prop| snapshot.base_prop(p));
            if b.freeze.is_none() && !b.reverse {
                b.src_in += (tb as f64 * b.speed).round() as Us;
            }
            b.start += tb;
            b.duration -= tb;
        }
    }
}
