//! Dark themes with a configurable accent colour, gradient and interface layout.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

const fn hex(c: u32) -> Color32 {
    rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

// ---------------------------------------------------------------- themes

/// Colours of the current theme. Read through the `BG()`, `ACCENT()`… functions so a new
/// theme or accent applies at once, without a restart.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Palette {
    bg: Color32,
    panel: Color32,
    panel2: Color32,
    panel3: Color32,
    border: Color32,
    timeline: Color32,
    track: Color32,
    track_main: Color32,
    accent: Color32,
}

/// (id, name, [bg, panel, panel2, panel3, border, timeline, track, main track]).
pub const THEMES: &[(&str, &str, [u32; 8])] = &[
    ("black", "Black", [0x050506, 0x0e0e11, 0x18181c, 0x232329, 0x1c1c22, 0x0a0a0d, 0x121216, 0x17171c]),
    ("oled", "Pure black", [0x000000, 0x09090a, 0x141416, 0x1f1f22, 0x18181a, 0x040405, 0x0d0d0f, 0x111113]),
    ("graphite", "Graphite", [0x0b0b0e, 0x16161a, 0x202025, 0x2b2b32, 0x26262d, 0x131317, 0x1a1a1f, 0x1e1e24]),
    ("midnight", "Midnight blue", [0x04060b, 0x0c1019, 0x151b28, 0x1f2738, 0x1a2131, 0x090d15, 0x101624, 0x141b2c]),
    ("plum", "Plum", [0x07050a, 0x110d16, 0x1b1622, 0x261f2f, 0x211a29, 0x0d0a11, 0x15111b, 0x1a1520]),
];

/// Accent presets (name, colour).
pub const ACCENTS: &[(&str, [u8; 3])] = &[
    ("Cyan", [0x22, 0xd3, 0xee]),
    ("Blue", [0x3b, 0x82, 0xf6]),
    ("Violet", [0x8b, 0x5c, 0xf6]),
    ("Pink", [0xec, 0x48, 0x99]),
    ("Red", [0xef, 0x44, 0x44]),
    ("Orange", [0xf9, 0x73, 0x16]),
    ("Yellow", [0xea, 0xb3, 0x08]),
    ("Green", [0x22, 0xc5, 0x5e]),
];

pub const DEFAULT_THEME: &str = "black";
pub const DEFAULT_ACCENT: [u8; 3] = [0x22, 0xd3, 0xee];

// ---------------------------------------------------------------- interface preferences

/// How the interface looks and where things go (Settings > Appearance / Layout).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPrefs {
    /// Gradient of the Export button, home banner…: 0 = accent -> neighbouring hue,
    /// 1 = two custom colours, 2 = off (solid accent).
    pub grad_mode: u8,
    /// Hue distance of the second colour in mode 0 (degrees).
    pub grad_hue: f32,
    pub grad_a: [u8; 3],
    pub grad_b: [u8; 3],
    /// 0 = horizontal, 1 = diagonal, 2 = vertical.
    pub grad_dir: u8,
    /// Corner radius of the panels (cards), in points.
    pub corner: u8,
    /// Space between the editor's panels.
    pub gap: u8,
    /// Thin outline around the panels.
    pub borders: bool,
    /// Brighter secondary text.
    pub contrast: bool,
    /// Interface zoom (1 = 100%).
    pub scale: f32,
    /// Interface animations and the moving home background.
    pub animations: bool,
    /// Editor: media library on the right (and the inspector on the left).
    pub library_right: bool,
    /// Editor: timeline above the player.
    pub timeline_top: bool,
    /// Timeline track height: 0 = compact, 1 = normal, 2 = large.
    pub track_size: u8,
    /// Notifications at the bottom of the window instead of the top.
    pub toasts_bottom: bool,
    /// Home navigation: 0 = top bar, 1 = left sidebar, 2 = right sidebar.
    pub home_nav: u8,
    /// Home banner with the greeting and the last project.
    pub home_hero: bool,
    /// Home project list: 0 = grid, 1 = list.
    pub home_view: u8,
    /// Home card size: 0 = small, 1 = medium, 2 = large.
    pub home_card: u8,
    /// Home order: 0 = last edited, 1 = name, 2 = duration.
    pub home_sort: u8,
}

