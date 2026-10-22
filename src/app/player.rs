//! Centre panel: the player (program preview), on-canvas transform gizmo
//! and playback controls.

use super::i18n::tr;
use super::theme::*;
use super::widgets::*;
use super::VeloApp;
use crate::model::keyframe::Prop;
use crate::model::*;
use crate::util::{self, Us};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use egui_phosphor::regular as ph;

#[derive(Clone, Copy, Debug)]
enum GizmoKind {
    Move,
    Scale,
    Rotate,
}

#[derive(Clone, Copy, Debug)]
struct GizmoDrag {
    kind: GizmoKind,
    clip: u64,
    start: Pos2,
    center: Pos2,
    x: f64,
    y: f64,
    scale: f64,
    rot: f64,
}

pub fn ui(app: &mut VeloApp, ui: &mut egui::Ui) {
    let full = ui.available_rect_before_wrap();
    // header: title, background work, preview quality
    let head = Rect::from_min_size(full.min, Vec2::new(full.width(), 34.0));
    ui.painter().text(Pos2::new(head.left() + 14.0, head.center().y), Align2::LEFT_CENTER, tr("Player"), bold(12.5), TEXT());
    let chip = quality_chip(app, ui, Pos2::new(head.right() - 10.0, head.center().y));
    let mut sx = chip.left() - 10.0;
    if let Some(f) = app.proxy_progress() {
        let label = format!("{}  {} {:.0}%", ph::LIGHTNING, tr("Optimizing media"), f * 100.0);
        let r = ui.painter().text(Pos2::new(sx, head.center().y), Align2::RIGHT_CENTER, label, prop(11.0), TEXT3());
        sx = r.left() - 10.0;
        ui.interact(r, ui.id().with("proxy-status"), Sense::hover())
            .on_hover_text(tr("Light copies of heavy videos are being made so the preview plays smoothly."));
    }
    if app.render_pending {
        ui.painter().text(Pos2::new(sx, head.center().y), Align2::RIGHT_CENTER, ph::CIRCLE_NOTCH, prop(13.0), TEXT3());
    }
    // controls
    let ctrl = Rect::from_min_max(Pos2::new(full.left(), full.bottom() - 44.0), full.max);
    let area = Rect::from_min_max(Pos2::new(full.left(), head.bottom()), Pos2::new(full.right(), ctrl.top()));
    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(area));
    preview_area(app, &mut cui, false);
    controls(app, ui, ctrl);
    ui.allocate_rect(full, Sense::hover());
}

pub const QUALITY_NAMES: [&str; 3] = ["High", "Medium", "Low"];

/// Render size factor and height cap of the preview for each quality level.
pub fn quality_scale(q: u8) -> (f32, f32) {
    [(1.0, 1080.0), (0.75, 720.0), (0.5, 480.0)][q.min(2) as usize]
}

