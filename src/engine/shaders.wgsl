// VeloMov GPU compositor.
// All textures hold premultiplied, gamma-encoded RGBA (like most mobile editors,
// grading happens in gamma space).

struct U {
  canvas: vec4f,   // target w, h, 1/w, 1/h
  src: vec4f,      // tex0 w, h, 1/w, 1/h
  m0a: vec4f, m0b: vec4f,   // output px -> texture uv at shutter start
  m1a: vec4f, m1b: vec4f,   // at shutter middle
  m2a: vec4f, m2b: vec4f,   // at shutter end
  cfg: vec4f,      // samples, opacity, blend mode, flags
  crop: vec4f,     // visible texture rect u0 v0 u1 v1
  ga: array<vec4f, 12>,     // adjust grade block
  gf: array<vec4f, 12>,     // filter grade block
  gmix: vec4f,     // filter intensity
  mask0: vec4f,    // kind, cx, cy, rotation (rad)
  mask1: vec4f,    // w, h, feather, round
  mask2: vec4f,    // invert, aspect
  chroma: vec4f,   // key rgb, intensity
  chroma2: vec4f,  // shadow
  p0: vec4f, p1: vec4f, p2: vec4f, p3: vec4f,
  time: vec4f,     // local seconds, progress, seed, kind
};

@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var tex0: texture_2d<f32>;
@group(1) @binding(1) var tex1: texture_2d<f32>;
@group(1) @binding(2) var tex2: texture_2d<f32>;
@group(1) @binding(3) var samp: sampler;
@group(1) @binding(4) var samp_n: sampler;

struct VsOut {
  @builtin(position) pos: vec4f,
  @location(0) uv: vec2f,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
  var o: VsOut;
  let x = f32(i32(vi & 1u) * 4 - 1);
  let y = f32(i32(vi >> 1u) * 4 - 1);
  o.pos = vec4f(x, y, 0.0, 1.0);
  o.uv = vec2f((x + 1.0) * 0.5, (1.0 - y) * 0.5);
  return o;
}

// ------------------------------------------------------------------ helpers

const PI: f32 = 3.14159265;
const TAU: f32 = 6.2831853;
const LUM_SAT = vec3f(0.208540, 0.702086, 0.089374);
const LUMA = vec3f(0.299, 0.587, 0.114);

fn s0(uv: vec2f) -> vec4f { return textureSampleLevel(tex0, samp, uv, 0.0); }
fn s1(uv: vec2f) -> vec4f { return textureSampleLevel(tex1, samp, uv, 0.0); }
fn s2(uv: vec2f) -> vec4f { return textureSampleLevel(tex2, samp, uv, 0.0); }

fn luma(c: vec3f) -> f32 { return dot(c, LUMA); }

fn hash12(p: vec2f) -> f32 {
  var p3 = fract(vec3f(p.x, p.y, p.x) * 0.1031);
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.x + p3.y) * p3.z);
}

fn hash22(p: vec2f) -> vec2f {
  var p3 = fract(vec3f(p.x, p.y, p.x) * vec3f(0.1031, 0.1030, 0.0973));
  p3 += dot(p3, p3.yzx + 33.33);
  return fract((p3.xx + p3.yz) * p3.zy);
}

fn noise2(p: vec2f) -> f32 {
  let i = floor(p);
  let f = fract(p);
  let a = hash12(i);
  let b = hash12(i + vec2f(1.0, 0.0));
  let c = hash12(i + vec2f(0.0, 1.0));
  let d = hash12(i + vec2f(1.0, 1.0));
  let w = f * f * (3.0 - 2.0 * f);
  return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}

fn unpremul(c: vec4f) -> vec3f {
  if (c.a <= 0.00001) { return vec3f(0.0); }
  return c.rgb / c.a;
}

fn hue_rotate(c: vec3f, a: f32) -> vec3f {
  let k = vec3f(0.57735);
  let ca = cos(a);
  return c * ca + cross(k, c) * sin(a) + k * dot(k, c) * (1.0 - ca);
}

fn softlight(b: vec3f, s: vec3f) -> vec3f {
  let d = select(sqrt(b), ((16.0 * b - 12.0) * b + 4.0) * b, b <= vec3f(0.25));
  return select(b + (2.0 * s - 1.0) * (d - b), b - (1.0 - 2.0 * s) * b * (1.0 - b), s <= vec3f(0.5));
}

fn hsv2rgb(h: f32, s: f32, v: f32) -> vec3f {
  let k = vec3f(1.0, 2.0 / 3.0, 1.0 / 3.0);
  let p = abs(fract(vec3f(h) + k) * 6.0 - vec3f(3.0));
  return v * mix(vec3f(1.0), clamp(p - vec3f(1.0), vec3f(0.0), vec3f(1.0)), s);
}

// ------------------------------------------------------------------ grading (adjust maths)

fn contrast_curve(x: f32, xf: f32, piv: f32, ld: f32, rd: f32, lsd: f32, rsd: f32, ps: f32) -> f32 {
  let s = 1.0 / (1.0 + exp(-xf * (x - piv)));
  var res = s + piv - 0.5;
  let k = xf * s * (1.0 - s);
  if (x <= piv) {
    var sc = (ps - k) / lsd;
    sc = sc * sc;
    res = res + sc * ld;
  } else {
    var sc = (ps - k) / rsd;
    sc = sc * sc;
    res = res + sc * rd;
  }
  return res;
}

fn grade(c_in: vec3f, g: array<vec4f, 12>) -> vec3f {
  var c = clamp(c_in, vec3f(0.0), vec3f(1.0));
  let z = vec3f(0.0);
  let o = vec3f(1.0);
  // exposure (stops, in linear light)
  if (g[7].x != 0.0) {
    let lin = pow(c, vec3f(2.2)) * exp2(g[7].x);
    c = pow(clamp(lin, z, o), vec3f(1.0 / 2.2));
  }
  // brightness: y = 1 - (1 - x)^p
  if (g[0].x != 0.0) {
    let bp = g[0].y;
    var p = 1.0;
    if (bp > 0.0) {
      p = 1.0 + bp * 5.0;
    } else {
      p = 1.0 / (1.0 - bp * 2.5);
      c = c - vec3f(-bp * 0.01);
    }
    c = clamp(c, z, o);
    c = clamp(o - pow(o - c, vec3f(p)), z, o);
  }
  // contrast: linear below 1, sigmoid above
  if (g[0].z != 0.0) {
    let cp = g[0].w;
    let piv = g[2].z;
    if (cp <= 1.0) {
      c = cp * (c - vec3f(piv)) + vec3f(piv);
    } else {
      c = vec3f(
        contrast_curve(c.r, g[1].x, piv, g[1].y, g[1].z, g[1].w, g[2].x, g[2].y),
        contrast_curve(c.g, g[1].x, piv, g[1].y, g[1].z, g[1].w, g[2].x, g[2].y),
        contrast_curve(c.b, g[1].x, piv, g[1].y, g[1].z, g[1].w, g[2].x, g[2].y));
    }
    c = clamp(c, z, o);
  }
  // saturation
  if (g[2].w != 1.0) {
    let l = dot(c, LUM_SAT);
    c = clamp(mix(vec3f(l), c, g[2].w), z, o);
  }
  // vibrance
  if (g[7].w != 0.0) {
    let mx = max(c.r, max(c.g, c.b));
    let mn = min(c.r, min(c.g, c.b));
    let amt = g[7].w * (1.0 - (mx - mn));
    let l = dot(c, LUM_SAT);
    c = clamp(mix(vec3f(l), c, 1.0 + amt), z, o);
  }
  // highlights
  if (g[3].x != 0.0) {
    let p = g[3].x;
    let t = o - c;
    c = clamp(o - pow(t, vec3f(p)) - (p - 1.0) * (t * t - t * t * t), z, o);
  }
  // shadows
  if (g[3].y != 0.0) {
    let p = g[3].y;
    c = clamp(pow(c, vec3f(p)) + (p - 1.0) * (c * c - c * c * c), z, o);
  }
  // whites / blacks
  c = clamp(c * g[3].z + vec3f(g[3].w), z, o);
  // temperature / tint (Bradford adaptation matrix)
  if (g[4].w != 0.0) {
    c = clamp(vec3f(dot(c, g[4].xyz), dot(c, g[5].xyz), dot(c, g[6].xyz)), z, o);
  }
  // hue shift
  if (g[7].z != 0.0) {
    c = clamp(hue_rotate(c, g[7].z * TAU), z, o);
  }
  // fade: lifted blacks, rolled-off whites, a little desaturation
  if (g[7].y != 0.0) {
    let l = dot(c, LUM_SAT);
    var faded = mix(vec3f(0.14, 0.13, 0.13), vec3f(0.93, 0.92, 0.9), c);
    faded = mix(vec3f(l * 0.86 + 0.07), faded, 0.85);
    c = mix(c, faded, g[7].y);
  }
  // monochrome
  if (g[10].x != 0.0) {
    c = mix(c, vec3f(luma(c)), g[10].x);
  }
  // split toning
  if (g[8].w != 0.0 || g[9].w != 0.0) {
    let l = luma(c);
    let ws = (1.0 - smoothstep(0.0, 0.65, l)) * g[8].w;
    let wh = smoothstep(0.35, 1.0, l) * g[9].w;
    c = mix(c, softlight(c, g[8].xyz), ws);
    c = mix(c, softlight(c, g[9].xyz), wh);
  }
  return clamp(c, z, o);
}

