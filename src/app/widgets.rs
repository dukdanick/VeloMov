//! Small widgets: icon buttons, slider rows with keyframe
//! diamonds, toggles, segmented controls, library tiles, colour pickers.

use super::i18n::tr;
use super::theme::*;
use eframe::egui::{
    self, text::LayoutJob, Align2, Color32, CornerRadius, Galley, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};
use std::sync::Arc;

/// True when no popup, window or modal covers `p` (panels draw on the background layer,
/// so raw pointer input must be checked against overlapping areas).
pub fn pointer_free(ui: &Ui, p: Pos2) -> bool {
    ui.ctx().layer_id_at(p).map(|l| l == ui.layer_id()).unwrap_or(true)
}

pub fn cr(r: u8) -> CornerRadius {
    CornerRadius::same(r)
}

pub fn elided(ui: &Ui, text: &str, font: egui::FontId, color: Color32, max_w: f32) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.to_string(), egui::TextFormat { font_id: font, color, ..Default::default() });
    job.wrap = egui::text::TextWrapping { max_width: max_w.max(4.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.fonts(|f| f.layout_job(job))
}

pub fn icon_btn(ui: &mut Ui, icon: &str, tip: &str, enabled: bool, active: bool) -> Response {
    icon_btn_sized(ui, icon, tip, enabled, active, 26.0, 16.0)
}

pub fn icon_btn_sized(ui: &mut Ui, icon: &str, tip: &str, enabled: bool, active: bool, size: f32, font: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    if enabled && (resp.hovered() || active) {
        p.rect_filled(rect, cr(5), if active { ACCENT_BG() } else { PANEL3() });
    }
    let col = if !enabled {
        TEXT3()
    } else if active {
        ACCENT()
    } else if resp.hovered() {
        Color32::WHITE
    } else {
        TEXT()
    };
    p.text(rect.center(), Align2::CENTER_CENTER, icon, prop(font), col);
    if tip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tip)
    }
}

/// Brand textures (logo, gradient), loaded on first use.
#[derive(Default)]
pub struct BrandTex {
    logos: Vec<(u32, egui::TextureHandle)>,
    grad: Option<(Color32, Color32, u8, egui::TextureHandle)>,
}

impl BrandTex {
    fn png(ctx: &egui::Context, name: &str, bytes: &[u8]) -> Option<egui::TextureHandle> {
        let img = image::load_from_memory(bytes).ok()?.to_rgba8();
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        Some(ctx.load_texture(name, ci, egui::TextureOptions::LINEAR))
    }

    /// The app icon, from the bitmap closest to `size` points on screen (no mipmaps in egui).
    pub fn logo(&mut self, ctx: &egui::Context, size: f32) -> Option<egui::TextureId> {
        let px = size * ctx.pixels_per_point();
        let want: u32 = if px <= 32.0 {
            32
        } else if px <= 64.0 {
            64
        } else {
            128
        };
        if let Some((_, t)) = self.logos.iter().find(|(s, _)| *s == want) {
            return Some(t.id());
        }
        let bytes: &[u8] = match want {
            32 => include_bytes!("../../assets/icons/velomov-32.png"),
            64 => include_bytes!("../../assets/icons/velomov-64.png"),
            _ => include_bytes!("../../assets/icons/velomov-128.png"),
        };
        let t = Self::png(ctx, &format!("logo-{}", want), bytes)?;
        let id = t.id();
        self.logos.push((want, t));
        Some(id)
    }

    /// Brand gradient (GRAD_A() -> GRAD_B(), in the direction chosen in the settings)
    /// to fill shapes with: use it with UVs (0,0)-(1,1).
    pub fn grad(&mut self, ctx: &egui::Context) -> egui::TextureId {
        let (a, b, dir) = (GRAD_A(), GRAD_B(), ui_prefs().grad_dir);
        if let Some((ga, gb, gd, t)) = &self.grad {
            if (*ga, *gb, *gd) == (a, b, dir) {
                return t.id();
            }
        }
        // (re)made when the accent colour or the gradient settings change
        let img = match dir {
            1 => {
                let n = 64;
                let px = (0..n * n)
                    .map(|i| lerp_color(a, b, ((i % n) + (i / n)) as f32 / (2 * (n - 1)) as f32))
                    .collect();
                egui::ColorImage { size: [n, n], pixels: px }
            }
            2 => {
                let n = 128;
                egui::ColorImage { size: [1, n], pixels: (0..n).map(|i| lerp_color(a, b, i as f32 / (n - 1) as f32)).collect() }
            }
            _ => {
                let n = 128;
                egui::ColorImage { size: [n, 1], pixels: (0..n).map(|i| lerp_color(a, b, i as f32 / (n - 1) as f32)).collect() }
            }
        };
        let t = ctx.load_texture("brand-grad", img, egui::TextureOptions::LINEAR);
        let id = t.id();
        self.grad = Some((a, b, dir, t));
        id
    }
}

