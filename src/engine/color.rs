//! Colour grading parameters.
//!
//! The slider -> shader parameter mappings are re-implemented from the maths
//! of the "Adjust" panel (combined colour-adjust effect): the
//! brightness curve `1 - (1-x)^p`, the sigmoid contrast around pivot 0.435,
//! the saturation matrix with luminance weights (0.2085, 0.7021, 0.0894),
//! polynomial highlight/shadow curves, the black/white control points and a
//! Bradford chromatic adaptation for temperature/tint.

use crate::model::catalog::Look;
use crate::model::Adjust;

/// 12 vec4 blocks consumed by `grade()` in shaders.wgsl.
pub type GradeBlock = [[f32; 4]; 12];

pub fn identity_block() -> GradeBlock {
    let mut g = [[0.0f32; 4]; 12];
    g[2] = [0.0, 1.0, 0.435, 1.0]; // pivot slope, pivot, saturation=1
    g[3] = [0.0, 0.0, 1.0, 0.0]; // highlight off, shadow off, bw slope 1, bias 0
    g
}

fn brightness_param(i: f64) -> f64 {
    if i > 0.0 && i <= 0.7 {
        0.3 * i
    } else if i > 0.7 {
        0.6333 * i - 0.2333
    } else {
        i
    }
}

fn contrast_xfactor(param: f64) -> f64 {
    (8.33 * param - 12.16).exp() + 5.82 * param - 1.72
}

fn contrast_sigmoid(x: f64, param: f64, pivot: f64) -> (f64, f64) {
    let a = contrast_xfactor(param);
    let s = 1.0 / (1.0 + (-a * (x - pivot)).exp());
    (s + pivot - 0.5, a * s * (1.0 - s))
}

/// Returns (param, xfactor, left_diff, right_diff, left_slope_diff, right_slope_diff, pivot_slope)
fn contrast_params(i: f64, pivot: f64) -> [f64; 7] {
    let param = if i <= 0.0 { i * 0.4 + 1.0 } else { i * 0.6 + 1.0 };
    let xf = contrast_xfactor(param);
    let (lv, ls) = contrast_sigmoid(0.0, param, pivot);
    let (rv, rs) = contrast_sigmoid(1.0, param, pivot);
    let (_pv, ps) = contrast_sigmoid(pivot, param, pivot);
    [param, xf, 0.0 - lv, 1.0 - rv, ps - ls, ps - rs, ps]
}

fn highlight_param(p: f64) -> f64 {
    1.0 + 0.503 * p + 0.183 * p * p + 0.147 * p.powi(3) + 0.067 * p.powi(4)
}

fn shadow_param(p: f64) -> f64 {
    1.0 - 0.503 * p + 0.183 * p * p - 0.147 * p.powi(3) + 0.067 * p.powi(4)
}

fn black_white_params(black: f64, white: f64) -> (f64, f64) {
    const MAX_RANGE: f64 = 0.5;
    const EPS: f64 = 0.005;
    let (mut xw, mut yw) = if white >= 0.0 { (1.0 - white * MAX_RANGE, 1.0) } else { (1.0, 1.0 + white * MAX_RANGE) };
    let (mut xb, mut yb) = if black >= 0.0 { (0.0, black * MAX_RANGE) } else { (-black * MAX_RANGE, 0.0) };
    xw = xw.max(EPS);
    yw = yw.max(EPS);
    xb = xb.min(xw - EPS);
    yb = yb.min(yw - EPS);
    let slope = (yw - yb) / (xw - xb);
    (slope, yb - slope * xb)
}

// Robertson isotemperature lines: (mired, u, v, slope) - standard colorimetry table.
const ROBERTSON: [[f64; 4]; 31] = [
    [0.0, 0.18006, 0.26352, -0.24341],
    [10.0, 0.18066, 0.26589, -0.25479],
    [20.0, 0.18133, 0.26846, -0.26876],
    [30.0, 0.18208, 0.27119, -0.28539],
    [40.0, 0.18293, 0.27407, -0.30470],
    [50.0, 0.18388, 0.27709, -0.32675],
    [60.0, 0.18494, 0.28021, -0.35156],
    [70.0, 0.18611, 0.28342, -0.37915],
    [80.0, 0.18740, 0.28668, -0.40955],
    [90.0, 0.18880, 0.28997, -0.44278],
    [100.0, 0.19032, 0.29326, -0.47888],
    [125.0, 0.19462, 0.30141, -0.58204],
    [150.0, 0.19962, 0.30921, -0.70471],
    [175.0, 0.20525, 0.31647, -0.84901],
    [200.0, 0.21142, 0.32312, -1.0182],
    [225.0, 0.21807, 0.32909, -1.2168],
    [250.0, 0.22511, 0.33439, -1.4512],
    [275.0, 0.23247, 0.33904, -1.7298],
    [300.0, 0.24010, 0.34308, -2.0637],
    [325.0, 0.24792, 0.34655, -2.4681],
    [350.0, 0.25591, 0.34951, -2.9641],
    [375.0, 0.26400, 0.35200, -3.5814],
    [400.0, 0.27218, 0.35407, -4.3633],
    [425.0, 0.28039, 0.35577, -5.3762],
    [450.0, 0.28863, 0.35714, -6.7262],
    [475.0, 0.29685, 0.35823, -8.5955],
    [500.0, 0.30505, 0.35907, -11.324],
    [525.0, 0.31320, 0.35968, -15.628],
    [550.0, 0.32129, 0.36011, -23.325],
    [575.0, 0.32931, 0.36038, -40.770],
    [600.0, 0.33724, 0.36051, -116.45],
];

