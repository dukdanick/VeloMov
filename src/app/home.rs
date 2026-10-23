//! Home screen: welcome banner (greeting, main actions, last project, tip of the day)
//! and the projects, as a grid or a list.

use super::i18n::tr;
use super::theme::*;
use super::widgets::*;
use super::{FilePurpose, VeloApp};
use crate::model::Project;
use crate::util::{self, Us};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as ph;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

#[derive(Clone)]
pub struct ProjectSummary {
    pub path: PathBuf,
    pub name: String,
    pub modified: i64,
    pub duration: Us,
    pub ratio: String,
}

#[derive(Default)]
pub struct HomeState {
    pub projects: Vec<ProjectSummary>,
    pub covers: HashMap<PathBuf, Option<egui::TextureHandle>>,
    pub search: String,
    /// Projects waiting for the "delete?" confirmation.
    pub confirm_delete: Vec<PathBuf>,
    /// Multiple selection (Ctrl+click, the round check on a card, or "Select").
    pub selected: std::collections::HashSet<PathBuf>,
    pub select_mode: bool,
    pub renaming: Option<(PathBuf, String)>,
    pub demo_rx: Option<Receiver<Result<PathBuf, String>>>,
    /// Tips skipped on the banner (added to the tip of the day).
    pub tip: usize,
}

impl HomeState {
    pub fn refresh(&mut self) {
        self.projects = list_projects();
        self.covers.clear();
        let alive: std::collections::HashSet<PathBuf> = self.projects.iter().map(|p| p.path.clone()).collect();
        self.selected.retain(|p| alive.contains(p));
    }
}

pub fn list_projects() -> Vec<ProjectSummary> {
    let mut paths = vec![];
    if let Ok(rd) = std::fs::read_dir(util::projects_dir()) {
        for e in rd.flatten() {
            let p = util::project_file(&e.path());
            let old = p.with_extension(util::OLD_PROJECT_EXT);
            if p.exists() {
                paths.push(p);
            } else if old.exists() {
                paths.push(util::upgrade_project_file(&old));
            }
        }
    }
    // projects saved elsewhere with "Save as" (the ones gone from disk are dropped)
    let external = external_projects();
    let alive: Vec<PathBuf> = external.iter().filter(|p| p.exists()).cloned().collect();
    if alive.len() != external.len() {
        write_external_projects(&alive);
    }
    paths.extend(alive);
    let mut v = vec![];
    for p in paths {
        if let Ok(s) = std::fs::read_to_string(&p) {
            if let Ok(proj) = serde_json::from_str::<Project>(&s) {
                v.push(ProjectSummary {
                    path: p,
                    name: proj.name.clone(),
                    modified: proj.modified,
                    duration: proj.duration(),
                    ratio: format!("{}x{}", proj.width, proj.height),
                });
            }
        }
    }
    v.sort_by_key(|p| -p.modified);
    v
}

fn external_list_path() -> PathBuf {
    util::config_dir().join("projects.json")
}

fn external_projects() -> Vec<PathBuf> {
    std::fs::read_to_string(external_list_path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn write_external_projects(v: &[PathBuf]) {
    if let Ok(s) = serde_json::to_string_pretty(v) {
        let _ = std::fs::write(external_list_path(), s);
    }
}

/// Show a project saved outside the projects directory on the home screen.
pub fn remember_project(path: &std::path::Path) {
    if util::is_internal_project(path) {
        return;
    }
    let mut v = external_projects();
    if !v.iter().any(|p| p == path) {
        v.push(path.to_path_buf());
        write_external_projects(&v);
    }
}

fn forget_project(path: &std::path::Path) {
    let mut v = external_projects();
    v.retain(|p| p != path);
    write_external_projects(&v);
}

/// Delete a project: its folder in the projects directory, or just the file (and its
/// cover) for one saved elsewhere.
fn delete_project(path: &std::path::Path) {
    if util::is_internal_project(path) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    } else {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(util::cover_path(path));
        forget_project(path);
    }
}

fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn fmt_date(ms: i64) -> String {
    let now = util::now_ms();
    let diff = (now - ms) / 1000;
    if diff < 60 {
        return tr("just now").to_string();
    }
    if diff < 3600 {
        return format!("{} {}", diff / 60, tr("min ago"));
    }
    if diff < 86400 {
        return format!("{} {}", diff / 3600, tr("h ago"));
    }
    if diff < 2 * 86400 {
        return tr("yesterday").to_string();
    }
    if diff < 7 * 86400 {
        return format!("{} {}", diff / 86400, tr("days ago"));
    }
    let (y, m, d) = civil(ms / 86_400_000);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Hour (0-23) and day of the year, in local time.
fn local_time() -> (u32, u32) {
    let secs = util::now_ms() / 1000;
    #[cfg(unix)]
    unsafe {
        let now = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&now, &mut tm).is_null() {
            return (tm.tm_hour as u32, tm.tm_yday as u32);
        }
    }
    #[cfg(windows)]
    unsafe {
        let now = secs as libc::time_t;
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_s(&mut tm, &now) == 0 {
            return (tm.tm_hour as u32, tm.tm_yday as u32);
        }
    }
    (((secs / 3600) % 24) as u32, ((secs / 86400) % 365) as u32)
}

fn greeting() -> &'static str {
    match local_time().0 {
        5..=11 => tr("Good morning"),
        12..=17 => tr("Good afternoon"),
        _ => tr("Good evening"),
    }
}

/// Tips shown on the home banner, one per day (click for the next one).
const TIPS: &[&str] = &[
    "Drop video files anywhere on the window to import them.",
    "Shift+click selects a range of clips, Ctrl+click adds one more.",
    "Press ? in the editor to see every keyboard shortcut.",
    "Star your favourite fonts in the font list to keep them at the top.",
    "Auto captions turn speech into animated subtitles, right on this computer.",
    "Settings > Layout puts the media library and the timeline where you like.",
    "Heavy 4K videos? Proxies make the preview smooth; exports use the originals.",
    "Settings > Appearance changes the gradient, corners and contrast of the interface.",
];