// Vignette: positive darkens corners, negative brightens.
fn vignette(c: vec3f, uv: vec2f, v: f32) -> vec3f {
  if (v == 0.0) { return c; }
  let l = length(uv * 2.0 - 1.0);
  let a = min(abs(v), 1.0);
  let w = smoothstep(mix(1.0, 0.1, a) * 1.415, mix(1.4, 0.85, a) * 1.415, l) * a;
  if (v > 0.0) { return mix(c, vec3f(0.0), w); }
  return mix(c, vec3f(1.0), w);
}

fn grain(c: vec3f, p: vec2f, amt: f32, seed: f32) -> vec3f {
  if (amt == 0.0) { return c; }
  let n = hash12(p + vec2f(seed * 91.7, seed * 37.3)) - 0.5;
  return clamp(c + vec3f(n * amt * 0.35), vec3f(0.0), vec3f(1.0));
}

// Sharpen: pick the strongest of four directional laplacians on luma.
fn sharpen_at(c: vec3f, uv: vec2f, texel: vec2f, amt: f32, use_tex1: bool) -> vec3f {
  var p: array<f32, 9>;
  var i = 0;
  for (var y: i32 = -1; y <= 1; y++) {
    for (var x: i32 = -1; x <= 1; x++) {
      let q = uv + texel * vec2f(f32(x), f32(y));
      var s = s0(q);
      if (use_tex1) { s = s1(q); }
      p[i] = luma(unpremul(s));
      i++;
    }
  }
  let f1 = 2.0 * p[4] - p[3] - p[5];
  let f2 = 2.0 * p[4] - p[1] - p[7];
  let f3 = 2.0 * p[4] - p[2] - p[6];
  let f4 = 2.0 * p[4] - p[0] - p[8];
  let m = max(max(abs(f1), abs(f2)), max(abs(f3), abs(f4)));
  var f = f1;
  if (abs(abs(f2) - m) <= 0.01) { f = f2; }
  if (abs(abs(f3) - m) <= 0.01) { f = f3; }
  if (abs(abs(f4) - m) <= 0.01) { f = f4; }
  if (abs(abs(f1) - m) <= 0.01) { f = f1; }
  return clamp(c + vec3f(amt * 0.8 * f * 0.5), vec3f(0.0), vec3f(1.0));
}

fn clarity_at(c: vec3f, uv: vec2f, radius: vec2f, amt: f32, use_tex1: bool) -> vec3f {
  var acc = 0.0;
  for (var k: i32 = 0; k < 8; k++) {
    let a = f32(k) * TAU / 8.0;
    let q = uv + radius * vec2f(cos(a), sin(a));
    var s = s0(q);
    if (use_tex1) { s = s1(q); }
    acc += luma(unpremul(s));
  }
  let mean = acc / 8.0;
  let l = luma(c);
  let detail = (l - mean) * amt * 1.5 * (1.0 - abs(l * 2.0 - 1.0) * 0.5);
  return clamp(c + vec3f(detail), vec3f(0.0), vec3f(1.0));
}

// ------------------------------------------------------------------ masks

fn dot2(v: vec2f) -> f32 { return dot(v, v); }

fn sd_heart(pin: vec2f) -> f32 {
  let p = vec2f(abs(pin.x), pin.y);
  if (p.y + p.x > 1.0) {
    return sqrt(dot2(p - vec2f(0.25, 0.75))) - sqrt(2.0) / 4.0;
  }
  return sqrt(min(dot2(p - vec2f(0.0, 1.0)), dot2(p - 0.5 * max(p.x + p.y, 0.0)))) * sign(p.x - p.y);
}

fn sd_star5(pin: vec2f, r: f32, rf: f32) -> f32 {
  let k1 = vec2f(0.809016994375, -0.587785252292);
  let k2 = vec2f(-k1.x, k1.y);
  var p = vec2f(abs(pin.x), pin.y);
  p -= 2.0 * max(dot(k1, p), 0.0) * k1;
  p -= 2.0 * max(dot(k2, p), 0.0) * k2;
  p.x = abs(p.x);
  p.y -= r;
  let ba = rf * vec2f(-k1.y, k1.x) - vec2f(0.0, 1.0);
  let h = clamp(dot(p, ba) / dot(ba, ba), 0.0, r);
  return length(p - ba * h) * sign(p.y * ba.x - p.x * ba.y);
}

fn local_uv(uvt: vec2f) -> vec2f {
  return (uvt - u.crop.xy) / max(u.crop.zw - u.crop.xy, vec2f(0.000001));
}

fn mask_alpha(uvt: vec2f) -> f32 {
  let lu = local_uv(uvt);
  let aspect = u.mask2.y;
  let kind = i32(u.mask0.x);
  let center = vec2f(0.5 + u.mask0.y, 0.5 - u.mask0.z);
  var q = (lu - center) * vec2f(aspect, 1.0);
  let r = -u.mask0.w;
  q = vec2f(q.x * cos(r) - q.y * sin(r), q.x * sin(r) + q.y * cos(r));
  let he = max(vec2f(u.mask1.x * aspect, u.mask1.y) * 0.5, vec2f(0.0001));
  let f = max(u.mask1.z * 0.25, 0.0015);
  var d = -1.0;
  switch kind {
    case 1: { d = q.y; }
    case 2: { d = abs(q.y) - he.y; }
    case 3: { d = (length(q / he) - 1.0) * min(he.x, he.y); }
    case 4: {
      let rr = u.mask1.w * min(he.x, he.y);
      let qq = abs(q) - he + vec2f(rr);
      d = length(max(qq, vec2f(0.0))) + min(max(qq.x, qq.y), 0.0) - rr;
    }
    case 5: {
      let k = 0.54 / he.y;
      d = sd_heart(vec2f(q.x, -q.y) * k + vec2f(0.0, 0.54)) / k;
    }
    case 6: { d = sd_star5(vec2f(q.x, -q.y), he.y, 0.45); }
    default: { d = -1.0; }
  }
  var a = 1.0 - smoothstep(-f, f, d);
  if (u.mask2.x > 0.5) { a = 1.0 - a; }
  return a;
}

