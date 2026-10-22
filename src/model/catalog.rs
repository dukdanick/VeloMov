//! Built-in libraries shown in the left panel: transitions, animations,
//! effects, filters, stickers, text styles, voice effects and speed curves.
//! The behaviour of each id lives in the render engine; this file is metadata.

use crate::util::{Us, SEC};

// ---------------------------------------------------------------- transitions

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TCat {
    Trending,
    Basic,
    Camera,
    Slide,
    Wipe,
    Blur,
    Glitch,
    Light,
    Shape,
    ThreeD,
}

impl TCat {
    pub const ALL: [TCat; 10] = [
        TCat::Trending,
        TCat::Basic,
        TCat::Camera,
        TCat::Slide,
        TCat::Wipe,
        TCat::Blur,
        TCat::Glitch,
        TCat::Light,
        TCat::Shape,
        TCat::ThreeD,
    ];
    pub fn label(self) -> &'static str {
        match self {
            TCat::Trending => "Trending",
            TCat::Basic => "Basic",
            TCat::Camera => "Camera",
            TCat::Slide => "Slide",
            TCat::Wipe => "Wipe",
            TCat::Blur => "Blur",
            TCat::Glitch => "Glitch",
            TCat::Light => "Light",
            TCat::Shape => "Shape",
            TCat::ThreeD => "3D",
        }
    }
}

pub struct TransitionDef {
    pub id: &'static str,
    pub name: &'static str,
    pub cat: TCat,
    pub dur: Us,
    /// Default motion blur strength (0 = transition has no movement blur).
    pub blur: f64,
    pub trending: bool,
}

const fn tr(id: &'static str, name: &'static str, cat: TCat, dur_ms: i64, blur: f64, trending: bool) -> TransitionDef {
    TransitionDef { id, name, cat, dur: dur_ms * 1000, blur, trending }
}

pub static TRANSITIONS: &[TransitionDef] = &[
    // Basic
    tr("mix", "Mix", TCat::Basic, 500, 0.0, true),
    tr("black_fade", "Black Fade", TCat::Basic, 600, 0.0, true),
    tr("white_flash", "White Flash", TCat::Basic, 500, 0.0, true),
    tr("dissolve", "Dissolve", TCat::Basic, 700, 0.0, false),
    tr("luma_fade", "Luma Fade", TCat::Basic, 700, 0.0, false),
    tr("zoom_mix", "Zoom Mix", TCat::Basic, 600, 0.3, false),
    // Camera (camera moves with motion blur)
    tr("pull_in", "Pull In", TCat::Camera, 500, 0.75, true),
    tr("pull_out", "Pull Out", TCat::Camera, 500, 0.75, true),
    tr("whip_left", "Left", TCat::Camera, 400, 0.9, true),
    tr("whip_right", "Right", TCat::Camera, 400, 0.9, true),
    tr("whip_up", "Up", TCat::Camera, 400, 0.9, true),
    tr("whip_down", "Down", TCat::Camera, 400, 0.9, true),
    tr("spin_cw", "Rotate CW", TCat::Camera, 500, 0.8, true),
    tr("spin_ccw", "Rotate CCW", TCat::Camera, 500, 0.8, false),
    tr("spin_360", "Spin", TCat::Camera, 700, 0.8, false),
    tr("shake", "Shake", TCat::Camera, 500, 0.6, false),
    tr("swing", "Swing", TCat::Camera, 600, 0.6, false),
    tr("zoom_in", "Zoom In", TCat::Camera, 500, 0.6, false),
    tr("zoom_out", "Zoom Out", TCat::Camera, 500, 0.6, false),
    tr("bounce", "Bounce", TCat::Camera, 600, 0.5, false),
    // Slide (push both clips)
    tr("slide_left", "Slide Left", TCat::Slide, 500, 0.6, true),
    tr("slide_right", "Slide Right", TCat::Slide, 500, 0.6, false),
    tr("slide_up", "Slide Up", TCat::Slide, 500, 0.6, false),
    tr("slide_down", "Slide Down", TCat::Slide, 500, 0.6, false),
    tr("cover_left", "Cover Left", TCat::Slide, 500, 0.5, false),
    tr("cover_right", "Cover Right", TCat::Slide, 500, 0.5, false),
    tr("uncover_left", "Uncover Left", TCat::Slide, 500, 0.5, false),
    tr("uncover_right", "Uncover Right", TCat::Slide, 500, 0.5, false),
    tr("squeeze", "Squeeze", TCat::Slide, 500, 0.4, false),
    // Wipe
    tr("wipe_left", "Wipe Left", TCat::Wipe, 600, 0.0, false),
    tr("wipe_right", "Wipe Right", TCat::Wipe, 600, 0.0, false),
    tr("wipe_up", "Wipe Up", TCat::Wipe, 600, 0.0, false),
    tr("wipe_down", "Wipe Down", TCat::Wipe, 600, 0.0, false),
    tr("clock_wipe", "Clock Wipe", TCat::Wipe, 700, 0.0, false),
    tr("split_open", "Split", TCat::Wipe, 600, 0.0, false),
    tr("blinds", "Blinds", TCat::Wipe, 700, 0.0, false),
    // Blur
    tr("blur", "Blur", TCat::Blur, 600, 0.0, true),
    tr("zoom_blur", "Zoom Blur", TCat::Blur, 500, 0.0, false),
    tr("swipe_blur", "Swipe", TCat::Blur, 450, 1.0, false),
    // Glitch
    tr("glitch", "Glitch", TCat::Glitch, 500, 0.0, true),
    tr("rgb_split", "RGB Split", TCat::Glitch, 500, 0.0, false),
    tr("pixelate", "Pixelate", TCat::Glitch, 600, 0.0, false),
    tr("tv_noise", "TV Noise", TCat::Glitch, 500, 0.0, false),
    // Light
    tr("flash", "Flash", TCat::Light, 400, 0.0, false),
    tr("light_leak", "Light Leak", TCat::Light, 800, 0.0, false),
    tr("film_burn", "Film Burn", TCat::Light, 800, 0.0, false),
    // Shape
    tr("circle_open", "Circle", TCat::Shape, 600, 0.0, false),
    tr("heart", "Heart", TCat::Shape, 700, 0.0, false),
    tr("star", "Star", TCat::Shape, 700, 0.0, false),
    tr("diamond", "Diamond", TCat::Shape, 600, 0.0, false),
    tr("ripple", "Ripple", TCat::Shape, 800, 0.0, false),
    tr("swirl", "Swirl", TCat::Shape, 700, 0.0, false),
    // 3D
    tr("cube_left", "Cube", TCat::ThreeD, 700, 0.3, false),
    tr("cube_up", "Cube Up", TCat::ThreeD, 700, 0.3, false),
    tr("flip", "Flip", TCat::ThreeD, 600, 0.3, false),
    tr("door", "Door", TCat::ThreeD, 700, 0.2, false),
];