/// "1 h 05 min", "3 min 20 s"…
fn fmt_total(us: Us) -> String {
    let s = (us / util::SEC).max(0);
    if s >= 3600 {
        format!("{} h {:02} min", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{} min {:02} s", s / 60, s % 60)
    } else {
        format!("{} s", s)
    }
}

/// Soft round light (the aurora on the banner): opaque centre fading to nothing.
fn blob(p: &egui::Painter, c: Pos2, r: f32, col: Color32, a: f32) {
    use egui::epaint::Mesh;
    let mut m = Mesh::default();
    let center = col.gamma_multiply(a);
    let mid = col.gamma_multiply(a * 0.5);
    m.colored_vertex(c, center);
    let n = 48u32;
    for ring in [(0.55_f32, mid), (1.0, Color32::TRANSPARENT)] {
        for i in 0..n {
            let ang = i as f32 / n as f32 * std::f32::consts::TAU;
            m.colored_vertex(c + Vec2::new(ang.cos(), ang.sin()) * r * ring.0, ring.1);
        }
    }
    for i in 0..n {
        let j = (i + 1) % n;
        m.add_triangle(0, 1 + i, 1 + j);
        m.add_triangle(1 + i, 1 + n + i, 1 + n + j);
        m.add_triangle(1 + i, 1 + n + j, 1 + j);
    }
    p.add(m);
}

/// Paint the outside of `r`'s rounded corners with `col` (round off what was clipped square).
fn mask_corners(p: &egui::Painter, r: Rect, rad: f32, col: Color32) {
    use egui::epaint::Mesh;
    use std::f32::consts::PI;
    let corners = [
        (r.left_top(), r.left_top() + Vec2::new(rad, rad), PI),
        (r.right_top(), r.right_top() + Vec2::new(-rad, rad), 1.5 * PI),
        (r.right_bottom(), r.right_bottom() + Vec2::new(-rad, -rad), 0.0),
        (r.left_bottom(), r.left_bottom() + Vec2::new(rad, -rad), 0.5 * PI),
    ];
    let mut m = Mesh::default();
    for (corner, center, a0) in corners {
        let base = m.vertices.len() as u32;
        m.colored_vertex(corner, col);
        let n = 10;
        for i in 0..=n {
            let a = a0 + i as f32 / n as f32 * PI * 0.5;
            m.colored_vertex(center + Vec2::new(a.cos(), a.sin()) * (rad + 0.5), col);
        }
        for i in 0..n {
            m.add_triangle(base, base + 1 + i, base + 2 + i);
        }
    }
    p.add(m);
}

/// Rectangle with a colour per corner (top-left, top-right, bottom-right, bottom-left).
fn quad(p: &egui::Painter, r: Rect, c: [Color32; 4]) {
    let mut m = egui::epaint::Mesh::default();
    m.colored_vertex(r.left_top(), c[0]);
    m.colored_vertex(r.right_top(), c[1]);
    m.colored_vertex(r.right_bottom(), c[2]);
    m.colored_vertex(r.left_bottom(), c[3]);
    m.add_triangle(0, 1, 2);
    m.add_triangle(0, 2, 3);
    p.add(m);
}

/// Button on the banner: white (primary) or frosted glass.
fn banner_btn(ui: &mut egui::Ui, icon: &str, label: &str, primary: bool, busy: bool) -> egui::Response {
    let txt = format!("{}  {}", icon, label);
    let col = if primary { Color32::from_gray(14) } else { Color32::WHITE };
    let g = ui.fonts(|f| f.layout_no_wrap(txt, if primary { bold(13.0) } else { prop(13.0) }, col));
    let (r, resp) = ui.allocate_exact_size(Vec2::new(g.size().x + 30.0, 36.0), Sense::click());
    let p = ui.painter();
    let hover = resp.hovered() && !busy;
    let fill = if primary {
        if hover {
            Color32::from_gray(230)
        } else {
            Color32::WHITE
        }
    } else if hover {
        Color32::from_white_alpha(52)
    } else {
        Color32::from_white_alpha(26)
    };
    p.rect_filled(r, cr(18), fill);
    if !primary {
        p.rect_stroke(r, cr(18), Stroke::new(1.0_f32, Color32::from_white_alpha(40)), StrokeKind::Inside);
    }
    p.galley(r.center() - g.size() / 2.0, g, col);
    if busy {
        resp
    } else {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    }
}

fn start_demo(app: &mut VeloApp) {
    if app.home.demo_rx.is_some() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    app.home.demo_rx = Some(rx);
    std::thread::spawn(move || {
        let dir = util::data_dir().join("demo-media");
        let r = crate::demo::make_demo(&dir).and_then(|mut p| {
            p.name = "VeloMov demo".into();
            let path = util::project_file(&util::projects_dir().join(&p.id));
            p.save(&path)?;
            Ok(path)
        });
        let _ = tx.send(r.map_err(|e| e.to_string()));
    });
}

/// Cover texture of a project (loaded once).
fn cover_tex(app: &mut VeloApp, ctx: &egui::Context, path: &std::path::Path) -> Option<egui::TextureHandle> {
    app.home
        .covers
        .entry(path.to_path_buf())
        .or_insert_with(|| {
            let im = image::open(util::cover_path(path)).ok()?.to_rgba8();
            let ci = egui::ColorImage::from_rgba_unmultiplied([im.width() as usize, im.height() as usize], im.as_raw());
            Some(ctx.load_texture(format!("cover-{}", path.display()), ci, egui::TextureOptions::LINEAR))
        })
        .clone()
}

/// Draw `tex` to cover `r` (cropped to fill it), with rounded corners.
fn paint_cover(p: &egui::Painter, r: Rect, tex: &egui::TextureHandle, round: u8) {
    let sz = tex.size_vec2();
    let (ta, ra) = (sz.x / sz.y.max(1.0), r.width() / r.height().max(1.0));
    let uv = if ta > ra {
        let w = ra / ta;
        Rect::from_min_max(Pos2::new(0.5 - w / 2.0, 0.0), Pos2::new(0.5 + w / 2.0, 1.0))
    } else {
        let h = ta / ra;
        Rect::from_min_max(Pos2::new(0.0, 0.5 - h / 2.0), Pos2::new(1.0, 0.5 + h / 2.0))
    };
    p.add(egui::epaint::RectShape::filled(r, cr(round), Color32::WHITE).with_texture(tex.id(), uv));
}

/// The welcome banner: moving aurora in the gradient colours, greeting, main actions,
/// "continue editing" card over a little animated timeline, and a tip.
fn banner(app: &mut VeloApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let prefs = ui_prefs();
    let w = ui.available_width();
    let h = 252.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let round = (CARD_R() + 6) as f32;
    let t = if prefs.animations { ctx.input(|i| i.time) as f32 } else { 0.0 };
    let p = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    p.rect_filled(rect, cr(round as u8), mix(BG(), PANEL2(), 0.55));
    // aurora
    let (l, tp) = (rect.left(), rect.top());
    let s = t * 0.16;
    blob(&p, Pos2::new(l + w * (0.16 + 0.07 * s.sin()), tp + h * (0.25 + 0.18 * (1.3 * s).cos())), h * 1.25, GRAD_A(), 0.62);
    blob(&p, Pos2::new(l + w * (0.6 + 0.09 * (0.8 * s).cos()), tp + h * (0.85 + 0.14 * (1.1 * s).sin())), h * 1.3, GRAD_B(), 0.58);
    blob(&p, Pos2::new(l + w * (0.92 + 0.05 * (1.7 * s).sin()), tp + h * (0.15 + 0.12 * s.cos())), h * 0.95, grad_at(0.5), 0.42);
    // darker on the left so the text reads well
    let shade = Color32::from_black_alpha(150);
    quad(&p, Rect::from_min_max(rect.min, Pos2::new(l + w * 0.62, rect.bottom())), [shade, Color32::TRANSPARENT, Color32::TRANSPARENT, shade]);
    // where the "continue editing" card goes (right side, wide windows only)
    let latest = app.home.projects.iter().max_by_key(|p| p.modified).cloned().filter(|_| w > 860.0);
    let card_w = (w * 0.3).min(330.0);
    let cover_h = (card_w - 20.0) * 9.0 / 16.0;
    let card_h = (cover_h + 88.0).min(h - 24.0);
    let card = Rect::from_min_size(Pos2::new(rect.right() - card_w - 30.0, rect.center().y - card_h / 2.0), Vec2::new(card_w, card_h));
    // little timeline drifting between the text and the card, fading out at both ends
    let deco_right = if latest.is_some() { card.left() - 26.0 } else { rect.right() - 30.0 };
    let deco = Rect::from_min_max(Pos2::new(l + w * 0.47, tp + 34.0), Pos2::new(deco_right, rect.bottom() - 34.0));
    if deco.width() > 80.0 {
        let dp = p.with_clip_rect(deco.intersect(p.clip_rect()));
        let fade = |x: f32| ((x - deco.left()) / 90.0).min((deco.right() - x) / 90.0).clamp(0.0, 1.0);
        // ruler
        let ruler_y = deco.top() + 6.0;
        let step = 22.0;
        let off = (t * 16.0) % step;
        let mut x = deco.left() - off;
        let mut k = (t * 16.0 / step) as i64;
        while x < deco.right() {
            let big = k % 4 == 0;
            let a = (fade(x) * if big { 120.0 } else { 60.0 }) as u8;
            dp.line_segment([Pos2::new(x, ruler_y - if big { 5.0 } else { 2.0 }), Pos2::new(x, ruler_y + 2.0)], Stroke::new(1.0_f32, Color32::from_white_alpha(a)));
            x += step;
            k += 1;
        }
        // clips on four tracks, the main one a bit brighter
        let lanes = 4;
        let top = ruler_y + 12.0;
        let lane_h = (deco.bottom() - top) / lanes as f32;
        for lane in 0..lanes {
            let speed = 16.0;
            let span = 900.0;
            let mut x = deco.left() - ((t * speed) % span);
            let mut k = lane * 7 + 3;
            while x < deco.right() + span {
                k = (k * 37 + 11) % 97;
                let cw = 46.0 + (k % 9) as f32 * 13.0;
                let r = Rect::from_min_size(Pos2::new(x, top + lane as f32 * lane_h + 3.0), Vec2::new(cw, lane_h - 7.0));
                let a = if lane == 2 { 7.0 } else { 3.0 };
                if r.right() > deco.left() && r.left() < deco.right() {
                    // per-corner alpha so the clips fade in and out at the ends
                    let col = |x: f32, k: f32| Color32::from_white_alpha((fade(x) * a * k) as u8);
                    let (cl, cr_) = (col(r.left(), 1.0), col(r.right(), 1.0));
                    quad(&dp, r, [cl, cr_, cr_, cl]);
                    let top = Rect::from_min_size(r.min, Vec2::new(r.width(), 2.0));
                    let (hl, hr) = (col(r.left(), 2.6), col(r.right(), 2.6));
                    quad(&dp, top, [hl, hr, hr, hl]);
                }
                x += cw + 6.0 + (k % 4) as f32 * 12.0;
            }
        }
        let ph_x = deco.left() + deco.width() * (0.42 + 0.05 * (t * 0.4).sin());
        dp.line_segment([Pos2::new(ph_x, ruler_y - 2.0), Pos2::new(ph_x, deco.bottom())], Stroke::new(2.0_f32, Color32::from_white_alpha(200)));
        dp.circle_filled(Pos2::new(ph_x, ruler_y - 2.0), 5.0, Color32::WHITE);
    }
    mask_corners(&p, rect, round, PANEL());
    // ---- left: greeting, title, actions, tip
    let mut cui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(Rect::from_min_max(rect.min + Vec2::new(30.0, 26.0), Pos2::new(l + w * 0.62, rect.bottom() - 18.0)))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let ui = &mut cui;
    ui.label(egui::RichText::new(format!("{}  {}", ph::HAND_WAVING, greeting())).size(14.0).color(Color32::from_white_alpha(210)));
    ui.add_space(2.0);
    ui.label(egui::RichText::new(tr("What are we creating today?")).font(bold(28.0)).color(Color32::WHITE));
    let n = app.home.projects.len();
    let total: Us = app.home.projects.iter().map(|p| p.duration).sum();
    let stats = if n == 0 {
        tr("Your first project starts here.").to_string()
    } else {
        format!("{} {}  •  {} {}", n, if n == 1 { tr("project") } else { tr("projects") }, fmt_total(total), tr("of video"))
    };
    ui.label(egui::RichText::new(stats).size(12.5).color(Color32::from_white_alpha(170)));
    ui.add_space(18.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        if banner_btn(ui, ph::PLUS, tr("Create project"), true, false).clicked() {
            app.open_new_project(false);
        }
        if banner_btn(ui, ph::DOWNLOAD_SIMPLE, tr("Import media"), false, false).on_hover_text(tr("New project from files")).clicked() {
            app.open_new_project(true);
        }
        let busy = app.home.demo_rx.is_some();
        let label = if busy { tr("Generating demo media…") } else { tr("Demo project") };
        if banner_btn(ui, ph::SPARKLE, label, false, busy).on_hover_text(tr("Transitions, keyframes, effects")).clicked() {
            start_demo(app);
        }
    });
    // tip of the day
    let (_, day) = local_time();
    let tip = TIPS[(day as usize + app.home.tip) % TIPS.len()];
    let tip_rect = Rect::from_min_size(Pos2::new(rect.left() + 30.0, rect.bottom() - 40.0), Vec2::new(w * 0.58, 22.0));
    let resp = ui.interact(tip_rect, ui.id().with("tip"), Sense::click());
    let g = elided(ui, &format!("{}  {}", ph::LIGHTBULB, tr(tip)), prop(12.0), Color32::WHITE, tip_rect.width());
    let alpha = if resp.hovered() { 235 } else { 165 };
    ui.painter().galley(Pos2::new(tip_rect.left(), tip_rect.center().y - g.size().y / 2.0), g, Color32::from_white_alpha(alpha));
    if resp.on_hover_text(tr("Next tip")).clicked() {
        app.home.tip += 1;
    }
    // ---- right: continue editing
    if let Some(last) = latest {
        let resp = ui.interact(card, ui.id().with("continue"), Sense::click());
        let lift = ctx.animate_bool_with_time(resp.id, resp.hovered(), if prefs.animations { 0.15 } else { 0.0 });
        let card = card.translate(Vec2::new(0.0, -4.0 * lift));
        let p = ui.painter();
        p.add(egui::epaint::Shadow { offset: [0, 10], blur: 28, spread: 0, color: Color32::from_black_alpha(110) }.as_shape(card, cr(14)));
        p.rect_filled(card, cr(14), Color32::from_rgba_premultiplied(8, 8, 12, 200));
        p.rect_stroke(card, cr(14), Stroke::new(1.0_f32, Color32::from_white_alpha(40 + (80.0 * lift) as u8)), StrokeKind::Inside);
        p.text(
            card.left_top() + Vec2::new(14.0, 16.0),
            Align2::LEFT_CENTER,
            format!("{}  {}", ph::CLOCK_COUNTER_CLOCKWISE, tr("Continue editing").to_uppercase()),
            bold(10.5),
            Color32::from_white_alpha(190),
        );
        let cov = Rect::from_min_size(card.left_top() + Vec2::new(10.0, 30.0), Vec2::new(card_w - 20.0, card_h - 88.0));
        let ui_ctx = ui.ctx().clone();
        match cover_tex(app, &ui_ctx, &last.path) {
            Some(tex) => paint_cover(ui.painter(), cov, &tex, 9),
            None => {
                ui.painter().rect_filled(cov, cr(9), PANEL2());
                ui.painter().text(cov.center(), Align2::CENTER_CENTER, ph::FILM_STRIP, prop(28.0), TEXT3());
            }
        }
        let p = ui.painter();
        p.circle_filled(cov.center(), 20.0 + 3.0 * lift, Color32::from_black_alpha(120 + (60.0 * lift) as u8));
        p.text(cov.center() + Vec2::new(1.5, 0.0), Align2::CENTER_CENTER, ph::PLAY, fill(18.0), Color32::WHITE);
        let name = elided(ui, &last.name, bold(13.5), Color32::WHITE, card_w - 28.0);
        ui.painter().galley(Pos2::new(card.left() + 14.0, cov.bottom() + 9.0), name, Color32::WHITE);
        ui.painter().text(
            Pos2::new(card.left() + 14.0, card.bottom() - 16.0),
            Align2::LEFT_CENTER,
            format!("{}  •  {}", fmt_date(last.modified), &util::timecode(last.duration, 30.0)[3..8]),
            prop(11.0),
            Color32::from_white_alpha(160),
        );
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            if let Err(e) = app.open_project(&last.path) {
                app.toast_err(&e.to_string());
            }
        }
    }
    if prefs.animations {
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    }
}

