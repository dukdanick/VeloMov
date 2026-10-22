//! Time-based behaviour of animations, transitions and motion effects.
//! Everything returns transform deltas so the compositor can evaluate them
//! at several instants of the shutter and produce real motion blur.

use crate::model::catalog::AnimKind;
use crate::util::noise1;

/// Transform delta. Translations are fractions of the canvas (+y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct XD {
    pub tx: f64,
    pub ty: f64,
    pub sx: f64,
    pub sy: f64,
    pub rot: f64,
    pub alpha: f64,
    /// Gaussian blur as a fraction of the canvas height.
    pub blur: f64,
}

impl XD {
    pub const ID: XD = XD { tx: 0.0, ty: 0.0, sx: 1.0, sy: 1.0, rot: 0.0, alpha: 1.0, blur: 0.0 };
    pub fn then(self, o: XD) -> XD {
        XD {
            tx: self.tx + o.tx,
            ty: self.ty + o.ty,
            sx: self.sx * o.sx,
            sy: self.sy * o.sy,
            rot: self.rot + o.rot,
            alpha: self.alpha * o.alpha,
            blur: self.blur.max(o.blur),
        }
    }
    pub fn scale(s: f64) -> XD {
        XD { sx: s, sy: s, ..XD::ID }
    }
    pub fn is_identity(&self) -> bool {
        *self == XD::ID
    }
}

