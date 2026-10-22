//! Procedurally synthesised sound effects (whooshes for transitions, pops, booms…).
//! Generated once into the cache directory as 48 kHz stereo WAV files.
//!
//! The recipes imitate how these sounds are made for real: a whoosh is air moving past a
//! microphone — broadband (pink) noise whose low-pass opens and closes as the object passes,
//! with a Doppler drop, turbulence and a left→right pan — and every sound gets a little room
//! (a small Schroeder reverb) so it does not sound like a dry synthesiser.

use crate::util;
use std::f32::consts::{PI, TAU};
use std::path::PathBuf;

/// Bump when the recipes change: new files are generated, projects are moved to them.
const VERSION: u32 = 2;
const SR: f32 = 48_000.0;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

/// Pink noise (Paul Kellet's filter): equal energy per octave, much fuller than white noise.
struct Pink {
    rng: Rng,
    b: [f32; 7],
}
impl Pink {
    fn new(seed: u64) -> Self {
        Pink { rng: Rng(seed | 1), b: [0.0; 7] }
    }
    fn next(&mut self) -> f32 {
        let w = self.rng.next();
        let b = &mut self.b;
        b[0] = 0.99886 * b[0] + w * 0.0555179;
        b[1] = 0.99332 * b[1] + w * 0.0750759;
        b[2] = 0.96900 * b[2] + w * 0.1538520;
        b[3] = 0.86650 * b[3] + w * 0.3104856;
        b[4] = 0.55000 * b[4] + w * 0.5329522;
        b[5] = -0.7616 * b[5] - w * 0.0168980;
        let out = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
        b[6] = w * 0.115926;
        out * 0.11
    }
}