/// "Quality" chip in the player header, right-aligned at `right`; returns its rect.
fn quality_chip(app: &mut VeloApp, ui: &mut egui::Ui, right: Pos2) -> Rect {
    let q = app.settings.preview_quality.min(2);
    let label = format!("{}  {}  {}", ph::GAUGE, tr(QUALITY_NAMES[q as usize]), ph::CARET_DOWN);
    let g = ui.fonts(|f| f.layout_no_wrap(label, prop(11.5), TEXT()));
    let rect = Rect::from_min_size(Pos2::new(right.x - g.size().x - 18.0, right.y - 11.0), Vec2::new(g.size().x + 18.0, 22.0));
    let resp = ui.interact(rect, ui.id().with("quality-chip"), Sense::click());
    let popup = egui::Id::new("quality-popup");
    let open = ui.memory(|m| m.is_popup_open(popup));
    ui.painter().rect_filled(rect, cr(11), if resp.hovered() || open { PANEL3() } else { PANEL2() });
    ui.painter().galley(rect.center() - g.size() / 2.0, g, TEXT());
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup));
    }
    let resp = resp.on_hover_text(tr("Preview quality — lower uses less CPU and GPU"));
    egui::popup::popup_above_or_below_widget(
        ui,
        popup,
        &resp,
        egui::AboveOrBelow::Below,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            ui.set_width(286.0);
            ui.label(egui::RichText::new(tr("Preview quality")).font(bold(13.0)).color(Color32::WHITE));
            ui.add_space(4.0);
            let descs = [
                "Original files at full resolution",
                "Light proxies for heavy video, up to 720p",
                "Fastest: proxies, up to 480p, lighter effects",
            ];
            for (i, (name, desc)) in QUALITY_NAMES.iter().zip(descs).enumerate() {
                let (r, rr) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 42.0), Sense::click());
                let on = i as u8 == q;
                if on || rr.hovered() {
                    ui.painter().rect_filled(r, cr(8), if on { ACCENT_BG() } else { PANEL3() });
                }
                ui.painter().text(
                    Pos2::new(r.left() + 16.0, r.center().y),
                    Align2::CENTER_CENTER,
                    if on { ph::CHECK } else { "" },
                    prop(13.0),
                    ACCENT(),
                );
                ui.painter().text(Pos2::new(r.left() + 32.0, r.top() + 13.0), Align2::LEFT_CENTER, tr(name), bold(12.5), Color32::WHITE);
                let dg = elided(ui, tr(desc), prop(10.5), TEXT2(), r.width() - 40.0);
                ui.painter().galley(Pos2::new(r.left() + 32.0, r.top() + 22.0), dg, TEXT2());
                if rr.clicked() {
                    app.set_preview_quality(i as u8);
                    ui.memory_mut(|m| m.close_popup());
                }
            }
            ui.add_space(2.0);
            // (a plain layout: menus justify wrapped text)
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                ui.label(egui::RichText::new(tr("Export always uses the original files at full quality.")).size(10.5).color(TEXT3()));
            });
        },
    );
    rect
}