type M3 = [[f64; 3]; 3];

const BRADFORD: M3 = [[0.8951, 0.2664, -0.1614], [-0.7502, 1.7135, 0.0367], [0.0389, -0.0685, 1.0296]];
const BRADFORD_INV: M3 = [[0.987, -0.1471, 0.16], [0.4323, 0.5184, 0.0493], [-0.0085, 0.04, 0.9685]];
const SRGB_TO_XYZ: M3 = [
    [0.4123907992659595, 0.357584339383878, 0.1804807884018343],
    [0.21263900587151036, 0.715168678767756, 0.07219231536073371],
    [0.019330818715591832, 0.11919477979462598, 0.9505321522496607],
];
const XYZ_TO_SRGB: M3 = [
    [3.2409699419045213, -1.5373831775700935, -0.4986107602930033],
    [-0.9692436362808796, 1.8759675015077208, 0.04155505740717562],
    [0.05563007969699364, -0.20397695888897655, 1.0569715142428784],
];

fn mul(a: &M3, b: &M3) -> M3 {
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    r
}

fn mulv(a: &M3, v: [f64; 3]) -> [f64; 3] {
    [
        a[0][0] * v[0] + a[0][1] * v[1] + a[0][2] * v[2],
        a[1][0] * v[0] + a[1][1] * v[1] + a[1][2] * v[2],
        a[2][0] * v[0] + a[2][1] * v[1] + a[2][2] * v[2],
    ]
}

/// White point (XYZ with Y=1) of a correlated colour temperature and tint.
fn white_xyz(temperature: f64, tint: f64) -> [f64; 3] {
    let x = 1_000_000.0 / temperature;
    let y = tint * 0.0001;
    // first row j>=1 whose mired exceeds x
    let mut j = 1;
    while j < ROBERTSON.len() - 1 && ROBERTSON[j][0] <= x {
        j += 1;
    }
    let (r0, r1) = (ROBERTSON[j - 1], ROBERTSON[j]);
    let f = ((r1[0] - x) / (r1[0] - r0[0])).clamp(0.0, 1.0);
    let mix = |a: f64, b: f64, t: f64| a * (1.0 - t) + b * t;
    let mut uv = [mix(r1[1], r0[1], f), mix(r1[2], r0[2], f)];
    let (a, b) = (r0[3], r1[3]);
    let (sqa, sqb) = ((a * a + 1.0).sqrt(), (b * b + 1.0).sqrt());
    let dir = [mix(1.0 / sqb, 1.0 / sqa, f), mix(b / sqb, a / sqa, f)];
    let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
    uv[0] += y * dir[0] / len;
    uv[1] += y * dir[1] / len;
    let den = -4.0 * uv[1] + uv[0] + 2.0;
    let mut xx = (uv[0] * 1.5 / den).clamp(0.000001, 0.999999);
    let mut yy = (uv[1] / den).clamp(0.000001, 0.999999);
    if xx + yy > 0.999999 {
        let t = 0.999999 / (xx + yy);
        xx *= t;
        yy *= t;
    }
    [xx / yy, 1.0, (1.0 - xx - yy) / yy]
}