fn chroma_key(col: vec3f) -> vec4f {
  let key = u.chroma.xyz;
  let inten = u.chroma.w;
  let shadow = u.chroma2.x;
  let y = luma(col);
  let cc = vec2f((col.b - y) * 0.564, (col.r - y) * 0.713);
  let ky = luma(key);
  let kc = vec2f((key.b - ky) * 0.564, (key.r - ky) * 0.713);
  let d = distance(cc, kc);
  let r0 = inten * 0.3;
  let a = smoothstep(r0, r0 + 0.04 + shadow * 0.12, d);
  let spill = 1.0 - smoothstep(r0, r0 + 0.25, d);
  let c2 = mix(col, vec3f(y), spill * 0.85);
  return vec4f(c2 * mix(1.0, a, shadow * 0.5), a);
}

// ------------------------------------------------------------------ blending (W3C separable modes)

fn blend_fn(cb: vec3f, cs: vec3f, mode: i32) -> vec3f {
  let o = vec3f(1.0);
  let z = vec3f(0.0);
  switch mode {
    case 1: { return min(cb, cs); }
    case 2: { return cb * cs; }
    case 3: { return max(cb + cs - o, z); }
    case 4: {
      let r = o - min(o, (o - cb) / max(cs, vec3f(0.00001)));
      return select(select(r, z, cs <= z), o, cb >= o);
    }
    case 5: { return max(cb, cs); }
    case 6: { return cb + cs - cb * cs; }
    case 7: { return min(cb + cs, o); }
    case 8: {
      let r = min(o, cb / max(o - cs, vec3f(0.00001)));
      return select(select(r, o, cs >= o), z, cb <= z);
    }
    case 9: { return select(o - 2.0 * (o - cb) * (o - cs), 2.0 * cb * cs, cb <= vec3f(0.5)); }
    case 10: { return softlight(cb, cs); }
    case 11: { return select(o - 2.0 * (o - cb) * (o - cs), 2.0 * cb * cs, cs <= vec3f(0.5)); }
    case 12: { return abs(cb - cs); }
    case 13: { return cb + cs - 2.0 * cb * cs; }
    default: { return cs; }
  }
}

fn blend_px(dst: vec4f, src: vec4f, mode: i32) -> vec4f {
  if (mode == 0) { return src + dst * (1.0 - src.a); }
  let sa = src.a;
  let da = dst.a;
  if (sa <= 0.0) { return dst; }
  let cs = src.rgb / sa;
  let cb = select(vec3f(0.0), dst.rgb / max(da, 0.00001), da > 0.0);
  let b = clamp(blend_fn(cb, cs, mode), vec3f(0.0), vec3f(1.0));
  let rgb = (1.0 - sa) * dst.rgb + (1.0 - da) * src.rgb + sa * da * b;
  return vec4f(rgb, sa + da * (1.0 - sa));
}

// ------------------------------------------------------------------ layer composite (+ motion blur)

fn xform(p: vec2f, f: f32) -> vec2f {
  var a: vec4f;
  var b: vec4f;
  if (f < 0.5) {
    let t = f * 2.0;
    a = mix(u.m0a, u.m1a, t);
    b = mix(u.m0b, u.m1b, t);
  } else {
    let t = (f - 0.5) * 2.0;
    a = mix(u.m1a, u.m2a, t);
    b = mix(u.m1b, u.m2b, t);
  }
  return vec2f(dot(a.xyz, vec3f(p, 1.0)), dot(b.xyz, vec3f(p, 1.0)));
}

fn sample_layer(uv: vec2f) -> vec4f {
  let c = u.crop;
  let fx = max(length(vec2f(u.m1a.x, u.m1a.y)), 0.000001);
  let fy = max(length(vec2f(u.m1b.x, u.m1b.y)), 0.000001);
  let dx = min(uv.x - c.x, c.z - uv.x) / fx;
  let dy = min(uv.y - c.y, c.w - uv.y) / fy;
  let cov = clamp(min(dx, dy) + 0.5, 0.0, 1.0);
  if (cov <= 0.0) { return vec4f(0.0); }
  return s0(uv) * cov;
}

@fragment
fn fs_composite(in: VsOut) -> @location(0) vec4f {
  let p = in.pos.xy;
  let dst = textureSampleLevel(tex1, samp_n, in.uv, 0.0);
  let n = max(i32(u.cfg.x), 1);
  let jit = hash12(p + vec2f(u.time.z * 13.1, u.time.z * 7.7)) - 0.5;
  var acc = vec4f(0.0);
  for (var k: i32 = 0; k < n; k++) {
    var f = 0.5;
    if (n > 1) { f = clamp((f32(k) + 0.5 + jit) / f32(n), 0.0, 1.0); }
    acc += sample_layer(xform(p, f));
  }
  let s = acc / f32(n);
  if (s.a <= 0.0002) { return dst; }
  let uvc = xform(p, 0.5);
  var col = s.rgb / s.a;
  let flags = u32(u.cfg.w);
  if ((flags & 4u) != 0u) {
    if (u.ga[10].z != 0.0) { col = sharpen_at(col, uvc, u.src.zw, u.ga[10].z, false); }
    if (u.ga[10].y != 0.0) { col = clarity_at(col, uvc, u.src.zw * max(u.src.y * 0.012, 2.0), u.ga[10].y, false); }
  }
  if ((flags & 2u) != 0u) {
    var g = grade(col, u.gf);
    g = vignette(g, local_uv(uvc), u.gf[11].x);
    g = grain(g, p, u.gf[11].y, u.time.z);
    col = mix(col, g, u.gmix.x);
  }
  if ((flags & 1u) != 0u) {
    col = grade(col, u.ga);
    col = vignette(col, local_uv(uvc), u.ga[11].x);
    col = grain(col, p, u.ga[11].y, u.time.z);
  }
  var a = s.a * u.cfg.y;
  if ((flags & 8u) != 0u) {
    let ck = chroma_key(col);
    col = ck.rgb;
    a *= ck.a;
  }
  if ((flags & 16u) != 0u) {
    a *= mask_alpha(uvc);
  }
  return blend_px(dst, vec4f(col * a, a), i32(u.cfg.z));
}

// Full-canvas grading (filter / adjustment tracks).
@fragment
fn fs_grade(in: VsOut) -> @location(0) vec4f {
  let c = textureSampleLevel(tex0, samp_n, in.uv, 0.0);
  if (c.a <= 0.0001) { return c; }
  var col = c.rgb / c.a;
  let flags = u32(u.cfg.w);
  if ((flags & 4u) != 0u) {
    if (u.ga[10].z != 0.0) { col = sharpen_at(col, in.uv, u.canvas.zw, u.ga[10].z, false); }
    if (u.ga[10].y != 0.0) { col = clarity_at(col, in.uv, u.canvas.zw * max(u.canvas.y * 0.012, 2.0), u.ga[10].y, false); }
  }
  if ((flags & 2u) != 0u) {
    var g = grade(col, u.gf);
    g = vignette(g, in.uv, u.gf[11].x);
    g = grain(g, in.pos.xy, u.gf[11].y, u.time.z);
    col = mix(col, g, u.gmix.x);
  }
  if ((flags & 1u) != 0u) {
    col = grade(col, u.ga);
    col = vignette(col, in.uv, u.ga[11].x);
    col = grain(col, in.pos.xy, u.ga[11].y, u.time.z);
  }
  return vec4f(col * c.a, c.a);
}