/// The video frame, letterboxed into the available rect.
pub fn preview_area(app: &mut VeloApp, ui: &mut egui::Ui, fullscreen: bool) {
    let area = ui.available_rect_before_wrap().shrink(if fullscreen { 0.0 } else { 10.0 });
    let aspect = app.project.width as f32 / app.project.height.max(1) as f32;
    let (mut w, mut h) = (area.width(), area.width() / aspect);
    if h > area.height() {
        h = area.height();
        w = h * aspect;
    }
    let rect = Rect::from_center_size(area.center(), Vec2::new(w.max(8.0), h.max(8.0)));
    let ppp = ui.ctx().pixels_per_point();
    // lower preview qualities render fewer pixels (the frame is scaled up on screen)
    let (qscale, qmax) = quality_scale(app.settings.preview_quality);
    let max_h = (app.project.height as f32).min(qmax);
    let scale = ((rect.height() * ppp * qscale).min(max_h) / rect.height().max(1.0)).max(0.1);
    let (tw, th) = (((rect.width() * scale) as u32).max(16), ((rect.height() * scale) as u32).max(16));
    let resp = ui.allocate_rect(rect, Sense::click_and_drag());
    if app.project.duration() == 0 && !fullscreen {
        ui.painter().rect_filled(rect, cr(6), Color32::BLACK);
        ui.painter().text(rect.center() - Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, ph::FILM_SLATE, prop(26.0), TEXT3());
        ui.painter().text(
            rect.center() + Vec2::new(0.0, 18.0),
            Align2::CENTER_CENTER,
            tr("Import media and drag it to the timeline"),
            prop(12.0),
            TEXT3(),
        );
        return;
    }
    if let Some(id) = app.render_preview(tw, th) {
        ui.painter().image(id, rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
    }
    if fullscreen {
        if resp.clicked() {
            app.toggle_play();
        }
        let hint = format!("{}  {}", util::timecode(app.playhead, app.project.fps), tr("Esc to exit full screen"));
        ui.painter().text(
            rect.left_bottom() + Vec2::new(12.0, -12.0),
            Align2::LEFT_BOTTOM,
            hint,
            prop(12.0),
            Color32::from_white_alpha(120),
        );
        return;
    }
    gizmo(app, ui, rect, &resp);
}

fn to_screen(rect: Rect, w: f32, h: f32, p: (f64, f64)) -> Pos2 {
    Pos2::new(rect.left() + p.0 as f32 / w * rect.width(), rect.top() + p.1 as f32 / h * rect.height())
}

fn point_in_quad(p: Pos2, q: &[Pos2; 4]) -> bool {
    let mut sign = 0.0f32;
    for i in 0..4 {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let c = (b - a).x * (p - a).y - (b - a).y * (p - a).x;
        if c != 0.0 {
            if sign == 0.0 {
                sign = c.signum();
            } else if c.signum() != sign {
                return false;
            }
        }
    }
    true
}

fn gizmo(app: &mut VeloApp, ui: &mut egui::Ui, rect: Rect, resp: &egui::Response) {
    let t = app.playhead;
    let (pw, ph_) = (app.project.width as f32, app.project.height as f32);
    let quad_of = |app: &mut VeloApp, c: &Clip| -> Option<[Pos2; 4]> {
        let q = app.engine.clip_quad(&app.project, c, t, app.project.width, app.project.height)?;
        Some([
            to_screen(rect, pw, ph_, q[0]),
            to_screen(rect, pw, ph_, q[1]),
            to_screen(rect, pw, ph_, q[2]),
            to_screen(rect, pw, ph_, q[3]),
        ])
    };
    let drag_id = egui::Id::new("gizmo-drag");
    let mut drag: Option<GizmoDrag> = ui.memory(|m| m.data.get_temp(drag_id));
    // click to select what is under the pointer (topmost first)
    if resp.clicked() && drag.is_none() {
        if let Some(p) = resp.interact_pointer_pos() {
            let mut hit = None;
            let clips: Vec<Clip> = app
                .project
                .tracks
                .iter()
                .filter(|tr| tr.kind.is_visual_layer() && !tr.hidden && !tr.locked)
                .flat_map(|tr| tr.clips.iter().filter(|c| c.contains(t)).cloned())
                .collect();
            for c in clips {
                if let Some(q) = quad_of(app, &c) {
                    if point_in_quad(p, &q) {
                        hit = Some(c.id);
                        break;
                    }
                }
            }
            match hit {
                Some(id) => app.select_only(id),
                None => app.sel = super::Selection::default(),
            }
        }
    }
    if resp.double_clicked() {
        if let Some(id) = app.sel.one() {
            if matches!(app.project.clip(id).map(|c| &c.kind), Some(ClipKind::Text(_))) {
                app.right_tab = super::RightTab::Video;
                ui.memory_mut(|m| m.data.insert_temp(egui::Id::new("focus-text"), true));
            }
        }
    }
    let Some(id) = app.sel.one() else { return };
    let Some(c) = app.project.clip(id).cloned() else { return };
    if !c.is_visual() || !c.contains(t) {
        return;
    }
    if app.project.track_of(id).map(|t| t.locked).unwrap_or(true) {
        return;
    }
    let Some(q) = quad_of(app, &c) else { return };
    let painter = ui.painter_at(rect.expand(30.0));
    let col = Color32::WHITE;
    for i in 0..4 {
        painter.line_segment([q[i], q[(i + 1) % 4]], Stroke::new(1.2_f32, col));
    }
    let center = Pos2::new((q[0].x + q[2].x) * 0.5, (q[0].y + q[2].y) * 0.5);
    for p in q.iter() {
        painter.circle_filled(*p, 5.0, Color32::WHITE);
        painter.circle_stroke(*p, 5.0, Stroke::new(1.0_f32, Color32::from_black_alpha(120)));
    }
    // rotation handle below the bottom edge
    let bottom_mid = Pos2::new((q[2].x + q[3].x) * 0.5, (q[2].y + q[3].y) * 0.5);
    let dir = (bottom_mid - center).normalized();
    let rh = bottom_mid + dir * 22.0;
    painter.circle_filled(rh, 9.0, Color32::from_rgba_premultiplied(30, 30, 34, 230));
    painter.text(rh, Align2::CENTER_CENTER, ph::ARROWS_CLOCKWISE, prop(11.0), Color32::WHITE);
    // interactions. A drag may only *start* inside the player area: a clip moved or scaled
    // past the canvas still has its frame (and handles) out there, over the library,
    // inspector or timeline, and those panels share this layer, so pointer_free() alone
    // would let a drag in another panel grab the video.
    let zone = ui.max_rect().intersect(ui.clip_rect());
    let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|p| pointer_free(ui, *p));
    let press_origin = ui.input(|i| i.pointer.press_origin());
    let in_zone = pointer.is_some_and(|p| zone.contains(p)) && press_origin.is_none_or(|o| zone.contains(o));
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    let down = ui.input(|i| i.pointer.primary_down());
    let ctrl_free = ui.input(|i| i.modifiers.ctrl);
    if let Some(p) = pointer {
        let near_corner = q.iter().any(|c| c.distance(p) < 9.0);
        let on_rot = rh.distance(p) < 11.0;
        if drag.is_none() && in_zone {
            if on_rot {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Alias);
            } else if near_corner {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            } else if point_in_quad(p, &q) && rect.contains(p) {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Move);
            }
        }
        if pressed && drag.is_none() && in_zone {
            let kind = if on_rot {
                Some(GizmoKind::Rotate)
            } else if near_corner {
                Some(GizmoKind::Scale)
            } else if point_in_quad(p, &q) {
                Some(GizmoKind::Move)
            } else {
                None
            };
            if let Some(kind) = kind {
                drag = Some(GizmoDrag {
                    kind,
                    clip: id,
                    start: p,
                    center,
                    x: c.prop(Prop::PosX, t),
                    y: c.prop(Prop::PosY, t),
                    scale: c.prop(Prop::Scale, t),
                    rot: c.prop(Prop::Rotation, t),
                });
            }
        }
    }
    if let Some(d) = drag {
        if !down || d.clip != id {
            ui.memory_mut(|m| m.data.remove::<GizmoDrag>(drag_id));
            return;
        }
        if let Some(p) = pointer {
            let k = pw / rect.width();
            match d.kind {
                GizmoKind::Move => {
                    let delta = p - d.start;
                    let mut nx = d.x + (delta.x * k) as f64;
                    let mut ny = d.y - (delta.y * k) as f64;
                    // snap the centre to the canvas axes (hold Ctrl to move freely)
                    let snap = 8.0 * k as f64;
                    let mut guides = (false, false);
                    if !ctrl_free {
                        if nx.abs() < snap {
                            nx = 0.0;
                            guides.0 = true;
                        }
                        if ny.abs() < snap {
                            ny = 0.0;
                            guides.1 = true;
                        }
                    }
                    if guides.0 {
                        painter.line_segment(
                            [Pos2::new(rect.center().x, rect.top()), Pos2::new(rect.center().x, rect.bottom())],
                            Stroke::new(1.0_f32, ACCENT()),
                        );
                    }
                    if guides.1 {
                        painter.line_segment(
                            [Pos2::new(rect.left(), rect.center().y), Pos2::new(rect.right(), rect.center().y)],
                            Stroke::new(1.0_f32, ACCENT()),
                        );
                    }
                    app.edit(|pr| {
                        if let Some(c) = pr.clip_mut(id) {
                            c.set_prop_at(Prop::PosX, t, nx.round());
                            c.set_prop_at(Prop::PosY, t, ny.round());
                        }
                    });
                }
                GizmoKind::Scale => {
                    let d0 = d.start.distance(d.center).max(1.0);
                    let d1 = p.distance(d.center);
                    let ns = (d.scale * (d1 / d0) as f64).clamp(0.01, 20.0);
                    app.edit(|pr| {
                        if let Some(c) = pr.clip_mut(id) {
                            c.set_prop_at(Prop::Scale, t, (ns * 1000.0).round() / 1000.0);
                        }
                    });
                }
                GizmoKind::Rotate => {
                    let a0 = (d.start - d.center).angle();
                    let a1 = (p - d.center).angle();
                    let mut nr = d.rot + (a1 - a0).to_degrees() as f64;
                    if ui.input(|i| i.modifiers.shift) {
                        nr = (nr / 15.0).round() * 15.0;
                    }
                    app.edit(|pr| {
                        if let Some(c) = pr.clip_mut(id) {
                            c.set_prop_at(Prop::Rotation, t, (nr * 10.0).round() / 10.0);
                        }
                    });
                }
            }
        }
        ui.memory_mut(|m| m.data.insert_temp(drag_id, d));
    }
}