pub fn transition(id: &str) -> Option<&'static TransitionDef> {
    TRANSITIONS.iter().find(|t| t.id == id)
}

// ---------------------------------------------------------------- animations

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimKind {
    In,
    Out,
    Combo,
}

pub struct AnimDef {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: AnimKind,
    pub dur: Us,
    /// Motion blur strength applied to the movement.
    pub blur: f64,
    pub text_only: bool,
}

const fn an(id: &'static str, name: &'static str, kind: AnimKind, dur_ms: i64, blur: f64) -> AnimDef {
    AnimDef { id, name, kind, dur: dur_ms * 1000, blur, text_only: false }
}
const fn ant(id: &'static str, name: &'static str, kind: AnimKind, dur_ms: i64) -> AnimDef {
    AnimDef { id, name, kind, dur: dur_ms * 1000, blur: 0.0, text_only: true }
}

pub static ANIMATIONS: &[AnimDef] = &[
    // In
    an("fade_in", "Fade In", AnimKind::In, 500, 0.0),
    an("zoom_in", "Zoom In", AnimKind::In, 500, 0.3),
    an("zoom_out_in", "Zoom Out", AnimKind::In, 500, 0.3),
    an("slide_right_in", "Slide Right", AnimKind::In, 500, 0.7),
    an("slide_left_in", "Slide Left", AnimKind::In, 500, 0.7),
    an("slide_up_in", "Slide Up", AnimKind::In, 500, 0.7),
    an("slide_down_in", "Slide Down", AnimKind::In, 500, 0.7),
    an("rotate_in", "Rotate", AnimKind::In, 600, 0.6),
    an("spin_in", "Spin", AnimKind::In, 700, 0.8),
    an("swing_in", "Swing", AnimKind::In, 800, 0.4),
    an("bounce_in", "Bounce", AnimKind::In, 700, 0.3),
    an("drop_in", "Drop", AnimKind::In, 700, 0.5),
    an("pop_in", "Pop", AnimKind::In, 400, 0.2),
    an("blur_in", "Blur In", AnimKind::In, 600, 0.0),
    an("flip_in", "Flip", AnimKind::In, 500, 0.2),
    an("shake_in", "Shake", AnimKind::In, 600, 0.5),
    an("rise_in", "Rise", AnimKind::In, 600, 0.3),
    ant("typewriter", "Typewriter", AnimKind::In, 1200),
    ant("fade_up_text", "Fade Up", AnimKind::In, 700),
    // Out
    an("fade_out", "Fade Out", AnimKind::Out, 500, 0.0),
    an("zoom_in_out", "Zoom In", AnimKind::Out, 500, 0.3),
    an("zoom_out_out", "Zoom Out", AnimKind::Out, 500, 0.3),
    an("slide_right_out", "Slide Right", AnimKind::Out, 500, 0.7),
    an("slide_left_out", "Slide Left", AnimKind::Out, 500, 0.7),
    an("slide_up_out", "Slide Up", AnimKind::Out, 500, 0.7),
    an("slide_down_out", "Slide Down", AnimKind::Out, 500, 0.7),
    an("rotate_out", "Rotate", AnimKind::Out, 600, 0.6),
    an("spin_out", "Spin", AnimKind::Out, 700, 0.8),
    an("shrink_out", "Shrink", AnimKind::Out, 500, 0.3),
    an("blur_out", "Blur Out", AnimKind::Out, 600, 0.0),
    an("flip_out", "Flip", AnimKind::Out, 500, 0.2),
    an("fall_out", "Fall", AnimKind::Out, 600, 0.5),
    ant("typewriter_out", "Erase", AnimKind::Out, 900),
    // Combo
    an("combo_swing", "Swing", AnimKind::Combo, 2000, 0.3),
    an("combo_pendulum", "Pendulum", AnimKind::Combo, 2000, 0.3),
    an("combo_bounce", "Bounce", AnimKind::Combo, 1000, 0.3),
    an("combo_pulse", "Pulse", AnimKind::Combo, 1000, 0.2),
    an("combo_shake", "Shake", AnimKind::Combo, 1000, 0.5),
    an("combo_rotate", "Rotate", AnimKind::Combo, 3000, 0.5),
    an("combo_zoom", "Zoom Cycle", AnimKind::Combo, 2000, 0.3),
    an("combo_float", "Float", AnimKind::Combo, 3000, 0.1),
    an("combo_wobble", "Wobble", AnimKind::Combo, 1500, 0.3),
    an("combo_heartbeat", "Heartbeat", AnimKind::Combo, 1000, 0.2),
];