/// Draw the logo into `rect`.
pub fn logo(ui: &Ui, brand: &mut BrandTex, rect: Rect) {
    if let Some(t) = brand.logo(ui.ctx(), rect.width()) {
        ui.painter().image(t, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
}

/// Primary call-to-action: rounded pill filled with the brand gradient.
pub fn grad_pill(ui: &mut Ui, brand: &mut BrandTex, icon: &str, label: &str) -> Response {
    let txt = if icon.is_empty() { label.to_string() } else { format!("{}  {}", icon, label) };
    let galley = ui.fonts(|f| f.layout_no_wrap(txt, bold(12.5), ON_GRAD()));
    let size = Vec2::new(galley.size().x + 28.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let tex = brand.grad(ui.ctx());
    let p = ui.painter();
    let shape = egui::epaint::RectShape::filled(rect, cr(15), Color32::WHITE)
        .with_texture(tex, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)));
    p.add(shape);
    if resp.hovered() {
        p.rect_filled(rect, cr(15), Color32::from_white_alpha(28));
    }
    if resp.is_pointer_button_down_on() {
        p.rect_filled(rect, cr(15), Color32::from_black_alpha(40));
    }
    p.galley(rect.center() - galley.size() / 2.0, galley, ON_GRAD());
    resp
}

/// Text button with optional icon, "pill" look.
pub fn pill(ui: &mut Ui, icon: &str, label: &str, fill: Color32, text: Color32) -> Response {
    let font = prop(12.5);
    let txt = if icon.is_empty() { label.to_string() } else { format!("{}  {}", icon, label) };
    let galley = ui.fonts(|f| f.layout_no_wrap(txt, font, text));
    let size = galley.size() + Vec2::new(22.0, 10.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(size.x, size.y.max(28.0)), Sense::click());
    let f = if resp.hovered() { fill.gamma_multiply(1.15) } else { fill };
    ui.painter().rect_filled(rect, cr(6), f);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, text);
    resp
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kf {
    None,
    Has,
    On,
}

pub fn diamond(ui: &mut Ui, state: Kf, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(18.0, 18.0), Sense::click());
    let c = rect.center();
    let r = 5.0;
    let pts = vec![Pos2::new(c.x, c.y - r), Pos2::new(c.x + r, c.y), Pos2::new(c.x, c.y + r), Pos2::new(c.x - r, c.y)];
    let hov = resp.hovered();
    match state {
        Kf::On => ui.painter().add(egui::Shape::convex_polygon(pts, ACCENT(), Stroke::new(1.0_f32, ACCENT()))),
        Kf::Has => ui.painter().add(egui::Shape::closed_line(pts, Stroke::new(1.3_f32, if hov { ACCENT() } else { TEXT() }))),
        Kf::None => ui.painter().add(egui::Shape::closed_line(pts, Stroke::new(1.1_f32, if hov { TEXT() } else { TEXT3() }))),
    };
    resp.on_hover_text(tip)
}

#[derive(Default, Clone, Copy)]
pub struct RowOut {
    pub changed: bool,
    pub kf_clicked: bool,
    pub editing: bool,
}