/// Navigation entries of the home screen (top bar or side bar).
fn nav_items(app: &mut VeloApp, ui: &mut egui::Ui, vertical: bool) {
    let item = |ui: &mut egui::Ui, icon: &str, label: &str, active: bool| -> egui::Response {
        let label = tr(label);
        let g = ui.fonts(|f| f.layout_no_wrap(label.to_string(), prop(13.0), TEXT()));
        let size = if vertical { Vec2::new(ui.available_width(), 36.0) } else { Vec2::new(g.size().x + 44.0, 34.0) };
        let (r, resp) = ui.allocate_exact_size(size, Sense::click());
        if active || resp.hovered() {
            ui.painter().rect_filled(r, cr(9), if active { PANEL2() } else { PANEL() });
        }
        if active && vertical {
            let bar = Rect::from_min_size(Pos2::new(r.left(), r.top() + 9.0), Vec2::new(3.0, r.height() - 18.0));
            ui.painter().rect_filled(bar, cr(2), ACCENT());
        }
        ui.painter().text(
            Pos2::new(r.left() + 14.0, r.center().y),
            Align2::LEFT_CENTER,
            icon,
            prop(16.0),
            if active { ACCENT() } else { TEXT2() },
        );
        let col = if active || resp.hovered() { Color32::WHITE } else { TEXT() };
        ui.painter().galley(Pos2::new(r.left() + 38.0, r.center().y - g.size().y / 2.0), g, col);
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    };
    if vertical {
        item(ui, ph::HOUSE, "Home", true);
    }
    if item(ui, ph::FOLDER_OPEN, "Open project", false).clicked() {
        app.pick_files(FilePurpose::OpenProject);
    }
    if item(ui, ph::GEAR, "Settings", app.show_settings).clicked() {
        app.open_settings();
    }
    if item(ui, ph::INFO, "About", app.show_about).clicked() {
        app.show_about = true;
    }
}