pub fn animation(id: &str) -> Option<&'static AnimDef> {
    ANIMATIONS.iter().find(|t| t.id == id)
}

// ---------------------------------------------------------------- effects

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ECat {
    Trending,
    Basic,
    Blur,
    Lens,
    Retro,
    Glitch,
    Light,
    Motion,
    Split,
    Party,
    Nature,
}

impl ECat {
    pub const ALL: [ECat; 11] = [
        ECat::Trending,
        ECat::Basic,
        ECat::Blur,
        ECat::Lens,
        ECat::Retro,
        ECat::Glitch,
        ECat::Light,
        ECat::Motion,
        ECat::Split,
        ECat::Party,
        ECat::Nature,
    ];
    pub fn label(self) -> &'static str {
        match self {
            ECat::Trending => "Trending",
            ECat::Basic => "Basic",
            ECat::Blur => "Blur",
            ECat::Lens => "Lens",
            ECat::Retro => "Retro",
            ECat::Glitch => "Glitch",
            ECat::Light => "Light",
            ECat::Motion => "Motion",
            ECat::Split => "Split screen",
            ECat::Party => "Party",
            ECat::Nature => "Nature",
        }
    }
}

pub struct ParamDef {
    pub name: &'static str,
    pub default: f64,
}

pub struct EffectDef {
    pub id: &'static str,
    pub name: &'static str,
    pub cat: ECat,
    pub params: &'static [ParamDef],
    pub trending: bool,
}

const P_INT: ParamDef = ParamDef { name: "Intensity", default: 0.6 };
const P_SPEED: ParamDef = ParamDef { name: "Speed", default: 0.5 };
const P_ANGLE: ParamDef = ParamDef { name: "Angle", default: 0.0 };
const P_SIZE: ParamDef = ParamDef { name: "Size", default: 0.5 };

const fn ef(id: &'static str, name: &'static str, cat: ECat, params: &'static [ParamDef], trending: bool) -> EffectDef {
    EffectDef { id, name, cat, params, trending }
}