/// Slider row: label | slider | value | keyframe diamond.
#[allow(clippy::too_many_arguments)]
pub fn slider_row(
    ui: &mut Ui,
    label: &str,
    v: &mut f64,
    min: f64,
    max: f64,
    default: f64,
    decimals: usize,
    suffix: &str,
    kf: Option<Kf>,
) -> RowOut {
    let mut out = RowOut::default();
    ui.horizontal(|ui| {
        ui.set_height(24.0);
        let lw = 92.0;
        let (lr, _) = ui.allocate_exact_size(Vec2::new(lw, 20.0), Sense::hover());
        let g = elided(ui, tr(label), prop(12.0), TEXT2(), lw - 4.0);
        ui.painter().galley(Pos2::new(lr.left(), lr.center().y - g.size().y / 2.0), g, TEXT2());
        // the row must never be wider than the panel: a panel grows to fit its content, and
        // while dragging (a repaint every frame) it would keep growing
        let value_w = value_box_width(ui, &format!("{:.*}{}", decimals, *v, suffix)).max(58.0);
        let gap = ui.spacing().item_spacing.x;
        let kf_w = if kf.is_some() { 18.0 + gap } else { 0.0 };
        let sw = (ui.available_width() - value_w - kf_w - gap - 1.0).max(24.0);
        let (sr, resp) = ui.allocate_exact_size(Vec2::new(sw, 20.0), Sense::click_and_drag());
        let span = (max - min).max(1e-9);
        if resp.double_clicked() {
            *v = default;
            out.changed = true;
        } else if resp.dragged() || resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let f = ((pos.x - sr.left()) / sr.width()).clamp(0.0, 1.0) as f64;
                let nv = min + f * span;
                let q = 10f64.powi(decimals as i32);
                let nv = (nv * q).round() / q;
                if nv != *v {
                    *v = nv;
                    out.changed = true;
                }
            }
        }
        out.editing |= resp.dragged();
        let p = ui.painter();
        let y = sr.center().y;
        let track = Rect::from_min_max(Pos2::new(sr.left(), y - 1.5), Pos2::new(sr.right(), y + 1.5));
        p.rect_filled(track, cr(2), PANEL3());
        let fx = |val: f64| sr.left() + (((val - min) / span).clamp(0.0, 1.0) as f32) * sr.width();
        let zero = if min < 0.0 && max > 0.0 { 0.0 } else { min };
        let (a, b) = (fx(zero).min(fx(*v)), fx(zero).max(fx(*v)));
        p.rect_filled(Rect::from_min_max(Pos2::new(a, y - 1.5), Pos2::new(b, y + 1.5)), cr(2), ACCENT());
        let kx = fx(*v);
        let hr = if resp.hovered() || resp.dragged() { 7.0 } else { 6.0 };
        p.circle_filled(Pos2::new(kx, y), hr, Color32::WHITE);
        let mut tmp = *v;
        let speed = span / 300.0;
        let dv = ui.add_sized(
            Vec2::new(value_w, 20.0),
            egui::DragValue::new(&mut tmp).speed(speed).range(min..=max).max_decimals(decimals).min_decimals(0).suffix(suffix),
        );
        if dv.changed() && tmp != *v {
            *v = tmp;
            out.changed = true;
        }
        out.editing |= dv.dragged() || dv.has_focus();
        if let Some(k) = kf {
            if diamond(ui, k, tr("Add / remove keyframe")).clicked() {
                out.kf_clicked = true;
            }
        }
    });
    out
}

/// Logarithmic slider row for speeds (0.1x .. 100x); edits `speed` directly.
pub fn speed_row(ui: &mut Ui, label: &str, speed: &mut f64) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.set_height(24.0);
        let lw = 92.0;
        let (lr, _) = ui.allocate_exact_size(Vec2::new(lw, 20.0), Sense::hover());
        let g = elided(ui, tr(label), prop(12.0), TEXT2(), lw - 4.0);
        ui.painter().galley(Pos2::new(lr.left(), lr.center().y - g.size().y / 2.0), g, TEXT2());
        let value_w = value_box_width(ui, &format!("{:.2}x", *speed)).max(64.0);
        let gap = ui.spacing().item_spacing.x;
        let sw = (ui.available_width() - value_w - gap - 1.0).max(24.0);
        let (sr, resp) = ui.allocate_exact_size(Vec2::new(sw, 20.0), Sense::click_and_drag());
        let (lo, hi) = (-1.0f64, 2.0f64);
        if resp.double_clicked() {
            *speed = 1.0;
            changed = true;
        } else if resp.dragged() || resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let f = ((pos.x - sr.left()) / sr.width()).clamp(0.0, 1.0) as f64;
                let v = 10f64.powf(lo + f * (hi - lo));
                let v = if v < 10.0 { (v * 10.0).round() / 10.0 } else { v.round() };
                if (v - *speed).abs() > 1e-9 {
                    *speed = v;
                    changed = true;
                }
            }
        }
        let p = ui.painter();
        let y = sr.center().y;
        p.rect_filled(Rect::from_min_max(Pos2::new(sr.left(), y - 1.5), Pos2::new(sr.right(), y + 1.5)), cr(2), PANEL3());
        let fx = ((speed.max(0.1).log10() - lo) / (hi - lo)).clamp(0.0, 1.0) as f32;
        let kx = sr.left() + fx * sr.width();
        let one = sr.left() + ((0.0 - lo) / (hi - lo)) as f32 * sr.width();
        p.rect_filled(Rect::from_min_max(Pos2::new(one.min(kx), y - 1.5), Pos2::new(one.max(kx), y + 1.5)), cr(2), ACCENT());
        for (v, t) in [(0.1f64, "0.1x"), (1.0, "1x"), (10.0, "10x"), (100.0, "100x")] {
            let x = sr.left() + ((v.log10() - lo) / (hi - lo)) as f32 * sr.width();
            p.text(Pos2::new(x, y + 9.0), Align2::CENTER_TOP, t, prop(9.0), TEXT3());
        }
        p.circle_filled(Pos2::new(kx, y), if resp.hovered() || resp.dragged() { 7.0 } else { 6.0 }, Color32::WHITE);
        let mut tmp = *speed;
        let dv = ui
            .add_sized(Vec2::new(value_w, 20.0), egui::DragValue::new(&mut tmp).speed(0.01).range(0.1..=100.0).max_decimals(2).suffix("x"));
        if dv.changed() && (tmp - *speed).abs() > 1e-9 {
            *speed = tmp;
            changed = true;
        }
    });
    ui.add_space(8.0);
    changed
}