fn brand_title(app: &mut VeloApp, ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
    logo(ui, &mut app.brand, r);
    ui.add_space(2.0);
    ui.label(egui::RichText::new("VeloMov").font(bold(19.0)).color(Color32::WHITE));
}

pub fn ui(app: &mut VeloApp, ctx: &egui::Context) {
    // demo generation finished?
    if let Some(rx) = &app.home.demo_rx {
        if let Ok(r) = rx.try_recv() {
            app.home.demo_rx = None;
            match r {
                Ok(p) => {
                    if let Err(e) = app.open_project(&p) {
                        app.toast_err(&e.to_string());
                    }
                }
                Err(e) => app.toast_err(&format!("{}: {}", tr("Demo failed"), e)),
            }
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        }
    }
    let prefs = ui_prefs();
    let version = format!("v{}  •  GPL-3.0", env!("CARGO_PKG_VERSION"));
    if prefs.home_nav == 0 {
        egui::TopBottomPanel::top("home-top")
            .exact_height(60.0)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(BG()).inner_margin(egui::Margin { left: 18, right: 14, top: 0, bottom: 0 }))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    brand_title(app, ui);
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(&version).size(11.0).color(TEXT3()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        // a left-to-right group, so they read Open, Settings, About
                        let w: f32 = ["Open project", "Settings", "About"]
                            .iter()
                            .map(|l| ui.fonts(|f| f.layout_no_wrap(tr(l).to_string(), prop(13.0), TEXT()).size().x) + 48.0)
                            .sum();
                        ui.allocate_ui_with_layout(Vec2::new(w, 40.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            nav_items(app, ui, false);
                        });
                    });
                });
            });
    } else {
        let panel = if prefs.home_nav == 2 { egui::SidePanel::right("home-nav") } else { egui::SidePanel::left("home-nav") };
        panel
            .exact_width(214.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::new().fill(BG()).inner_margin(egui::Margin::symmetric(14, 16)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| brand_title(app, ui));
                ui.add_space(22.0);
                ui.spacing_mut().item_spacing.y = 4.0;
                nav_items(app, ui, true);
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(egui::RichText::new(&version).size(11.0).color(TEXT3()));
                    ui.label(egui::RichText::new(tr("Open source video editor for Linux and Windows")).size(11.0).color(TEXT3()));
                });
            });
    }
    let margin = match prefs.home_nav {
        0 => egui::Margin { left: 8, right: 8, top: 0, bottom: 8 },
        1 => egui::Margin { left: 0, right: 8, top: 8, bottom: 8 },
        _ => egui::Margin { left: 8, right: 0, top: 8, bottom: 8 },
    };
    let content = egui::Frame::new()
        .fill(PANEL())
        .corner_radius(CARD_R() + 2)
        .stroke(card_stroke())
        .outer_margin(margin)
        .inner_margin(egui::Margin::symmetric(24, 20));
    egui::CentralPanel::default().frame(content).show(ctx, |ui| {
        egui::ScrollArea::vertical().id_salt("home-scroll").drag_to_scroll(false).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if prefs.home_hero {
                banner(app, ui);
                ui.add_space(24.0);
            } else {
                compact_actions(app, ui);
                ui.add_space(20.0);
            }
            projects(app, ui);
        });
    });
    dialogs(app, ctx);
    super::export_ui::about_window(app, ctx);
}