// ------------------------------------------------------------------ utility passes

@fragment
fn fs_yuv(in: VsOut) -> @location(0) vec4f {
  let y = textureSampleLevel(tex0, samp, in.uv, 0.0).r;
  let cb = textureSampleLevel(tex1, samp, in.uv, 0.0).r - 0.5;
  let cr = textureSampleLevel(tex2, samp, in.uv, 0.0).r - 0.5;
  let yy = (y - 16.0 / 255.0) * (255.0 / 219.0);
  let cbb = cb * (255.0 / 224.0);
  let crr = cr * (255.0 / 224.0);
  let r = yy + 1.5748 * crr;
  let g = yy - 0.1873 * cbb - 0.4681 * crr;
  let b = yy + 1.8556 * cbb;
  return vec4f(clamp(vec3f(r, g, b), vec3f(0.0), vec3f(1.0)), 1.0);
}

// NV12 from hardware decoders: Y plane + interleaved CbCr plane.
@fragment
fn fs_nv12(in: VsOut) -> @location(0) vec4f {
  let y = textureSampleLevel(tex0, samp, in.uv, 0.0).r;
  let c = textureSampleLevel(tex1, samp, in.uv, 0.0).rg - vec2f(0.5);
  let yy = (y - 16.0 / 255.0) * (255.0 / 219.0);
  let cbb = c.x * (255.0 / 224.0);
  let crr = c.y * (255.0 / 224.0);
  let r = yy + 1.5748 * crr;
  let g = yy - 0.1873 * cbb - 0.4681 * crr;
  let b = yy + 1.8556 * cbb;
  return vec4f(clamp(vec3f(r, g, b), vec3f(0.0), vec3f(1.0)), 1.0);
}

@fragment
fn fs_copy(in: VsOut) -> @location(0) vec4f {
  let uv = u.p0.xy + in.uv * u.p0.zw;
  let c = textureSampleLevel(tex0, samp, uv, 0.0);
  // p1.x > 0: force opaque over p1.yzw background colour (for display / export)
  if (u.p1.x > 0.5) {
    return vec4f(c.rgb + u.p1.yzw * (1.0 - c.a), 1.0);
  }
  return c;
}

@fragment
fn fs_blur(in: VsOut) -> @location(0) vec4f {
  let dir = u.p0.xy;
  let sigma = max(u.p0.z, 0.001);
  let r = i32(min(ceil(sigma * 3.0), 16.0));
  var acc = vec4f(0.0);
  var wsum = 0.0;
  for (var i: i32 = -r; i <= r; i++) {
    let x = f32(i);
    let w = exp(-x * x / (2.0 * sigma * sigma));
    acc += textureSampleLevel(tex0, samp, in.uv + dir * x, 0.0) * w;
    wsum += w;
  }
  return acc / wsum;
}

// ------------------------------------------------------------------ transitions

fn radial_zoom(uv: vec2f, strength: f32, which: i32) -> vec4f {
  var acc = vec4f(0.0);
  let d = uv - 0.5;
  for (var i: i32 = 0; i < 20; i++) {
    let f = 1.0 - strength * f32(i) / 19.0;
    let q = vec2f(0.5) + d * f;
    if (which == 0) { acc += s0(q); } else { acc += s1(q); }
  }
  return acc / 20.0;
}

fn rgb_split_s(uv: vec2f, sp: f32, which: i32) -> vec4f {
  var r: vec4f;
  var g: vec4f;
  var b: vec4f;
  if (which == 0) {
    r = s0(uv + vec2f(sp, 0.0)); g = s0(uv); b = s0(uv - vec2f(sp, 0.0));
  } else {
    r = s1(uv + vec2f(sp, 0.0)); g = s1(uv); b = s1(uv - vec2f(sp, 0.0));
  }
  return vec4f(r.r, g.g, b.b, max(g.a, max(r.a, b.a)));
}

fn leak_color(q: vec2f, p: f32, variant: f32) -> vec3f {
  let c1 = vec2f(-0.2 + p * 1.6, 0.35 + 0.2 * sin(p * 3.0));
  let c2 = vec2f(1.5 - p * 1.2, 0.85);
  let a = exp(-dot2(q - c1) * 3.0);
  let b = exp(-dot2(q - c2) * 4.0);
  var col = vec3f(1.0, 0.55, 0.15) * a + vec3f(1.0, 0.25, 0.35) * b;
  if (variant > 0.5) {
    col = col * (0.5 + noise2(q * 5.0 + vec2f(p * 4.0))) + vec3f(1.0, 0.9, 0.6) * pow(a, 3.0);
  }
  return col;
}

// Ray-traced cube rotation. axis 0: B comes from the right, 1: from below.
fn cube_tr(uv: vec2f, p: f32, axis: i32) -> vec4f {
  let aspect = u.canvas.x / u.canvas.y;
  let th = p * PI * 0.5;
  let sx = (uv.x * 2.0 - 1.0) * aspect;
  let sy = (1.0 - uv.y * 2.0);
  let focal = 2.0;
  var hw = aspect;
  if (axis == 1) { hw = 1.0; }
  let pull = sin(p * PI) * hw * 0.5;
  var o = vec3f(0.0, 0.0, -hw - focal - pull);
  var d = vec3f(sx, sy, focal);
  // rotate ray into cube space (inverse rotation)
  let c = cos(-th);
  let s = sin(-th);
  if (axis == 0) {
    o = vec3f(o.x * c + o.z * s, o.y, -o.x * s + o.z * c);
    d = vec3f(d.x * c + d.z * s, d.y, -d.x * s + d.z * c);
  } else {
    o = vec3f(o.x, o.y * c - o.z * s, o.y * s + o.z * c);
    d = vec3f(d.x, d.y * c - d.z * s, d.y * s + d.z * c);
  }
  var best = 1e9;
  var col = vec4f(0.0, 0.0, 0.0, 1.0);
  // face A: z = -hw
  if (d.z > 0.0) {
    let t = (-hw - o.z) / d.z;
    let h = o + d * t;
    if (t > 0.0 && abs(h.x) <= aspect && abs(h.y) <= 1.0 && t < best) {
      best = t;
      let shade = mix(1.0, 0.55, p);
      let a = s0(vec2f(h.x / aspect * 0.5 + 0.5, 0.5 - h.y * 0.5));
      col = vec4f(a.rgb * shade, a.a);
    }
  }
  if (axis == 0) {
    // face B: x = +hw (right)
    if (d.x < 0.0) {
      let t = (hw - o.x) / d.x;
      let h = o + d * t;
      if (t > 0.0 && abs(h.z) <= hw && abs(h.y) <= 1.0 && t < best) {
        best = t;
        let shade = mix(0.55, 1.0, p);
        let b = s1(vec2f(h.z / hw * 0.5 + 0.5, 0.5 - h.y * 0.5));
        col = vec4f(b.rgb * shade, b.a);
      }
    }
  } else {
    // face B: y = -hw (bottom), comes up
    if (d.y > 0.0) {
      let t = (-hw - o.y) / d.y;
      let h = o + d * t;
      if (t > 0.0 && abs(h.x) <= aspect && abs(h.z) <= hw && t < best) {
        best = t;
        let shade = mix(0.55, 1.0, p);
        let b = s1(vec2f(h.x / aspect * 0.5 + 0.5, h.z / hw * 0.5 + 0.5));
        col = vec4f(b.rgb * shade, b.a);
      }
    }
  }
  return col;
}