pub fn ease_in_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * t
}
pub fn ease_out_cubic(t: f64) -> f64 {
    let t = 1.0 - t.clamp(0.0, 1.0);
    1.0 - t * t * t
}
pub fn ease_in_out_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}
pub fn ease_in_out_quint(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        16.0 * t.powi(5)
    } else {
        1.0 - (-2.0 * t + 2.0).powi(5) / 2.0
    }
}
pub fn ease_out_back(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
}
pub fn ease_out_elastic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    if t == 0.0 || t == 1.0 {
        return t;
    }
    let c4 = std::f64::consts::TAU / 3.0;
    2f64.powf(-10.0 * t) * ((t * 10.0 - 0.75) * c4).sin() + 1.0
}
pub fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Animation delta. `p` is the progress inside the animation (0..1); for
/// combos it is the phase of the loop. `seed` decorrelates shakes.
pub fn anim_xd(id: &str, kind: AnimKind, p: f64, seed: u64) -> XD {
    use std::f64::consts::{PI, TAU};
    let e = ease_out_cubic(p);
    let ei = ease_in_cubic(p);
    match kind {
        AnimKind::In => match id {
            "fade_in" => XD { alpha: smoothstep(0.0, 1.0, p), ..XD::ID },
            "zoom_in" => XD { alpha: (p * 2.5).min(1.0), ..XD::scale(0.45 + 0.55 * e) },
            "zoom_out_in" => XD { alpha: (p * 2.5).min(1.0), ..XD::scale(1.7 - 0.7 * e) },
            "slide_right_in" => XD { tx: -(1.0 - e), ..XD::ID },
            "slide_left_in" => XD { tx: 1.0 - e, ..XD::ID },
            "slide_up_in" => XD { ty: 1.0 - e, ..XD::ID },
            "slide_down_in" => XD { ty: -(1.0 - e), ..XD::ID },
            "rotate_in" => XD { rot: -180.0 * (1.0 - e), alpha: (p * 2.0).min(1.0), ..XD::scale(e.max(0.001)) },
            "spin_in" => XD { rot: 360.0 * (1.0 - e), alpha: (p * 3.0).min(1.0), ..XD::scale(0.6 + 0.4 * e) },
            "swing_in" => XD { rot: 28.0 * (1.0 - p).powi(2) * (p * PI * 4.0).cos(), alpha: (p * 4.0).min(1.0), ..XD::ID },
            "bounce_in" => XD { alpha: (p * 4.0).min(1.0), ..XD::scale(ease_out_elastic(p).max(0.0)) },
            "drop_in" => XD { ty: -(1.0 - crate::model::keyframe::bounce_out(p)), ..XD::ID },
            "pop_in" => XD { alpha: (p * 3.0).min(1.0), ..XD::scale(0.3 + 0.7 * ease_out_back(p)) },
            "blur_in" => XD { blur: (1.0 - e) * 0.05, alpha: smoothstep(0.0, 0.7, p), ..XD::ID },
            "flip_in" => XD { sx: e.max(0.001), ..XD::ID },
            "shake_in" => {
                let a = (1.0 - p) * 0.06;
                XD {
                    tx: noise1(p * 25.0, seed) * a,
                    ty: noise1(p * 25.0, seed + 3) * a,
                    rot: noise1(p * 20.0, seed + 7) * 6.0 * (1.0 - p),
                    alpha: (p * 4.0).min(1.0),
                    ..XD::ID
                }
            }
            "rise_in" => XD { ty: 0.15 * (1.0 - e), alpha: e, ..XD::ID },
            "fade_up_text" => XD { ty: 0.06 * (1.0 - e), alpha: smoothstep(0.0, 1.0, p), ..XD::ID },
            _ => XD::ID,
        },
        AnimKind::Out => match id {
            "fade_out" => XD { alpha: 1.0 - smoothstep(0.0, 1.0, p), ..XD::ID },
            "zoom_in_out" => XD { alpha: 1.0 - smoothstep(0.55, 1.0, p), ..XD::scale(1.0 + 0.7 * ei) },
            "zoom_out_out" => XD { alpha: 1.0 - smoothstep(0.55, 1.0, p), ..XD::scale(1.0 - 0.55 * ei) },
            "slide_right_out" => XD { tx: ei, ..XD::ID },
            "slide_left_out" => XD { tx: -ei, ..XD::ID },
            "slide_up_out" => XD { ty: -ei, ..XD::ID },
            "slide_down_out" => XD { ty: ei, ..XD::ID },
            "rotate_out" => XD { rot: 180.0 * ei, alpha: 1.0 - smoothstep(0.5, 1.0, p), ..XD::scale((1.0 - ei).max(0.001)) },
            "spin_out" => XD { rot: -360.0 * ei, alpha: 1.0 - smoothstep(0.66, 1.0, p), ..XD::scale(1.0 - 0.4 * ei) },
            "shrink_out" => XD::scale((1.0 - ei).max(0.001)),
            "blur_out" => XD { blur: ei * 0.05, alpha: 1.0 - smoothstep(0.3, 1.0, p), ..XD::ID },
            "flip_out" => XD { sx: (1.0 - ei).max(0.001), ..XD::ID },
            "fall_out" => XD { ty: ei * 1.2, rot: 25.0 * ei, ..XD::ID },
            _ => XD::ID,
        },
        AnimKind::Combo => {
            let w = TAU * p;
            match id {
                "combo_swing" => XD { rot: 8.0 * w.sin(), ..XD::ID },
                "combo_pendulum" => XD { rot: 15.0 * w.sin(), tx: 0.02 * w.sin(), ..XD::ID },
                "combo_bounce" => XD { ty: -0.05 * (w * 0.5).sin().abs(), ..XD::ID },
                "combo_pulse" => XD::scale(1.0 + 0.06 * w.sin()),
                "combo_shake" => XD {
                    tx: noise1(p * 30.0, seed) * 0.02,
                    ty: noise1(p * 30.0, seed + 5) * 0.02,
                    rot: noise1(p * 24.0, seed + 9) * 2.0,
                    ..XD::ID
                },
                "combo_rotate" => XD { rot: 360.0 * p, ..XD::ID },
                "combo_zoom" => XD::scale(1.0 + 0.15 * (0.5 - 0.5 * w.cos())),
                "combo_float" => XD { ty: 0.015 * w.sin(), tx: 0.01 * (2.0 * w).sin(), ..XD::ID },
                "combo_wobble" => XD { sx: 1.0 + 0.05 * (2.0 * w).sin(), sy: 1.0 - 0.05 * (2.0 * w).sin(), ..XD::ID },
                "combo_heartbeat" => {
                    let beat = |c: f64| (-((p - c) * 18.0).powi(2)).exp();
                    XD::scale(1.0 + 0.09 * beat(0.15) + 0.06 * beat(0.38))
                }
                _ => XD::ID,
            }
        }
    }
}