/// Without the banner: a slim row of actions.
fn compact_actions(app: &mut VeloApp, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("What are we creating today?")).font(bold(20.0)).color(Color32::WHITE));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let busy = app.home.demo_rx.is_some();
            if ui.add_enabled(!busy, egui::Button::new(format!("{}  {}", ph::SPARKLE, tr("Demo project")))).clicked() {
                start_demo(app);
            }
            if ui.button(format!("{}  {}", ph::DOWNLOAD_SIMPLE, tr("Import media"))).clicked() {
                app.open_new_project(true);
            }
            if grad_pill(ui, &mut app.brand, ph::PLUS, tr("Create project")).clicked() {
                app.open_new_project(false);
            }
        });
    });
}

/// "Projects" header (search, order, view) and the projects as cards or rows.
fn projects(app: &mut VeloApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let mut prefs = app.settings.ui;
    let mut prefs_changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("Your projects")).font(bold(18.0)).color(Color32::WHITE));
        let n = app.home.projects.len();
        let g = ui.fonts(|f| f.layout_no_wrap(n.to_string(), bold(11.0), TEXT()));
        let (r, _) = ui.allocate_exact_size(Vec2::new(g.size().x + 14.0, 20.0), Sense::hover());
        ui.painter().rect_filled(r, cr(10), PANEL3());
        ui.painter().galley(r.center() - g.size() / 2.0, g, TEXT());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !app.home.projects.is_empty() && !app.home.select_mode && app.home.selected.is_empty() {
                if ui.button(format!("{}  {}", ph::CHECK_SQUARE, tr("Select"))).clicked() {
                    app.home.select_mode = true;
                }
            }
            ui.add_space(4.0);
            for (v, icon, tip) in [(1u8, ph::ROWS, "List"), (0u8, ph::SQUARES_FOUR, "Grid")] {
                if icon_btn(ui, icon, tr(tip), true, prefs.home_view == v).clicked() && prefs.home_view != v {
                    prefs.home_view = v;
                    prefs_changed = true;
                }
            }
            ui.add_space(4.0);
            let sorts = ["Last edited", "Name", "Duration"];
            egui::ComboBox::from_id_salt("home-sort")
                .selected_text(format!("{}  {}", ph::ARROWS_DOWN_UP, tr(sorts[prefs.home_sort.min(2) as usize])))
                .width(150.0)
                .show_ui(ui, |ui| {
                    for (i, s) in sorts.iter().enumerate() {
                        if ui.selectable_label(prefs.home_sort as usize == i, tr(s)).clicked() {
                            prefs.home_sort = i as u8;
                            prefs_changed = true;
                        }
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut app.home.search)
                    .hint_text(format!("{}  {}", ph::MAGNIFYING_GLASS, tr("Search projects")))
                    .desired_width(200.0),
            );
        });
    });
    if prefs_changed {
        app.set_ui(&ctx, prefs);
    }
    selection_bar(app, ui);
    ui.add_space(12.0);
    if app.home.projects.is_empty() {
        empty_state(app, ui);
        return;
    }
    let search = app.home.search.to_lowercase();
    let mut list: Vec<ProjectSummary> =
        app.home.projects.iter().filter(|p| search.is_empty() || p.name.to_lowercase().contains(&search)).cloned().collect();
    match prefs.home_sort {
        1 => list.sort_by_key(|p| p.name.to_lowercase()),
        2 => list.sort_by_key(|p| -p.duration),
        _ => list.sort_by_key(|p| -p.modified),
    }
    if list.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(format!("{}  {}", ph::MAGNIFYING_GLASS, tr("No project matches the search."))).color(TEXT3()));
        });
        return;
    }
    if prefs.home_view == 1 {
        for p in &list {
            project_row(app, ui, p);
        }
        return;
    }
    let want = [168.0, 214.0, 276.0][prefs.home_card.min(2) as usize];
    let gap = 16.0;
    let avail = ui.available_width();
    let cols = ((avail + gap) / (want + gap)).floor().max(1.0) as usize;
    let card_w = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).min(want * 1.25);
    for row in list.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for p in row {
                project_card(app, ui, p, card_w);
            }
        });
        ui.add_space(14.0);
    }
}