/// Width a DragValue needs to show `text` without growing past its box.
fn value_box_width(ui: &Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let w = ui.fonts(|f| f.layout_no_wrap(text.to_string(), font, Color32::WHITE).size().x);
    (w + ui.spacing().button_padding.x * 2.0 + 4.0).ceil()
}

pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(30.0, 16.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let bg = lerp_color(PANEL3(), ACCENT(), t);
    ui.painter().rect_filled(rect, cr(8), bg);
    let x = egui::lerp(rect.left() + 8.0..=rect.right() - 8.0, t);
    ui.painter().circle_filled(Pos2::new(x, rect.center().y), 6.0, Color32::WHITE);
    resp
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// Bold section title with an optional toggle on the right.
pub fn section(ui: &mut Ui, title: &str, toggle_state: Option<&mut bool>) -> bool {
    let mut changed = false;
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr(title)).font(bold(13.0)).color(TEXT()));
        if let Some(on) = toggle_state {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if toggle(ui, on).changed() {
                    changed = true;
                }
            });
        }
    });
    ui.add_space(2.0);
    changed
}

pub fn separator(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 9.0), Sense::hover());
    ui.painter().line_segment([Pos2::new(r.left(), r.center().y), Pos2::new(r.right(), r.center().y)], Stroke::new(1.0_f32, BORDER()));
}

/// Segmented control; returns true when the selection changed.
pub fn segmented(ui: &mut Ui, opts: &[&str], sel: &mut usize) -> bool {
    let mut changed = false;
    let h = 26.0;
    let w = ui.available_width().min(320.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    ui.painter().rect_filled(rect, cr(6), PANEL2());
    let n = opts.len().max(1) as f32;
    let cw = rect.width() / n;
    for (i, o) in opts.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(rect.left() + i as f32 * cw, rect.top()), Vec2::new(cw, h)).shrink(2.0);
        let resp = ui.interact(r, ui.id().with(("seg", i, *o)), Sense::click());
        if resp.clicked() && *sel != i {
            *sel = i;
            changed = true;
        }
        if *sel == i {
            ui.painter().rect_filled(r, cr(5), PANEL3());
        } else if resp.hovered() {
            ui.painter().rect_filled(r, cr(5), Color32::from_white_alpha(8));
        }
        let col = if *sel == i { Color32::WHITE } else { TEXT2() };
        let g = elided(ui, tr(o), prop(12.0), col, r.width() - 6.0);
        ui.painter().galley(r.center() - g.size() / 2.0, g, col);
    }
    changed
}

/// Library tile: picture + label underneath. `hover_tex` replaces the picture while hovered.
pub fn tile(
    ui: &mut Ui,
    size: Vec2,
    tex: Option<egui::TextureId>,
    label: &str,
    selected: bool,
    badge: Option<&str>,
    placeholder: &str,
) -> Response {
    let label_h = if label.is_empty() { 0.0 } else { 18.0 };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(size.x, size.y + label_h), Sense::click_and_drag());
    let img = Rect::from_min_size(rect.min, size);
    let p = ui.painter();
    p.rect_filled(img, cr(8), PANEL2());
    if let Some(t) = tex {
        p.add(
            egui::epaint::RectShape::filled(img, cr(8), Color32::WHITE)
                .with_texture(t, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))),
        );
    } else if !placeholder.is_empty() {
        p.text(img.center(), Align2::CENTER_CENTER, placeholder, prop(22.0), TEXT2());
    }
    if let Some(b) = badge {
        let g = ui.fonts(|f| f.layout_no_wrap(b.to_string(), prop(10.5), Color32::WHITE));
        let br = Rect::from_min_size(
            Pos2::new(img.right() - g.size().x - 10.0, img.bottom() - g.size().y - 6.0),
            g.size() + Vec2::new(6.0, 2.0),
        );
        p.rect_filled(br, cr(4), Color32::from_black_alpha(170));
        p.galley(br.min + Vec2::new(3.0, 1.0), g, Color32::WHITE);
    }
    if selected {
        p.rect_stroke(img, cr(8), Stroke::new(2.0_f32, ACCENT()), StrokeKind::Inside);
    } else if resp.hovered() {
        p.rect_stroke(img, cr(8), Stroke::new(1.0_f32, Color32::from_white_alpha(90)), StrokeKind::Inside);
        let c = Pos2::new(img.right() - 13.0, img.bottom() - 13.0);
        if badge.is_none() {
            p.circle_filled(c, 9.0, ACCENT());
            p.text(c, Align2::CENTER_CENTER, egui_phosphor::regular::PLUS, prop(12.0), ON_ACCENT());
        }
    }
    if !label.is_empty() {
        let g = elided(ui, label, prop(11.5), if selected { ACCENT() } else { TEXT2() }, size.x);
        ui.painter().galley(Pos2::new(rect.left() + (size.x - g.size().x) / 2.0, img.bottom() + 3.0), g, TEXT2());
    }
    resp
}