pub static EFFECTS: &[EffectDef] = &[
    ef("blur", "Blur", ECat::Blur, &[P_INT], true),
    ef("zoom_lens", "Zoom Lens", ECat::Blur, &[P_INT, P_SPEED], true),
    ef("directional_blur", "Motion Blur", ECat::Blur, &[P_INT, P_ANGLE], true),
    ef("tilt_shift", "Tilt-Shift", ECat::Blur, &[P_INT, P_SIZE], false),
    ef("dreamy", "Dreamy", ECat::Blur, &[P_INT], false),
    ef("shake", "Shake", ECat::Motion, &[P_INT, P_SPEED], true),
    ef("heartbeat", "Heartbeat", ECat::Motion, &[P_INT, P_SPEED], false),
    ef("swing", "Swing", ECat::Motion, &[P_INT, P_SPEED], false),
    ef("ken_burns", "Slow Zoom", ECat::Motion, &[P_INT], false),
    ef("wave", "Wave", ECat::Motion, &[P_INT, P_SPEED], false),
    ef("glitch", "Glitch", ECat::Glitch, &[P_INT, P_SPEED], true),
    ef("rgb_split", "RGB Split", ECat::Glitch, &[P_INT, P_SPEED], true),
    ef("pixelate", "Mosaic", ECat::Glitch, &[P_INT], false),
    ef("vhs", "VHS", ECat::Retro, &[P_INT], true),
    ef("old_film", "Old Film", ECat::Retro, &[P_INT], false),
    ef("film_grain", "Film Grain", ECat::Retro, &[P_INT], false),
    ef("crt", "CRT", ECat::Retro, &[P_INT], false),
    ef("halftone", "Halftone", ECat::Retro, &[P_INT, P_SIZE], false),
    ef("vignette", "Vignette", ECat::Basic, &[P_INT], false),
    ef("bw", "Black & White", ECat::Basic, &[P_INT], false),
    ef("invert", "Invert", ECat::Basic, &[P_INT], false),
    ef("posterize", "Posterize", ECat::Basic, &[P_INT], false),
    ef("letterbox", "Cinema", ECat::Basic, &[P_SIZE], true),
    ef("opening", "Opening", ECat::Basic, &[P_SPEED], false),
    ef("fade_black", "Fade to Black", ECat::Basic, &[P_INT], false),
    ef("fisheye", "Fisheye", ECat::Lens, &[P_INT], false),
    ef("mirror", "Mirror", ECat::Lens, &[P_INT], false),
    ef("kaleidoscope", "Kaleidoscope", ECat::Lens, &[P_INT, P_SPEED], false),
    ef("chromatic", "Chromatic", ECat::Lens, &[P_INT], false),
    ef("bloom", "Glow", ECat::Light, &[P_INT], true),
    ef("strobe", "Strobe", ECat::Light, &[P_INT, P_SPEED], false),
    ef("light_leak", "Light Leak", ECat::Light, &[P_INT, P_SPEED], false),
    ef("edge_glow", "Neon Edges", ECat::Light, &[P_INT], false),
    ef("rainbow", "Rainbow", ECat::Party, &[P_INT, P_SPEED], false),
    ef("disco", "Disco", ECat::Party, &[P_INT, P_SPEED], false),
    ef("sparkle", "Sparkle", ECat::Party, &[P_INT, P_SPEED], false),
    ef("split2", "Split 2", ECat::Split, &[], false),
    ef("split3", "Split 3", ECat::Split, &[], false),
    ef("split4", "Split 4", ECat::Split, &[], true),
    ef("split9", "Split 9", ECat::Split, &[], false),
    ef("snow", "Snow", ECat::Nature, &[P_INT, P_SPEED], false),
    ef("rain", "Rain", ECat::Nature, &[P_INT, P_SPEED], false),
    ef("night_vision", "Night Vision", ECat::Nature, &[P_INT], false),
    ef("thermal", "Thermal", ECat::Nature, &[P_INT], false),
];

pub fn effect(id: &str) -> Option<&'static EffectDef> {
    EFFECTS.iter().find(|t| t.id == id)
}

// ---------------------------------------------------------------- filters

/// A colour look, applied with the same grading math as Adjust.
#[derive(Clone, Copy, Debug)]
pub struct Look {
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
    pub fade: f64,
    pub vibrance: f64,
    pub hue: f64,
    pub mono: f64,
    pub split_sh: [f32; 3],
    pub split_sh_amt: f64,
    pub split_hi: [f32; 3],
    pub split_hi_amt: f64,
    pub grain: f64,
    pub vignette: f64,
}

pub const LOOK0: Look = Look {
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
    fade: 0.0,
    vibrance: 0.0,
    hue: 0.0,
    mono: 0.0,
    split_sh: [0.0, 0.0, 0.0],
    split_sh_amt: 0.0,
    split_hi: [0.0, 0.0, 0.0],
    split_hi_amt: 0.0,
    grain: 0.0,
    vignette: 0.0,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FCat {
    Featured,
    Life,
    Movies,
    Retro,
    Mono,
    Night,
}

impl FCat {
    pub const ALL: [FCat; 6] = [FCat::Featured, FCat::Life, FCat::Movies, FCat::Retro, FCat::Mono, FCat::Night];
    pub fn label(self) -> &'static str {
        match self {
            FCat::Featured => "Featured",
            FCat::Life => "Life",
            FCat::Movies => "Movies",
            FCat::Retro => "Retro",
            FCat::Mono => "B&W",
            FCat::Night => "Night",
        }
    }
}

pub struct FilterDef {
    pub id: &'static str,
    pub name: &'static str,
    pub cat: FCat,
    pub look: Look,
}

macro_rules! look {
    ($($f:ident : $v:expr),* $(,)?) => { Look { $($f: $v,)* ..LOOK0 } };
}