fn empty_state(app: &mut VeloApp, ui: &mut egui::Ui) {
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 190.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(r, cr(CARD_R() + 4), if resp.hovered() { PANEL2() } else { mix(PANEL(), PANEL2(), 0.5) });
    let dash = Stroke::new(1.5_f32, if resp.hovered() { ACCENT() } else { mix(BORDER(), TEXT3(), 0.5) });
    let rr = r.shrink(1.0);
    for (a, b) in [
        (rr.left_top(), rr.right_top()),
        (rr.right_top(), rr.right_bottom()),
        (rr.right_bottom(), rr.left_bottom()),
        (rr.left_bottom(), rr.left_top()),
    ] {
        p.extend(egui::Shape::dashed_line(&[a, b], dash, 8.0, 6.0));
    }
    p.circle_filled(r.center() - Vec2::new(0.0, 30.0), 28.0, PANEL3());
    p.text(r.center() - Vec2::new(0.0, 30.0), Align2::CENTER_CENTER, ph::FILM_STRIP, prop(26.0), ACCENT());
    p.text(r.center() + Vec2::new(0.0, 16.0), Align2::CENTER_CENTER, tr("No projects yet"), bold(15.0), Color32::WHITE);
    p.text(
        r.center() + Vec2::new(0.0, 40.0),
        Align2::CENTER_CENTER,
        tr("Drop video files here or click to create your first project."),
        prop(12.0),
        TEXT2(),
    );
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.open_new_project(false);
    }
}

/// Click on a project card or row: select it (while selecting / with Ctrl) or open it.
fn click_project(app: &mut VeloApp, ui: &egui::Ui, p: &ProjectSummary, check_hit: bool) {
    let selecting = app.home.select_mode || !app.home.selected.is_empty();
    let ctrl = ui.input(|i| i.modifiers.command || i.modifiers.ctrl || i.modifiers.shift);
    if selecting || ctrl || check_hit {
        if !app.home.selected.remove(&p.path) {
            app.home.selected.insert(p.path.clone());
        }
    } else {
        let path = p.path.clone();
        if let Err(e) = app.open_project(&path) {
            app.toast_err(&e.to_string());
        }
    }
}

fn project_menu(app: &mut VeloApp, ui: &mut egui::Ui, p: &ProjectSummary) {
    let path = p.path.clone();
    if ui.button(format!("{}  {}", ph::FOLDER_OPEN, tr("Open"))).clicked() {
        if let Err(e) = app.open_project(&path) {
            app.toast_err(&e.to_string());
        }
        ui.close_menu();
    }
    if ui.button(format!("{}  {}", ph::TAG, tr("Rename"))).clicked() {
        app.home.renaming = Some((path.clone(), p.name.clone()));
        ui.close_menu();
    }
    if ui.button(format!("{}  {}", ph::COPY, tr("Duplicate"))).clicked() {
        duplicate_project(&path);
        app.home.refresh();
        ui.close_menu();
    }
    if ui.button(format!("{}  {}", ph::CHECK_SQUARE, tr("Select"))).clicked() {
        app.home.selected.insert(path.clone());
        ui.close_menu();
    }
    ui.separator();
    if ui.button(egui::RichText::new(format!("{}  {}", ph::TRASH, tr("Delete"))).color(DANGER)).clicked() {
        app.home.confirm_delete = vec![path.clone()];
        ui.close_menu();
    }
}

/// Round check box drawn at `c`; returns whether the pointer is on it.
fn select_check(ui: &egui::Ui, c: Pos2, selected: bool, show: bool) -> bool {
    let hit = ui.input(|i| i.pointer.hover_pos()).map(|h| h.distance(c) < 13.0).unwrap_or(false);
    if show {
        let p = ui.painter();
        if selected {
            p.circle_filled(c, 10.0, ACCENT());
            p.text(c, Align2::CENTER_CENTER, ph::CHECK, bold(12.0), ON_ACCENT());
        } else {
            p.circle_filled(c, 10.0, Color32::from_black_alpha(if hit { 200 } else { 140 }));
            p.circle_stroke(c, 10.0, Stroke::new(1.5_f32, Color32::from_white_alpha(220)));
        }
    }
    hit
}