pub const DEFAULT_UI: UiPrefs = UiPrefs {
    grad_mode: 0,
    grad_hue: 55.0,
    grad_a: [0x22, 0xd3, 0xee],
    grad_b: [0x6d, 0x4a, 0xff],
    grad_dir: 0,
    corner: 10,
    gap: 3,
    borders: false,
    contrast: false,
    scale: 1.0,
    animations: true,
    library_right: false,
    timeline_top: false,
    track_size: 1,
    toasts_bottom: false,
    home_nav: 0,
    home_hero: true,
    home_view: 0,
    home_card: 1,
    home_sort: 0,
};

impl Default for UiPrefs {
    fn default() -> Self {
        DEFAULT_UI
    }
}

static UI: std::sync::RwLock<UiPrefs> = std::sync::RwLock::new(DEFAULT_UI);

/// Current interface preferences.
pub fn ui_prefs() -> UiPrefs {
    *UI.read().unwrap_or_else(|e| e.into_inner())
}

/// Use new interface preferences (call `apply_visuals` afterwards).
pub fn set_ui_prefs(p: UiPrefs) {
    *UI.write().unwrap_or_else(|e| e.into_inner()) = p;
}

const fn palette_of(c: [u32; 8], accent: Color32) -> Palette {
    Palette {
        bg: hex(c[0]),
        panel: hex(c[1]),
        panel2: hex(c[2]),
        panel3: hex(c[3]),
        border: hex(c[4]),
        timeline: hex(c[5]),
        track: hex(c[6]),
        track_main: hex(c[7]),
        accent,
    }
}

static PAL: std::sync::RwLock<Palette> = std::sync::RwLock::new(palette_of(THEMES[0].2, rgb(0x22, 0xd3, 0xee)));

fn pal() -> Palette {
    *PAL.read().unwrap_or_else(|e| e.into_inner())
}

/// Switch theme and accent (call `apply_visuals` afterwards).
pub fn set_palette(theme: &str, accent: [u8; 3]) {
    let colors = THEMES.iter().find(|t| t.0 == theme).unwrap_or(&THEMES[0]).2;
    *PAL.write().unwrap_or_else(|e| e.into_inner()) = palette_of(colors, rgb(accent[0], accent[1], accent[2]));
}

