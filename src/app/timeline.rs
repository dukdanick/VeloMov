//! Bottom panel: toolbar + multi-track timeline.

use super::i18n::tr;
use super::theme::*;
use super::widgets::*;
use super::{DragItem, LeftTab, Selection, VeloApp};
use crate::model::edit::Target as Place;
use crate::model::keyframe::KEY_EPS;
use crate::model::*;
use crate::util::{self, us_to_s, Us, SEC};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as ph;

const HEADER_W: f32 = 84.0;
const RULER_H: f32 = 26.0;
const TOOLBAR_H: f32 = 36.0;
const LEFT_PAD: f32 = 12.0;
const EDGE: f32 = 7.0;

#[derive(Clone, Debug)]
pub enum DragKind {
    Move { grab: Us, start_track: usize },
    TrimL,
    TrimR,
    Keyframe { from: Us },
    Scrub,
}

#[derive(Clone, Debug)]
pub struct TlDrag {
    pub kind: DragKind,
    pub clip: u64,
    pub press: Pos2,
    pub orig: Box<Project>,
    pub active: bool,
}

pub fn track_h(t: &Track) -> f32 {
    if t.main {
        64.0
    } else {
        match t.kind {
            TrackKind::Video => 50.0,
            TrackKind::Audio => 42.0,
            _ => 28.0,
        }
    }
}

struct Geo {
    content: Rect,
    pps: f32,
    scroll_x: f32,
}

impl Geo {
    fn x(&self, t: Us) -> f32 {
        self.content.left() + LEFT_PAD + us_to_s(t) as f32 * self.pps - self.scroll_x
    }
    fn t(&self, x: f32) -> Us {
        util::s_to_us(((x - self.content.left() - LEFT_PAD + self.scroll_x) / self.pps).max(0.0) as f64)
    }
}

pub fn follow_playhead(app: &mut VeloApp) {
    let pps = app.tl.pps;
    let x = us_to_s(app.playhead) as f32 * pps;
    let vis = app.tl_visible_w();
    if x - app.tl.scroll_x > vis * 0.88 || x < app.tl.scroll_x {
        app.tl.scroll_x = (x - vis * 0.12).max(0.0);
    }
}

impl VeloApp {
    pub fn tl_visible_w(&self) -> f32 {
        self.tl.width.max(200.0)
    }
}

fn snap(app: &VeloApp, geo: &Geo, t: Us, exclude: &[u64]) -> (Us, Option<Us>) {
    if !app.project.snapping {
        return (t, None);
    }
    let mut pts = app.project.snap_points(exclude);
    pts.push(app.playhead);
    let thr = util::s_to_us((8.0 / geo.pps) as f64);
    let mut best: Option<(Us, Us)> = None;
    for p in pts {
        let d = (p - t).abs();
        if d <= thr && best.map(|b| d < b.1).unwrap_or(true) {
            best = Some((p, d));
        }
    }
    match best {
        Some((p, _)) => (p, Some(p)),
        None => (t, None),
    }
}