fn controls(app: &mut VeloApp, ui: &mut egui::Ui, r: Rect) {
    ui.painter().line_segment([Pos2::new(r.left() + 10.0, r.top()), Pos2::new(r.right() - 10.0, r.top())], Stroke::new(1.0_f32, BORDER()));
    let fps = app.project.fps;
    let cur = util::timecode(app.playhead, fps);
    let tot = util::timecode(app.project.duration(), fps);
    let font = egui::FontId::monospace(12.5);
    let g = ui.fonts(|f| f.layout_no_wrap(cur.clone(), font.clone(), ACCENT()));
    ui.painter().text(Pos2::new(r.left() + 14.0, r.center().y), Align2::LEFT_CENTER, &cur, font.clone(), ACCENT());
    ui.painter().text(Pos2::new(r.left() + 20.0 + g.size().x, r.center().y), Align2::LEFT_CENTER, format!("/ {}", tot), font, TEXT3());
    // play button
    let pb = Rect::from_center_size(r.center(), Vec2::splat(32.0));
    let resp = ui.interact(pb, ui.id().with("play"), Sense::click());
    ui.painter().circle_filled(pb.center(), 16.0, if resp.hovered() { PANEL3() } else { PANEL2() });
    let icon = if app.playing { ph::PAUSE } else { ph::PLAY };
    let nudge = if app.playing { 0.0 } else { 1.0 };
    ui.painter().text(pb.center() + Vec2::new(nudge, 0.0), Align2::CENTER_CENTER, icon, fill(16.0), Color32::WHITE);
    if resp.on_hover_text(tr("Play / Pause (Space)")).clicked() {
        app.toggle_play();
    }
    // step buttons
    for (dx, icon, tip, fwd) in [(-40.0, ph::SKIP_BACK, "Previous frame (←)", false), (40.0, ph::SKIP_FORWARD, "Next frame (→)", true)]
    {
        let br = Rect::from_center_size(r.center() + Vec2::new(dx, 0.0), Vec2::splat(26.0));
        let rr = ui.interact(br, ui.id().with(("step", fwd)), Sense::click());
        if rr.hovered() {
            ui.painter().rect_filled(br, cr(6), PANEL2());
        }
        ui.painter().text(br.center(), Align2::CENTER_CENTER, icon, fill(13.0), if rr.hovered() { Color32::WHITE } else { TEXT2() });
        if rr.on_hover_text(tr(tip)).clicked() {
            let fd = app.project.frame_dur();
            let t = app.playhead + if fwd { fd } else { -fd };
            app.seek(t);
        }
    }
    // right side: fullscreen, ratio, volume
    let small_btn = |ui: &mut egui::Ui, rect: Rect, id: &str, icon: &str, tip: &str| -> bool {
        let rr = ui.interact(rect, ui.id().with(id), Sense::click());
        if rr.hovered() {
            ui.painter().rect_filled(rect, cr(6), PANEL2());
        }
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, icon, prop(16.0), if rr.hovered() { Color32::WHITE } else { TEXT2() });
        rr.on_hover_text(tip).clicked()
    };
    let fs = Rect::from_center_size(Pos2::new(r.right() - 24.0, r.center().y), Vec2::splat(28.0));
    if small_btn(ui, fs, "fs", ph::CORNERS_OUT, tr("Full screen (Ctrl+Shift+F)")) {
        app.fullscreen = true;
    }
    let rb = Rect::from_min_size(Pos2::new(fs.left() - 86.0, r.center().y - 12.0), Vec2::new(78.0, 24.0));
    let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(rb));
    let cur_label = tr(app.project.ratio.label()).to_string();
    egui::ComboBox::from_id_salt("ratio").width(74.0).selected_text(egui::RichText::new(cur_label).size(12.0)).show_ui(&mut rui, |ui| {
        ui.set_min_width(170.0);
        for r in Ratio::ALL {
            let label = if r.hint().is_empty() { tr(r.label()).to_string() } else { format!("{}   {}", tr(r.label()), r.hint()) };
            if ui.selectable_label(app.project.ratio == r, label).clicked() {
                app.set_ratio(r);
            }
        }
    });
    let vr = Rect::from_center_size(Pos2::new(rb.left() - 20.0, r.center().y), Vec2::splat(28.0));
    let muted = app.player.shared.muted.load(std::sync::atomic::Ordering::Relaxed);
    if small_btn(ui, vr, "mute", if muted { ph::SPEAKER_X } else { ph::SPEAKER_HIGH }, tr("Mute preview")) {
        app.player.set_muted(!muted);
    }
    let _: Us = 0;
}