fn scale(c: Color32, k: f32) -> Color32 {
    let f = |v: u8| (v as f32 * k).round().clamp(0.0, 255.0) as u8;
    rgb(f(c.r()), f(c.g()), f(c.b()))
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// Rotate the hue of a colour by `deg` degrees.
fn hue_shift(c: Color32, deg: f32) -> Color32 {
    let (r, g, b) = (c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let mut h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    h = (h + deg).rem_euclid(360.0);
    let s = if max == 0.0 { 0.0 } else { d / max };
    let v = max;
    let cc = v * s;
    let x = cc * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let (r1, g1, b1) = match (h / 60.0) as i32 {
        0 => (cc, x, 0.0),
        1 => (x, cc, 0.0),
        2 => (0.0, cc, x),
        3 => (0.0, x, cc),
        4 => (x, 0.0, cc),
        _ => (cc, 0.0, x),
    };
    let m = v - cc;
    let f = |v: f32| ((v + m) * 255.0).round() as u8;
    rgb(f(r1), f(g1), f(b1))
}

#[allow(non_snake_case)]
pub fn BG() -> Color32 {
    pal().bg
}
#[allow(non_snake_case)]
pub fn PANEL() -> Color32 {
    pal().panel
}
#[allow(non_snake_case)]
pub fn PANEL2() -> Color32 {
    pal().panel2
}
#[allow(non_snake_case)]
pub fn PANEL3() -> Color32 {
    pal().panel3
}
#[allow(non_snake_case)]
pub fn BORDER() -> Color32 {
    pal().border
}
#[allow(non_snake_case)]
pub fn TEXT() -> Color32 {
    rgb(0xec, 0xec, 0xf1)
}
#[allow(non_snake_case)]
pub fn TEXT2() -> Color32 {
    if ui_prefs().contrast {
        rgb(0xcd, 0xcd, 0xd6)
    } else {
        rgb(0xa1, 0xa1, 0xac)
    }
}
#[allow(non_snake_case)]
pub fn TEXT3() -> Color32 {
    if ui_prefs().contrast {
        rgb(0x9e, 0x9e, 0xab)
    } else {
        rgb(0x6b, 0x6b, 0x77)
    }
}
#[allow(non_snake_case)]
pub fn ACCENT() -> Color32 {
    pal().accent
}
#[allow(non_snake_case)]
pub fn ACCENT_DARK() -> Color32 {
    scale(pal().accent, 0.48)
}
/// Translucent accent behind selected items.
#[allow(non_snake_case)]
pub fn ACCENT_BG() -> Color32 {
    let c = scale(pal().accent, 0.27);
    Color32::from_rgba_premultiplied(c.r(), c.g(), c.b(), 0xc8)
}
/// Text/icons drawn on an accent fill: black on light accents, white on dark ones.
#[allow(non_snake_case)]
pub fn ON_ACCENT() -> Color32 {
    let c = pal().accent;
    let l = 0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32;
    if l > 140.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}
/// Brand gradient (Export button, home): accent -> a neighbouring hue, two custom
/// colours, or just the accent when the gradient is turned off.
#[allow(non_snake_case)]
pub fn GRAD_A() -> Color32 {
    let u = ui_prefs();
    match u.grad_mode {
        1 => rgb(u.grad_a[0], u.grad_a[1], u.grad_a[2]),
        2 => pal().accent,
        _ => scale(pal().accent, 0.86),
    }
}
#[allow(non_snake_case)]
pub fn GRAD_B() -> Color32 {
    let u = ui_prefs();
    match u.grad_mode {
        1 => rgb(u.grad_b[0], u.grad_b[1], u.grad_b[2]),
        2 => pal().accent,
        _ => scale(hue_shift(pal().accent, u.grad_hue), 0.92),
    }
}
/// Colour of the brand gradient at `t` (0..1).
pub fn grad_at(t: f32) -> Color32 {
    mix(GRAD_A(), GRAD_B(), t.clamp(0.0, 1.0))
}
/// Text on the brand gradient.
#[allow(non_snake_case)]
pub fn ON_GRAD() -> Color32 {
    let c = mix(GRAD_A(), GRAD_B(), 0.5);
    let l = 0.2126 * c.r() as f32 + 0.7152 * c.g() as f32 + 0.0722 * c.b() as f32;
    if l > 165.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}
#[allow(non_snake_case)]
pub fn TIMELINE_BG() -> Color32 {
    pal().timeline
}
#[allow(non_snake_case)]
pub fn TRACK_BG() -> Color32 {
    pal().track
}
#[allow(non_snake_case)]
pub fn TRACK_MAIN() -> Color32 {
    pal().track_main
}

pub const CLIP_VIDEO: Color32 = rgb(0x17, 0x4c, 0x55);
pub const CLIP_AUDIO: Color32 = rgb(0x12, 0x46, 0x3b);
pub const WAVE: Color32 = rgb(0x3d, 0xd6, 0xa3);
pub const CLIP_TEXT: Color32 = rgb(0x8f, 0x55, 0x27);
pub const CLIP_STICKER: Color32 = rgb(0x99, 0x70, 0x1b);
pub const CLIP_EFFECT: Color32 = rgb(0x5d, 0x3d, 0xb5);
pub const CLIP_FILTER: Color32 = rgb(0x3f, 0x37, 0x9a);
pub const CLIP_ADJUST: Color32 = rgb(0x27, 0x55, 0xa6);
pub const DANGER: Color32 = rgb(0xff, 0x4d, 0x5e);
pub const KEYFRAME: Color32 = rgb(0xff, 0xff, 0xff);
/// Corner radius of the editor's panels (cards).
#[allow(non_snake_case)]
pub fn CARD_R() -> u8 {
    ui_prefs().corner.min(24)
}
/// Gap between the editor's panels.
#[allow(non_snake_case)]
pub fn GAP() -> i8 {
    ui_prefs().gap.min(16) as i8
}
/// Outline of a panel card (none unless "Panel borders" is on).
pub fn card_stroke() -> Stroke {
    if ui_prefs().borders {
        Stroke::new(1.0_f32, mix(BORDER(), TEXT3(), 0.35))
    } else {
        Stroke::NONE
    }
}

/// Text colour palette.
pub const PALETTE: &[[u8; 3]] = &[
    [0xff, 0xff, 0xff],
    [0xcc, 0xcc, 0xcc],
    [0x99, 0x99, 0x99],
    [0x66, 0x66, 0x66],
    [0x40, 0x40, 0x40],
    [0x00, 0x00, 0x00],
    [0xff, 0xcd, 0xd2],
    [0xfe, 0x8a, 0x80],
    [0xfe, 0x52, 0x52],
    [0xff, 0x18, 0x37],
    [0xff, 0x00, 0x00],
    [0xb7, 0x1c, 0x1c],
    [0xff, 0xd9, 0xc6],
    [0xff, 0xb1, 0x8c],
    [0xfa, 0x86, 0x4c],
    [0xff, 0x89, 0x20],
    [0xff, 0x71, 0x00],
    [0xab, 0x4a, 0x37],
    [0xf9, 0xf3, 0xc4],
    [0xff, 0xf5, 0x89],
    [0xff, 0xde, 0x00],
    [0xff, 0xbf, 0x17],
    [0xff, 0xa8, 0x00],
    [0xab, 0x74, 0x33],
    [0xff, 0xd9, 0xe8],
    [0xff, 0xaa, 0xc1],
    [0xff, 0x61, 0x9d],
    [0xff, 0x28, 0x7f],
    [0xfe, 0x00, 0xff],
    [0x90, 0x1e, 0x4b],
    [0xe7, 0xe2, 0xff],
    [0xc5, 0xc4, 0xff],
    [0x8e, 0x80, 0xe5],
    [0x88, 0x63, 0xff],
    [0x90, 0x55, 0xff],
    [0x42, 0x36, 0x9a],
    [0xb7, 0xdc, 0xf6],
    [0x8e, 0xc2, 0xf9],
    [0x46, 0x9d, 0xf3],
    [0x04, 0x7b, 0xff],
    [0x00, 0x23, 0xff],
    [0x2d, 0x50, 0x94],
    [0xc0, 0xf1, 0xf5],
    [0x95, 0xe7, 0xed],
    [0x61, 0xdb, 0xe4],
    [0x00, 0xcd, 0xe5],
    [0x00, 0xa9, 0xcd],
    [0x00, 0x6f, 0x87],
    [0xb9, 0xf5, 0xca],
    [0x69, 0xf0, 0xae],
    [0x00, 0xe6, 0x76],
    [0x34, 0xee, 0x4e],
    [0x00, 0xff, 0x4b],
    [0x18, 0x89, 0x3e],
];

fn fc_match(pattern: &str) -> Option<Vec<u8>> {
    #[cfg(windows)]
    {
        // no fontconfig: the Segoe UI files that ship with Windows
        let dir = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into())).join("Fonts");
        let bold = pattern.contains("Bold") || pattern.contains("SemiBold");
        let files: &[&str] = if bold { &["seguisb.ttf", "segoeuib.ttf"] } else { &["segoeui.ttf"] };
        return files.iter().find_map(|f| std::fs::read(dir.join(f)).ok());
    }
    #[allow(unreachable_code)]
    let out = std::process::Command::new("fc-match").args(["-f", "%{file}", pattern]).output().ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() || !(path.ends_with(".ttf") || path.ends_with(".otf")) {
        return None;
    }
    std::fs::read(path).ok()
}

pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let regular = ["Inter:style=Regular", "Noto Sans:style=Regular", "Open Sans:style=Regular", "DejaVu Sans:style=Book"]
        .iter()
        .find_map(|p| fc_match(p));
    let bold =
        ["Inter:style=SemiBold", "Noto Sans:style=SemiBold", "Noto Sans:style=Bold", "Open Sans:style=Bold", "DejaVu Sans:style=Bold"]
            .iter()
            .find_map(|p| fc_match(p));
    if let Some(r) = regular {
        fonts.font_data.insert("ui".into(), Arc::new(FontData::from_owned(r)));
        if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
            f.insert(0, "ui".into());
        }
    }
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    // wide-coverage fallback (arrows, symbols, more scripts)
    if let Some(fb) = fc_match("DejaVu Sans:style=Book") {
        fonts.font_data.insert("fallback".into(), Arc::new(FontData::from_owned(fb)));
        if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
            f.push("fallback".into());
        }
    }
    fonts.font_data.insert("phosphor-fill".into(), Arc::new(egui_phosphor::Variant::Fill.font_data()));
    let mut fill_fam = vec!["phosphor-fill".to_string()];
    fill_fam.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Name("fill".into()), fill_fam);
    let mut bold_fam = vec![];
    if let Some(b) = bold {
        fonts.font_data.insert("ui-bold".into(), Arc::new(FontData::from_owned(b)));
        bold_fam.push("ui-bold".to_string());
    }
    bold_fam.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Name("bold".into()), bold_fam);
    fonts
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("bold".into()))
}