pub fn ui(app: &mut VeloApp, ui: &mut egui::Ui) {
    let full = ui.available_rect_before_wrap();
    let bar = Rect::from_min_size(full.min, Vec2::new(full.width(), TOOLBAR_H));
    toolbar(app, ui, bar);
    let body = Rect::from_min_max(Pos2::new(full.left(), bar.bottom()), full.max);
    let header = Rect::from_min_size(body.min, Vec2::new(HEADER_W, body.height()));
    let content = Rect::from_min_max(Pos2::new(header.right(), body.top()), body.max);
    app.tl.width = content.width();
    let ruler = Rect::from_min_size(content.min, Vec2::new(content.width(), RULER_H));
    let tracks_area = Rect::from_min_max(Pos2::new(body.left(), ruler.bottom()), body.max);
    // fit-to-window on request
    let dur = app.project.duration().max(SEC * 10);
    if app.tl.fit_request {
        app.tl.pps = ((content.width() - LEFT_PAD - 60.0) / us_to_s(dur) as f32).clamp(2.0, 400.0);
        app.tl.scroll_x = 0.0;
        app.tl.fit_request = false;
    }
    let geo = Geo { content, pps: app.tl.pps, scroll_x: app.tl.scroll_x };
    let painter = ui.painter().clone();
    painter.line_segment([ruler.left_bottom(), ruler.right_bottom() - Vec2::new(8.0, 0.0)], Stroke::new(1.0_f32, BORDER()));
    // ---------------------------------------------------------------- layout of tracks
    let n = app.project.tracks.len();
    let gap = 6.0;
    let total: f32 = app.project.tracks.iter().map(|t| track_h(t) + gap).sum::<f32>() + 40.0;
    let avail = tracks_area.height();
    let max_scroll = (total - avail).max(0.0);
    app.tl.scroll_y = app.tl.scroll_y.clamp(0.0, max_scroll);
    let mut y = if total < avail {
        // keep the main track a bit below the centre
        tracks_area.top() + (avail - total) * 0.55 + 20.0
    } else {
        tracks_area.top() + 20.0 - app.tl.scroll_y
    };
    let mut rows: Vec<(usize, Rect)> = Vec::with_capacity(n);
    for (i, t) in app.project.tracks.iter().enumerate() {
        let h = track_h(t);
        rows.push((i, Rect::from_min_size(Pos2::new(content.left(), y), Vec2::new(content.width(), h))));
        y += h + gap;
    }
    app.tl.rows = rows.clone();
    app.tl.x0 = content.left() + LEFT_PAD - app.tl.scroll_x;
    app.tl.ruler_y = ruler.center().y;
    // ---------------------------------------------------------------- input
    let resp = ui.interact(Rect::from_min_max(content.min, body.max), ui.id().with("tl-content"), Sense::click_and_drag());
    let pointer = ui.input(|i| i.pointer.hover_pos());
    // scroll bars (right and bottom edges of the tracks): not part of the editing area
    let vbar =
        Rect::from_min_max(Pos2::new(content.right() - 15.0, ruler.bottom() + 4.0), Pos2::new(content.right() - 4.0, body.bottom() - 19.0));
    let hbar =
        Rect::from_min_max(Pos2::new(content.left() + 4.0, body.bottom() - 15.0), Pos2::new(content.right() - 19.0, body.bottom() - 4.0));
    let on_bars = |p: Pos2| bar_hit(vbar, true).contains(p) || bar_hit(hbar, false).contains(p);
    let hover_in = pointer.map(|p| content.contains(p) && p.y > ruler.bottom() && pointer_free(ui, p) && !on_bars(p)).unwrap_or(false);
    let mods = ui.input(|i| i.modifiers);
    // wheel
    if hover_in {
        let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
        if mods.ctrl || zoom != 1.0 {
            let factor = if zoom != 1.0 { zoom } else { (scroll.y / 200.0).exp() };
            if factor != 1.0 {
                if let Some(p) = pointer {
                    let t = geo.t(p.x);
                    let new_pps = (app.tl.pps * factor).clamp(1.0, 1500.0);
                    app.tl.scroll_x = (us_to_s(t) as f32 * new_pps - (p.x - content.left() - LEFT_PAD)).max(0.0);
                    app.tl.pps = new_pps;
                }
            }
        } else if mods.shift {
            app.tl.scroll_y = (app.tl.scroll_y - scroll.y - scroll.x).clamp(0.0, max_scroll);
        } else {
            app.tl.scroll_x = (app.tl.scroll_x - scroll.y - scroll.x).max(0.0);
        }
    }
    let geo = Geo { pps: app.tl.pps, scroll_x: app.tl.scroll_x, ..geo };
    app.tl.hover_time = if hover_in || app.drag.is_some() { pointer.map(|p| geo.t(p.x)) } else { None };
    app.preview_axis_time =
        if app.project.preview_axis && hover_in && app.tl.drag.is_none() && !app.playing { pointer.map(|p| geo.t(p.x)) } else { None };
    if app.preview_axis_time.is_some() {
        ui.ctx().request_repaint();
    }

    // ---------------------------------------------------------------- ruler
    draw_ruler(app, ui, &geo, ruler);
    let ruler_resp = ui.interact(ruler, ui.id().with("ruler"), Sense::click_and_drag());
    if ruler_resp.drag_started() || ruler_resp.clicked() {
        if app.playing {
            app.pause();
        }
    }
    if ruler_resp.dragged() || ruler_resp.clicked() {
        if let Some(p) = ruler_resp.interact_pointer_pos() {
            let t = util::snap_frame(geo.t(p.x), app.project.fps);
            app.seek(t);
        }
    }

    // ---------------------------------------------------------------- tracks
    let clip_painter = painter.with_clip_rect(Rect::from_min_max(Pos2::new(content.left(), ruler.bottom()), content.max));
    for (i, row) in &rows {
        let trk = &app.project.tracks[*i];
        let bgc = if trk.main { TRACK_MAIN() } else { TRACK_BG() };
        clip_painter.rect_filled(*row, cr(6), bgc);
    }
    // header column
    let hp = painter.with_clip_rect(Rect::from_min_max(Pos2::new(header.left(), ruler.bottom()), header.max));
    for (i, row) in &rows {
        let hr = Rect::from_min_max(Pos2::new(header.left() + 6.0, row.top()), Pos2::new(header.right() - 4.0, row.bottom()));
        if hr.bottom() < ruler.bottom() || hr.top() > body.bottom() {
            continue;
        }
        let trk = app.project.tracks[*i].clone();
        hp.rect_filled(hr, cr(6), PANEL2());
        let icon = match trk.kind {
            TrackKind::Video => {
                if trk.main {
                    ph::FILM_STRIP
                } else {
                    ph::SQUARES_FOUR
                }
            }
            TrackKind::Audio => ph::MUSIC_NOTES,
            TrackKind::Text => ph::TEXT_T,
            TrackKind::Sticker => ph::STICKER,
            TrackKind::Effect => ph::SPARKLE,
            TrackKind::Filter => ph::CIRCLE_HALF,
            TrackKind::Adjust => ph::SLIDERS_HORIZONTAL,
        };
        hp.text(Pos2::new(hr.left() + 11.0, hr.center().y), Align2::CENTER_CENTER, icon, prop(12.0), TEXT3());
        // buttons: lock, eye, mute
        let mut bx = hr.left() + 21.0;
        let mut btn = |ui: &mut egui::Ui, icon: &str, on: bool, key: &str, tip: &str| -> bool {
            let r = Rect::from_center_size(Pos2::new(bx + 9.0, hr.center().y), Vec2::splat(18.0));
            bx += 19.0;
            if r.top() < ruler.bottom() {
                return false;
            }
            let rr = ui.interact(r, ui.id().with(("trk", trk.id, key)), Sense::click());
            let col = if on {
                ACCENT()
            } else if rr.hovered() {
                Color32::WHITE
            } else {
                TEXT2()
            };
            hp.text(r.center(), Align2::CENTER_CENTER, icon, prop(12.5), col);
            rr.on_hover_text(tip).clicked()
        };
        if btn(ui, if trk.locked { ph::LOCK_SIMPLE } else { ph::LOCK_SIMPLE_OPEN }, trk.locked, "lock", tr("Lock track")) {
            let id = trk.id;
            app.edit(|p| {
                if let Some(t) = p.tracks.iter_mut().find(|t| t.id == id) {
                    t.locked = !t.locked;
                }
            });
        }
        if trk.kind != TrackKind::Audio && btn(ui, if trk.hidden { ph::EYE_SLASH } else { ph::EYE }, trk.hidden, "eye", tr("Hide track")) {
            let id = trk.id;
            app.edit(|p| {
                if let Some(t) = p.tracks.iter_mut().find(|t| t.id == id) {
                    t.hidden = !t.hidden;
                }
            });
        }
        if matches!(trk.kind, TrackKind::Video | TrackKind::Audio)
            && btn(ui, if trk.muted { ph::SPEAKER_SLASH } else { ph::SPEAKER_HIGH }, trk.muted, "mute", tr("Mute track"))
        {
            let id = trk.id;
            app.edit(|p| {
                if let Some(t) = p.tracks.iter_mut().find(|t| t.id == id) {
                    t.muted = !t.muted;
                }
            });
        }
        if trk.main && hr.height() > 40.0 {
            hp.text(Pos2::new(hr.left() + 12.0, hr.bottom() - 9.0), Align2::LEFT_CENTER, tr("Main"), prop(9.5), TEXT3());
        }
    }

    // clips
    let sel = app.sel.clone();
    let t_vis0 = geo.t(content.left());
    let t_vis1 = geo.t(content.right());
    let mut hovered_clip: Option<(u64, usize, Rect)> = None;
    for (i, row) in &rows {
        if row.bottom() < ruler.bottom() || row.top() > body.bottom() {
            continue;
        }
        let trk = app.project.tracks[*i].clone();
        for (ci, c) in trk.clips.iter().enumerate() {
            if c.end() < t_vis0 || c.start > t_vis1 {
                continue;
            }
            let x0 = geo.x(c.start);
            let x1 = geo.x(c.end());
            let r = Rect::from_min_max(Pos2::new(x0, row.top() + 1.0), Pos2::new(x1.max(x0 + 2.0), row.bottom() - 1.0));
            // keep the label clear of an incoming transition overlap
            let label_dx = match ci.checked_sub(1).map(|j| &trk.clips[j]) {
                Some(prev) if trk.main => prev.transition.as_ref().map(|t| t.dur as f32 / 2e6 * geo.pps).unwrap_or(0.0),
                _ => 0.0,
            };
            draw_clip(app, ui, &clip_painter, &trk, c, r, sel.clips.contains(&c.id), &geo, label_dx);
            if let Some(p) = pointer {
                if r.contains(p) && content.contains(p) {
                    hovered_clip = Some((c.id, *i, r));
                }
            }
        }
        if trk.locked {
            clip_painter.rect_filled(*row, cr(6), Color32::from_black_alpha(90));
        }
    }
    // transitions / junctions on the main track (a transition is centred on the cut)
    let mi = app.project.main_track_index();
    let main_row = rows.iter().find(|r| r.0 == mi).map(|r| r.1);
    let mut junction_hit: Option<u64> = None;
    if let Some(row) = main_row {
        let clips = app.project.tracks[mi].clips.clone();
        for w in clips.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            let has = a.transition.is_some();
            let cut = geo.x(a.end());
            let half = a.transition.as_ref().map(|t| t.dur as f32 / 2e6 * geo.pps).unwrap_or(0.0);
            let (xa, xb) = (cut - half, cut + half);
            let cx = cut;
            let selected = sel.transition == Some(a.id);
            let _ = b;
            if has {
                let tr_rect = Rect::from_min_max(Pos2::new(xa, row.top() + 1.0), Pos2::new(xb.max(xa + 4.0), row.bottom() - 1.0));
                clip_painter.rect_filled(tr_rect, cr(6), Color32::from_black_alpha(95));
                // diagonal hatching marks the transition area
                let hp = clip_painter.with_clip_rect(tr_rect.shrink(1.0).intersect(clip_painter.clip_rect()));
                let mut x = tr_rect.left() - tr_rect.height();
                while x < tr_rect.right() {
                    hp.line_segment(
                        [Pos2::new(x, tr_rect.bottom()), Pos2::new(x + tr_rect.height(), tr_rect.top())],
                        Stroke::new(1.0_f32, Color32::from_white_alpha(22)),
                    );
                    x += 6.0;
                }
                clip_painter.rect_stroke(
                    tr_rect,
                    cr(6),
                    Stroke::new(if selected { 2.0_f32 } else { 1.0_f32 }, if selected { ACCENT() } else { Color32::from_white_alpha(70) }),
                    StrokeKind::Inside,
                );
                if tr_rect.width() > 52.0 {
                    let name = crate::model::catalog::transition(&a.transition.as_ref().unwrap().id).map(|d| tr(d.name)).unwrap_or("");
                    let g = elided(ui, name, prop(9.5), Color32::WHITE, tr_rect.width() - 8.0);
                    clip_painter.galley(
                        Pos2::new(tr_rect.center().x - g.size().x / 2.0, tr_rect.bottom() - g.size().y - 4.0),
                        g,
                        Color32::WHITE,
                    );
                }
            }
            let jr = Rect::from_center_size(
                Pos2::new(cx, row.center().y - if has && (xb - xa) > 52.0 { 7.0 } else { 0.0 }),
                Vec2::new(18.0, 16.0),
            );
            let hov = pointer.map(|p| jr.expand(2.0).contains(p)).unwrap_or(false);
            if has || hov || app.junction == Some(a.id) || matches!(app.drag, Some(DragItem::Transition(_))) {
                let armed = selected || app.junction == Some(a.id);
                let fillc = if armed {
                    ACCENT()
                } else if hov {
                    Color32::WHITE
                } else if has {
                    ACCENT_DARK()
                } else {
                    Color32::from_rgba_premultiplied(40, 40, 46, 235)
                };
                clip_painter.rect_filled(jr, cr(6), fillc);
                let ink = if armed {
                    ON_ACCENT()
                } else if hov {
                    Color32::BLACK
                } else {
                    Color32::WHITE
                };
                clip_painter.text(jr.center(), Align2::CENTER_CENTER, if has { ph::SWAP } else { ph::PLUS }, prop(12.0), ink);
            }
            if hov {
                junction_hit = Some(a.id);
            }
        }
    }

    // markers
    for m in app.project.markers.clone() {
        let x = geo.x(m);
        if x >= content.left() && x <= content.right() {
            painter.add(Shape::convex_polygon(
                vec![Pos2::new(x - 5.0, ruler.top() + 2.0), Pos2::new(x + 5.0, ruler.top() + 2.0), Pos2::new(x, ruler.top() + 10.0)],
                Color32::from_rgb(255, 196, 0),
                Stroke::NONE,
            ));
        }
    }
    // in/out marks
    if let (Some(i), Some(o)) = app.tl.in_out {
        let r = Rect::from_min_max(Pos2::new(geo.x(i), ruler.bottom() - 4.0), Pos2::new(geo.x(o), ruler.bottom()));
        painter.rect_filled(r, cr(0), ACCENT());
    }

    // ---------------------------------------------------------------- clip mouse interaction
    let press = ui.input(|i| i.pointer.primary_pressed());
    let released = ui.input(|i| i.pointer.primary_released());
    if press && hover_in && app.tl.drag.is_none() && app.drag.is_none() {
        if let Some(p) = pointer {
            if let Some(jid) = junction_hit {
                // select the transition or arm the junction
                let has = app.project.clip(jid).map(|c| c.transition.is_some()).unwrap_or(false);
                if has {
                    app.sel = Selection { clips: vec![], transition: Some(jid) };
                } else {
                    app.junction = Some(jid);
                    app.left_tab = LeftTab::Transitions;
                    app.toast(tr("Pick a transition in the Transitions tab"));
                }
            } else if let Some((cid, ti, r)) = hovered_clip {
                let locked = app.project.tracks[ti].locked;
                if app.tl.split_mode && !locked {
                    let t = util::snap_frame(geo.t(p.x), app.project.fps);
                    app.edit(|pr| {
                        pr.split_clip(cid, t);
                    });
                } else if !locked {
                    // keyframe diamond?
                    let mut kf_from = None;
                    if sel.clips.contains(&cid) {
                        if let Some(c) = app.project.clip(cid) {
                            for k in c.kf.times() {
                                let kx = geo.x(c.start + k);
                                if (p.x - kx).abs() <= 6.0 && (p.y - (r.bottom() - 8.0)).abs() <= 7.0 {
                                    kf_from = Some(k);
                                }
                            }
                        }
                    }
                    // selection: Shift = range from the last clicked clip, Ctrl = add/remove one
                    let anchor = app.tl.anchor.filter(|a| app.project.clip(*a).is_some() && *a != cid);
                    let mut reselect_on_release = false;
                    if mods.shift && anchor.is_some() {
                        let range = range_selection(&app.project, anchor.unwrap(), cid);
                        for id in range {
                            if !app.sel.clips.contains(&id) {
                                app.sel.clips.push(id);
                            }
                        }
                        // the clicked clip is the one shown in the inspector
                        app.sel.clips.retain(|x| *x != cid);
                        app.sel.clips.push(cid);
                        app.sel.transition = None;
                    } else if mods.ctrl || mods.shift {
                        if let Some(pos) = app.sel.clips.iter().position(|x| *x == cid) {
                            app.sel.clips.remove(pos);
                        } else {
                            app.sel.clips.push(cid);
                        }
                        app.sel.transition = None;
                        app.tl.anchor = Some(cid);
                    } else if !app.sel.clips.contains(&cid) {
                        let members = app.project.group_members(cid);
                        app.sel = Selection { clips: members, transition: None };
                        app.on_selection_changed();
                        app.tl.anchor = Some(cid);
                    } else {
                        // a click (not a drag) inside a multiple selection selects just this clip
                        reselect_on_release = app.sel.clips.len() > app.project.group_members(cid).len();
                        app.tl.anchor = Some(cid);
                        // keep it last: it is the one shown in the inspector
                        app.sel.clips.retain(|x| *x != cid);
                        app.sel.clips.push(cid);
                    }
                    app.tl.reselect = if reselect_on_release { Some(cid) } else { None };
                    app.junction = None;
                    let c = app.project.clip(cid).cloned().unwrap();
                    let kind = if let Some(k) = kf_from {
                        DragKind::Keyframe { from: k }
                    } else if p.x - r.left() <= EDGE && r.width() > 3.0 * EDGE {
                        DragKind::TrimL
                    } else if r.right() - p.x <= EDGE && r.width() > 3.0 * EDGE {
                        DragKind::TrimR
                    } else {
                        DragKind::Move { grab: geo.t(p.x) - c.start, start_track: ti }
                    };
                    app.flush_edit();
                    app.tl.drag = Some(TlDrag { kind, clip: cid, press: p, orig: Box::new(app.project.clone()), active: false });
                }
            } else {
                // empty space: deselect + move playhead (scrub while dragging)
                app.sel = Selection::default();
                app.junction = None;
                if app.playing {
                    app.pause();
                }
                let t = util::snap_frame(geo.t(p.x), app.project.fps);
                app.seek(t);
                app.tl.drag = Some(TlDrag { kind: DragKind::Scrub, clip: 0, press: p, orig: Box::new(Project::default()), active: true });
            }
        }
    }
    // hover cursors
    if app.tl.drag.is_none() && app.drag.is_none() {
        if let (Some(p), Some((_, _, r))) = (pointer, hovered_clip) {
            if app.tl.split_mode {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                clip_painter
                    .line_segment([Pos2::new(p.x, r.top() - 2.0), Pos2::new(p.x, r.bottom() + 2.0)], Stroke::new(1.5_f32, ACCENT()));
            } else if (p.x - r.left() <= EDGE || r.right() - p.x <= EDGE) && r.width() > 3.0 * EDGE {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
    }
    // drag in progress
    if let Some(mut d) = app.tl.drag.take() {
        let down = ui.input(|i| i.pointer.primary_down());
        if let Some(p) = pointer {
            if !d.active && p.distance(d.press) > 4.0 {
                d.active = true;
            }
            if d.active {
                apply_drag(app, ui, &geo, &rows, &d, p, &clip_painter);
            }
        }
        if down {
            app.tl.drag = Some(d);
        } else {
            app.tl.snap_line = None;
            if let Some(cid) = app.tl.reselect.take() {
                if !d.active && cid == d.clip {
                    app.sel = Selection { clips: app.project.group_members(cid), transition: None };
                    app.on_selection_changed();
                }
            }
        }
    }
    // auto-scroll while dragging near the edges
    if app.tl.drag.as_ref().map(|d| d.active).unwrap_or(false) || app.drag.is_some() {
        if let Some(p) = pointer {
            if p.x > content.right() - 30.0 && p.x < content.right() + 40.0 {
                app.tl.scroll_x += 8.0;
                ui.ctx().request_repaint();
            } else if p.x < content.left() + 20.0 && p.x > content.left() - 40.0 && app.tl.scroll_x > 0.0 {
                app.tl.scroll_x = (app.tl.scroll_x - 8.0).max(0.0);
                ui.ctx().request_repaint();
            }
        }
    }
    // library drag & drop onto the timeline
    if let Some(item) = app.drag.clone() {
        if let Some(p) = pointer {
            if Rect::from_min_max(content.min, body.max).contains(p) {
                let t = util::snap_frame(geo.t(p.x), app.project.fps);
                let over = rows.iter().find(|r| r.1.contains(p)).map(|r| r.0);
                // drop indicator
                let ind_x = match (&item, over) {
                    (DragItem::Media(_), Some(ti)) if app.project.tracks[ti].main && app.project.magnet => {
                        let idx = app.project.main_insert_index(t);
                        geo.x(app.project.main_insert_time(idx))
                    }
                    _ => geo.x(t),
                };
                painter.line_segment([Pos2::new(ind_x, ruler.bottom()), Pos2::new(ind_x, body.bottom())], Stroke::new(2.0_f32, ACCENT()));
                if let Some(ti) = over {
                    if let Some((_, r)) = rows.iter().find(|r| r.0 == ti) {
                        clip_painter.rect_stroke(*r, cr(6), Stroke::new(1.0_f32, ACCENT()), StrokeKind::Inside);
                    }
                }
                if released {
                    drop_item(app, item, t, over);
                    app.drag = None;
                }
            }
        }
    }
    // context menu on clips
    if let Some((cid, _, _)) = hovered_clip {
        if resp.secondary_clicked() {
            if !app.sel.clips.contains(&cid) {
                app.select_only(cid);
            }
        }
    }
    resp.context_menu(|ui| clip_menu(app, ui));

    // ---------------------------------------------------------------- playhead + snap line
    let px = geo.x(app.playhead);
    if px >= content.left() - 1.0 && px <= content.right() + 1.0 {
        painter.line_segment([Pos2::new(px, ruler.top() + 6.0), Pos2::new(px, body.bottom() - 6.0)], Stroke::new(1.5_f32, Color32::WHITE));
        let head = Rect::from_center_size(Pos2::new(px, ruler.top() + 8.0), Vec2::new(12.0, 11.0));
        painter.rect_filled(head, cr(3), Color32::WHITE);
        painter.add(Shape::convex_polygon(
            vec![
                Pos2::new(head.left() + 1.0, head.bottom() - 1.0),
                Pos2::new(head.right() - 1.0, head.bottom() - 1.0),
                Pos2::new(px, head.bottom() + 4.0),
            ],
            Color32::WHITE,
            Stroke::NONE,
        ));
    }
    if let Some(t) = app.preview_axis_time {
        let x = geo.x(t);
        painter.line_segment(
            [Pos2::new(x, ruler.bottom()), Pos2::new(x, body.bottom())],
            Stroke::new(1.0_f32, Color32::from_rgb(255, 196, 0)),
        );
    }
    if let Some(s) = app.tl.snap_line {
        let x = geo.x(s);
        painter.line_segment(
            [Pos2::new(x, ruler.bottom()), Pos2::new(x, body.bottom())],
            Stroke::new(1.0_f32, Color32::from_rgb(255, 214, 0)),
        );
    }
    // empty timeline hint
    if app.project.tracks.iter().all(|t| t.clips.is_empty()) {
        if let Some(row) = main_row {
            clip_painter.rect_stroke(row.shrink(2.0), cr(8), Stroke::new(1.0_f32, Color32::from_white_alpha(40)), StrokeKind::Inside);
            clip_painter.text(
                row.center(),
                Align2::CENTER_CENTER,
                format!("{}  {}", ph::FILM_STRIP, tr("Drag media here to start creating")),
                prop(12.5),
                TEXT3(),
            );
        }
    }
    scrollbars(app, ui, vbar, hbar, total, avail, content.width());
    // separator line under the toolbar
    painter.line_segment(
        [Pos2::new(full.left() + 10.0, bar.bottom()), Pos2::new(full.right() - 10.0, bar.bottom())],
        Stroke::new(1.0_f32, BORDER()),
    );
    ui.allocate_rect(full, Sense::hover());
}

fn draw_ruler(app: &VeloApp, ui: &egui::Ui, geo: &Geo, ruler: Rect) {
    let p = ui.painter().with_clip_rect(ruler);
    let fps = app.project.fps.max(1.0);
    let steps = [1.0 / fps, 2.0 / fps, 5.0 / fps, 10.0 / fps, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0];
    let major = steps.iter().copied().find(|s| (*s as f32) * geo.pps >= 90.0).unwrap_or(600.0);
    let minor = major / 5.0;
    let t0 = us_to_s(geo.t(ruler.left()));
    let t1 = us_to_s(geo.t(ruler.right()));
    let mut k = (t0 / minor).floor() as i64;
    loop {
        let ts = k as f64 * minor;
        if ts > t1 + minor {
            break;
        }
        if ts >= 0.0 {
            let x = geo.x(util::s_to_us(ts));
            let is_major = ((ts / major).round() * major - ts).abs() < 1e-6;
            let h = if is_major { 9.0 } else { 4.0 };
            p.line_segment(
                [Pos2::new(x, ruler.bottom() - h), Pos2::new(x, ruler.bottom())],
                Stroke::new(1.0_f32, if is_major { TEXT3() } else { Color32::from_gray(70) }),
            );
            if is_major {
                let label = if major < 1.0 && (ts - ts.round()).abs() > 1e-6 {
                    format!("{}f", ((ts - ts.floor()) * fps).round() as i64)
                } else {
                    util::ruler_label(util::s_to_us(ts))
                };
                p.text(Pos2::new(x + 3.0, ruler.top() + 8.0), Align2::LEFT_CENTER, label, prop(10.0), TEXT3());
            }
        }
        k += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_clip(
    app: &mut VeloApp,
    ui: &egui::Ui,
    p: &egui::Painter,
    trk: &Track,
    c: &Clip,
    r: Rect,
    selected: bool,
    geo: &Geo,
    label_dx: f32,
) {
    let media = c.media_id().and_then(|m| app.project.media(m)).cloned();
    let is_audio_track = trk.kind == TrackKind::Audio;
    let base = clip_color(trk.kind, is_audio_track);
    let alpha = if c.disabled { 0.35 } else { 1.0 };
    p.rect_filled(r, cr(6), base.gamma_multiply(alpha));
    let inner = r.shrink(1.0);
    let cp = p.with_clip_rect(inner.intersect(p.clip_rect()));
    // room for the trim handle of a selected clip
    let pad = if selected && r.width() > 22.0 { 7.0 } else { 0.0 };

    match (&c.kind, media.as_ref()) {
        (ClipKind::Media { .. }, Some(m)) if m.kind != MediaKind::Audio && !is_audio_track => {
            // filmstrip
            let aspect = m.width as f32 / m.height.max(1) as f32;
            let th = inner.height();
            let tw = (th * aspect).clamp(20.0, 200.0);
            let count = crate::media::thumbs::Thumbs::count_for(us_to_s(m.duration));
            let mut x = inner.left();
            // align thumbnails to a grid so they don't swim while scrolling
            let vis_left = cp.clip_rect().left();
            if x < vis_left - tw {
                x += ((vis_left - x) / tw).floor() * tw;
            }
            while x < inner.right() && x < cp.clip_rect().right() + tw {
                let slot = Rect::from_min_size(Pos2::new(x, inner.top()), Vec2::new(tw, th));
                // the last picture is cut at the clip's end; the ends get rounded corners
                let vis = Rect::from_min_max(slot.min, Pos2::new(slot.right().min(inner.right()), slot.bottom()));
                let u1 = (vis.width() / tw).clamp(0.0, 1.0);
                let (first, last) = (slot.left() <= inner.left() + 0.5, slot.right() >= inner.right() - 0.5);
                let (a, b) = (if first { 5 } else { 0 }, if last { 5 } else { 0 });
                let rad = egui::CornerRadius { nw: a, sw: a, ne: b, se: b };
                let tc = geo.t(slot.center().x).clamp(c.start, c.end() - 1);
                let tex = if m.kind == MediaKind::Image {
                    app.thumb_tex.get(&(m.id, u32::MAX))
                } else {
                    let st = us_to_s(c.source_time(tc, m.duration));
                    let idx = ((st / us_to_s(m.duration).max(0.001)) * count as f64).floor().clamp(0.0, (count - 1) as f64) as u32;
                    app.thumb_tex.get(&(m.id, idx)).or_else(|| app.thumb_tex.get(&(m.id, u32::MAX)))
                };
                match tex {
                    Some(t) => {
                        let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(u1, 1.0));
                        cp.add(egui::epaint::RectShape::filled(vis, rad, Color32::WHITE.gamma_multiply(alpha)).with_texture(t.id(), uv));
                    }
                    None => {
                        cp.rect_filled(vis.shrink(1.0), rad, Color32::from_rgb(0x22, 0x3a, 0x40));
                    }
                }
                x += tw;
            }
            // embedded audio waveform strip
            if c.has_audio(&app.project) && trk.kind == TrackKind::Video && inner.height() > 40.0 {
                let strip = Rect::from_min_max(Pos2::new(inner.left(), inner.bottom() - 14.0), inner.max);
                cp.rect_filled(strip, egui::CornerRadius { nw: 0, ne: 0, sw: 5, se: 5 }, Color32::from_black_alpha(150));
                draw_wave(app, &cp, c, m, strip, geo, Color32::from_rgb(0x6f, 0xc9, 0xb0));
            }
            // label
            let name = c.display_name(&app.project);
            let mut label = name;
            if (c.speed - 1.0).abs() > 1e-3 || c.curve.is_some() {
                label = format!(
                    "{}  {}",
                    if c.curve.is_some() { format!("{} {}", ph::SPEEDOMETER, tr("Curve")) } else { format!("{:.2}x", c.speed) },
                    label
                );
            }
            if c.reverse {
                label = format!("{} {}", ph::ARROW_COUNTER_CLOCKWISE, label);
            }
            if c.freeze.is_some() {
                label = format!("{} {}", ph::SNOWFLAKE, label);
            }
            label = format!("{}  {}", label, util::short_dur(c.duration));
            let label_dx = label_dx.max(pad);
            let g = elided(ui, &label, prop(10.5), Color32::WHITE, inner.width() - 10.0 - label_dx);
            let lr = Rect::from_min_size(inner.min + Vec2::new(3.0 + label_dx, 3.0), g.size() + Vec2::new(8.0, 2.0));
            if inner.width() - label_dx > 24.0 {
                cp.rect_filled(lr, cr(4), Color32::from_black_alpha(150));
                cp.galley(lr.min + Vec2::new(4.0, 1.0), g, Color32::WHITE);
            }
        }
        (ClipKind::Media { .. }, Some(m)) => {
            // audio clip: waveform
            let wr = Rect::from_min_max(Pos2::new(inner.left(), inner.top() + 14.0), inner.max);
            draw_wave(app, &cp, c, m, wr, geo, WAVE);
            let g = elided(
                ui,
                &format!("{}  {}", c.display_name(&app.project), util::short_dur(c.duration)),
                prop(10.5),
                Color32::WHITE,
                inner.width() - 8.0 - pad,
            );
            cp.galley(inner.min + Vec2::new(5.0 + pad, 1.0), g, Color32::WHITE);
            // fades
            let fi = c.fade_in as f32 / c.duration.max(1) as f32 * inner.width();
            let fo = c.fade_out as f32 / c.duration.max(1) as f32 * inner.width();
            if fi > 1.0 {
                cp.add(Shape::convex_polygon(
                    vec![inner.left_top(), Pos2::new(inner.left() + fi, inner.top()), inner.left_bottom()],
                    Color32::from_black_alpha(110),
                    Stroke::NONE,
                ));
            }
            if fo > 1.0 {
                cp.add(Shape::convex_polygon(
                    vec![Pos2::new(inner.right() - fo, inner.top()), inner.right_top(), inner.right_bottom()],
                    Color32::from_black_alpha(110),
                    Stroke::NONE,
                ));
            }
        }
        _ => {
            let icon = match &c.kind {
                ClipKind::Text(_) => ph::TEXT_T,
                ClipKind::Sticker(_) => ph::STICKER,
                ClipKind::Effect(_) => ph::SPARKLE,
                ClipKind::Filter(_) => ph::CIRCLE_HALF,
                ClipKind::Adjust => ph::SLIDERS_HORIZONTAL,
                _ => ph::QUESTION,
            };
            let name = c.display_name(&app.project);
            let label = format!("{}  {}", icon, tr(&name));
            let g = elided(ui, &label, prop(11.0), Color32::WHITE, inner.width() - 8.0 - pad);
            cp.galley(Pos2::new(inner.left() + 5.0 + pad, inner.center().y - g.size().y / 2.0), g, Color32::WHITE);
        }
    }
    // animation markers (in/out/combo) as subtle bars at the top
    let dur = c.duration.max(1) as f32;
    if let Some(a) = &c.anim_in {
        let w = a.dur.min(c.duration) as f32 / dur * inner.width();
        cp.rect_filled(Rect::from_min_size(inner.left_top(), Vec2::new(w, 2.5)), cr(0), Color32::from_rgb(120, 255, 170));
    }
    if let Some(a) = &c.anim_out {
        let w = a.dur.min(c.duration) as f32 / dur * inner.width();
        cp.rect_filled(
            Rect::from_min_size(Pos2::new(inner.right() - w, inner.top()), Vec2::new(w, 2.5)),
            cr(0),
            Color32::from_rgb(255, 150, 120),
        );
    }
    if c.anim_combo.is_some() {
        cp.rect_filled(Rect::from_min_size(inner.left_top(), Vec2::new(inner.width(), 1.5)), cr(0), Color32::from_rgb(255, 220, 90));
    }
    if c.disabled {
        cp.line_segment([inner.left_bottom(), inner.right_top()], Stroke::new(1.0_f32, Color32::from_white_alpha(50)));
    }
    if c.group != 0 {
        cp.text(
            Pos2::new(inner.right() - 8.0, inner.top() + 8.0),
            Align2::CENTER_CENTER,
            ph::LINK,
            prop(9.0),
            Color32::from_white_alpha(170),
        );
    }
    // keyframes
    if selected && !c.kf.is_empty() {
        let t_local = app.playhead - c.start;
        for k in c.kf.times() {
            let x = geo.x(c.start + k);
            let cy = r.bottom() - 8.0;
            let on = (k - t_local).abs() <= KEY_EPS;
            let pts = vec![Pos2::new(x, cy - 5.0), Pos2::new(x + 5.0, cy), Pos2::new(x, cy + 5.0), Pos2::new(x - 5.0, cy)];
            p.add(Shape::convex_polygon(pts, if on { ACCENT() } else { KEYFRAME }, Stroke::new(1.0_f32, Color32::BLACK)));
        }
    }
    // border
    if selected {
        p.rect_stroke(r, cr(6), Stroke::new(2.0_f32, Color32::WHITE), StrokeKind::Inside);
        // trim handles (white tabs with a grip)
        if r.width() > 22.0 {
            let hw = 7.0;
            let left = Rect::from_min_max(r.left_top(), Pos2::new(r.left() + hw, r.bottom()));
            let right = Rect::from_min_max(Pos2::new(r.right() - hw, r.top()), r.right_bottom());
            p.rect_filled(left, egui::CornerRadius { nw: 6, sw: 6, ne: 0, se: 0 }, Color32::WHITE);
            p.rect_filled(right, egui::CornerRadius { nw: 0, sw: 0, ne: 6, se: 6 }, Color32::WHITE);
            for h in [left, right] {
                let c = h.center();
                p.line_segment([Pos2::new(c.x, c.y - 6.0), Pos2::new(c.x, c.y + 6.0)], Stroke::new(1.5_f32, Color32::from_gray(60)));
            }
        }
    } else {
        p.rect_stroke(r, cr(6), Stroke::new(1.0_f32, Color32::from_black_alpha(120)), StrokeKind::Inside);
    }
}

fn draw_wave(app: &mut VeloApp, p: &egui::Painter, c: &Clip, m: &MediaAsset, r: Rect, geo: &Geo, col: Color32) {
    let Some(pcm) = app.audio.base(&m.path) else {
        p.line_segment(
            [Pos2::new(r.left(), r.center().y), Pos2::new(r.right(), r.center().y)],
            Stroke::new(1.0_f32, col.gamma_multiply(0.4)),
        );
        return;
    };
    let peaks = &pcm.peaks;
    if peaks.is_empty() {
        return;
    }
    let vis = r.intersect(p.clip_rect());
    if vis.width() <= 0.0 {
        return;
    }
    let gain = (c.volume as f32).clamp(0.0, 3.0);
    let step = 2.0;
    let mut x = vis.left();
    while x < vis.right() {
        let ta = geo.t(x).clamp(c.start, c.end());
        let tb = geo.t(x + step).clamp(c.start, c.end());
        let sa = c.source_time(ta, m.duration);
        let sb = c.source_time(tb, m.duration);
        let (lo, hi) = if sa <= sb { (sa, sb) } else { (sb, sa) };
        let ia = (us_to_s(lo) * 48_000.0 / crate::media::audio::PEAK_BLOCK as f64) as usize;
        let ib = ((us_to_s(hi) * 48_000.0 / crate::media::audio::PEAK_BLOCK as f64) as usize).max(ia + 1);
        let mut v = 0u8;
        for i in ia..ib.min(peaks.len()) {
            v = v.max(peaks[i]);
        }
        let a = (v as f32 / 255.0 * gain).min(1.0) * (r.height() * 0.5 - 1.0);
        if a > 0.3 {
            p.line_segment([Pos2::new(x, r.center().y - a), Pos2::new(x, r.center().y + a)], Stroke::new(1.4_f32, col));
        }
        x += step;
    }
}

/// One scroll bar: returns the new offset. `len` = whole length, `view` = visible part.
/// Area that grabs a scroll bar: much bigger than the bar, out to the panel's edge.
fn bar_hit(track: Rect, vertical: bool) -> Rect {
    if vertical {
        Rect::from_min_max(Pos2::new(track.left() - 8.0, track.top()), Pos2::new(track.right() + 10.0, track.bottom()))
    } else {
        Rect::from_min_max(Pos2::new(track.left(), track.top() - 10.0), Pos2::new(track.right(), track.bottom() + 10.0))
    }
}

fn scrollbar(ui: &egui::Ui, id: &str, track: Rect, vertical: bool, offset: f32, len: f32, view: f32) -> f32 {
    let max = (len - view).max(0.0);
    if max <= 0.5 {
        return offset;
    }
    let span = if vertical { track.height() } else { track.width() };
    let thumb = (span * view / len).clamp(28.0, span);
    let pos = (offset / max).clamp(0.0, 1.0) * (span - thumb);
    let thumb_rect = if vertical {
        Rect::from_min_size(Pos2::new(track.left(), track.top() + pos), Vec2::new(track.width(), thumb))
    } else {
        Rect::from_min_size(Pos2::new(track.left() + pos, track.top()), Vec2::new(thumb, track.height()))
    };
    let resp = ui.interact(bar_hit(track, vertical), ui.id().with(("tl-bar", id)), Sense::click_and_drag());
    let mut offset = offset;
    let along = |p: Pos2| if vertical { p.y - track.top() } else { p.x - track.left() };
    let jump = |p: Pos2| ((along(p) - thumb / 2.0) / (span - thumb).max(1.0)) * max;
    // pressing outside the thumb jumps there, and the same press keeps dragging
    let on_thumb = |p: Pos2| {
        if vertical {
            (p.y - thumb_rect.center().y).abs() <= thumb / 2.0
        } else {
            (p.x - thumb_rect.center().x).abs() <= thumb / 2.0
        }
    };
    if resp.drag_started() || resp.clicked() {
        if let Some(p) = resp.interact_pointer_pos() {
            if !on_thumb(p) {
                offset = jump(p);
            }
        }
    } else if resp.dragged() {
        let d = if vertical { resp.drag_delta().y } else { resp.drag_delta().x };
        offset += d * max / (span - thumb).max(1.0);
    }
    let p = ui.painter();
    let active = resp.hovered() || resp.dragged();
    p.rect_filled(track, cr(6), Color32::from_white_alpha(if active { 16 } else { 8 }));
    let shown = if active { thumb_rect } else { thumb_rect.shrink(1.5) };
    p.rect_filled(
        shown,
        cr(6),
        if resp.dragged() {
            ACCENT()
        } else if active {
            TEXT2()
        } else {
            PANEL3()
        },
    );
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
    }
    offset.clamp(0.0, max)
}

fn scrollbars(app: &mut VeloApp, ui: &egui::Ui, vbar: Rect, hbar: Rect, total_h: f32, view_h: f32, view_w: f32) {
    // vertical: the tracks
    app.tl.scroll_y = scrollbar(ui, "v", vbar, true, app.tl.scroll_y, total_h, view_h);
    // horizontal: the whole project plus some room after it
    let len = LEFT_PAD + us_to_s(app.project.duration().max(SEC * 10)) as f32 * app.tl.pps + view_w * 0.5;
    let len = len.max(app.tl.scroll_x + view_w);
    app.tl.scroll_x = scrollbar(ui, "h", hbar, false, app.tl.scroll_x, len, view_w);
}

/// Clips between two clicked clips (Shift+click): every clip on the tracks between them
/// that starts between their starts.
fn range_selection(p: &Project, a: u64, b: u64) -> Vec<u64> {
    let (Some((ta, _)), Some((tb, _))) = (p.find_clip(a), p.find_clip(b)) else { return vec![b] };
    let (Some(ca), Some(cb)) = (p.clip(a), p.clip(b)) else { return vec![b] };
    let (t0, t1) = (ca.start.min(cb.start), ca.start.max(cb.start));
    let (r0, r1) = (ta.min(tb), ta.max(tb));
    let mut out = vec![];
    for (i, tr) in p.tracks.iter().enumerate() {
        if i < r0 || i > r1 || tr.locked {
            continue;
        }
        for c in &tr.clips {
            if c.start >= t0 && c.start <= t1 {
                out.extend(p.group_members(c.id));
            }
        }
    }
    out
}

fn apply_drag(app: &mut VeloApp, ui: &egui::Ui, geo: &Geo, rows: &[(usize, Rect)], d: &TlDrag, p: Pos2, painter: &egui::Painter) {
    let fps = app.project.fps;
    match d.kind.clone() {
        DragKind::Scrub => {
            let t = util::snap_frame(geo.t(p.x), fps);
            app.seek(t);
        }
        DragKind::Move { grab, start_track } => {
            let orig = (*d.orig).clone();
            let Some(oc) = orig.clip(d.clip).cloned() else { return };
            let mut ns = geo.t(p.x) - grab;
            // snap either edge
            let (s1, l1) = snap(app, geo, ns, &[d.clip]);
            let (s2, l2) = snap(app, geo, ns + oc.duration, &[d.clip]);
            if l1.is_some() {
                ns = s1;
                app.tl.snap_line = l1;
            } else if l2.is_some() {
                ns = s2 - oc.duration;
                app.tl.snap_line = l2;
            } else {
                app.tl.snap_line = None;
            }
            ns = util::snap_frame(ns.max(0), fps);
            let over = rows.iter().find(|r| p.y >= r.1.top() - 3.0 && p.y <= r.1.bottom() + 3.0).map(|r| r.0);
            let above_all = rows.first().map(|r| p.y < r.1.top() - 3.0).unwrap_or(false);
            let delta = ns - oc.start;
            let id = d.clip;
            let main_idx = orig.main_track_index();
            let to_main = over == Some(main_idx);
            let main_index = if to_main && orig.magnet { Some(orig.main_insert_index(geo.t(p.x))) } else { None };
            let target = if above_all { None } else { over.or(Some(start_track)) };
            // the other selected clips move by the same amount on their own tracks
            let companions = orig.group_members(id);
            let others: Vec<u64> = if app.sel.clips.contains(&id) {
                app.sel.clips.iter().copied().filter(|x| *x != id && !companions.contains(x)).collect()
            } else {
                vec![]
            };
            let mi_orig = orig.main_track_index();
            app.edit(|pr| {
                *pr = orig.clone();
                for sid in &others {
                    let Some((ti, _)) = orig.find_clip(*sid) else { continue };
                    if ti == mi_orig && orig.magnet {
                        continue; // the magnetic main track keeps its own order
                    }
                    if let Some(c) = pr.clip_mut(*sid) {
                        c.start = (c.start + delta).max(0);
                    }
                }
                if above_all {
                    let kind = pr.track_kind_for(&oc);
                    let ti = pr.add_track(kind);
                    pr.move_clip(id, ns, Some(ti), None);
                } else {
                    pr.move_clip(id, ns, target, main_index);
                }
                pr.move_companions(id, delta);
            });
            if let Some(mi) = main_index {
                let x = geo.x(app.project.main_insert_time(mi));
                if let Some((_, r)) = rows.iter().find(|r| r.0 == main_idx) {
                    painter.line_segment([Pos2::new(x, r.top() - 4.0), Pos2::new(x, r.bottom() + 4.0)], Stroke::new(2.0_f32, ACCENT()));
                }
            }
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        DragKind::TrimL | DragKind::TrimR => {
            let orig = (*d.orig).clone();
            let Some(oc) = orig.clip(d.clip).cloned() else { return };
            let (t, line) = snap(app, geo, geo.t(p.x), &[d.clip]);
            app.tl.snap_line = line;
            let t = util::snap_frame(t, fps);
            let id = d.clip;
            let left = matches!(d.kind, DragKind::TrimL);
            // every selected clip gets the same change (the dragged one first)
            let delta = if left { t - oc.start } else { t - oc.end() };
            let mut ids = vec![id];
            if app.sel.clips.contains(&id) {
                ids.extend(app.sel.clips.iter().copied().filter(|x| *x != id));
            }
            app.edit(|pr| {
                *pr = orig.clone();
                for sid in &ids {
                    // positions are read now: trimming main-track clips moves the ones after
                    let Some(c) = pr.clip(*sid).cloned() else { continue };
                    if left {
                        pr.trim_start(*sid, c.start + delta, false);
                    } else {
                        pr.trim_end(*sid, c.end() + delta);
                    }
                }
            });
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
        }
        DragKind::Keyframe { from } => {
            let orig = (*d.orig).clone();
            let Some(oc) = orig.clip(d.clip).cloned() else { return };
            let to = (util::snap_frame(geo.t(p.x), fps) - oc.start).clamp(0, oc.duration);
            let id = d.clip;
            app.edit(|pr| {
                *pr = orig.clone();
                if let Some(c) = pr.clip_mut(id) {
                    c.kf.move_time(from, to);
                }
            });
            app.seek(oc.start + to);
        }
    }
}

fn drop_item(app: &mut VeloApp, item: DragItem, t: Us, over: Option<usize>) {
    match item {
        DragItem::Media(id) => {
            let kind = app.project.media(id).map(|m| m.kind);
            let place = match (kind, over) {
                (Some(MediaKind::Audio), Some(ti)) => Place::Track(ti, t),
                (Some(MediaKind::Audio), None) => Place::Overlay(t),
                (_, Some(ti)) => {
                    let trk = &app.project.tracks[ti];
                    if trk.main {
                        Place::Main(t)
                    } else if trk.kind == TrackKind::Video {
                        Place::Track(ti, t)
                    } else {
                        Place::Overlay(t)
                    }
                }
                (_, None) => {
                    if app.project.tracks[app.project.main_track_index()].clips.is_empty() {
                        Place::Main(t)
                    } else {
                        Place::NewTrackAbove(t)
                    }
                }
            };
            app.place_media(id, place);
        }
        DragItem::Sfx(s) => {
            if let Some(mid) = app.add_builtin_sfx(s) {
                let place = match over {
                    Some(ti) if app.project.tracks[ti].kind == TrackKind::Audio => Place::Track(ti, t),
                    _ => Place::Overlay(t),
                };
                app.place_media(mid, place);
            }
        }
        DragItem::Text(pr) => app.add_text(pr, Some(t)),
        DragItem::Sticker(s) => app.add_sticker(s, Some(t)),
        DragItem::Effect(e) => app.add_effect(e, Some(t)),
        DragItem::Filter(f) => app.add_filter(f, Some(t)),
        DragItem::Adjust(i) => app.add_adjust_layer(i, Some(t)),
        DragItem::Transition(tid) => {
            let mi = app.project.main_track_index();
            let owner = app.project.tracks[mi].clips.windows(2).min_by_key(|w| (w[1].start - t).abs()).map(|w| w[0].id);
            if let Some(o) = owner {
                app.apply_transition(tid, Some(o));
            }
        }
    }
}

fn clip_menu(app: &mut VeloApp, ui: &mut egui::Ui) {
    ui.set_min_width(220.0);
    let has_sel = !app.sel.clips.is_empty();
    let one = app.sel.one();
    let sel_ids = app.sel.clips.clone();
    let (is_video, has_audio) = {
        let media_of = |id: &u64| app.project.clip(*id).and_then(|c| c.media_id()).and_then(|m| app.project.media(m));
        (
            sel_ids.iter().any(|id| media_of(id).map(|m| m.kind == MediaKind::Video).unwrap_or(false)),
            sel_ids.iter().any(|id| media_of(id).map(|m| m.has_audio).unwrap_or(false)),
        )
    };
    let item = |ui: &mut egui::Ui, icon: &str, label: &str, key: &str, enabled: bool| -> bool {
        let text = format!("{}  {}", icon, tr(label));
        ui.add_enabled(enabled, egui::Button::new(text).shortcut_text(key)).clicked()
    };
    if item(ui, ph::SCISSORS, "Split", "Ctrl+B", true) {
        app.split();
        ui.close_menu();
    }
    if item(ui, ph::COPY, "Copy", "Ctrl+C", has_sel) {
        app.copy();
        ui.close_menu();
    }
    if item(ui, ph::SCISSORS, "Cut", "Ctrl+X", has_sel) {
        app.cut();
        ui.close_menu();
    }
    if item(ui, ph::CLIPBOARD, "Paste", "Ctrl+V", !app.clipboard.is_empty()) {
        app.paste();
        ui.close_menu();
    }
    if item(ui, ph::COPY, "Duplicate", "Ctrl+D", has_sel) {
        app.duplicate();
        ui.close_menu();
    }
    if item(ui, ph::TRASH, "Delete", "Del", has_sel) {
        app.delete_selected();
        ui.close_menu();
    }
    ui.separator();
    if item(ui, ph::SNOWFLAKE, "Freeze frame", "F", is_video) {
        app.freeze();
        ui.close_menu();
    }
    if item(ui, ph::ARROW_COUNTER_CLOCKWISE, "Reverse", "Ctrl+Shift+R", is_video) {
        app.reverse();
        ui.close_menu();
    }
    if item(ui, ph::FLIP_HORIZONTAL, "Mirror", "", has_sel) {
        app.mirror();
        ui.close_menu();
    }
    if item(ui, ph::ARROWS_CLOCKWISE, "Rotate 90°", "R", has_sel) {
        app.rotate90();
        ui.close_menu();
    }
    if item(ui, ph::SPEAKER_SIMPLE_SLASH, "Remove silences", "", has_audio) {
        for id in &sel_ids {
            app.remove_silences(*id);
        }
        ui.close_menu();
    }
    if item(ui, ph::WAVEFORM, "Extract audio", "Ctrl+Shift+S", is_video) {
        app.edit(|p| {
            for id in &sel_ids {
                p.toggle_extract_audio(*id);
            }
        });
        ui.close_menu();
    }
    if item(ui, ph::DIAMOND, "Add / remove keyframe", "Alt+K", has_sel) {
        app.toggle_keyframe();
        ui.close_menu();
    }
    ui.separator();
    if item(ui, ph::EYE_SLASH, "Enable / disable clip", "V", has_sel) {
        app.toggle_enabled();
        ui.close_menu();
    }
    if item(ui, ph::SELECTION_PLUS, "Group", "Ctrl+G", app.sel.clips.len() > 1) {
        let ids = app.sel.clips.clone();
        app.edit(|p| p.group(&ids));
        ui.close_menu();
    }
    if item(ui, ph::SELECTION, "Ungroup", "Ctrl+Shift+G", has_sel) {
        let ids = app.sel.clips.clone();
        app.edit(|p| p.ungroup(&ids));
        ui.close_menu();
    }
    if item(ui, ph::PAINT_BRUSH, "Copy attributes", "Ctrl+Alt+C", one.is_some()) {
        app.attr_clip = one.and_then(|id| app.project.clip(id)).cloned();
        app.toast(tr("Attributes copied"));
        ui.close_menu();
    }
    if item(ui, ph::PAINT_BUCKET, "Paste attributes", "Ctrl+Alt+V", app.attr_clip.is_some() && has_sel) {
        if let Some(src) = app.attr_clip.clone() {
            let ids = app.sel.clips.clone();
            app.edit(|p| p.paste_attributes(&src, &ids));
        }
        ui.close_menu();
    }
}

fn toolbar(app: &mut VeloApp, ui: &mut egui::Ui, bar: Rect) {
    let mut tb = ui.new_child(
        egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(10.0, 5.0))).layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let ui = &mut tb;
    ui.spacing_mut().item_spacing.x = 2.0;
    let has_sel = !app.sel.clips.is_empty() || app.sel.transition.is_some();
    if icon_btn(ui, ph::CURSOR, &format!("{} (A)", tr("Select")), true, !app.tl.split_mode).clicked() {
        app.tl.split_mode = false;
    }
    if icon_btn(ui, ph::SCISSORS, &format!("{} (B)", tr("Split mode")), true, app.tl.split_mode).clicked() {
        app.tl.split_mode = !app.tl.split_mode;
    }
    ui.add_space(6.0);
    if icon_btn(ui, ph::ARROW_COUNTER_CLOCKWISE, &format!("{} (Ctrl+Z)", tr("Undo")), !app.undo.is_empty(), false).clicked() {
        app.undo();
    }
    if icon_btn(ui, ph::ARROW_CLOCKWISE, &format!("{} (Ctrl+Shift+Z)", tr("Redo")), !app.redo.is_empty(), false).clicked() {
        app.redo();
    }
    sep(ui);
    if icon_btn(ui, ph::SPLIT_HORIZONTAL, &format!("{} (Ctrl+B)", tr("Split")), true, false).clicked() {
        app.split();
    }
    if icon_btn(ui, ph::ARROW_LINE_LEFT, &format!("{} (Q)", tr("Delete left")), true, false).clicked() {
        app.delete_left();
    }
    if icon_btn(ui, ph::ARROW_LINE_RIGHT, &format!("{} (W)", tr("Delete right")), true, false).clicked() {
        app.delete_right();
    }
    if icon_btn(ui, ph::TRASH, &format!("{} (Del)", tr("Delete")), has_sel, false).clicked() {
        app.delete_selected();
    }
    sep(ui);
    let one_video = app
        .sel
        .one()
        .and_then(|id| app.project.clip(id))
        .and_then(|c| c.media_id())
        .and_then(|m| app.project.media(m))
        .map(|m| m.kind == MediaKind::Video)
        .unwrap_or(false);
    if icon_btn(ui, ph::SNOWFLAKE, &format!("{} (F)", tr("Freeze")), true, false).clicked() {
        app.freeze();
    }
    if icon_btn(ui, ph::REWIND, &format!("{} (Ctrl+Shift+R)", tr("Reverse")), one_video, false).clicked() {
        app.reverse();
    }
    if icon_btn(ui, ph::FLIP_HORIZONTAL, tr("Mirror"), has_sel, false).clicked() {
        app.mirror();
    }
    if icon_btn(ui, ph::ARROWS_CLOCKWISE, &format!("{} (R)", tr("Rotate")), has_sel, false).clicked() {
        app.rotate90();
    }
    if icon_btn(ui, ph::CROP, tr("Crop"), one_video, false).clicked() {
        app.right_tab = super::RightTab::Video;
        app.video_sub = 0;
        app.toast(tr("Crop is in the Video › Basic panel"));
    }
    if icon_btn(ui, ph::DIAMOND, &format!("{} (Alt+K)", tr("Keyframe")), !app.sel.clips.is_empty(), false).clicked() {
        app.toggle_keyframe();
    }
    if icon_btn(ui, ph::BOOKMARK_SIMPLE, &format!("{} (M)", tr("Add marker")), true, false).clicked() {
        let t = app.playhead;
        app.edit(|p| {
            if let Some(i) = p.markers.iter().position(|m| (*m - t).abs() < 20_000) {
                p.markers.remove(i);
            } else {
                p.markers.push(t);
            }
        });
    }
    // right side
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if icon_btn(ui, ph::ARROWS_OUT_LINE_HORIZONTAL, &format!("{} (Shift+Z)", tr("Fit timeline")), true, false).clicked() {
            app.tl.fit_request = true;
        }
        if icon_btn(ui, ph::MAGNIFYING_GLASS_PLUS, &format!("{} (Ctrl+=)", tr("Zoom in")), true, false).clicked() {
            app.tl.pps = (app.tl.pps * 1.4).min(1500.0);
        }
        let mut z = app.tl.pps.ln();
        ui.spacing_mut().slider_width = 90.0;
        if ui.add(egui::Slider::new(&mut z, 0.0..=7.3).show_value(false)).changed() {
            app.tl.pps = z.exp();
        }
        if icon_btn(ui, ph::MAGNIFYING_GLASS_MINUS, &format!("{} (Ctrl+-)", tr("Zoom out")), true, false).clicked() {
            app.tl.pps = (app.tl.pps / 1.4).max(1.0);
        }
        sep(ui);
        let axis = app.project.preview_axis;
        if icon_btn(ui, ph::USER_FOCUS, &format!("{} (S)", tr("Preview axis")), true, axis).clicked() {
            app.edit(|p| p.preview_axis = !axis);
        }
        let link = app.project.linkage;
        if icon_btn(ui, ph::LINK, &format!("{} (~)", tr("Linkage")), true, link).clicked() {
            app.edit(|p| p.linkage = !link);
        }
        let snapping = app.project.snapping;
        if icon_btn(ui, ph::MAGNET_STRAIGHT, &format!("{} (N)", tr("Auto snapping")), true, snapping).clicked() {
            app.edit(|p| p.snapping = !snapping);
        }
        let magnet = app.project.magnet;
        if icon_btn(ui, ph::MAGNET, &format!("{} (P)", tr("Main track magnet")), true, magnet).clicked() {
            app.edit(|p| {
                p.magnet = !magnet;
                p.normalize();
            });
        }
    });
}

fn sep(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    let (r, _) = ui.allocate_exact_size(Vec2::new(1.0, 18.0), Sense::hover());
    ui.painter().rect_filled(r, cr(0), BORDER());
    ui.add_space(4.0);
}
