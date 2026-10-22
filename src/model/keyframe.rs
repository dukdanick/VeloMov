//! Keyframes: per-property lists of (time, value, easing), the
//! `common_keyframes` (KFTypePositionX, KFTypeScale, ...).

use crate::util::Us;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Prop {
    PosX,
    PosY,
    Scale,
    ScaleX,
    ScaleY,
    Rotation,
    Opacity,
    Volume,
    Brightness,
    Contrast,
    Saturation,
    Exposure,
    Temperature,
    Tint,
    Highlights,
    Shadows,
    Whites,
    Blacks,
    Sharpen,
    Clarity,
    Fade,
    Vignette,
    Grain,
    Hue,
    FilterIntensity,
    EffectAmount,
    MaskX,
    MaskY,
    MaskW,
    MaskH,
    MaskRotation,
    MaskFeather,
}

impl Prop {
    pub const TRANSFORM: [Prop; 7] = [Prop::PosX, Prop::PosY, Prop::Scale, Prop::ScaleX, Prop::ScaleY, Prop::Rotation, Prop::Opacity];
    pub const ADJUST: [Prop; 16] = [
        Prop::Brightness,
        Prop::Contrast,
        Prop::Saturation,
        Prop::Exposure,
        Prop::Temperature,
        Prop::Tint,
        Prop::Highlights,
        Prop::Shadows,
        Prop::Whites,
        Prop::Blacks,
        Prop::Sharpen,
        Prop::Clarity,
        Prop::Fade,
        Prop::Vignette,
        Prop::Grain,
        Prop::Hue,
    ];
    pub const MASK: [Prop; 6] = [Prop::MaskX, Prop::MaskY, Prop::MaskW, Prop::MaskH, Prop::MaskRotation, Prop::MaskFeather];

    pub fn label(self) -> &'static str {
        match self {
            Prop::PosX => "Position X",
            Prop::PosY => "Position Y",
            Prop::Scale => "Scale",
            Prop::ScaleX => "Scale width",
            Prop::ScaleY => "Scale height",
            Prop::Rotation => "Rotate",
            Prop::Opacity => "Opacity",
            Prop::Volume => "Volume",
            Prop::Brightness => "Brightness",
            Prop::Contrast => "Contrast",
            Prop::Saturation => "Saturation",
            Prop::Exposure => "Exposure",
            Prop::Temperature => "Temp",
            Prop::Tint => "Hue",
            Prop::Highlights => "Highlights",
            Prop::Shadows => "Shadows",
            Prop::Whites => "Whites",
            Prop::Blacks => "Blacks",
            Prop::Sharpen => "Sharpen",
            Prop::Clarity => "Clarity",
            Prop::Fade => "Fade",
            Prop::Vignette => "Vignette",
            Prop::Grain => "Grain",
            Prop::Hue => "Hue shift",
            Prop::FilterIntensity => "Filter intensity",
            Prop::EffectAmount => "Intensity",
            Prop::MaskX => "Mask X",
            Prop::MaskY => "Mask Y",
            Prop::MaskW => "Mask width",
            Prop::MaskH => "Mask height",
            Prop::MaskRotation => "Mask rotate",
            Prop::MaskFeather => "Feather",
        }
    }
}

/// Interpolation of the segment that starts at a keyframe ("keyframe graph").
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum Ease {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Smooth,
    Overshoot,
    Bounce,
    Hold,
}