pub static FILTERS: &[FilterDef] = &[
    FilterDef {
        id: "vivid",
        name: "Vivid",
        cat: FCat::Featured,
        look: look!(saturation: 0.35, contrast: 0.2, vibrance: 0.3, highlights: -0.1),
    },
    FilterDef {
        id: "clear",
        name: "Clear",
        cat: FCat::Featured,
        look: look!(contrast: 0.15, whites: 0.15, saturation: 0.1, shadows: 0.15),
    },
    FilterDef { id: "warm", name: "Warm", cat: FCat::Featured, look: look!(temperature: 0.45, saturation: 0.1, highlights: -0.1) },
    FilterDef { id: "cool", name: "Cool", cat: FCat::Featured, look: look!(temperature: -0.45, tint: 0.05, saturation: -0.05) },
    FilterDef {
        id: "cinematic",
        name: "Cinematic",
        cat: FCat::Featured,
        look: look!(contrast: 0.25, saturation: -0.1, split_sh: [0.0, 0.45, 0.55], split_sh_amt: 0.45, split_hi: [1.0, 0.62, 0.3], split_hi_amt: 0.35, blacks: -0.1, vignette: 0.25),
    },
    FilterDef {
        id: "moody",
        name: "Moody",
        cat: FCat::Featured,
        look: look!(exposure: -0.15, contrast: 0.3, saturation: -0.3, temperature: -0.15, fade: 0.15, vignette: 0.35),
    },
    FilterDef {
        id: "film",
        name: "Film",
        cat: FCat::Featured,
        look: look!(fade: 0.3, contrast: 0.1, saturation: -0.15, temperature: 0.1, grain: 0.25, split_hi: [1.0, 0.9, 0.7], split_hi_amt: 0.2),
    },
    FilterDef { id: "faded", name: "Faded", cat: FCat::Featured, look: look!(fade: 0.55, contrast: -0.2, saturation: -0.2) },
    FilterDef {
        id: "sunny",
        name: "Sunny",
        cat: FCat::Life,
        look: look!(exposure: 0.15, temperature: 0.35, saturation: 0.2, shadows: 0.2),
    },
    FilterDef {
        id: "fresh",
        name: "Fresh",
        cat: FCat::Life,
        look: look!(brightness: 0.1, saturation: 0.15, tint: -0.12, temperature: -0.1, vibrance: 0.2),
    },
    FilterDef {
        id: "pastel",
        name: "Pastel",
        cat: FCat::Life,
        look: look!(brightness: 0.18, contrast: -0.3, saturation: -0.15, fade: 0.25, tint: 0.08),
    },
    FilterDef {
        id: "bright",
        name: "Bright",
        cat: FCat::Life,
        look: look!(exposure: 0.3, shadows: 0.3, highlights: -0.2, saturation: 0.1),
    },
    FilterDef {
        id: "food",
        name: "Delicious",
        cat: FCat::Life,
        look: look!(saturation: 0.3, temperature: 0.25, contrast: 0.15, highlights: -0.15),
    },
    FilterDef {
        id: "teal_orange",
        name: "Teal & Orange",
        cat: FCat::Movies,
        look: look!(contrast: 0.2, split_sh: [0.0, 0.55, 0.6], split_sh_amt: 0.65, split_hi: [1.0, 0.55, 0.2], split_hi_amt: 0.5, saturation: 0.1),
    },
    FilterDef {
        id: "blockbuster",
        name: "Blockbuster",
        cat: FCat::Movies,
        look: look!(contrast: 0.4, saturation: -0.15, blacks: -0.2, temperature: -0.1, split_hi: [1.0, 0.75, 0.4], split_hi_amt: 0.3, vignette: 0.3),
    },
    FilterDef {
        id: "scifi",
        name: "Sci-Fi",
        cat: FCat::Movies,
        look: look!(temperature: -0.5, tint: -0.15, contrast: 0.25, saturation: -0.2, split_sh: [0.0, 0.6, 0.7], split_sh_amt: 0.5),
    },
    FilterDef {
        id: "western",
        name: "Western",
        cat: FCat::Movies,
        look: look!(temperature: 0.6, saturation: -0.25, contrast: 0.25, fade: 0.15, split_hi: [1.0, 0.8, 0.4], split_hi_amt: 0.35),
    },
    FilterDef {
        id: "matrix",
        name: "Matrix",
        cat: FCat::Movies,
        look: look!(tint: -0.6, saturation: -0.3, contrast: 0.3, split_sh: [0.1, 0.6, 0.2], split_sh_amt: 0.5),
    },
    FilterDef {
        id: "vintage",
        name: "Vintage",
        cat: FCat::Retro,
        look: look!(fade: 0.35, saturation: -0.3, temperature: 0.3, split_sh: [0.3, 0.2, 0.5], split_sh_amt: 0.25, grain: 0.3, vignette: 0.3),
    },
    FilterDef {
        id: "polaroid",
        name: "Polaroid",
        cat: FCat::Retro,
        look: look!(fade: 0.3, contrast: -0.1, temperature: 0.15, tint: 0.1, split_hi: [1.0, 0.95, 0.75], split_hi_amt: 0.3),
    },
    FilterDef {
        id: "seventies",
        name: "70s",
        cat: FCat::Retro,
        look: look!(temperature: 0.45, saturation: 0.15, fade: 0.25, hue: -0.03, grain: 0.35, split_sh: [0.5, 0.3, 0.1], split_sh_amt: 0.3),
    },
    FilterDef {
        id: "gold",
        name: "Gold",
        cat: FCat::Retro,
        look: look!(temperature: 0.35, contrast: 0.15, saturation: 0.2, split_hi: [1.0, 0.8, 0.3], split_hi_amt: 0.45),
    },
    FilterDef {
        id: "sepia",
        name: "Sepia",
        cat: FCat::Retro,
        look: look!(mono: 1.0, split_sh: [0.45, 0.3, 0.15], split_sh_amt: 0.7, split_hi: [1.0, 0.88, 0.65], split_hi_amt: 0.6, fade: 0.1),
    },
    FilterDef { id: "mono", name: "Mono", cat: FCat::Mono, look: look!(mono: 1.0) },
    FilterDef { id: "noir", name: "Noir", cat: FCat::Mono, look: look!(mono: 1.0, contrast: 0.55, blacks: -0.25, vignette: 0.4) },
    FilterDef { id: "silver", name: "Silver", cat: FCat::Mono, look: look!(mono: 1.0, fade: 0.25, contrast: -0.1, brightness: 0.1) },
    FilterDef {
        id: "bw_blue",
        name: "Cyanotype",
        cat: FCat::Mono,
        look: look!(mono: 1.0, split_sh: [0.0, 0.25, 0.6], split_sh_amt: 0.8, split_hi: [0.7, 0.85, 1.0], split_hi_amt: 0.5),
    },
    FilterDef {
        id: "neon",
        name: "Neon",
        cat: FCat::Night,
        look: look!(saturation: 0.45, contrast: 0.3, split_sh: [0.5, 0.0, 0.9], split_sh_amt: 0.45, split_hi: [0.1, 0.9, 1.0], split_hi_amt: 0.3),
    },
    FilterDef {
        id: "cyber",
        name: "Cyber",
        cat: FCat::Night,
        look: look!(tint: 0.35, temperature: -0.35, saturation: 0.3, contrast: 0.25, split_sh: [0.2, 0.0, 0.6], split_sh_amt: 0.4),
    },
    FilterDef {
        id: "midnight",
        name: "Midnight",
        cat: FCat::Night,
        look: look!(exposure: -0.25, temperature: -0.45, saturation: -0.2, contrast: 0.2, vignette: 0.4),
    },
    FilterDef {
        id: "night_glow",
        name: "City Lights",
        cat: FCat::Night,
        look: look!(highlights: 0.25, shadows: -0.15, saturation: 0.25, temperature: 0.15, split_hi: [1.0, 0.6, 0.3], split_hi_amt: 0.25),
    },
];