fn flip_tr(uv: vec2f, p: f32) -> vec4f {
  let aspect = u.canvas.x / u.canvas.y;
  let th = p * PI;
  let sx = (uv.x * 2.0 - 1.0) * aspect;
  let sy = (1.0 - uv.y * 2.0);
  let focal = 2.4;
  let pull = sin(p * PI) * 0.7;
  var o = vec3f(0.0, 0.0, -focal - pull);
  var d = vec3f(sx, sy, focal);
  let c = cos(-th);
  let s = sin(-th);
  o = vec3f(o.x * c + o.z * s, o.y, -o.x * s + o.z * c);
  d = vec3f(d.x * c + d.z * s, d.y, -d.x * s + d.z * c);
  if (abs(d.z) < 0.00001) { return vec4f(0.0, 0.0, 0.0, 1.0); }
  let t = -o.z / d.z;
  let h = o + d * t;
  if (t <= 0.0 || abs(h.x) > aspect || abs(h.y) > 1.0) { return vec4f(0.0, 0.0, 0.0, 1.0); }
  let shade = 0.55 + 0.45 * abs(cos(th));
  if (p < 0.5) {
    let a = s0(vec2f(h.x / aspect * 0.5 + 0.5, 0.5 - h.y * 0.5));
    return vec4f(a.rgb * shade, a.a);
  }
  let b = s1(vec2f(0.5 - h.x / aspect * 0.5, 0.5 - h.y * 0.5));
  return vec4f(b.rgb * shade, b.a);
}

fn door_tr(uv: vec2f, p: f32) -> vec4f {
  let aspect = u.canvas.x / u.canvas.y;
  let phi = p * PI * 0.5;
  let sx = (uv.x * 2.0 - 1.0) * aspect;
  let sy = (1.0 - uv.y * 2.0);
  let focal = 2.0;
  let o = vec3f(0.0, 0.0, -focal);
  let d = vec3f(sx, sy, focal);
  var best = 1e9;
  let zs = mix(0.85, 1.0, p);
  let buv = vec2f(0.5) + (uv - 0.5) * zs;
  var col = s1(buv);
  // left door: hinge at x=-aspect
  {
    let hinge = vec3f(-aspect, 0.0, 0.0);
    let e = vec3f(cos(phi), 0.0, -sin(phi));
    let n = vec3f(sin(phi), 0.0, cos(phi));
    let den = dot(d, n);
    if (abs(den) > 0.00001) {
      let t = dot(hinge - o, n) / den;
      let h = o + d * t;
      let sv = dot(h - hinge, e);
      if (t > 0.0 && sv >= 0.0 && sv <= aspect && abs(h.y) <= 1.0 && t < best) {
        best = t;
        let a = s0(vec2f(sv / (2.0 * aspect), 0.5 - h.y * 0.5));
        col = vec4f(a.rgb * mix(1.0, 0.5, p), a.a);
      }
    }
  }
  {
    let hinge = vec3f(aspect, 0.0, 0.0);
    let e = vec3f(-cos(phi), 0.0, -sin(phi));
    let n = vec3f(sin(phi), 0.0, -cos(phi));
    let den = dot(d, n);
    if (abs(den) > 0.00001) {
      let t = dot(hinge - o, n) / den;
      let h = o + d * t;
      let sv = dot(h - hinge, e);
      if (t > 0.0 && sv >= 0.0 && sv <= aspect && abs(h.y) <= 1.0 && t < best) {
        best = t;
        let a = s0(vec2f(1.0 - sv / (2.0 * aspect), 0.5 - h.y * 0.5));
        col = vec4f(a.rgb * mix(1.0, 0.5, p), a.a);
      }
    }
  }
  return col;
}

@fragment
fn fs_transition(in: VsOut) -> @location(0) vec4f {
  let uv = in.uv;
  let p = clamp(u.time.y, 0.0, 1.0);
  let px = in.pos.xy;
  let res = u.canvas.xy;
  let aspect = res.x / res.y;
  let peak = 1.0 - abs(p * 2.0 - 1.0);
  let kind = i32(u.time.w);
  switch kind {
    case 0: { return mix(s0(uv), s1(uv), clamp(u.p0.x, 0.0, 1.0)); }
    case 1: {
      if (p < 0.5) {
        let a = s0(uv);
        return vec4f(a.rgb * (1.0 - p * 2.0), a.a);
      }
      let b = s1(uv);
      return vec4f(b.rgb * (p * 2.0 - 1.0), b.a);
    }
    case 2: {
      let w = pow(peak, max(u.p0.x, 0.5));
      let base = mix(s0(uv), s1(uv), smoothstep(0.42, 0.58, p));
      return vec4f(mix(base.rgb, vec3f(base.a), w), base.a);
    }
    case 3: {
      let dir = u.p0.xy;
      let soft = max(u.p0.z, 0.001);
      let q = (uv - 0.5) * vec2f(aspect, 1.0);
      let ext = 0.5 * (abs(dir.x) * aspect + abs(dir.y));
      let d = (dot(q, dir) / ext) * 0.5 + 0.5;
      let e = p * (1.0 + 2.0 * soft) - soft;
      let m = 1.0 - smoothstep(e - soft, e + soft, d);
      return mix(s0(uv), s1(uv), m);
    }
    case 4: {
      let q = (uv - 0.5) * vec2f(aspect, 1.0);
      var ang = atan2(q.x, -q.y) / TAU;
      if (ang < 0.0) { ang += 1.0; }
      let m = 1.0 - smoothstep(p - 0.008, p + 0.008, ang);
      return mix(s0(uv), s1(uv), select(m, 1.0, p >= 0.999));
    }
    case 5: {
      let q = (uv - 0.5) * vec2f(aspect, 1.0);
      let rmax = 0.5 * sqrt(aspect * aspect + 1.0);
      let r = p * (rmax + 0.05);
      let m = 1.0 - smoothstep(r - 0.02, r + 0.002, length(q));
      return mix(s0(uv), s1(uv), m);
    }
    case 6: {
      let q = (uv - 0.5) * vec2f(aspect, 1.0);
      let sc = max(p * p * 2.4 * max(aspect, 1.0), 0.0001);
      var d = 0.0;
      let sh = i32(u.p0.x);
      if (sh == 0) {
        let k = 0.54 / sc;
        d = sd_heart(vec2f(q.x, -q.y) * k + vec2f(0.0, 0.54)) / k;
      } else if (sh == 1) {
        d = sd_star5(vec2f(q.x, -q.y), sc, 0.45);
      } else {
        d = (abs(q.x) + abs(q.y)) - sc;
      }
      let m = 1.0 - smoothstep(-0.004, 0.004, d);
      return mix(s0(uv), s1(uv), m);
    }
    case 7: {
      let m = 1.0 - smoothstep(p * 0.5 - 0.004, p * 0.5 + 0.004, abs(uv.x - 0.5));
      return mix(s0(uv), s1(uv), select(m, 1.0, p >= 0.999));
    }
    case 8: {
      let f = fract(uv.y * 8.0);
      let m = 1.0 - smoothstep(p - 0.02, p, f);
      return mix(s0(uv), s1(uv), select(m, 1.0, p >= 0.999));
    }
    case 9: {
      let nz = noise2(px / 7.0) * 0.7 + noise2(px / 2.0) * 0.3;
      let e = p * 1.2 - 0.1;
      let m = smoothstep(nz - 0.08, nz + 0.08, e);
      return mix(s0(uv), s1(uv), m);
    }
    case 10: {
      let a = s0(uv);
      let l = luma(unpremul(a));
      let e = p * 1.3 - 0.15;
      let m = smoothstep(l - 0.15, l + 0.15, e);
      return mix(a, s1(uv), m);
    }
    case 11: {
      let t = floor(p * 20.0);
      let row = floor(uv.y * 26.0);
      var off = 0.0;
      if (hash12(vec2f(row, t)) > 0.55) { off = (hash12(vec2f(row, t + 7.0)) - 0.5) * 0.3 * peak; }
      let uv2 = vec2f(fract(uv.x + off), uv.y);
      let sp = 0.03 * peak;
      let blk = hash12(vec2f(floor(uv.y * 10.0), floor(uv.x * 6.0) + t * 3.0));
      let useb = select(0.0, 1.0, blk < smoothstep(0.25, 0.75, p));
      let a = rgb_split_s(uv2, sp, 0);
      let b = rgb_split_s(uv2, sp, 1);
      var c = mix(a, b, useb);
      let lines = (hash12(vec2f(uv.y * 400.0, t)) - 0.5) * 0.2 * peak;
      return vec4f(clamp(c.rgb + vec3f(lines) * c.a, vec3f(0.0), vec3f(c.a)), c.a);
    }
    case 12: {
      let sp = peak * 0.07;
      let m = smoothstep(0.4, 0.6, p);
      return mix(rgb_split_s(uv, sp, 0), rgb_split_s(uv, -sp, 1), m);
    }
    case 13: {
      let cs = max(1.0, pow(peak, 1.5) * 0.09 * res.y);
      let q = (floor(px / cs) + 0.5) * cs / res;
      return mix(s0(q), s1(q), smoothstep(0.45, 0.55, p));
    }
    case 14: {
      let nz = hash12(px + vec2f(p * 1000.0, p * 733.0));
      let base = mix(s0(uv), s1(uv), smoothstep(0.45, 0.55, p));
      let lines = 0.85 + 0.15 * sin(uv.y * res.y * 1.5);
      return vec4f(mix(base.rgb, vec3f(nz) * base.a, pow(peak, 1.3) * 0.9) * lines, base.a);
    }
    case 15: {
      let base = mix(s0(uv), s1(uv), smoothstep(0.3, 0.7, p));
      let q = uv * vec2f(aspect, 1.0);
      let col = base.rgb + leak_color(q, p, u.p0.x) * peak * 1.25 * base.a;
      return vec4f(min(col, vec3f(base.a)), base.a);
    }
    case 16: {
      let q = uv - 0.5;
      let r = length(q * vec2f(aspect, 1.0));
      let off = q / max(r, 0.0001) * sin(r * 60.0 - p * 30.0) * 0.02 * peak;
      return mix(s0(uv + off), s1(uv + off), smoothstep(0.3, 0.7, p));
    }
    case 17: {
      let q = (uv - 0.5) * vec2f(aspect, 1.0);
      let r = length(q);
      let fall = max(0.0, 1.0 - r / 0.9);
      let ang = peak * 7.0 * fall * fall;
      let c = cos(ang);
      let s = sin(ang);
      let q2 = vec2f(q.x * c - q.y * s, q.x * s + q.y * c);
      let uv2 = q2 / vec2f(aspect, 1.0) + 0.5;
      return mix(s0(uv2), s1(uv2), smoothstep(0.4, 0.6, p));
    }
    case 18: { return cube_tr(uv, p, i32(u.p0.x)); }
    case 19: { return flip_tr(uv, p); }
    case 20: { return door_tr(uv, p); }
    case 21: {
      let strength = peak * 0.3;
      return mix(radial_zoom(uv, strength, 0), radial_zoom(uv, strength, 1), smoothstep(0.4, 0.6, p));
    }
    case 22: {
      let a = s0(uv);
      let b = s1(uv);
      return b + a * (1.0 - b.a);
    }
    default: { return mix(s0(uv), s1(uv), p); }
  }
}