/// Transition shader kinds (must match fs_transition).
pub mod tk {
    pub const MIX: u32 = 0;
    pub const FADE_BLACK: u32 = 1;
    pub const FLASH: u32 = 2;
    pub const WIPE: u32 = 3;
    pub const CLOCK: u32 = 4;
    pub const CIRCLE: u32 = 5;
    pub const SHAPE: u32 = 6;
    pub const SPLIT: u32 = 7;
    pub const BLINDS: u32 = 8;
    pub const DISSOLVE: u32 = 9;
    pub const LUMA: u32 = 10;
    pub const GLITCH: u32 = 11;
    pub const RGB: u32 = 12;
    pub const PIXELATE: u32 = 13;
    pub const TV: u32 = 14;
    pub const LEAK: u32 = 15;
    pub const RIPPLE: u32 = 16;
    pub const SWIRL: u32 = 17;
    pub const CUBE: u32 = 18;
    pub const FLIP: u32 = 19;
    pub const DOOR: u32 = 20;
    pub const ZOOM_BLUR: u32 = 21;
    pub const OVER: u32 = 22;
}

/// How a transition combines the two sides at progress p.
#[derive(Clone, Copy, Debug)]
pub struct TPlan {
    pub a: XD,
    pub b: XD,
    /// When no mix shader is used: draw B above A (otherwise A above B).
    pub b_on_top: bool,
    pub mix: Option<(u32, [f32; 4])>,
}

impl TPlan {
    fn draw(a: XD, b: XD) -> TPlan {
        TPlan { a, b, b_on_top: true, mix: None }
    }
    fn mix(kind: u32, params: [f32; 4]) -> TPlan {
        TPlan { a: XD::ID, b: XD::ID, b_on_top: true, mix: Some((kind, params)) }
    }
    fn mix_xd(kind: u32, params: [f32; 4], a: XD, b: XD) -> TPlan {
        TPlan { a, b, b_on_top: true, mix: Some((kind, params)) }
    }
}