/// Colour swatch that opens the text palette + a full picker.
pub fn color_button(ui: &mut Ui, id: egui::Id, c: &mut [u8; 4], allow_alpha: bool) -> bool {
    let mut changed = false;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(40.0, 20.0), Sense::click());
    // checkerboard for alpha
    let p = ui.painter();
    p.rect_filled(rect, cr(4), Color32::from_gray(200));
    let half = Rect::from_min_size(rect.min, rect.size() / 2.0);
    p.rect_filled(half, cr(0), Color32::from_gray(150));
    p.rect_filled(half.translate(rect.size() / 2.0), cr(0), Color32::from_gray(150));
    p.rect_filled(rect, cr(4), c32(*c));
    p.rect_stroke(rect, cr(4), Stroke::new(1.0_f32, Color32::from_white_alpha(60)), StrokeKind::Inside);
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(id));
    }
    egui::popup::popup_below_widget(ui, id, &resp, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        ui.set_min_width(212.0);
        egui::Grid::new(id.with("pal")).spacing(Vec2::new(3.0, 3.0)).show(ui, |ui| {
            for (i, pc) in PALETTE.iter().enumerate() {
                let (r, rr) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                ui.painter().rect_filled(r, cr(3), Color32::from_rgb(pc[0], pc[1], pc[2]));
                if [c[0], c[1], c[2]] == *pc {
                    ui.painter().rect_stroke(r, cr(3), Stroke::new(2.0_f32, ACCENT()), StrokeKind::Outside);
                }
                if rr.clicked() {
                    c[0] = pc[0];
                    c[1] = pc[1];
                    c[2] = pc[2];
                    if c[3] == 0 {
                        c[3] = 255;
                    }
                    changed = true;
                }
                if i % 6 == 5 {
                    ui.end_row();
                }
            }
        });
        ui.separator();
        let mut col = c32(*c);
        let alpha = if allow_alpha { egui::color_picker::Alpha::OnlyBlend } else { egui::color_picker::Alpha::Opaque };
        if egui::color_picker::color_picker_color32(ui, &mut col, alpha) {
            let [r, g, b, a] = col.to_srgba_unmultiplied();
            *c = [r, g, b, if allow_alpha { a } else { 255 }];
            changed = true;
        }
    });
    changed
}

/// "Label: [combo]" row.
pub fn combo_row<T: PartialEq + Copy>(ui: &mut Ui, label: &str, value: &mut T, options: &[(T, String)]) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let (lr, _) = ui.allocate_exact_size(Vec2::new(92.0, 20.0), Sense::hover());
        ui.painter().text(Pos2::new(lr.left(), lr.center().y), Align2::LEFT_CENTER, tr(label), prop(12.0), TEXT2());
        let cur = options.iter().find(|o| o.0 == *value).map(|o| o.1.clone()).unwrap_or_default();
        egui::ComboBox::from_id_salt(ui.id().with(label)).width(ui.available_width() - 6.0).selected_text(cur).show_ui(ui, |ui| {
            for (v, l) in options {
                if ui.selectable_label(*v == *value, l.as_str()).clicked() && *v != *value {
                    *value = *v;
                    changed = true;
                }
            }
        });
    });
    changed
}

pub fn label_row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let (lr, _) = ui.allocate_exact_size(Vec2::new(92.0, 20.0), Sense::hover());
        ui.painter().text(Pos2::new(lr.left(), lr.center().y), Align2::LEFT_CENTER, tr(label), prop(12.0), TEXT2());
        add(ui);
    });
}