// ------------------------------------------------------------------ effects (applied to the canvas)

fn warp_sample(p: vec2f) -> vec4f {
  let n = max(i32(u.cfg.x), 1);
  let jit = hash12(p + vec2f(u.time.z * 3.1, 1.7)) - 0.5;
  var acc = vec4f(0.0);
  for (var k: i32 = 0; k < n; k++) {
    var f = 0.5;
    if (n > 1) { f = clamp((f32(k) + 0.5 + jit) / f32(n), 0.0, 1.0); }
    let q = xform(p, f);
    acc += s0(clamp(q, vec2f(0.0), vec2f(1.0)));
  }
  return acc / f32(n);
}

fn heat(t: f32) -> vec3f {
  let c0 = vec3f(0.0, 0.0, 0.1);
  let c1 = vec3f(0.3, 0.0, 0.6);
  let c2 = vec3f(0.9, 0.1, 0.2);
  let c3 = vec3f(1.0, 0.7, 0.0);
  let c4 = vec3f(1.0, 1.0, 0.85);
  if (t < 0.25) { return mix(c0, c1, t / 0.25); }
  if (t < 0.5) { return mix(c1, c2, (t - 0.25) / 0.25); }
  if (t < 0.75) { return mix(c2, c3, (t - 0.5) / 0.25); }
  return mix(c3, c4, (t - 0.75) / 0.25);
}