/// Brown noise: a slow random walk (rumble).
struct Brown {
    rng: Rng,
    y: f32,
}
impl Brown {
    fn next(&mut self) -> f32 {
        self.y = (self.y + self.rng.next() * 0.02) * 0.998;
        self.y * 3.5
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Low,
    Band,
    High,
}

/// RBJ biquad whose frequency may change every sample.
struct Biquad {
    kind: Kind,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}
impl Biquad {
    fn new(kind: Kind) -> Self {
        Biquad { kind, x1: 0.0, x2: 0.0, y1: 0.0, y2: 0.0 }
    }
    fn run(&mut self, x: f32, f: f32, q: f32) -> f32 {
        let w = TAU * (f / SR).clamp(0.0003, 0.48);
        let (sn, cs) = w.sin_cos();
        let alpha = sn / (2.0 * q);
        let (b0, b1, b2) = match self.kind {
            Kind::Low => ((1.0 - cs) / 2.0, 1.0 - cs, (1.0 - cs) / 2.0),
            Kind::Band => (alpha, 0.0, -alpha),
            Kind::High => ((1.0 + cs) / 2.0, -(1.0 + cs), (1.0 + cs) / 2.0),
        };
        let (a0, a1, a2) = (1.0 + alpha, -2.0 * cs, 1.0 - alpha);
        let y = (b0 * x + b1 * self.x1 + b2 * self.x2 - a1 * self.y1 - a2 * self.y2) / a0;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
}

/// Smoothed random value changing `rate` times per second (turbulence, flutter).
struct Wobble {
    rng: Rng,
    cur: f32,
    target: f32,
    k: f32,
}
impl Wobble {
    fn new(seed: u64, rate: f32) -> Self {
        Wobble { rng: Rng(seed | 1), cur: 0.0, target: 0.0, k: (rate / SR).min(1.0) }
    }
    fn next(&mut self) -> f32 {
        if self.rng.next().abs() < self.k * 2.0 {
            self.target = self.rng.next();
        }
        self.cur += (self.target - self.cur) * self.k * 6.0;
        self.cur
    }
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Swell that peaks at `peak` (0..1): slow rise, smooth fall.
fn swell(p: f32, peak: f32, rise: f32, fall: f32) -> f32 {
    if p < peak {
        (p / peak).powf(rise) * smoothstep(p / peak)
    } else {
        ((1.0 - p) / (1.0 - peak)).max(0.0).powf(fall)
    }
}

/// Equal-power pan, -1 = left, 1 = right.
fn pan(s: f32, x: f32) -> (f32, f32) {
    let a = (x.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
    (s * a.cos(), s * a.sin())
}

fn sat(x: f32) -> f32 {
    x.tanh()
}

/// Small stereo room (Schroeder: 4 combs + 2 all-passes per side), `wet` 0..1.
fn reverb(dry: &mut Vec<(f32, f32)>, wet: f32, size: f32, tail_s: f32) {
    if wet <= 0.0 {
        return;
    }
    dry.extend(std::iter::repeat((0.0, 0.0)).take((tail_s * SR) as usize));
    let combs_l = [1116, 1188, 1277, 1356];
    let aps = [556, 441];
    let side = |input: &mut dyn Iterator<Item = f32>, spread: usize| -> Vec<f32> {
        let x: Vec<f32> = input.collect();
        let mut out = vec![0.0f32; x.len()];
        for &c in &combs_l {
            let len = (((c + spread) as f32) * size) as usize + 1;
            let mut buf = vec![0.0f32; len];
            let (mut idx, mut lp) = (0usize, 0.0f32);
            for (i, v) in x.iter().enumerate() {
                let y = buf[idx];
                lp = y * 0.6 + lp * 0.4; // damping: the room absorbs highs
                buf[idx] = v + lp * 0.80;
                idx = (idx + 1) % len;
                out[i] += y * 0.25;
            }
        }
        for &a in &aps {
            let len = a + spread / 2;
            let mut buf = vec![0.0f32; len];
            let mut idx = 0usize;
            for v in out.iter_mut() {
                let b = buf[idx];
                let y = -*v + b;
                buf[idx] = *v + b * 0.5;
                idx = (idx + 1) % len;
                *v = y;
            }
        }
        out
    };
    let l = side(&mut dry.iter().map(|s| s.0), 0);
    let r = side(&mut dry.iter().map(|s| s.1), 23);
    for (i, s) in dry.iter_mut().enumerate() {
        s.0 += l[i] * wet;
        s.1 += r[i] * wet;
    }
}

/// Moving air: `f_lo`→`f_hi` is how far the low-pass opens as the object passes.
fn air_pass(dur: f32, seed: u64, f_lo: f32, f_hi: f32, peak: f32, body: f32, pan_width: f32) -> Vec<(f32, f32)> {
    let n = (dur * SR) as usize;
    let mut pink = Pink::new(seed);
    let mut brown = Brown { rng: Rng(seed ^ 0xABCD), y: 0.0 };
    let mut turb = Wobble::new(seed ^ 0x1234, 28.0);
    let mut flutter = Wobble::new(seed ^ 0x9876, 70.0);
    let (mut lp1, mut lp2, mut bp, mut hp) =
        (Biquad::new(Kind::Low), Biquad::new(Kind::Low), Biquad::new(Kind::Band), Biquad::new(Kind::High));
    let mut far = Biquad::new(Kind::Low);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let p = i as f32 / n as f32;
        let env = swell(p, peak, 1.6, 1.4);
        // the filter opens slightly before the loudest point and closes lower than it started
        // (the pitch falls once the object has passed: Doppler)
        let open = swell(p, peak * 0.92, 1.3, 1.1);
        let doppler = if p > peak { 1.0 - 0.35 * smoothstep((p - peak) / (1.0 - peak)) } else { 1.0 };
        let fc = (f_lo + (f_hi - f_lo) * open) * doppler;
        let tb = 1.0 + turb.next() * 0.35 + flutter.next() * 0.12;
        let x = pink.next();
        let air = lp2.run(lp1.run(x, fc, 0.6), fc * 1.15, 0.7);
        // a hint of resonance where the air "sings" around the object
        let tone = bp.run(x, fc * 0.8, 2.2) * 0.35;
        let rumble = brown.next() * body * open;
        let s = hp.run((air + tone) * tb + rumble, 45.0, 0.7) * env;
        // left → right, the far side duller
        let x_pan = (smoothstep(p) * 2.0 - 1.0) * pan_width;
        let (l, r) = pan(s, x_pan);
        let dull = far.run(if x_pan > 0.0 { l } else { r }, fc * 0.6 + 200.0, 0.7);
        out.push(if x_pan > 0.0 { (l * 0.4 + dull * 0.6, r) } else { (l, r * 0.4 + dull * 0.6) });
    }
    out
}

/// Returns stereo samples (reverb tail included).
fn synth(id: &str, dur: f32) -> Vec<(f32, f32)> {
    let seed = 0x9E3779B97F4A7C15 ^ util::hash_str(id);
    let n = (dur * SR) as usize;
    let mut rng = Rng(seed);
    let mut pink = Pink::new(seed ^ 77);
    let (mut f1, mut f2, mut f3) = (Biquad::new(Kind::Low), Biquad::new(Kind::Band), Biquad::new(Kind::High));
    let mut phase = 0.0f32;
    let mut phase2 = 0.0f32;
    let mut out: Vec<(f32, f32)> = Vec::with_capacity(n);
    let (wet, size, tail) = match id {
        "whoosh" => {
            out = air_pass(dur, seed, 180.0, 3200.0, 0.58, 0.5, 0.85);
            (0.18, 1.0, 0.6)
        }
        "swoosh" => {
            out = air_pass(dur, seed, 300.0, 5200.0, 0.5, 0.3, 0.7);
            (0.14, 0.8, 0.45)
        }
        "whip" => {
            // a fast swish that ends in the crack of the tip
            out = air_pass(dur * 0.75, seed, 600.0, 7000.0, 0.8, 0.15, 0.6);
            let crack_at = out.len();
            for i in 0..(n - crack_at.min(n)) {
                let t = i as f32 / SR;
                let c = f3.run(rng.next(), 1800.0, 0.7) * (-t * 90.0).exp() * 2.2 + (TAU * 2400.0 * t).sin() * (-t * 160.0).exp() * 0.6;
                out.push((c, c));
            }
            (0.22, 0.9, 0.5)
        }
        _ => {
            for i in 0..n {
                let t = i as f32 / SR;
                let p = t / dur;
                let s: (f32, f32) = match id {
                    "pop" => {
                        // lips/bubble: a pitch-dropping tone with a soft attack and a puff of air
                        let f = 110.0 + 520.0 * (-t * 45.0).exp();
                        phase += TAU * f / SR;
                        let att = (t * 1500.0).min(1.0);
                        let puff = f1.run(pink.next(), 900.0, 0.7) * (-t * 60.0).exp() * 0.6;
                        let s = (phase.sin() + (2.0 * phase).sin() * 0.15) * (-t * 26.0).exp() * att + puff;
                        (s, s)
                    }
                    "click" => {
                        // two close plastic ticks (press + release)
                        let tick = |d: f32| if d >= 0.0 { (-d * 900.0).exp() } else { 0.0 };
                        let e = tick(t - 0.002) + tick(t - 0.045) * 0.55;
                        let s = f2.run(rng.next(), 3800.0, 1.6) * e * 2.5 + (TAU * 1900.0 * t).sin() * e * 0.25;
                        (s, s)
                    }
                    "ding" => {
                        // small bell: inharmonic partials, the high ones die first, slightly wide
                        let mut l = 0.0;
                        let mut r = 0.0;
                        for (k, (m, a)) in [(1.0f32, 1.0f32), (2.0, 0.35), (2.76, 0.45), (5.4, 0.2), (8.93, 0.08)].iter().enumerate() {
                            let decay = (-t * (1.8 + k as f32 * 1.6)).exp();
                            l += (TAU * 1046.5 * m * t).sin() * a * decay;
                            r += (TAU * 1046.5 * m * 1.0015 * t).sin() * a * decay;
                        }
                        let att = (t * 2000.0).min(1.0);
                        (l * att * 0.4, r * att * 0.4)
                    }
                    "boom" => {
                        // cinematic impact: transient + falling sub + rumble
                        let f = 34.0 + 70.0 * (-t * 7.0).exp();
                        phase += TAU * f / SR;
                        let sub = phase.sin() * (-t * 2.2).exp();
                        let rumble = f1.run(pink.next(), 140.0, 0.7) * 3.0 * (-t * 2.6).exp();
                        let hit = f2.run(rng.next(), 1200.0, 0.8) * (-t * 40.0).exp() * 0.7;
                        let s = sat((sub * 1.2 + rumble + hit) * 1.3);
                        (s, s)
                    }
                    "riser" => {
                        // tension: detuned saws and noise climbing, filter opening, cut at the top
                        let f = 110.0 * (8.0f32).powf(p * p);
                        phase += TAU * f / SR;
                        phase2 += TAU * f * 1.012 / SR;
                        let saw = ((phase / TAU).fract() * 2.0 - 1.0) * 0.5 + ((phase2 / TAU).fract() * 2.0 - 1.0) * 0.5;
                        let nz = f2.run(pink.next(), 300.0 + 6000.0 * p * p, 0.9) * 2.5;
                        let lp = f1.run(saw, 200.0 + 5000.0 * p * p, 0.9);
                        let env = p.powf(1.8) * (1.0 - smoothstep((p - 0.985) / 0.015));
                        let s = (lp * 0.45 + nz) * env;
                        let w = (t * 0.7 * TAU).sin() * 0.5;
                        pan(s, w)
                    }
                    "shutter" => {
                        // mirror slap + curtain: two metallic, resonant clicks
                        let hit = |d: f32, k: f32| if d >= 0.0 { (-d * k).exp() } else { 0.0 };
                        let e = hit(t - 0.005, 180.0) + hit(t - 0.11, 140.0) * 0.8;
                        let metal = f2.run(rng.next(), 3300.0, 6.0) * 1.8 + f3.run(rng.next(), 5000.0, 0.7) * 0.6;
                        let thud = f1.run(rng.next(), 400.0, 0.7) * 2.0;
                        let s = (metal + thud) * e;
                        (s, s)
                    }
                    "glitch_sfx" => {
                        let seg = (t * 30.0).floor();
                        let mut r2 = Rng(seg as u64 * 7919 + 13);
                        let f = 200.0 + (r2.next().abs()) * 2400.0;
                        phase += TAU * f / SR;
                        let sq = if phase.sin() > 0.0 { 1.0 } else { -1.0 };
                        let gate = if r2.next() > -0.2 { 1.0 } else { 0.0 };
                        let crushed = (sq * 0.5 + rng.next() * 0.3) * gate;
                        let s = (crushed * 8.0).round() / 8.0 * (1.0 - p * 0.5) * 0.4;
                        pan(s, r2.next() * 0.6)
                    }
                    "notify" => {
                        // two soft chime notes (C6 → G6) with a little shimmer
                        let note = |t: f32, f: f32| {
                            if t < 0.0 {
                                return 0.0;
                            }
                            let att = (t * 400.0).min(1.0);
                            ((TAU * f * t).sin() + (TAU * f * 2.0 * t).sin() * 0.2 + (TAU * f * 3.01 * t).sin() * 0.06)
                                * (-t * 6.0).exp()
                                * att
                        };
                        let s = note(t, 1046.5) * 0.6 + note(t - 0.14, 1568.0) * 0.6;
                        (s, s)
                    }
                    "heartbeat_sfx" => {
                        let ph = t.fract();
                        let thump = |c: f32, k: f32| {
                            let d = ph - c;
                            if d >= 0.0 {
                                (-d * k).exp() * (d * 400.0).min(1.0)
                            } else {
                                0.0
                            }
                        };
                        let e = thump(0.0, 22.0) + thump(0.28, 26.0) * 0.75;
                        phase2 += TAU * (48.0 + 20.0 * e) / SR;
                        let s = f1.run(phase2.sin() + pink.next() * 0.3, 160.0, 0.7) * e * 2.0;
                        (s, s)
                    }
                    _ => (0.0, 0.0),
                };
                out.push(s);
            }
            match id {
                "ding" => (0.22, 1.1, 1.0),
                "notify" => (0.2, 0.9, 0.6),
                "boom" => (0.25, 1.3, 1.2),
                "riser" => (0.2, 1.1, 0.8),
                "pop" | "click" | "shutter" => (0.08, 0.6, 0.2),
                _ => (0.0, 1.0, 0.0),
            }
        }
    };
    // fade the very end so nothing clicks, add the room, then normalise
    let len = out.len();
    let fade = ((0.1 * dur).min(0.08) * SR) as usize;
    for (i, s) in out.iter_mut().enumerate().skip(len.saturating_sub(fade)) {
        let k = (len - i) as f32 / fade as f32;
        s.0 *= k;
        s.1 *= k;
    }
    reverb(&mut out, wet, size, tail);
    let peak = out.iter().map(|(l, r)| l.abs().max(r.abs())).fold(0.0f32, f32::max).max(1e-6);
    let g = 0.89 / peak;
    let mut out: Vec<(f32, f32)> = out.iter().map(|(l, r)| (l * g, r * g)).collect();
    // cut the reverb tail once it is inaudible (-50 dB), with a short fade
    let end = out.iter().rposition(|s| s.0.abs().max(s.1.abs()) > 0.003).unwrap_or(0).max(n.min(out.len()));
    out.truncate((end + (0.02 * SR) as usize).min(out.len()));
    let len = out.len();
    let fade = (0.02 * SR) as usize;
    for (i, s) in out.iter_mut().enumerate().skip(len.saturating_sub(fade)) {
        let k = (len - i) as f32 / fade as f32;
        s.0 *= k;
        s.1 *= k;
    }
    out
}

fn write_wav(path: &std::path::Path, samples: &[(f32, f32)], sr: u32) -> std::io::Result<()> {
    let mut bytes = Vec::with_capacity(44 + samples.len() * 4);
    let data_len = (samples.len() * 4) as u32;
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&sr.to_le_bytes());
    bytes.extend_from_slice(&(sr * 4).to_le_bytes());
    bytes.extend_from_slice(&4u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for (l, r) in samples {
        bytes.extend_from_slice(&((l.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        bytes.extend_from_slice(&((r.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(tmp, path)
}

/// Path of the WAV for a built-in sound effect (generated on first use).
pub fn path_for(id: &str) -> Option<PathBuf> {
    let def = crate::model::catalog::SFX.iter().find(|s| s.id == id)?;
    let dir = util::cache_dir().join("sfx");
    let _ = std::fs::create_dir_all(&dir);
    let p = dir.join(format!("{}-v{}.wav", id, VERSION));
    if p.exists() {
        util::touch(&p);
    } else {
        let samples = synth(id, util::us_to_s(def.dur) as f32);
        write_wav(&p, &samples, SR as u32).ok()?;
    }
    Some(p)
}

/// For a built-in effect file of a project ("…/sfx/whoosh-v1.wav"): the current file for
/// the same effect, made again if the cache was cleaned. `None` for other files.
pub fn current_path(old: &str) -> Option<PathBuf> {
    let name = std::path::Path::new(old).file_name()?.to_string_lossy().to_string();
    let stem = name.strip_suffix(".wav")?;
    let (id, ver) = stem.rsplit_once("-v")?;
    ver.parse::<u32>().ok()?;
    path_for(id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_effect_renders() {
        for s in crate::model::catalog::SFX {
            let v = super::synth(s.id, crate::util::us_to_s(s.dur) as f32);
            assert!(v.len() >= (crate::util::us_to_s(s.dur) as f32 * super::SR) as usize - 1, "{}", s.id);
            let peak = v.iter().map(|(l, r)| l.abs().max(r.abs())).fold(0.0f32, f32::max);
            assert!(peak > 0.5 && peak <= 0.9 && v.iter().all(|(l, r)| l.is_finite() && r.is_finite()), "{}: {}", s.id, peak);
        }
    }
}

#[cfg(test)]
mod dump {
    /// `VELOMOV_SFX_DUMP=dir cargo test dump_sfx -- --ignored` writes every effect as WAV.
    #[test]
    #[ignore]
    fn dump_sfx() {
        let dir = std::path::PathBuf::from(std::env::var("VELOMOV_SFX_DUMP").expect("set VELOMOV_SFX_DUMP"));
        std::fs::create_dir_all(&dir).unwrap();
        for s in crate::model::catalog::SFX {
            let v = super::synth(s.id, crate::util::us_to_s(s.dur) as f32);
            super::write_wav(&dir.join(format!("{}.wav", s.id)), &v, super::SR as u32).unwrap();
        }
    }
}