impl Ease {
    pub const ALL: [Ease; 8] =
        [Ease::Linear, Ease::EaseIn, Ease::EaseOut, Ease::EaseInOut, Ease::Smooth, Ease::Overshoot, Ease::Bounce, Ease::Hold];
    pub fn label(self) -> &'static str {
        match self {
            Ease::Linear => "Linear",
            Ease::EaseIn => "Ease in",
            Ease::EaseOut => "Ease out",
            Ease::EaseInOut => "Ease in-out",
            Ease::Smooth => "Smooth",
            Ease::Overshoot => "Overshoot",
            Ease::Bounce => "Bounce",
            Ease::Hold => "Hold",
        }
    }
    pub fn apply(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::EaseIn => cubic_bezier(0.42, 0.0, 1.0, 1.0, t),
            Ease::EaseOut => cubic_bezier(0.0, 0.0, 0.58, 1.0, t),
            Ease::EaseInOut => cubic_bezier(0.42, 0.0, 0.58, 1.0, t),
            Ease::Smooth => cubic_bezier(0.25, 0.1, 0.25, 1.0, t),
            Ease::Overshoot => cubic_bezier(0.34, 1.56, 0.64, 1.0, t),
            Ease::Bounce => bounce_out(t),
            Ease::Hold => {
                if t >= 1.0 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

pub fn bounce_out(t: f64) -> f64 {
    let n1 = 7.5625;
    let d1 = 2.75;
    if t < 1.0 / d1 {
        n1 * t * t
    } else if t < 2.0 / d1 {
        let t = t - 1.5 / d1;
        n1 * t * t + 0.75
    } else if t < 2.5 / d1 {
        let t = t - 2.25 / d1;
        n1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / d1;
        n1 * t * t + 0.984375
    }
}

/// CSS-style cubic bezier easing with (0,0) and (1,1) endpoints.
pub fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let bx = |t: f64| 3.0 * (1.0 - t) * (1.0 - t) * t * x1 + 3.0 * (1.0 - t) * t * t * x2 + t * t * t;
    let by = |t: f64| 3.0 * (1.0 - t) * (1.0 - t) * t * y1 + 3.0 * (1.0 - t) * t * t * y2 + t * t * t;
    let dbx = |t: f64| 3.0 * (1.0 - t) * (1.0 - t) * x1 + 6.0 * (1.0 - t) * t * (x2 - x1) + 3.0 * t * t * (1.0 - x2);
    // Newton then bisection fallback
    let mut t = x;
    for _ in 0..8 {
        let err = bx(t) - x;
        if err.abs() < 1e-7 {
            return by(t);
        }
        let d = dbx(t);
        if d.abs() < 1e-6 {
            break;
        }
        t -= err / d;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    t = x;
    for _ in 0..40 {
        let v = bx(t);
        if (v - x).abs() < 1e-7 {
            break;
        }
        if v < x {
            lo = t;
        } else {
            hi = t;
        }
        t = (lo + hi) * 0.5;
    }
    by(t)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    /// Time relative to the clip start (timeline time).
    pub t: Us,
    pub v: f64,
    #[serde(default)]
    pub ease: Ease,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KfTrack {
    pub prop: Prop,
    pub keys: Vec<Key>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Keyframes(pub Vec<KfTrack>);

/// Keyframes closer than this are considered the same instant (half a frame at 60 fps).
pub const KEY_EPS: Us = 8_000;

impl Keyframes {
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|t| t.keys.is_empty())
    }
    pub fn track(&self, p: Prop) -> Option<&KfTrack> {
        self.0.iter().find(|t| t.prop == p && !t.keys.is_empty())
    }
    fn track_mut(&mut self, p: Prop) -> &mut KfTrack {
        if let Some(i) = self.0.iter().position(|t| t.prop == p) {
            return &mut self.0[i];
        }
        self.0.push(KfTrack { prop: p, keys: vec![] });
        let n = self.0.len();
        &mut self.0[n - 1]
    }
    pub fn has(&self, p: Prop) -> bool {
        self.track(p).is_some()
    }
    pub fn value(&self, p: Prop, t: Us, base: f64) -> f64 {
        let Some(tr) = self.track(p) else { return base };
        let k = &tr.keys;
        if t <= k[0].t {
            return k[0].v;
        }
        let last = k[k.len() - 1];
        if t >= last.t {
            return last.v;
        }
        // binary search the segment
        let i = match k.binary_search_by(|key| key.t.cmp(&t)) {
            Ok(i) => return k[i].v,
            Err(i) => i - 1,
        };
        let a = k[i];
        let b = k[i + 1];
        let f = (t - a.t) as f64 / (b.t - a.t).max(1) as f64;
        let e = a.ease.apply(f);
        a.v + (b.v - a.v) * e
    }
    pub fn key_at(&self, p: Prop, t: Us) -> Option<usize> {
        let tr = self.track(p)?;
        tr.keys.iter().position(|k| (k.t - t).abs() <= KEY_EPS)
    }
    /// Insert or update a key.
    pub fn set(&mut self, p: Prop, t: Us, v: f64) {
        let tr = self.track_mut(p);
        if let Some(k) = tr.keys.iter_mut().find(|k| (k.t - t).abs() <= KEY_EPS) {
            k.v = v;
            return;
        }
        let ease = tr.keys.iter().rev().find(|k| k.t < t).map(|k| k.ease).unwrap_or_default();
        tr.keys.push(Key { t, v, ease });
        tr.keys.sort_by_key(|k| k.t);
    }
    pub fn remove(&mut self, p: Prop, t: Us) -> bool {
        let tr = self.track_mut(p);
        let n = tr.keys.len();
        tr.keys.retain(|k| (k.t - t).abs() > KEY_EPS);
        let removed = tr.keys.len() != n;
        self.0.retain(|t| !t.keys.is_empty());
        removed
    }
    pub fn clear(&mut self, p: Prop) {
        self.0.retain(|t| t.prop != p);
    }
    pub fn set_ease(&mut self, p: Prop, t: Us, ease: Ease) {
        let tr = self.track_mut(p);
        for k in tr.keys.iter_mut() {
            if (k.t - t).abs() <= KEY_EPS {
                k.ease = ease;
            }
        }
        self.0.retain(|t| !t.keys.is_empty());
    }
    /// All distinct key times (sorted).
    pub fn times(&self) -> Vec<Us> {
        let mut v: Vec<Us> = self.0.iter().flat_map(|t| t.keys.iter().map(|k| k.t)).collect();
        v.sort();
        v.dedup_by(|a, b| (*a - *b).abs() <= KEY_EPS);
        v
    }
    /// Keys of any of the given props at time t?
    pub fn any_at(&self, props: &[Prop], t: Us) -> bool {
        props.iter().any(|p| self.key_at(*p, t).is_some())
    }
    /// Move all keys at time `from` to `to` (dragging diamonds on the timeline).
    pub fn move_time(&mut self, from: Us, to: Us) {
        for tr in self.0.iter_mut() {
            for k in tr.keys.iter_mut() {
                if (k.t - from).abs() <= KEY_EPS {
                    k.t = to;
                }
            }
            tr.keys.sort_by_key(|k| k.t);
            // collapse duplicates
            tr.keys.dedup_by(|a, b| (a.t - b.t).abs() <= KEY_EPS / 2);
        }
    }
    pub fn remove_time(&mut self, t: Us) {
        for tr in self.0.iter_mut() {
            tr.keys.retain(|k| (k.t - t).abs() > KEY_EPS);
        }
        self.0.retain(|t| !t.keys.is_empty());
    }
    /// Split at local time `t`: returns keys for the right part (shifted to start at 0).
    /// Boundary keys with the interpolated values are added on both sides so the
    /// animation is preserved.
    pub fn split(&mut self, t: Us, bases: &dyn Fn(Prop) -> f64) -> Keyframes {
        let mut right = Keyframes::default();
        for tr in self.0.iter_mut() {
            if tr.keys.is_empty() {
                continue;
            }
            let p = tr.prop;
            let tmp = Keyframes(vec![tr.clone()]);
            let v = tmp.value(p, t, bases(p));
            let mut rkeys: Vec<Key> = tr.keys.iter().filter(|k| k.t > t + KEY_EPS).map(|k| Key { t: k.t - t, ..*k }).collect();
            let ease = tr.keys.iter().rev().find(|k| k.t <= t).map(|k| k.ease).unwrap_or_default();
            rkeys.insert(0, Key { t: 0, v, ease });
            tr.keys.retain(|k| k.t < t - KEY_EPS);
            tr.keys.push(Key { t, v, ease });
            right.0.push(KfTrack { prop: p, keys: rkeys });
        }
        right
    }
    /// Remove keys before `t` (local) and shift the rest so that `t` becomes 0.
    pub fn trim_start(&mut self, t: Us, bases: &dyn Fn(Prop) -> f64) {
        if t == 0 {
            return;
        }
        let right = self.split(t, bases);
        *self = right;
    }
    pub fn trim_end(&mut self, len: Us, bases: &dyn Fn(Prop) -> f64) {
        let _ = self.split(len, bases);
    }
    pub fn scale_time(&mut self, f: f64) {
        for tr in self.0.iter_mut() {
            for k in tr.keys.iter_mut() {
                k.t = (k.t as f64 * f).round() as Us;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interp() {
        let mut k = Keyframes::default();
        k.set(Prop::PosX, 0, 0.0);
        k.set(Prop::PosX, 1_000_000, 100.0);
        assert_eq!(k.value(Prop::PosX, 500_000, 0.0), 50.0);
        assert_eq!(k.value(Prop::PosX, -5, 0.0), 0.0);
        assert_eq!(k.value(Prop::PosX, 2_000_000, 0.0), 100.0);
        assert_eq!(k.value(Prop::PosY, 0, 7.0), 7.0);
        let r = k.split(500_000, &|_| 0.0);
        assert_eq!(k.value(Prop::PosX, 500_000, 0.0), 50.0);
        assert_eq!(r.value(Prop::PosX, 0, 0.0), 50.0);
        assert_eq!(r.value(Prop::PosX, 500_000, 0.0), 100.0);
    }

    #[test]
    fn bezier() {
        assert!((cubic_bezier(0.42, 0.0, 0.58, 1.0, 0.5) - 0.5).abs() < 1e-4);
        assert!(cubic_bezier(0.42, 0.0, 1.0, 1.0, 0.25) < 0.25);
    }
}