pub fn transition_plan(id: &str, p: f64, seed: u64) -> TPlan {
    let p = p.clamp(0.0, 1.0);
    let e = ease_in_out_cubic(p);
    let q = ease_in_out_quint(p);
    let first = (p / 0.5).min(1.0);
    let second = ((p - 0.5) / 0.5).max(0.0);
    let cut = smoothstep(0.47, 0.53, p) as f32;
    let pf = p as f32;
    let slide = |dx: f64, dy: f64, e: f64| {
        TPlan::draw(XD { tx: -dx * e, ty: -dy * e, ..XD::ID }, XD { tx: dx * (1.0 - e), ty: dy * (1.0 - e), ..XD::ID })
    };
    match id {
        "mix" => TPlan::mix(tk::MIX, [pf, 0.0, 0.0, 0.0]),
        "black_fade" => TPlan::mix(tk::FADE_BLACK, [0.0; 4]),
        "white_flash" => TPlan::mix(tk::FLASH, [1.5, 0.0, 0.0, 0.0]),
        "flash" => TPlan::mix(tk::FLASH, [4.0, 0.0, 0.0, 0.0]),
        "dissolve" => TPlan::mix(tk::DISSOLVE, [0.0; 4]),
        "luma_fade" => TPlan::mix(tk::LUMA, [0.0; 4]),
        "zoom_mix" => TPlan::mix_xd(tk::MIX, [e as f32, 0.0, 0.0, 0.0], XD::scale(1.0 + 0.18 * e), XD::scale(1.18 - 0.18 * e)),
        "pull_in" => TPlan::mix_xd(
            tk::MIX,
            [cut, 0.0, 0.0, 0.0],
            XD::scale(1.0 + 1.6 * ease_in_cubic(first)),
            XD::scale(0.45 + 0.55 * ease_out_cubic(second)),
        ),
        "pull_out" => TPlan::mix_xd(
            tk::MIX,
            [cut, 0.0, 0.0, 0.0],
            XD::scale(1.0 - 0.45 * ease_in_cubic(first)),
            XD::scale(2.4 - 1.4 * ease_out_cubic(second)),
        ),
        "whip_left" => slide(1.0, 0.0, q),
        "whip_right" => slide(-1.0, 0.0, q),
        "whip_up" => slide(0.0, 1.0, q),
        "whip_down" => slide(0.0, -1.0, q),
        "swipe_blur" => slide(1.0, 0.0, ease_in_out_quint(p)),
        "slide_left" => slide(1.0, 0.0, e),
        "slide_right" => slide(-1.0, 0.0, e),
        "slide_up" => slide(0.0, 1.0, e),
        "slide_down" => slide(0.0, -1.0, e),
        "cover_left" => TPlan::draw(XD::ID, XD { tx: 1.0 - e, ..XD::ID }),
        "cover_right" => TPlan::draw(XD::ID, XD { tx: -(1.0 - e), ..XD::ID }),
        "uncover_left" => TPlan { a: XD { tx: -e, ..XD::ID }, b: XD::ID, b_on_top: false, mix: None },
        "uncover_right" => TPlan { a: XD { tx: e, ..XD::ID }, b: XD::ID, b_on_top: false, mix: None },
        "squeeze" => {
            TPlan::draw(XD { sx: (1.0 - e).max(0.001), tx: -e * 0.5, ..XD::ID }, XD { sx: e.max(0.001), tx: (1.0 - e) * 0.5, ..XD::ID })
        }
        "spin_cw" | "spin_ccw" => {
            let s = if id == "spin_cw" { 1.0 } else { -1.0 };
            let e1 = ease_in_cubic(first);
            let e2 = ease_out_cubic(second);
            TPlan::mix_xd(
                tk::MIX,
                [cut, 0.0, 0.0, 0.0],
                XD { rot: s * 90.0 * e1, ..XD::scale(1.0 + 0.95 * e1) },
                XD { rot: -s * 90.0 * (1.0 - e2), ..XD::scale(1.95 - 0.95 * e2) },
            )
        }
        "spin_360" => {
            let e1 = ease_in_cubic(first);
            let e2 = ease_out_cubic(second);
            TPlan::mix_xd(
                tk::MIX,
                [cut, 0.0, 0.0, 0.0],
                XD { rot: 180.0 * e1, ..XD::scale(1.0 + 0.9 * e1) },
                XD { rot: -180.0 * (1.0 - e2), ..XD::scale(1.9 - 0.9 * e2) },
            )
        }
        "shake" => {
            let amp = (p * std::f64::consts::PI).sin() * 0.07;
            let j = |s: u64| XD {
                tx: noise1(p * 22.0, seed + s) * amp,
                ty: noise1(p * 22.0, seed + s + 11) * amp,
                rot: noise1(p * 18.0, seed + s + 23) * 4.0 * amp / 0.07,
                ..XD::scale(1.0 + amp * 2.0)
            };
            TPlan::mix_xd(tk::MIX, [smoothstep(0.35, 0.65, p) as f32, 0.0, 0.0, 0.0], j(0), j(0))
        }
        "swing" => {
            let r = 14.0 * (p * std::f64::consts::PI).sin() * (p * std::f64::consts::PI * 2.0).cos();
            TPlan::mix_xd(
                tk::MIX,
                [smoothstep(0.35, 0.65, p) as f32, 0.0, 0.0, 0.0],
                XD { rot: r, ..XD::scale(1.3) },
                XD { rot: r, ..XD::scale(1.3) },
            )
        }
        "zoom_in" => TPlan::mix_xd(tk::MIX, [e as f32, 0.0, 0.0, 0.0], XD::scale(1.0 + 0.5 * e), XD::scale(0.8 + 0.2 * e)),
        "zoom_out" => TPlan::mix_xd(tk::MIX, [e as f32, 0.0, 0.0, 0.0], XD::scale(1.0 - 0.2 * e), XD::scale(1.4 - 0.4 * e)),
        "bounce" => TPlan::draw(XD::ID, XD { ty: -(1.0 - crate::model::keyframe::bounce_out(p)), ..XD::ID }),
        "wipe_left" => TPlan::mix(tk::WIPE, [-1.0, 0.0, 0.02, 0.0]),
        "wipe_right" => TPlan::mix(tk::WIPE, [1.0, 0.0, 0.02, 0.0]),
        "wipe_up" => TPlan::mix(tk::WIPE, [0.0, -1.0, 0.02, 0.0]),
        "wipe_down" => TPlan::mix(tk::WIPE, [0.0, 1.0, 0.02, 0.0]),
        "clock_wipe" => TPlan::mix(tk::CLOCK, [0.0; 4]),
        "split_open" => TPlan::mix(tk::SPLIT, [0.0; 4]),
        "blinds" => TPlan::mix(tk::BLINDS, [0.0; 4]),
        "blur" => TPlan::mix_xd(
            tk::MIX,
            [smoothstep(0.35, 0.65, p) as f32, 0.0, 0.0, 0.0],
            XD { blur: 0.045 * smoothstep(0.0, 0.55, p), ..XD::ID },
            XD { blur: 0.045 * (1.0 - smoothstep(0.45, 1.0, p)), ..XD::ID },
        ),
        "zoom_blur" => TPlan::mix(tk::ZOOM_BLUR, [0.0; 4]),
        "glitch" => TPlan::mix(tk::GLITCH, [0.0; 4]),
        "rgb_split" => TPlan::mix(tk::RGB, [0.0; 4]),
        "pixelate" => TPlan::mix(tk::PIXELATE, [0.0; 4]),
        "tv_noise" => TPlan::mix(tk::TV, [0.0; 4]),
        "light_leak" => TPlan::mix(tk::LEAK, [0.0; 4]),
        "film_burn" => TPlan::mix(tk::LEAK, [1.0, 0.0, 0.0, 0.0]),
        "circle_open" => TPlan::mix(tk::CIRCLE, [0.0; 4]),
        "heart" => TPlan::mix(tk::SHAPE, [0.0; 4]),
        "star" => TPlan::mix(tk::SHAPE, [1.0, 0.0, 0.0, 0.0]),
        "diamond" => TPlan::mix(tk::SHAPE, [2.0, 0.0, 0.0, 0.0]),
        "ripple" => TPlan::mix(tk::RIPPLE, [0.0; 4]),
        "swirl" => TPlan::mix(tk::SWIRL, [0.0; 4]),
        "cube_left" => TPlan::mix(tk::CUBE, [0.0; 4]),
        "cube_up" => TPlan::mix(tk::CUBE, [1.0, 0.0, 0.0, 0.0]),
        "flip" => TPlan::mix(tk::FLIP, [0.0; 4]),
        "door" => TPlan::mix(tk::DOOR, [0.0; 4]),
        _ => TPlan::mix(tk::MIX, [pf, 0.0, 0.0, 0.0]),
    }
}