pub fn filter(id: &str) -> Option<&'static FilterDef> {
    FILTERS.iter().find(|t| t.id == id)
}

// ---------------------------------------------------------------- stickers

pub struct StickerDef {
    pub id: &'static str,
    pub name: &'static str,
    /// Emoji stickers are rendered with the colour emoji font.
    pub emoji: Option<&'static str>,
    pub default_color: [u8; 4],
}

const fn st(id: &'static str, name: &'static str, c: [u8; 4]) -> StickerDef {
    StickerDef { id, name, emoji: None, default_color: c }
}
const fn em(id: &'static str, e: &'static str) -> StickerDef {
    StickerDef { id, name: e, emoji: Some(e), default_color: [255, 255, 255, 255] }
}

pub static STICKERS: &[StickerDef] = &[
    st("heart", "Heart", [255, 64, 96, 255]),
    st("star", "Star", [255, 205, 40, 255]),
    st("sparkle", "Sparkle", [255, 255, 255, 255]),
    st("circle", "Circle", [32, 213, 236, 255]),
    st("ring", "Ring", [255, 255, 255, 255]),
    st("arrow", "Arrow", [255, 214, 0, 255]),
    st("arrow_curve", "Curved arrow", [255, 80, 80, 255]),
    st("bubble", "Speech bubble", [255, 255, 255, 255]),
    st("thought", "Thought bubble", [255, 255, 255, 255]),
    st("bolt", "Lightning", [255, 220, 30, 255]),
    st("check", "Check", [60, 220, 120, 255]),
    st("cross", "Cross", [255, 70, 70, 255]),
    st("burst", "Burst", [255, 120, 30, 255]),
    st("subscribe", "Subscribe", [230, 33, 23, 255]),
    st("like", "Like", [32, 120, 255, 255]),
    st("location", "Pin", [255, 70, 90, 255]),
    st("crown", "Crown", [255, 200, 40, 255]),
    st("underline", "Brush line", [255, 214, 0, 255]),
    em("e_fire", "🔥"),
    em("e_heart_eyes", "😍"),
    em("e_joy", "😂"),
    em("e_party", "🎉"),
    em("e_100", "💯"),
    em("e_thumbs", "👍"),
    em("e_sparkles", "✨"),
    em("e_star_eyes", "🤩"),
    em("e_cool", "😎"),
    em("e_mind", "🤯"),
    em("e_clap", "👏"),
    em("e_eyes", "👀"),
    em("e_skull", "💀"),
    em("e_rocket", "🚀"),
    em("e_crown", "👑"),
    em("e_music", "🎵"),
];

pub fn sticker(id: &str) -> Option<&'static StickerDef> {
    STICKERS.iter().find(|s| s.id == id)
}

pub fn sticker_name(id: &str) -> &'static str {
    sticker(id).map(|s| s.name).unwrap_or("Sticker")
}

// ---------------------------------------------------------------- text styles

pub struct TextPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub color: [u8; 4],
    pub stroke: Option<([u8; 4], f64)>,
    pub bg: Option<[u8; 4]>,
    pub shadow: bool,
    pub bold: bool,
    pub italic: bool,
}

const fn tp(
    id: &'static str,
    name: &'static str,
    color: [u8; 4],
    stroke: Option<([u8; 4], f64)>,
    bg: Option<[u8; 4]>,
    shadow: bool,
    bold: bool,
) -> TextPreset {
    TextPreset { id, name, color, stroke, bg, shadow, bold, italic: false }
}