/// 3x3 RGB matrix for the temperature/tint sliders (-1..1).
pub fn temperature_tint_matrix(temp_i: f64, tint_i: f64) -> M3 {
    let t = temp_i.clamp(-1.0, 1.0);
    let temperature = 6500.0 - 1970.0 * t + 876.0 * t * t - 2630.0 * t * t * t;
    let tint = tint_i.clamp(-1.0, 1.0) * 100.0;
    let w = mulv(&BRADFORD, white_xyz(temperature, tint));
    let wb = mulv(&BRADFORD, white_xyz(6500.0, 0.0));
    let d: M3 = [[w[0] / wb[0], 0.0, 0.0], [0.0, w[1] / wb[1], 0.0], [0.0, 0.0, w[2] / wb[2]]];
    let m = mul(&d, &BRADFORD);
    let m = mul(&BRADFORD_INV, &m);
    let m = mul(&m, &SRGB_TO_XYZ);
    mul(&XYZ_TO_SRGB, &m)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GradeInput {
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
    pub sharpen: f64,
    pub clarity: f64,
    pub vignette: f64,
    pub grain: f64,
}

impl From<&Adjust> for GradeInput {
    fn from(a: &Adjust) -> Self {
        GradeInput {
            brightness: a.brightness,
            contrast: a.contrast,
            saturation: a.saturation,
            exposure: a.exposure,
            temperature: a.temperature,
            tint: a.tint,
            highlights: a.highlights,
            shadows: a.shadows,
            whites: a.whites,
            blacks: a.blacks,
            fade: a.fade,
            hue: a.hue,
            sharpen: a.sharpen,
            clarity: a.clarity,
            vignette: a.vignette,
            grain: a.grain,
            ..Default::default()
        }
    }
}

impl From<&Look> for GradeInput {
    fn from(l: &Look) -> Self {
        GradeInput {
            brightness: l.brightness,
            contrast: l.contrast,
            saturation: l.saturation,
            exposure: l.exposure,
            temperature: l.temperature,
            tint: l.tint,
            highlights: l.highlights,
            shadows: l.shadows,
            whites: l.whites,
            blacks: l.blacks,
            fade: l.fade,
            vibrance: l.vibrance,
            hue: l.hue,
            mono: l.mono,
            split_sh: l.split_sh,
            split_sh_amt: l.split_sh_amt,
            split_hi: l.split_hi,
            split_hi_amt: l.split_hi_amt,
            vignette: l.vignette,
            grain: l.grain,
            ..Default::default()
        }
    }
}

const EPS: f64 = 1e-5;

/// Build the shader block. Layout must match `grade()` in shaders.wgsl.
pub fn block(g: &GradeInput) -> GradeBlock {
    let mut b = identity_block();
    if g.brightness.abs() > EPS {
        b[0][0] = 1.0;
        b[0][1] = brightness_param(g.brightness.clamp(-1.0, 1.0)) as f32;
    }
    if g.contrast.abs() > EPS {
        let c = contrast_params(g.contrast.clamp(-1.0, 1.0), 0.435);
        b[0][2] = 1.0;
        b[0][3] = c[0] as f32;
        b[1] = [c[1] as f32, c[2] as f32, c[3] as f32, c[4] as f32];
        b[2][0] = c[5] as f32;
        b[2][1] = c[6] as f32;
    }
    b[2][2] = 0.435;
    b[2][3] = (g.saturation.clamp(-1.0, 1.0) + 1.0) as f32;
    if g.highlights.abs() > EPS {
        b[3][0] = highlight_param(g.highlights.clamp(-1.0, 1.0)) as f32;
    }
    if g.shadows.abs() > EPS {
        b[3][1] = shadow_param(g.shadows.clamp(-1.0, 1.0)) as f32;
    }
    if g.whites.abs() > EPS || g.blacks.abs() > EPS {
        let (s, o) = black_white_params(g.blacks.clamp(-1.0, 1.0), g.whites.clamp(-1.0, 1.0));
        b[3][2] = s as f32;
        b[3][3] = o as f32;
    }
    if g.temperature.abs() > EPS || g.tint.abs() > EPS {
        let m = temperature_tint_matrix(g.temperature, g.tint);
        for r in 0..3 {
            b[4 + r] = [m[r][0] as f32, m[r][1] as f32, m[r][2] as f32, 1.0];
        }
    }
    // exposure in stops (slider -1..1 -> -2..2 stops), fade, hue (turns), vibrance
    b[7] = [(g.exposure * 2.0) as f32, g.fade.clamp(0.0, 1.0) as f32, (g.hue * 0.5) as f32, g.vibrance as f32];
    b[8] = [g.split_sh[0], g.split_sh[1], g.split_sh[2], g.split_sh_amt as f32];
    b[9] = [g.split_hi[0], g.split_hi[1], g.split_hi[2], g.split_hi_amt as f32];
    b[10] = [g.mono as f32, g.clarity as f32, g.sharpen as f32, 0.0];
    b[11] = [g.vignette as f32, g.grain as f32, 0.0, 0.0];
    b
}

pub fn has_spatial(g: &GradeInput) -> bool {
    g.sharpen.abs() > EPS || g.clarity.abs() > EPS
}

pub fn is_identity(g: &GradeInput) -> bool {
    let z = |v: f64| v.abs() <= EPS;
    z(g.brightness)
        && z(g.contrast)
        && z(g.saturation)
        && z(g.exposure)
        && z(g.temperature)
        && z(g.tint)
        && z(g.highlights)
        && z(g.shadows)
        && z(g.whites)
        && z(g.blacks)
        && z(g.fade)
        && z(g.vibrance)
        && z(g.hue)
        && z(g.mono)
        && z(g.split_sh_amt)
        && z(g.split_hi_amt)
        && z(g.sharpen)
        && z(g.clarity)
        && z(g.vignette)
        && z(g.grain)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neutral_temperature_is_identity() {
        let m = temperature_tint_matrix(0.0, 0.0);
        for i in 0..3 {
            for j in 0..3 {
                let e = if i == j { 1.0 } else { 0.0 };
                assert!((m[i][j] - e).abs() < 1e-3, "{:?}", m);
            }
        }
    }
    #[test]
    fn warm_boosts_red() {
        let m = temperature_tint_matrix(1.0, 0.0);
        let white = [m[0][0] + m[0][1] + m[0][2], m[1][0] + m[1][1] + m[1][2], m[2][0] + m[2][1] + m[2][2]];
        assert!(white[0] > white[2], "{:?}", white);
    }
}