@fragment
fn fs_effect(in: VsOut) -> @location(0) vec4f {
  let uv = in.uv;
  let px = in.pos.xy;
  let res = u.canvas.xy;
  let aspect = res.x / res.y;
  let t = u.time.x;
  let amt = u.p0.x;
  let spd = u.p0.y;
  let kind = i32(u.time.w);
  let orig = s0(uv);
  switch kind {
    case 1: { return warp_sample(px); }
    case 2: {
      let strength = amt * 0.18 * (0.65 + 0.35 * sin(t * (2.0 + spd * 10.0)));
      var acc = vec4f(0.0);
      let d = uv - 0.5;
      for (var i: i32 = 0; i < 24; i++) {
        acc += s0(vec2f(0.5) + d * (1.0 - strength * f32(i) / 23.0));
      }
      return acc / 24.0;
    }
    case 3: {
      let ang = u.p0.y * TAU;
      let dir = vec2f(cos(ang), sin(ang)) * amt * 0.09 * vec2f(1.0 / aspect, 1.0);
      var acc = vec4f(0.0);
      for (var i: i32 = 0; i < 24; i++) {
        acc += s0(uv + dir * (f32(i) / 23.0 - 0.5));
      }
      return acc / 24.0;
    }
    case 4: {
      let tt = t * (2.0 + spd * 10.0);
      let burst = step(0.5, hash12(vec2f(floor(tt * 1.5), 3.1)));
      let a = amt * (0.3 + 0.7 * burst);
      let row = floor(uv.y * 30.0);
      var off = 0.0;
      if (hash12(vec2f(row, floor(tt * 8.0))) > 0.7) { off = (hash12(vec2f(row, floor(tt * 8.0) + 5.0)) - 0.5) * 0.15 * a; }
      let uv2 = vec2f(fract(uv.x + off), uv.y);
      let sp = 0.012 * a * (0.5 + burst);
      let r = s0(uv2 + vec2f(sp, 0.0));
      let g = s0(uv2);
      let b = s0(uv2 - vec2f(sp, 0.0));
      var c = vec3f(r.r, g.g, b.b);
      c += vec3f((hash12(vec2f(uv.y * 500.0, floor(tt * 12.0))) - 0.5) * 0.12 * a);
      return vec4f(clamp(c, vec3f(0.0), vec3f(1.0)), g.a);
    }
    case 5: {
      let ang = t * (1.0 + spd * 6.0);
      let sp = vec2f(cos(ang), sin(ang)) * amt * 0.012 * vec2f(1.0, aspect);
      let r = s0(uv + sp);
      let g = s0(uv);
      let b = s0(uv - sp);
      return vec4f(r.r, g.g, b.b, g.a);
    }
    case 6: {
      let cs = max(1.0, mix(3.0, 0.08 * res.y, amt));
      let q = (floor(px / cs) + 0.5) * cs / res;
      return s0(q);
    }
    case 7: {
      var q = uv;
      q.x += sin(uv.y * 30.0 + t * 4.0) * 0.0015 * amt;
      let band = fract(t * 0.15);
      if (abs(uv.y - band) < 0.025) { q.x += 0.01 * amt; }
      let sp = 0.004 * amt;
      var c = vec3f(s0(q + vec2f(sp, 0.0)).r, s0(q).g, s0(q - vec2f(sp, 0.0)).b);
      let l = luma(c);
      c = mix(c, mix(vec3f(l), c, 0.7) * vec3f(1.02, 1.0, 1.06), amt);
      c *= 1.0 - 0.1 * amt * (0.5 + 0.5 * sin(uv.y * res.y * PI * 0.5));
      c += vec3f((hash12(px + vec2f(t * 60.0)) - 0.5) * 0.1 * amt);
      c = vignette(c, uv, 0.25 * amt);
      return vec4f(clamp(c, vec3f(0.0), vec3f(1.0)), 1.0);
    }
    case 8: {
      let fr = floor(t * 18.0);
      var c = unpremul(orig);
      let l = luma(c);
      c = mix(c, vec3f(l) * vec3f(1.07, 0.92, 0.72), 0.85 * amt);
      c *= 1.0 - amt * 0.12 * hash12(vec2f(fr, 1.7));
      let x0 = hash12(vec2f(fr, 4.2));
      if (abs(uv.x - x0) < 0.0012 && hash12(vec2f(fr, 9.1)) > 0.4) { c += vec3f(0.25 * amt); }
      if (hash12(floor(px / 3.0) + vec2f(fr * 13.0)) > 0.9993) { c *= 0.3; }
      c += vec3f((hash12(px + vec2f(fr)) - 0.5) * 0.12 * amt);
      c = vignette(c, uv, 0.5 * amt);
      return vec4f(clamp(c, vec3f(0.0), vec3f(1.0)), 1.0);
    }
    case 9: {
      var c = unpremul(orig);
      let n = hash12(px + vec2f(floor(t * 24.0) * 17.0, 3.0)) - 0.5;
      let l = luma(c);
      c += vec3f(n * 0.3 * amt * (1.2 - l));
      return vec4f(clamp(c, vec3f(0.0), vec3f(1.0)), 1.0);
    }
    case 10: {
      var q = uv * 2.0 - 1.0;
      q *= 1.0 + 0.1 * amt * vec2f(q.y * q.y, q.x * q.x);
      let uv2 = q * 0.5 + 0.5;
      if (uv2.x < 0.0 || uv2.x > 1.0 || uv2.y < 0.0 || uv2.y > 1.0) { return vec4f(0.0, 0.0, 0.0, 1.0); }
      var c = unpremul(s0(uv2));
      let col = i32(px.x) % 3;
      var m = vec3f(0.75);
      if (col == 0) { m.r = 1.0; } else if (col == 1) { m.g = 1.0; } else { m.b = 1.0; }
      c = mix(c, c * m * 1.25, amt);
      c *= 1.0 - 0.2 * amt * (0.5 + 0.5 * sin(uv2.y * res.y * PI));
      c = vignette(c, uv2, 0.4 * amt);
      return vec4f(clamp(c, vec3f(0.0), vec3f(1.0)), 1.0);
    }
    case 11: {
      let size = mix(5.0, 22.0, u.p0.y) * res.y / 1080.0;
      let ang = 0.785398;
      let rot = mat2x2f(cos(ang), -sin(ang), sin(ang), cos(ang));
      let q = rot * px;
      let cell = floor(q / size);
      let center_q = (cell + 0.5) * size;
      let center = transpose(rot) * center_q;
      let c = unpremul(s0(center / res));
      let l = luma(c);
      let r = sqrt(1.0 - l) * size * 0.62;
      let d = length(q - center_q);
      let ink = 1.0 - smoothstep(r - 1.0, r + 1.0, d);
      let paper = vec3f(0.97, 0.95, 0.9);
      let ht = mix(paper, c * 0.9, ink);
      return vec4f(mix(unpremul(orig), ht, amt), 1.0);
    }
    case 12: {
      let c = vignette(unpremul(orig), uv, amt);
      return vec4f(c, 1.0) * orig.a;
    }
    case 13: {
      var c = unpremul(orig);
      let l = luma(c);
      let bw = clamp((l - 0.5) * 1.15 + 0.5, 0.0, 1.0);
      return vec4f(mix(c, vec3f(bw), amt) * orig.a, orig.a);
    }
    case 14: {
      let c = unpremul(orig);
      return vec4f(mix(c, vec3f(1.0) - c, amt) * orig.a, orig.a);
    }
    case 15: {
      let lv = mix(16.0, 3.0, amt);
      let c = floor(unpremul(orig) * lv + 0.5) / lv;
      return vec4f(c * orig.a, orig.a);
    }
    case 16: {
      // letterbox: p0.x = bar height fraction, p0.z = open progress (opening)
      let h = u.p0.x * mix(0.5, 1.0, u.p0.z);
      let bar = max(h, 0.0);
      if (uv.y < bar || uv.y > 1.0 - bar) { return vec4f(0.0, 0.0, 0.0, 1.0); }
      return orig;
    }
    case 17: {
      return vec4f(orig.rgb * (1.0 - amt), orig.a);
    }
    case 18: {
      var q = (uv - 0.5) * vec2f(aspect, 1.0);
      let r = length(q);
      let k = amt * 0.9;
      let f = 1.0 / (1.0 + k * r * r * 2.0);
      q = q * f * (1.0 + k * 0.4);
      let uv2 = q / vec2f(aspect, 1.0) + 0.5;
      return s0(uv2);
    }
    case 19: {
      return s0(vec2f(0.5 - abs(uv.x - 0.5), uv.y));
    }
    case 20: {
      let q = (uv - 0.5) * vec2f(aspect, 1.0);
      let r = length(q);
      let seg = TAU / 6.0;
      var a = atan2(q.y, q.x) + t * spd * 0.8;
      a = abs((a - seg * floor(a / seg)) - seg * 0.5);
      let q2 = vec2f(cos(a), sin(a)) * r;
      let uv2 = q2 / vec2f(aspect, 1.0) + 0.5;
      return mix(orig, s0(uv2), amt);
    }
    case 21: {
      let d = uv - 0.5;
      let k = amt * 0.02;
      let r = s0(vec2f(0.5) + d * (1.0 + k));
      let b = s0(vec2f(0.5) + d * (1.0 - k));
      return vec4f(r.r, orig.g, b.b, orig.a);
    }
    case 22: {
      let b = s1(uv);
      let bright = max(b.rgb - vec3f(0.45), vec3f(0.0)) * 1.8;
      let c = orig.rgb + bright * amt * 1.6 + b.rgb * amt * 0.15;
      return vec4f(min(c, vec3f(1.0)), orig.a);
    }
    case 23: {
      let rate = 2.0 + spd * 14.0;
      let pulse = pow(max(0.0, sin(t * rate * TAU)), 10.0);
      let c = mix(unpremul(orig), vec3f(1.0), pulse * amt);
      return vec4f(c * orig.a, orig.a);
    }
    case 24: {
      let q = uv * vec2f(aspect, 1.0);
      let ph = fract(t * (0.08 + spd * 0.25));
      let col = leak_color(q, ph * 1.3 - 0.15, 0.0);
      let c = 1.0 - (1.0 - unpremul(orig)) * (1.0 - col * amt);
      return vec4f(c * orig.a, orig.a);
    }
    case 25: {
      let tx = u.canvas.zw;
      let l00 = luma(unpremul(s0(uv + tx * vec2f(-1.0, -1.0))));
      let l10 = luma(unpremul(s0(uv + tx * vec2f(0.0, -1.0))));
      let l20 = luma(unpremul(s0(uv + tx * vec2f(1.0, -1.0))));
      let l01 = luma(unpremul(s0(uv + tx * vec2f(-1.0, 0.0))));
      let l21 = luma(unpremul(s0(uv + tx * vec2f(1.0, 0.0))));
      let l02 = luma(unpremul(s0(uv + tx * vec2f(-1.0, 1.0))));
      let l12 = luma(unpremul(s0(uv + tx * vec2f(0.0, 1.0))));
      let l22 = luma(unpremul(s0(uv + tx * vec2f(1.0, 1.0))));
      let gx = (l20 + 2.0 * l21 + l22) - (l00 + 2.0 * l01 + l02);
      let gy = (l02 + 2.0 * l12 + l22) - (l00 + 2.0 * l10 + l20);
      let e = clamp(length(vec2f(gx, gy)) * 2.5, 0.0, 1.0);
      let hue = fract(atan2(gy, gx) / TAU + t * 0.1);
      let neon = hsv2rgb(hue, 0.8, 1.0) * e;
      let c = mix(unpremul(orig), unpremul(orig) * 0.25 + neon * 1.4, amt);
      return vec4f(clamp(c, vec3f(0.0), vec3f(1.0)), 1.0);
    }
    case 26: {
      let c = hue_rotate(unpremul(orig), t * (0.5 + spd * 3.0) * TAU * 0.25);
      return vec4f(mix(unpremul(orig), clamp(c, vec3f(0.0), vec3f(1.0)), amt) * orig.a, orig.a);
    }
    case 27: {
      var c = unpremul(orig) * mix(1.0, 0.55, amt);
      let q = uv * vec2f(aspect, 1.0);
      let sp = 0.6 + spd * 2.0;
      for (var i: i32 = 0; i < 4; i++) {
        let fi = f32(i);
        let center = vec2f(aspect * (0.5 + 0.45 * sin(t * sp * (0.7 + fi * 0.3) + fi * 2.0)), 0.5 + 0.4 * cos(t * sp * (0.9 + fi * 0.2) + fi));
        let col = hsv2rgb(fract(fi * 0.27 + t * 0.1), 0.9, 1.0);
        c += col * exp(-dot2(q - center) * 9.0) * amt * 0.9;
      }
      return vec4f(min(c, vec3f(1.0)), 1.0);
    }
    case 28: {
      var c = unpremul(orig);
      let q = uv * vec2f(aspect, 1.0) * 7.0;
      for (var layer: i32 = 0; layer < 2; layer++) {
        let sc = 1.0 + f32(layer) * 0.7;
        let qq = q * sc;
        let cell = floor(qq);
        let rnd = hash22(cell + vec2f(f32(layer) * 17.0));
        let pos = cell + rnd;
        let d = (qq - pos) / sc;
        let tw = pow(max(0.0, sin(t * (1.5 + spd * 5.0) + rnd.x * TAU)), 6.0);
        let s = exp(-length(d) * 30.0) * 0.8 + exp(-abs(d.x) * 220.0 - abs(d.y) * 14.0) * 0.6 + exp(-abs(d.y) * 220.0 - abs(d.x) * 14.0) * 0.6;
        c += vec3f(1.0, 0.97, 0.85) * s * tw * amt * step(0.55, rnd.y);
      }
      return vec4f(min(c, vec3f(1.0)), 1.0);
    }
    case 29: {
      let n = max(u.p1.xy, vec2f(1.0));
      let cell = fract(uv * n);
      let cell_aspect = (res.x / n.x) / (res.y / n.y);
      var q = cell;
      if (cell_aspect > aspect) {
        q.y = 0.5 + (cell.y - 0.5) * (aspect / cell_aspect);
      } else {
        q.x = 0.5 + (cell.x - 0.5) * (cell_aspect / aspect);
      }
      return s0(q);
    }
    case 30: {
      var c = unpremul(orig);
      for (var layer: i32 = 0; layer < 3; layer++) {
        let fl = f32(layer);
        let sc = 10.0 + fl * 9.0;
        var q = uv * vec2f(aspect, 1.0) * sc;
        q.y -= t * (0.6 + spd * 1.5) * (1.0 + fl * 0.4) * sc * 0.12;
        q.x += sin(q.y * 0.5 + fl) * 0.3;
        let cell = floor(q);
        let rnd = hash22(cell + vec2f(fl * 31.0));
        let d = length(q - cell - rnd);
        let size = 0.06 + 0.08 * hash12(cell + vec2f(5.0));
        c = mix(c, vec3f(1.0), (1.0 - smoothstep(size * 0.5, size, d)) * amt * step(0.4, rnd.y) * (1.0 - fl * 0.2));
      }
      return vec4f(c, 1.0);
    }
    case 31: {
      var c = unpremul(orig) * mix(1.0, 0.8, amt);
      for (var layer: i32 = 0; layer < 2; layer++) {
        let fl = f32(layer);
        var q = uv * vec2f(aspect * 40.0, 4.0) * (1.0 + fl * 0.6);
        q.x += q.y * 0.15;
        q.y -= t * (2.0 + spd * 4.0) * (1.0 + fl * 0.5);
        let cell = floor(q);
        let rnd = hash22(cell + vec2f(fl * 13.0));
        let f = fract(q);
        let streak = (1.0 - smoothstep(0.0, 0.08, abs(f.x - rnd.x))) * smoothstep(0.0, 0.5, f.y) * (1.0 - smoothstep(0.5, 1.0, f.y));
        c += vec3f(0.6, 0.65, 0.75) * streak * step(0.75, rnd.y) * amt * 0.6;
      }
      return vec4f(min(c, vec3f(1.0)), 1.0);
    }
    case 32: {
      let l = luma(unpremul(orig));
      var c = vec3f(0.1, 1.0, 0.25) * pow(l, 0.8) * 1.4;
      c += vec3f((hash12(px + vec2f(t * 50.0)) - 0.5) * 0.18);
      c *= 0.9 + 0.1 * sin(uv.y * res.y * 1.2);
      c = vignette(c, uv, 0.8);
      return vec4f(mix(unpremul(orig), clamp(c, vec3f(0.0), vec3f(1.0)), amt), 1.0);
    }
    case 33: {
      let l = luma(unpremul(orig));
      return vec4f(mix(unpremul(orig), heat(l), amt), 1.0);
    }
    case 34: {
      let b = unpremul(s1(uv));
      let c0 = unpremul(orig);
      var c = 1.0 - (1.0 - c0) * (1.0 - b * 0.75);
      c = mix(vec3f(luma(c)), c, 0.85) * vec3f(1.03, 1.0, 0.97);
      return vec4f(mix(c0, clamp(c, vec3f(0.0), vec3f(1.0)), amt), 1.0);
    }
    case 35: {
      let b = s1(uv);
      let focus = mix(0.05, 0.3, u.p0.y);
      let m = smoothstep(focus, focus + 0.25, abs(uv.y - 0.5));
      var c = mix(orig, b, m * min(amt * 1.5, 1.0));
      let cu = unpremul(c);
      let sat = mix(vec3f(luma(cu)), cu, 1.0 + 0.25 * amt);
      return vec4f(clamp(sat, vec3f(0.0), vec3f(1.0)) * c.a, c.a);
    }
    case 36: {
      var q = uv;
      q.x += sin(uv.y * 12.0 + t * (1.0 + spd * 6.0)) * 0.012 * amt;
      q.y += cos(uv.x * 10.0 + t * (1.0 + spd * 5.0)) * 0.007 * amt;
      return s0(q);
    }
    default: { return orig; }
  }
}