pub static TEXT_PRESETS: &[TextPreset] = &[
    tp("default", "Default text", [255, 255, 255, 255], None, None, false, false),
    tp("outline", "Outline", [255, 255, 255, 255], Some(([0, 0, 0, 255], 0.5)), None, false, true),
    tp("yellow_pop", "Yellow pop", [255, 221, 0, 255], Some(([0, 0, 0, 255], 0.6)), None, true, true),
    tp("caption_box", "Caption box", [255, 255, 255, 255], None, Some([0, 0, 0, 170]), false, true),
    tp("white_box", "White box", [20, 20, 20, 255], None, Some([255, 255, 255, 255]), false, true),
    tp("neon_cyan", "Neon", [180, 255, 255, 255], Some(([0, 200, 255, 255], 0.35)), None, true, true),
    tp("red_title", "Red title", [255, 255, 255, 255], Some(([230, 30, 50, 255], 0.7)), None, true, true),
    tp("soft_shadow", "Soft shadow", [255, 255, 255, 255], None, None, true, false),
    tp("pink_box", "Pink box", [255, 255, 255, 255], None, Some([255, 70, 140, 230]), false, true),
    tp("green_glow", "Lime", [200, 255, 80, 255], Some(([20, 60, 0, 255], 0.5)), None, true, true),
    tp("cinema", "Cinema", [240, 230, 210, 255], None, None, true, false),
    tp("purple", "Purple", [230, 200, 255, 255], Some(([110, 40, 200, 255], 0.5)), None, false, true),
];

// ---------------------------------------------------------------- animated caption styles

pub struct CaptionStyle {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub color: [u8; 4],
    /// Colour of the spoken word (box / underline / glow colour for those styles).
    pub hl: [u8; 4],
    pub stroke: Option<([u8; 4], f64)>,
    pub bg: Option<[u8; 4]>,
    pub shadow: bool,
}

const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

#[rustfmt::skip]
pub static CAPTION_STYLES: &[CaptionStyle] = &[
    CaptionStyle { id: "karaoke", name: "Karaoke", desc: "Each word fills with colour as it is spoken", color: WHITE, hl: [255, 221, 0, 255], stroke: Some((BLACK, 0.4)), bg: None, shadow: true },
    CaptionStyle { id: "highlight", name: "Highlight", desc: "The spoken word changes colour", color: WHITE, hl: [90, 255, 120, 255], stroke: Some((BLACK, 0.45)), bg: None, shadow: false },
    CaptionStyle { id: "box", name: "Word box", desc: "A coloured box slides under the spoken word", color: WHITE, hl: [124, 77, 255, 255], stroke: None, bg: None, shadow: true },
    CaptionStyle { id: "pop", name: "Pop", desc: "The spoken word grows and changes colour", color: WHITE, hl: [255, 221, 0, 255], stroke: Some((BLACK, 0.5)), bg: None, shadow: true },
    CaptionStyle { id: "bounce", name: "Bounce", desc: "Words jump in as they are spoken", color: WHITE, hl: [0, 220, 255, 255], stroke: Some((BLACK, 0.45)), bg: None, shadow: true },
    CaptionStyle { id: "reveal", name: "Reveal", desc: "Words fade in as they are spoken", color: WHITE, hl: WHITE, stroke: None, bg: None, shadow: true },
    CaptionStyle { id: "single", name: "One word", desc: "Only the spoken word, big, in the middle", color: WHITE, hl: [255, 221, 0, 255], stroke: Some((BLACK, 0.6)), bg: None, shadow: true },
    CaptionStyle { id: "glow", name: "Glow", desc: "The spoken word glows", color: WHITE, hl: [255, 64, 200, 255], stroke: None, bg: None, shadow: false },
    CaptionStyle { id: "underline", name: "Underline", desc: "A bar slides under the spoken word", color: WHITE, hl: [255, 82, 82, 255], stroke: Some((BLACK, 0.35)), bg: None, shadow: true },
    CaptionStyle { id: "dim", name: "Spotlight", desc: "Words still to come are dimmed", color: WHITE, hl: WHITE, stroke: Some((BLACK, 0.35)), bg: None, shadow: true },
    CaptionStyle { id: "bubble", name: "Bubble", desc: "White box, the spoken word in colour", color: [24, 24, 28, 255], hl: [255, 90, 40, 255], stroke: None, bg: Some(WHITE), shadow: false },
    CaptionStyle { id: "classic", name: "Classic", desc: "Plain subtitles, no animation", color: WHITE, hl: WHITE, stroke: Some((BLACK, 0.4)), bg: None, shadow: true },
];

pub fn caption_style(id: &str) -> Option<&'static CaptionStyle> {
    CAPTION_STYLES.iter().find(|s| s.id == id)
}

impl CaptionStyle {
    /// Give a text the look of this caption style (keeps its words, font and size).
    pub fn apply(&self, td: &mut crate::model::TextData) {
        td.caption = self.id.into();
        td.color = self.color;
        td.hl_color = self.hl;
        td.stroke = self.stroke.is_some();
        if let Some((c, w)) = self.stroke {
            td.stroke_color = c;
            td.stroke_width = w;
        }
        td.background = self.bg.is_some();
        if let Some(c) = self.bg {
            td.bg_color = c;
            td.bg_round = 0.35;
        }
        td.shadow = self.shadow;
        td.bold = true;
    }
}