pub fn fill(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("fill".into()))
}

pub fn prop(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    apply_visuals(ctx);
}

/// Interface zoom from the preferences (only touches egui when it changed).
pub fn apply_scale(ctx: &egui::Context) {
    let z = ui_prefs().scale.clamp(0.7, 1.6);
    if (ctx.zoom_factor() - z).abs() > 0.001 {
        ctx.set_zoom_factor(z);
    }
}

/// Spacing and colours (call again after `set_palette`).
pub fn apply_visuals(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(11.0)),
        (TextStyle::Body, FontId::proportional(12.5)),
        (TextStyle::Button, FontId::proportional(12.5)),
        (TextStyle::Heading, FontId::new(15.0, FontFamily::Name("bold".into()))),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(6.0, 6.0);
    style.spacing.button_padding = egui::vec2(9.0, 4.0);
    style.spacing.interact_size = egui::vec2(28.0, 24.0);
    style.spacing.slider_width = 120.0;
    style.spacing.slider_rail_height = 4.0;
    style.spacing.combo_width = 120.0;
    style.spacing.menu_margin = egui::Margin::same(6);
    style.spacing.scroll = egui::style::ScrollStyle::thin();
    style.interaction.tooltip_delay = 0.35;
    let u = ui_prefs();
    style.animation_time = if u.animations { 1.0 / 12.0 } else { 0.0 };
    let mut v = egui::Visuals::dark();
    v.dark_mode = true;
    v.panel_fill = BG();
    v.window_fill = mix(PANEL(), PANEL2(), 0.5);
    v.extreme_bg_color = mix(BG(), PANEL(), 0.5);
    v.faint_bg_color = PANEL2();
    v.code_bg_color = PANEL2();
    v.window_stroke = Stroke::new(1.0_f32, mix(BORDER(), TEXT3(), 0.15));
    let corner = CARD_R();
    v.window_corner_radius = CornerRadius::same(corner.saturating_add(2));
    v.menu_corner_radius = CornerRadius::same(corner);
    v.selection.bg_fill = ACCENT_DARK();
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT());
    v.hyperlink_color = ACCENT();
    v.override_text_color = Some(TEXT());
    v.text_cursor.stroke = Stroke::new(2.0_f32, ACCENT());
    let r = CornerRadius::same((corner as f32 * 0.6).round() as u8);
    v.widgets.noninteractive.bg_fill = PANEL();
    v.widgets.noninteractive.weak_bg_fill = PANEL();
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER());
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT2());
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.bg_fill = PANEL2();
    v.widgets.inactive.weak_bg_fill = PANEL2();
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT());
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.bg_fill = PANEL3();
    v.widgets.hovered.weak_bg_fill = PANEL3();
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, mix(PANEL3(), TEXT3(), 0.3));
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    v.widgets.hovered.corner_radius = r;
    v.widgets.hovered.expansion = 0.0;
    v.widgets.active.bg_fill = mix(PANEL3(), TEXT3(), 0.12);
    v.widgets.active.weak_bg_fill = mix(PANEL3(), TEXT3(), 0.12);
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT());
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    v.widgets.active.corner_radius = r;
    v.widgets.active.expansion = 0.0;
    v.widgets.open.bg_fill = PANEL3();
    v.widgets.open.weak_bg_fill = PANEL3();
    v.widgets.open.corner_radius = r;
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(150) };
    v.window_shadow = egui::epaint::Shadow { offset: [0, 10], blur: 32, spread: 0, color: Color32::from_black_alpha(170) };
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.striped = false;
    style.visuals = v;
    // always the app's own dark look, also when the system (e.g. Windows) uses a light theme
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style(style);
}

pub fn clip_color(kind: crate::model::TrackKind, audio_media: bool) -> Color32 {
    use crate::model::TrackKind as K;
    match kind {
        K::Video => CLIP_VIDEO,
        K::Audio => {
            let _ = audio_media;
            CLIP_AUDIO
        }
        K::Text => CLIP_TEXT,
        K::Sticker => CLIP_STICKER,
        K::Effect => CLIP_EFFECT,
        K::Filter => CLIP_FILTER,
        K::Adjust => CLIP_ADJUST,
    }
}

pub fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}