/// Effect shader kinds (must match fs_effect).
pub fn effect_kind(id: &str) -> Option<u32> {
    Some(match id {
        "zoom_lens" => 2,
        "directional_blur" => 3,
        "glitch" => 4,
        "rgb_split" => 5,
        "pixelate" => 6,
        "vhs" => 7,
        "old_film" => 8,
        "film_grain" => 9,
        "crt" => 10,
        "halftone" => 11,
        "vignette" => 12,
        "bw" => 13,
        "invert" => 14,
        "posterize" => 15,
        "letterbox" | "opening" => 16,
        "fade_black" => 17,
        "fisheye" => 18,
        "mirror" => 19,
        "kaleidoscope" => 20,
        "chromatic" => 21,
        "bloom" => 22,
        "strobe" => 23,
        "light_leak" => 24,
        "edge_glow" => 25,
        "rainbow" => 26,
        "disco" => 27,
        "sparkle" => 28,
        "split2" | "split3" | "split4" | "split9" => 29,
        "snow" => 30,
        "rain" => 31,
        "night_vision" => 32,
        "thermal" => 33,
        "dreamy" => 34,
        "tilt_shift" => 35,
        "wave" => 36,
        _ => return None,
    })
}

/// Camera-like effects implemented as a warp of the canvas: returns the delta at local time `t`.
pub fn warp_effect(id: &str, t: f64, dur: f64, amt: f64, speed: f64, seed: u64) -> Option<XD> {
    let w = std::f64::consts::TAU;
    Some(match id {
        "shake" => {
            let f = 6.0 + speed * 18.0;
            let a = 0.035 * amt;
            XD {
                tx: noise1(t * f, seed) * a,
                ty: noise1(t * f, seed + 13) * a,
                rot: noise1(t * f * 0.8, seed + 29) * 3.0 * amt,
                ..XD::scale(1.0 + 0.08 * amt)
            }
        }
        "heartbeat" => {
            let period = 1.2 - speed * 0.8;
            let ph = (t / period.max(0.15)).fract();
            let beat = |c: f64| (-((ph - c) * 16.0).powi(2)).exp();
            XD::scale(1.0 + amt * (0.12 * beat(0.12) + 0.08 * beat(0.34)))
        }
        "swing" => {
            let f = 0.4 + speed * 1.2;
            XD { rot: 7.0 * amt * (t * f * w).sin(), ..XD::scale(1.0 + 0.18 * amt) }
        }
        "ken_burns" => {
            let p = if dur > 0.0 { (t / dur).clamp(0.0, 1.0) } else { 0.0 };
            XD { tx: -0.03 * amt * p, ..XD::scale(1.0 + 0.25 * amt * p) }
        }
        _ => return None,
    })
}