fn project_card(app: &mut VeloApp, ui: &mut egui::Ui, p: &ProjectSummary, w: f32) {
    let h = w * 9.0 / 16.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h + 48.0), Sense::click());
    let anim = if ui_prefs().animations { 0.12 } else { 0.0 };
    let lift = ui.ctx().animate_bool_with_time(resp.id.with("lift"), resp.hovered(), anim);
    let img = Rect::from_min_size(rect.min, Vec2::new(w, h)).translate(Vec2::new(0.0, -3.0 * lift));
    let round = (CARD_R() + 2).min(16);
    if lift > 0.0 {
        let shadow = egui::epaint::Shadow { offset: [0, 8], blur: 22, spread: 0, color: Color32::from_black_alpha((140.0 * lift) as u8) };
        ui.painter().add(shadow.as_shape(img, cr(round)));
    }
    ui.painter().rect_filled(img, cr(round), PANEL2());
    let ctx = ui.ctx().clone();
    match cover_tex(app, &ctx, &p.path) {
        Some(t) => paint_cover(ui.painter(), img, &t, round),
        None => {
            ui.painter().text(img.center(), Align2::CENTER_CENTER, ph::FILM_STRIP, prop(28.0), TEXT3());
        }
    }
    let painter = ui.painter();
    let selected = app.home.selected.contains(&p.path);
    let selecting = app.home.select_mode || !app.home.selected.is_empty();
    if selected {
        painter.rect_filled(img, cr(round), ACCENT().gamma_multiply(0.14));
        painter.rect_stroke(img, cr(round), Stroke::new(2.5_f32, ACCENT()), StrokeKind::Inside);
    } else if lift > 0.0 {
        painter.rect_stroke(img, cr(round), Stroke::new(2.0_f32, grad_at(0.3).gamma_multiply(lift)), StrokeKind::Inside);
        if !selecting {
            // "open" button in the middle
            painter.circle_filled(img.center(), 22.0, Color32::from_black_alpha((150.0 * lift) as u8));
            painter.text(img.center() + Vec2::new(1.5, 0.0), Align2::CENTER_CENTER, ph::PLAY, fill(18.0), Color32::WHITE.gamma_multiply(lift));
        }
    }
    let check_hit = select_check(ui, Pos2::new(img.left() + 16.0, img.top() + 16.0), selected, selecting || resp.hovered());
    // duration badge
    let dur = util::timecode(p.duration, 30.0);
    let g = ui.fonts(|f| f.layout_no_wrap(dur[3..8].to_string(), prop(10.5), Color32::WHITE));
    let br = Rect::from_min_size(Pos2::new(img.right() - g.size().x - 14.0, img.bottom() - g.size().y - 10.0), g.size() + Vec2::new(8.0, 4.0));
    ui.painter().rect_filled(br, cr(5), Color32::from_black_alpha(170));
    ui.painter().galley(br.min + Vec2::new(4.0, 2.0), g, Color32::WHITE);
    let name = elided(ui, &p.name, bold(13.0), Color32::WHITE, w - 8.0);
    ui.painter().galley(Pos2::new(rect.left() + 2.0, rect.top() + h + 8.0), name, Color32::WHITE);
    ui.painter().text(
        Pos2::new(rect.left() + 2.0, rect.top() + h + 35.0),
        Align2::LEFT_CENTER,
        format!("{}  •  {}", fmt_date(p.modified), p.ratio),
        prop(11.0),
        TEXT3(),
    );
    if resp.clicked() {
        click_project(app, ui, p, check_hit);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).context_menu(|ui| project_menu(app, ui, p));
}

/// A project in the list view: cover, name, last edit, size and length.
fn project_row(app: &mut VeloApp, ui: &mut egui::Ui, p: &ProjectSummary) {
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 66.0), Sense::click());
    let selected = app.home.selected.contains(&p.path);
    let selecting = app.home.select_mode || !app.home.selected.is_empty();
    let painter = ui.painter();
    if selected {
        painter.rect_filled(r, cr(10), ACCENT_BG());
    } else if resp.hovered() {
        painter.rect_filled(r, cr(10), PANEL2());
    }
    let mut x = r.left() + 10.0;
    let mut check_hit = false;
    if selecting {
        check_hit = select_check(ui, Pos2::new(x + 10.0, r.center().y), selected, true);
        x += 30.0;
    }
    let thumb = Rect::from_min_size(Pos2::new(x, r.top() + 7.0), Vec2::new(92.0, 52.0));
    let ctx = ui.ctx().clone();
    match cover_tex(app, &ctx, &p.path) {
        Some(t) => paint_cover(ui.painter(), thumb, &t, 7),
        None => {
            ui.painter().rect_filled(thumb, cr(7), PANEL3());
            ui.painter().text(thumb.center(), Align2::CENTER_CENTER, ph::FILM_STRIP, prop(18.0), TEXT3());
        }
    }
    let tx = thumb.right() + 16.0;
    let cols_x = r.right() - 330.0;
    let name = elided(ui, &p.name, bold(13.5), Color32::WHITE, (cols_x - tx - 10.0).max(60.0));
    ui.painter().galley(Pos2::new(tx, r.center().y - name.size().y - 1.0), name, Color32::WHITE);
    ui.painter().text(Pos2::new(tx, r.center().y + 10.0), Align2::LEFT_CENTER, fmt_date(p.modified), prop(11.5), TEXT3());
    let dur = util::timecode(p.duration, 30.0);
    let p2 = ui.painter();
    p2.text(Pos2::new(cols_x, r.center().y), Align2::LEFT_CENTER, format!("{}  {}", ph::FRAME_CORNERS, p.ratio), prop(12.0), TEXT2());
    p2.text(Pos2::new(cols_x + 150.0, r.center().y), Align2::LEFT_CENTER, format!("{}  {}", ph::CLOCK, &dur[3..8]), prop(12.0), TEXT2());
    let dots = Rect::from_center_size(Pos2::new(r.right() - 26.0, r.center().y), Vec2::splat(28.0));
    let dresp = ui.interact(dots, ui.id().with(("row-menu", &p.path)), Sense::click());
    if dresp.hovered() {
        ui.painter().rect_filled(dots, cr(6), PANEL3());
    }
    ui.painter().text(dots.center(), Align2::CENTER_CENTER, ph::DOTS_THREE, prop(18.0), if resp.hovered() || dresp.hovered() { TEXT() } else { TEXT3() });
    let menu_id = ui.id().with(("row-popup", &p.path));
    if dresp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(menu_id));
    }
    egui::popup_below_widget(ui, menu_id, &dresp, egui::PopupCloseBehavior::CloseOnClick, |ui| {
        ui.set_min_width(170.0);
        project_menu(app, ui, p);
    });
    if resp.clicked() {
        click_project(app, ui, p, check_hit);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).context_menu(|ui| project_menu(app, ui, p));
    ui.add_space(4.0);
}