// ---------------------------------------------------------------- voice effects (ffmpeg filters)

pub struct VoiceDef {
    pub id: &'static str,
    pub name: &'static str,
    pub filter: &'static str,
}

pub static VOICES: &[VoiceDef] = &[
    VoiceDef { id: "chipmunk", name: "Chipmunk", filter: "asetrate=48000*1.45,aresample=48000,atempo=1/1.45" },
    VoiceDef { id: "deep", name: "Deep", filter: "asetrate=48000*0.75,aresample=48000,atempo=1/0.75" },
    VoiceDef {
        id: "robot",
        name: "Robot",
        filter: "afftfilt=real='hypot(re,im)*sin(0)':imag='hypot(re,im)*cos(0)':win_size=512:overlap=0.75,volume=1.4",
    },
    VoiceDef { id: "echo", name: "Echo", filter: "aecho=0.8:0.85:120|240:0.5|0.3" },
    VoiceDef { id: "hall", name: "Big Hall", filter: "aecho=0.8:0.9:40|80|160|320:0.4|0.3|0.2|0.1" },
    VoiceDef { id: "megaphone", name: "Megaphone", filter: "highpass=f=500,lowpass=f=3200,acrusher=bits=8:mix=0.25,volume=1.8" },
    VoiceDef { id: "telephone", name: "Telephone", filter: "highpass=f=400,lowpass=f=3000,volume=1.5" },
    VoiceDef { id: "underwater", name: "Underwater", filter: "lowpass=f=450,aecho=0.7:0.6:60:0.4,volume=1.6" },
    VoiceDef { id: "vibrato", name: "Vibrato", filter: "vibrato=f=6:d=0.6" },
    VoiceDef { id: "tremolo", name: "Tremolo", filter: "tremolo=f=8:d=0.7" },
];

pub fn voice(id: &str) -> Option<&'static VoiceDef> {
    VOICES.iter().find(|v| v.id == id)
}

// ---------------------------------------------------------------- speed curves ("Curve")

pub struct CurveDef {
    pub id: &'static str,
    pub name: &'static str,
    pub points: &'static [(f64, f64)],
}

pub static SPEED_CURVES: &[CurveDef] = &[
    CurveDef { id: "montage", name: "Montage", points: &[(0.0, 1.0), (0.25, 0.4), (0.5, 3.0), (0.75, 0.4), (1.0, 1.0)] },
    CurveDef { id: "hero", name: "Hero", points: &[(0.0, 1.5), (0.35, 1.5), (0.5, 0.2), (0.65, 1.5), (1.0, 1.5)] },
    CurveDef { id: "bullet", name: "Bullet", points: &[(0.0, 2.5), (0.3, 2.5), (0.45, 0.25), (0.6, 0.25), (0.75, 2.5), (1.0, 2.5)] },
    CurveDef { id: "jump_cut", name: "Jump Cut", points: &[(0.0, 1.0), (0.4, 1.0), (0.5, 6.0), (0.6, 1.0), (1.0, 1.0)] },
    CurveDef { id: "flash_in", name: "Flash In", points: &[(0.0, 5.0), (0.35, 5.0), (0.55, 1.0), (1.0, 1.0)] },
    CurveDef { id: "flash_out", name: "Flash Out", points: &[(0.0, 1.0), (0.45, 1.0), (0.65, 5.0), (1.0, 5.0)] },
    CurveDef { id: "velocity", name: "Velocity", points: &[(0.0, 0.3), (0.3, 0.3), (0.45, 4.0), (0.55, 4.0), (0.7, 0.3), (1.0, 0.3)] },
];

// ---------------------------------------------------------------- sound effects (synthesised)

pub struct SfxDef {
    pub id: &'static str,
    pub name: &'static str,
    pub dur: Us,
}

pub static SFX: &[SfxDef] = &[
    SfxDef { id: "whoosh", name: "Whoosh", dur: SEC * 9 / 10 },
    SfxDef { id: "swoosh", name: "Swoosh (short)", dur: SEC * 45 / 100 },
    SfxDef { id: "whip", name: "Whip", dur: SEC * 4 / 10 },
    SfxDef { id: "pop", name: "Pop", dur: SEC / 4 },
    SfxDef { id: "click", name: "Click", dur: SEC * 12 / 100 },
    SfxDef { id: "ding", name: "Ding", dur: SEC * 16 / 10 },
    SfxDef { id: "boom", name: "Boom", dur: SEC * 2 },
    SfxDef { id: "riser", name: "Riser", dur: SEC * 2 },
    SfxDef { id: "shutter", name: "Camera shutter", dur: SEC * 35 / 100 },
    SfxDef { id: "glitch_sfx", name: "Glitch", dur: SEC / 2 },
    SfxDef { id: "notify", name: "Notification", dur: SEC * 9 / 10 },
    SfxDef { id: "heartbeat_sfx", name: "Heartbeat", dur: SEC * 2 },
];