fn dialogs(app: &mut VeloApp, ctx: &egui::Context) {
    if !app.home.confirm_delete.is_empty() {
        let paths = app.home.confirm_delete.clone();
        let modal = egui::Modal::new(egui::Id::new("confirm-delete")).show(ctx, |ui| {
            ui.set_width(340.0);
            let title = if paths.len() == 1 {
                tr("Delete this project?").to_string()
            } else {
                format!("{} {} {}?", tr("Delete"), paths.len(), tr("projects"))
            };
            ui.label(egui::RichText::new(title).font(bold(15.0)));
            ui.label(egui::RichText::new(tr("Media files are not deleted.")).color(TEXT2()));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button(tr("Cancel")).clicked() {
                    app.home.confirm_delete.clear();
                }
                if pill(ui, ph::TRASH, tr("Delete"), DANGER, Color32::WHITE).clicked() {
                    for path in &paths {
                        delete_project(path);
                    }
                    app.home.confirm_delete.clear();
                    app.home.selected.clear();
                    app.home.select_mode = false;
                    app.home.refresh();
                    app.toast(&format!("{} {}", paths.len(), tr("deleted")));
                }
            });
        });
        if modal.should_close() {
            app.home.confirm_delete.clear();
        }
    }
    if let Some((path, mut name)) = app.home.renaming.clone() {
        let mut done = false;
        egui::Modal::new(egui::Id::new("rename")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.label(egui::RichText::new(tr("Rename project")).font(bold(15.0)));
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(300.0));
            r.request_focus();
            ui.horizontal(|ui| {
                if ui.button(tr("Cancel")).clicked() {
                    app.home.renaming = None;
                    done = true;
                }
                if pill(ui, "", tr("OK"), ACCENT_DARK(), Color32::WHITE).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if let Ok(mut p) = Project::load(&path) {
                        p.name = name.clone();
                        let _ = p.save(&path);
                    }
                    app.home.renaming = None;
                    app.home.refresh();
                    done = true;
                }
            });
        });
        if !done {
            app.home.renaming = Some((path, name));
        }
    }
}

/// Copy a project under a new id ("<name> copy"), cover included.
pub fn duplicate_project(path: &std::path::Path) {
    if let Ok(mut pj) = Project::load(path) {
        pj.id = format!("{:x}", util::new_id());
        pj.name = format!("{} {}", pj.name, tr("copy"));
        let np = util::project_file(&util::projects_dir().join(&pj.id));
        let _ = pj.save(&np);
        let _ = std::fs::copy(util::cover_path(path), util::cover_path(&np));
    }
}

/// Bar shown while projects are selected: count, select all, duplicate, delete.
fn selection_bar(app: &mut VeloApp, ui: &mut egui::Ui) {
    if !app.home.select_mode && app.home.selected.is_empty() {
        return;
    }
    let n = app.home.selected.len();
    // keyboard: Ctrl+A selects all, Delete asks, Escape leaves (not while a dialog is open)
    let free = app.home.confirm_delete.is_empty() && app.home.renaming.is_none() && !ui.ctx().wants_keyboard_input();
    let (all, del, esc) = ui.input(|i| {
        (i.modifiers.command && i.key_pressed(egui::Key::A), i.key_pressed(egui::Key::Delete), i.key_pressed(egui::Key::Escape))
    });
    let (all, del, esc) = (all && free, del && free, esc && free);
    ui.add_space(8.0);
    egui::Frame::new().fill(PANEL2()).corner_radius(cr(10)).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.horizontal(|ui| {
            let label = if n == 0 { tr("Click projects to select them").to_string() } else { format!("{} {}", n, tr("selected")) };
            ui.label(egui::RichText::new(label).color(Color32::WHITE));
            ui.add_space(10.0);
            if ui.button(format!("{}  {}", ph::CHECKS, tr("Select all"))).clicked() || all {
                app.home.selected = app.home.projects.iter().map(|p| p.path.clone()).collect();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr("Cancel")).clicked() || esc {
                    app.home.selected.clear();
                    app.home.select_mode = false;
                }
                let delete =
                    ui.add_enabled(n > 0, egui::Button::new(egui::RichText::new(format!("{}  {}", ph::TRASH, tr("Delete"))).color(DANGER)));
                if delete.clicked() || (del && n > 0) {
                    let mut v: Vec<PathBuf> = app.home.selected.iter().cloned().collect();
                    v.sort();
                    app.home.confirm_delete = v;
                }
                if ui.add_enabled(n > 0, egui::Button::new(format!("{}  {}", ph::COPY, tr("Duplicate")))).clicked() {
                    let paths: Vec<PathBuf> = app.home.selected.iter().cloned().collect();
                    for p in &paths {
                        duplicate_project(p);
                    }
                    app.home.selected.clear();
                    app.home.select_mode = false;
                    app.home.refresh();
                    app.toast(&format!("{} {}", paths.len(), tr("duplicated")));
                }
            });
        });
    });
}
