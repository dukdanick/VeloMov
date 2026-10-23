//! Right panel: properties of the selection ("Details" panel).

use super::i18n::tr;
use super::theme::*;
use super::widgets::*;
use super::{RightTab, VeloApp};
use crate::model::catalog::{self, AnimKind};
use crate::model::keyframe::{Ease, Prop};
use crate::model::*;
use crate::util::{self, Us, SEC};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as ph;

fn kf_state(c: &Clip, props: &[Prop], t: Us) -> Kf {
    if !props.iter().any(|p| c.kf.has(*p)) {
        Kf::None
    } else if c.kf.any_at(props, t - c.start) {
        Kf::On
    } else {
        Kf::Has
    }
}

fn toggle_kf(app: &mut VeloApp, id: u64, props: &[Prop]) {
    let t = app.playhead;
    let Some(c) = app.project.clip(id) else { return };
    if !c.contains(t) {
        app.toast(tr("Move the playhead over the clip to add keyframes"));
        return;
    }
    let local = t - c.start;
    let on = c.kf.any_at(props, local);
    let props = props.to_vec();
    app.edit(|p| {
        if let Some(c) = p.clip_mut(id) {
            for pr in props {
                if on {
                    c.kf.remove(pr, local);
                } else {
                    let v = c.prop(pr, t);
                    c.kf.set(pr, local, v);
                }
            }
        }
    });
}

/// Slider bound to a keyframable property. `mul` converts stored -> displayed units.
#[allow(clippy::too_many_arguments)]
fn prop_slider(
    app: &mut VeloApp,
    ui: &mut egui::Ui,
    id: u64,
    label: &str,
    props: &[Prop],
    mul: f64,
    min: f64,
    max: f64,
    default: f64,
    dec: usize,
    suffix: &str,
) {
    let t = app.playhead;
    let Some(c) = app.project.clip(id) else { return };
    let prop = props[0];
    let mut v = c.prop(prop, t) * mul;
    let kf = kf_state(c, props, t);
    let out = slider_row(ui, label, &mut v, min, max, default, dec, suffix, Some(kf));
    if out.changed {
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.set_prop_at(prop, t, v / mul);
            }
        });
    }
    if out.kf_clicked {
        toggle_kf(app, id, props);
    }
}

fn tabs_bar(ui: &mut egui::Ui, tabs: &[(RightTab, &str)], cur: &mut RightTab) {
    let r = ui.available_rect_before_wrap();
    let bar = Rect::from_min_size(r.min, Vec2::new(r.width(), 40.0));
    ui.painter().line_segment(
        [Pos2::new(bar.left() + 10.0, bar.bottom()), Pos2::new(bar.right() - 10.0, bar.bottom())],
        Stroke::new(1.0_f32, BORDER()),
    );
    let mut x = bar.left() + 10.0;
    for (t, label) in tabs {
        let g = ui.fonts(|f| f.layout_no_wrap(tr(label).to_string(), prop(13.0), TEXT()));
        let w = g.size().x + 16.0;
        let tr_ = Rect::from_min_size(Pos2::new(x, bar.top()), Vec2::new(w, bar.height()));
        let resp = ui.interact(tr_, ui.id().with(("rtab", *label)), Sense::click());
        let active = *cur == *t;
        let col = if active {
            Color32::WHITE
        } else if resp.hovered() {
            TEXT()
        } else {
            TEXT2()
        };
        ui.painter().text(tr_.center(), Align2::CENTER_CENTER, tr(label), if active { bold(13.0) } else { prop(13.0) }, col);
        if active {
            ui.painter().rect_filled(
                Rect::from_center_size(Pos2::new(tr_.center().x, bar.bottom() - 1.5), Vec2::new(18.0, 3.0)),
                cr(2),
                ACCENT(),
            );
        }
        if resp.clicked() {
            *cur = *t;
        }
        x += w + 4.0;
    }
    ui.allocate_rect(bar, Sense::hover());
}

pub fn ui(app: &mut VeloApp, ui: &mut egui::Ui) {
    if let Some(owner) = app.sel.transition {
        transition_panel(app, ui, owner);
        return;
    }
    // several clips: edit the last one clicked, then copy what changed to the others
    let multi = app.sel.clips.len() > 1;
    let primary = if multi { app.sel.clips.iter().rev().copied().find(|id| app.project.clip(*id).is_some()) } else { app.sel.one() };
    let Some(id) = primary else {
        draft_panel(app, ui);
        return;
    };
    let Some(c) = app.project.clip(id).cloned() else {
        draft_panel(app, ui);
        return;
    };
    let track_kind = app.project.track_of(id).map(|t| t.kind).unwrap_or(TrackKind::Video);
    let media = c.media_id().and_then(|m| app.project.media(m)).cloned();
    let is_audio_clip = track_kind == TrackKind::Audio || media.as_ref().map(|m| m.kind == MediaKind::Audio).unwrap_or(false);
    let mut tabs: Vec<(RightTab, &str)> = vec![];
    match &c.kind {
        ClipKind::Media { .. } if is_audio_clip => {
            tabs.push((RightTab::Audio, "Audio"));
            tabs.push((RightTab::Speed, "Speed"));
        }
        ClipKind::Media { .. } => {
            tabs.push((RightTab::Video, "Video"));
            if c.has_audio(&app.project) {
                tabs.push((RightTab::Audio, "Audio"));
            }
            if media.as_ref().map(|m| m.kind == MediaKind::Video).unwrap_or(false) {
                tabs.push((RightTab::Speed, "Speed"));
            }
            tabs.push((RightTab::Animation, "Animation"));
            tabs.push((RightTab::Adjust, "Adjust"));
        }
        ClipKind::Text(_) => {
            tabs.push((RightTab::Video, "Text"));
            tabs.push((RightTab::Animation, "Animation"));
        }
        ClipKind::Sticker(_) => {
            tabs.push((RightTab::Video, "Sticker"));
            tabs.push((RightTab::Animation, "Animation"));
        }
        ClipKind::Effect(_) => tabs.push((RightTab::Video, "Effect")),
        ClipKind::Filter(_) => tabs.push((RightTab::Video, "Filter")),
        ClipKind::Adjust => tabs.push((RightTab::Adjust, "Adjust")),
    }
    if !tabs.iter().any(|t| t.0 == app.right_tab) {
        app.right_tab = tabs[0].0;
    }
    let mut cur = app.right_tab;
    tabs_bar(ui, &tabs, &mut cur);
    app.right_tab = cur;
    if multi {
        multi_header(app, ui);
    }
    egui::ScrollArea::vertical().drag_to_scroll(false).id_salt(("insp", id)).auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 12, top: 8, bottom: 16 }).show(ui, |ui| {
            match (&c.kind, app.right_tab) {
                (ClipKind::Media { .. }, RightTab::Video) => video_tab(app, ui, &c, media.as_ref()),
                (ClipKind::Text(_), RightTab::Video) => text_tab(app, ui, &c),
                (ClipKind::Sticker(_), RightTab::Video) => sticker_tab(app, ui, &c),
                (ClipKind::Effect(_), _) => effect_tab(app, ui, &c),
                (ClipKind::Filter(_), _) => filter_clip_tab(app, ui, &c),
                (_, RightTab::Audio) => audio_tab(app, ui, &c),
                (_, RightTab::Speed) => speed_tab(app, ui, &c),
                (_, RightTab::Animation) => animation_tab(app, ui, &c),
                (_, RightTab::Adjust) => adjust_tab(app, ui, &c),
                _ => {}
            }
        });
    });
    if multi {
        propagate(app, id, &c);
    }
}

/// Properties never copied between clips: identity, place on the timeline, which media.
const NOT_SHARED: &[&str] = &[
    "/id",
    "/start",
    "/duration",
    "/src_in",
    "/link",
    "/group",
    "/transition",
    "/kind/Media",
    "/kind/Text/text",
    "/kind/Text/words",
    "/speed",
    "/curve",
    "/reverse",
    "/freeze",
    "/audio_detached",
    "/disabled",
];

/// Leaf-level differences between two JSON values: (JSON pointer, new value).
fn json_diff(path: &str, a: &serde_json::Value, b: &serde_json::Value, out: &mut Vec<(String, serde_json::Value)>) {
    match (a, b) {
        (serde_json::Value::Object(ma), serde_json::Value::Object(mb)) => {
            for (k, vb) in mb {
                let p = format!("{}/{}", path, k.replace('~', "~0").replace('/', "~1"));
                match ma.get(k) {
                    Some(va) => json_diff(&p, va, vb, out),
                    None => out.push((p, vb.clone())),
                }
            }
        }
        _ if a != b => out.push((path.to_string(), b.clone())),
        _ => {}
    }
}

/// Copy the properties the inspector just changed on `primary` to the other selected clips
/// (only where they make sense: a text style does not land on a video).
fn propagate(app: &mut VeloApp, primary: u64, before: &Clip) {
    let Some(after) = app.project.clip(primary).cloned() else { return };
    if after == *before {
        return;
    }
    let others: Vec<u64> = app.sel.clips.iter().copied().filter(|x| *x != primary).collect();
    // speed changes the length of a clip: use the editing operations
    let speed_changed = (after.speed - before.speed).abs() > 1e-9 && after.curve.is_none();
    let curve_changed = after.curve != before.curve;
    let (Ok(a), Ok(b)) = (serde_json::to_value(before), serde_json::to_value(&after)) else { return };
    let mut changes = vec![];
    json_diff("", &a, &b, &mut changes);
    changes.retain(|(path, _)| !NOT_SHARED.iter().any(|n| path == n || path.starts_with(&format!("{}/", n))));
    if changes.is_empty() && !speed_changed && !curve_changed {
        return;
    }
    app.edit(|p| {
        for id in &others {
            if curve_changed {
                p.set_curve(*id, after.curve.clone());
            } else if speed_changed {
                p.set_speed(*id, after.speed);
            }
            let Some(c) = p.clip_mut(*id) else { continue };
            let Ok(mut v) = serde_json::to_value(&*c) else { continue };
            let mut touched = false;
            for (path, val) in &changes {
                let (parent, key) = path.rsplit_once('/').unwrap_or(("", path));
                let key = key.replace("~1", "/").replace("~0", "~");
                // only into places the other clip has (same kind of clip)
                let slot = if parent.is_empty() { Some(&mut v) } else { v.pointer_mut(parent) };
                if let Some(serde_json::Value::Object(m)) = slot {
                    if let Some(old) = m.get(&key) {
                        let compatible = std::mem::discriminant(old) == std::mem::discriminant(val) || old.is_null() || val.is_null();
                        if compatible {
                            m.insert(key, val.clone());
                            touched = true;
                        }
                    }
                }
            }
            if touched {
                if let Ok(nc) = serde_json::from_value::<Clip>(v) {
                    *c = nc;
                }
            }
        }
    });
}

/// Line above the properties when several clips are selected.
fn multi_header(app: &mut VeloApp, ui: &mut egui::Ui) {
    egui::Frame::new()
        .fill(ACCENT_BG())
        .corner_radius(cr(8))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .outer_margin(egui::Margin { left: 10, right: 10, top: 6, bottom: 0 })
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{} {}", app.sel.clips.len(), tr("clips selected"))).font(bold(12.5)).color(Color32::WHITE),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_btn(ui, ph::TRASH, tr("Delete"), true, false).clicked() {
                        app.delete_selected();
                    }
                    if icon_btn(ui, ph::SELECTION, tr("Ungroup"), true, false).clicked() {
                        let ids = app.sel.clips.clone();
                        app.edit(|p| p.ungroup(&ids));
                    }
                    if icon_btn(ui, ph::SELECTION_PLUS, tr("Group (Ctrl+G)"), true, false).clicked() {
                        let ids = app.sel.clips.clone();
                        app.edit(|p| p.group(&ids));
                    }
                    if let Some(src) = app.attr_clip.clone() {
                        if icon_btn(ui, ph::CLIPBOARD, tr("Paste attributes"), true, false).clicked() {
                            let ids = app.sel.clips.clone();
                            app.edit(|p| p.paste_attributes(&src, &ids));
                        }
                    }
                });
            });
            ui.label(egui::RichText::new(tr("Changes apply to every selected clip.")).size(11.0).color(TEXT2()));
        });
}

// ---------------------------------------------------------------- draft (nothing selected)

fn draft_panel(app: &mut VeloApp, ui: &mut egui::Ui) {
    let mut t = RightTab::Video;
    tabs_bar(ui, &[(RightTab::Video, "Details")], &mut t);
    egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 12, top: 10, bottom: 10 }).show(ui, |ui| {
        section(ui, "Draft settings", None);
        let mut name = app.project.name.clone();
        label_row(ui, "Name", |ui| {
            if ui.add(egui::TextEdit::singleline(&mut name).desired_width(ui.available_width() - 6.0)).changed() {
                app.edit(|p| p.name = name.clone());
            }
        });
        let mut ratio = app.project.ratio;
        let opts: Vec<(Ratio, String)> = Ratio::ALL.iter().chain([Ratio::Custom].iter()).map(|r| (*r, tr(r.label()).to_string())).collect();
        if combo_row(ui, "Ratio", &mut ratio, &opts) {
            if ratio == Ratio::Custom {
                let (w, h) = (app.project.width, app.project.height);
                app.edit(|p| p.set_custom_size(w, h));
            } else {
                app.set_ratio(ratio);
            }
        }
        if app.project.ratio == Ratio::Custom {
            label_row(ui, "Resolution", |ui| {
                let (mut w, mut h) = (app.project.width, app.project.height);
                let a = ui.add(egui::DragValue::new(&mut w).range(16..=8192).speed(4));
                ui.label("×");
                let b = ui.add(egui::DragValue::new(&mut h).range(16..=8192).speed(4));
                if (a.changed() || b.changed()) && (w, h) != (app.project.width, app.project.height) {
                    app.edit(|p| p.set_custom_size(w, h));
                    app.engine.clear_cache();
                }
            });
        } else {
            let mut short = app.project.short_side();
            let mut res: Vec<(u32, String)> = super::new_project::RESOLUTIONS
                .iter()
                .map(|(s, n)| {
                    let aspect = app.project.width.max(1) as f64 / app.project.height.max(1) as f64;
                    let (w, h) = crate::model::canvas_for_aspect(aspect, *s);
                    (*s, format!("{}  ({} × {})", n, w, h))
                })
                .collect();
            if !res.iter().any(|r| r.0 == short) {
                res.push((short, format!("{} × {}", app.project.width, app.project.height)));
            }
            if combo_row(ui, "Resolution", &mut short, &res) {
                app.edit(|p| p.set_short_side(short));
                app.engine.clear_cache();
            }
        }
        let mut fps = app.project.fps.round() as i32;
        let fps_opts: Vec<(i32, String)> = [24, 25, 30, 50, 60].iter().map(|f| (*f, format!("{} fps", f))).collect();
        if combo_row(ui, "Frame rate", &mut fps, &fps_opts) {
            app.edit(|p| p.fps = fps as f64);
        }
        label_row(ui, "Background", |ui| {
            let mut c = app.project.bg_color;
            if color_button(ui, egui::Id::new("draft-bg"), &mut c, false) {
                app.edit(|p| p.bg_color = c);
            }
        });
        label_row(ui, "Duration", |ui| {
            ui.label(egui::RichText::new(util::timecode(app.project.duration(), app.project.fps)).color(TEXT()));
        });
        separator(ui);
        section(ui, "Timeline", None);
        let mut magnet = app.project.magnet;
        ui.horizontal(|ui| {
            if toggle(ui, &mut magnet).changed() {
                app.edit(|p| {
                    p.magnet = magnet;
                    p.normalize();
                });
            }
            ui.label(tr("Main track magnet (P)"));
        });
        let mut snap = app.project.snapping;
        ui.horizontal(|ui| {
            if toggle(ui, &mut snap).changed() {
                app.edit(|p| p.snapping = snap);
            }
            ui.label(tr("Auto snapping (N)"));
        });
        let mut link = app.project.linkage;
        ui.horizontal(|ui| {
            if toggle(ui, &mut link).changed() {
                app.edit(|p| p.linkage = link);
            }
            ui.label(tr("Audio/video linkage (~)"));
        });
        separator(ui);
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(tr("Select a clip on the timeline to edit its properties. Press ? to see all shortcuts."))
                .size(11.5)
                .color(TEXT3()),
        );
    });
}

// ---------------------------------------------------------------- transitions

fn transition_panel(app: &mut VeloApp, ui: &mut egui::Ui, owner: u64) {
    let mut t = RightTab::Video;
    tabs_bar(ui, &[(RightTab::Video, "Transition")], &mut t);
    let Some(c) = app.project.clip(owner).cloned() else { return };
    let Some(trn) = c.transition.clone() else { return };
    let def = catalog::transition(&trn.id);
    let mi = app.project.main_track_index();
    let next =
        app.project.tracks[mi].clips.iter().position(|x| x.id == owner).and_then(|i| app.project.tracks[mi].clips.get(i + 1)).cloned();
    egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(def.map(|d| tr(d.name)).unwrap_or(&trn.id)).font(bold(15.0)).color(Color32::WHITE));
            if let Some(d) = def {
                ui.label(egui::RichText::new(tr(d.cat.label())).size(11.0).color(TEXT3()));
            }
        });
        ui.add_space(6.0);
        let maxd = next.map(|n| c.duration.min(n.duration)).unwrap_or(SEC).min(5 * SEC);
        let mut d = util::us_to_s(trn.dur);
        let out = slider_row(ui, "Duration", &mut d, 0.1, util::us_to_s(maxd).max(0.1), 0.5, 1, " s", None);
        if out.changed {
            let nd = util::s_to_us(d);
            app.edit(|p| {
                if let Some(c) = p.clip_mut(owner) {
                    if let Some(t) = c.transition.as_mut() {
                        t.dur = nd;
                    }
                }
                p.normalize();
            });
        }
        let mut b = trn.blur * 100.0;
        let out = slider_row(ui, "Blur amount", &mut b, 0.0, 100.0, def.map(|d| d.blur * 100.0).unwrap_or(60.0), 0, "%", None);
        if out.changed {
            app.edit(|p| {
                if let Some(c) = p.clip_mut(owner) {
                    if let Some(t) = c.transition.as_mut() {
                        t.blur = b / 100.0;
                    }
                }
            });
        }
        ui.label(egui::RichText::new(tr("Motion blur smears fast movement (slides, whips, zooms, spins).")).size(11.0).color(TEXT3()));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if pill(ui, ph::CHECK_SQUARE_OFFSET, tr("Apply to all"), PANEL3(), Color32::WHITE).clicked() {
                let tu = trn.clone();
                app.edit(|p| p.transition_all(&tu));
                app.toast(tr("Transition applied to all cuts"));
            }
            if pill(ui, ph::TRASH, tr("Remove"), PANEL3(), DANGER).clicked() {
                app.edit(|p| p.set_transition(owner, None));
                app.sel.transition = None;
            }
        });
        ui.add_space(8.0);
        ui.label(egui::RichText::new(tr("Pick another transition in the Transitions tab to replace it.")).size(11.0).color(TEXT3()));
    });
}

// ---------------------------------------------------------------- video (media) tab

fn video_tab(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip, media: Option<&MediaAsset>) {
    let id = c.id;
    let mut sub = app.video_sub;
    if segmented(ui, &["Basic", "Remove BG", "Mask"], &mut sub) {
        app.video_sub = sub;
    }
    ui.add_space(6.0);
    match app.video_sub {
        1 => chroma_section(app, ui, c),
        2 => mask_section(app, ui, c),
        _ => {
            transform_section(app, ui, c);
            separator(ui);
            blend_section(app, ui, c);
            separator(ui);
            motion_blur_section(app, ui, c);
            let is_main = app.project.track_of(id).map(|t| t.main).unwrap_or(false);
            if is_main {
                separator(ui);
                canvas_section(app, ui, c);
            }
            separator(ui);
            crop_section(app, ui, c);
            if c.filter.is_some() {
                separator(ui);
                clip_filter_section(app, ui, c);
            }
            if let Some(m) = media {
                separator(ui);
                ui.label(
                    egui::RichText::new(format!("{}  {}×{}  {:.2} fps  {}", m.name, m.width, m.height, m.fps, util::short_dur(m.duration)))
                        .size(10.5)
                        .color(TEXT3()),
                );
                if c.reverse && m.reversed.is_none() {
                    let pct = app.proxy_jobs.iter().find(|j| j.media == m.id).map(|j| j.frac() * 100.0).unwrap_or(0.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new(format!("{} {:.0}%", tr("Reversing..."), pct)).size(11.0).color(TEXT2()));
                    });
                }
            }
        }
    }
}

fn transform_section(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("Transform")).font(bold(13.0)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_btn(ui, ph::ARROW_COUNTER_CLOCKWISE, tr("Reset"), true, false).clicked() {
                app.edit(|p| {
                    if let Some(c) = p.clip_mut(id) {
                        let flip = (c.tf.flip_h, c.tf.flip_v);
                        c.tf = Transform::default();
                        c.tf.flip_h = flip.0;
                        c.tf.flip_v = flip.1;
                        for pr in Prop::TRANSFORM {
                            c.kf.clear(pr);
                        }
                    }
                });
            }
            let kfs = kf_state(c, &Prop::TRANSFORM, app.playhead);
            if diamond(ui, kfs, tr("Keyframe all transform properties (Alt+K)")).clicked() {
                toggle_kf(app, id, &[Prop::PosX, Prop::PosY, Prop::Scale, Prop::Rotation]);
            }
        });
    });
    let (pw, ph_) = (app.project.width as f64, app.project.height as f64);
    let uniform = c.tf.uniform;
    if uniform {
        prop_slider(app, ui, id, "Scale", &[Prop::Scale], 100.0, 1.0, 500.0, 100.0, 0, "%");
    } else {
        prop_slider(app, ui, id, "Scale width", &[Prop::ScaleX], 100.0, 1.0, 500.0, 100.0, 0, "%");
        prop_slider(app, ui, id, "Scale height", &[Prop::ScaleY], 100.0, 1.0, 500.0, 100.0, 0, "%");
    }
    ui.horizontal(|ui| {
        ui.add_space(96.0);
        let mut u = uniform;
        if toggle(ui, &mut u).changed() {
            app.edit(|p| {
                if let Some(c) = p.clip_mut(id) {
                    c.tf.uniform = u;
                    if !u {
                        c.tf.scale_x = 1.0;
                        c.tf.scale_y = 1.0;
                    }
                }
            });
        }
        ui.label(egui::RichText::new(tr("Uniform scale")).size(12.0).color(TEXT2()));
    });
    prop_slider(app, ui, id, "Position X", &[Prop::PosX, Prop::PosY], 1.0, -pw * 1.5, pw * 1.5, 0.0, 0, "");
    prop_slider(app, ui, id, "Position Y", &[Prop::PosY, Prop::PosX], 1.0, -ph_ * 1.5, ph_ * 1.5, 0.0, 0, "");
    prop_slider(app, ui, id, "Rotate", &[Prop::Rotation], 1.0, -360.0, 360.0, 0.0, 1, "°");
    ui.horizontal(|ui| {
        ui.add_space(96.0);
        if ui.add(egui::Button::new(format!("{}  {}", ph::FLIP_HORIZONTAL, tr("Mirror"))).selected(c.tf.flip_h)).clicked() {
            app.edit(|p| {
                if let Some(c) = p.clip_mut(id) {
                    c.tf.flip_h = !c.tf.flip_h;
                }
            });
        }
        if ui.add(egui::Button::new(format!("{}  {}", ph::ARROWS_CLOCKWISE, tr("Flip vertical"))).selected(c.tf.flip_v)).clicked() {
            app.edit(|p| {
                if let Some(c) = p.clip_mut(id) {
                    c.tf.flip_v = !c.tf.flip_v;
                }
            });
        }
    });
    ease_row(app, ui, c);
}

/// Easing of the keyframe segment at the playhead ("keyframe graph").
fn ease_row(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    if c.kf.is_empty() {
        return;
    }
    let local = app.playhead - c.start;
    // the segment that starts at the last key <= playhead
    let times = c.kf.times();
    let Some(seg) = times.iter().rev().find(|k| **k <= local + 8_000).copied() else { return };
    let cur = c.kf.0.iter().flat_map(|t| t.keys.iter()).find(|k| (k.t - seg).abs() <= 8_000).map(|k| k.ease).unwrap_or_default();
    let mut e = cur;
    let opts: Vec<(Ease, String)> = Ease::ALL.iter().map(|e| (*e, tr(e.label()).to_string())).collect();
    if combo_row(ui, "Curve", &mut e, &opts) {
        let id = c.id;
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                let props: Vec<Prop> = c.kf.0.iter().map(|t| t.prop).collect();
                for pr in props {
                    c.kf.set_ease(pr, seg, e);
                }
            }
        });
    }
}

fn blend_section(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    section(ui, "Blend", None);
    let mut b = c.blend;
    let opts: Vec<(Blend, String)> = Blend::ALL.iter().map(|b| (*b, tr(b.label()).to_string())).collect();
    if combo_row(ui, "Mode", &mut b, &opts) {
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.blend = b;
            }
        });
    }
    prop_slider(app, ui, id, "Opacity", &[Prop::Opacity], 100.0, 0.0, 100.0, 100.0, 0, "%");
}

fn motion_blur_section(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    let mut on = c.motion_blur > 0.0;
    if section(ui, "Motion blur", Some(&mut on)) {
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.motion_blur = if on { 0.6 } else { 0.0 };
            }
        });
    }
    if c.motion_blur > 0.0 {
        let mut v = c.motion_blur * 100.0;
        if slider_row(ui, "Strength", &mut v, 0.0, 100.0, 60.0, 0, "%", None).changed {
            app.edit(|p| {
                if let Some(c) = p.clip_mut(id) {
                    c.motion_blur = v / 100.0;
                }
            });
        }
        ui.label(
            egui::RichText::new(tr("Blurs movement from keyframes and animations (like a camera shutter).")).size(11.0).color(TEXT3()),
        );
    }
}

fn canvas_section(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    section(ui, "Canvas", None);
    let mut sel = match c.canvas {
        CanvasFill::None => 0usize,
        CanvasFill::Blur(_) => 1,
        CanvasFill::Color(_) => 2,
    };
    if segmented(ui, &["None", "Blur", "Color"], &mut sel) {
        let v = match sel {
            1 => CanvasFill::Blur(0.5),
            2 => CanvasFill::Color([20, 20, 24, 255]),
            _ => CanvasFill::None,
        };
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.canvas = v;
            }
        });
    }
    match c.canvas {
        CanvasFill::Blur(l) => {
            let mut v = l * 100.0;
            if slider_row(ui, "Blur", &mut v, 0.0, 100.0, 50.0, 0, "%", None).changed {
                app.edit(|p| {
                    if let Some(c) = p.clip_mut(id) {
                        c.canvas = CanvasFill::Blur(v / 100.0);
                    }
                });
            }
        }
        CanvasFill::Color(col) => {
            let mut col = col;
            label_row(ui, "Color", |ui| {
                if color_button(ui, egui::Id::new(("canvas-col", id)), &mut col, false) {
                    app.edit(|p| {
                        if let Some(c) = p.clip_mut(id) {
                            c.canvas = CanvasFill::Color(col);
                        }
                    });
                }
            });
        }
        CanvasFill::None => {}
    }
    if ui.small_button(tr("Apply to all")).clicked() {
        let v = c.canvas;
        app.edit(|p| {
            let mi = p.main_track_index();
            for c in p.tracks[mi].clips.iter_mut() {
                c.canvas = v;
            }
        });
    }
}

fn crop_section(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    let mut open = !c.crop.is_identity();
    if section(ui, "Crop", Some(&mut open)) && !open {
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.crop = Crop::default();
            }
        });
    }
    if !open && c.crop.is_identity() {
        let mut ratio_btns = |ui: &mut egui::Ui| {
            for (label, a) in [("16:9", 16.0 / 9.0), ("9:16", 9.0 / 16.0), ("1:1", 1.0), ("4:3", 4.0 / 3.0)] {
                if ui.small_button(label).clicked() {
                    let Some(m) = c.media_id().and_then(|m| app.project.media(m)) else { continue };
                    let ma = m.width as f64 / m.height.max(1) as f64;
                    let mut cr = Crop::default();
                    if ma > a {
                        let keep = a / ma;
                        cr.left = (1.0 - keep) / 2.0;
                        cr.right = cr.left;
                    } else {
                        let keep = ma / a;
                        cr.top = (1.0 - keep) / 2.0;
                        cr.bottom = cr.top;
                    }
                    app.edit(|p| {
                        if let Some(c) = p.clip_mut(id) {
                            c.crop = cr;
                        }
                    });
                }
            }
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("Quick crop")).size(11.5).color(TEXT3()));
            ratio_btns(ui);
        });
        return;
    }
    let mut cr = c.crop;
    let mut changed = false;
    changed |= slider_row(ui, "Left", &mut cr.left, 0.0, 0.95, 0.0, 3, "", None).changed;
    changed |= slider_row(ui, "Right", &mut cr.right, 0.0, 0.95, 0.0, 3, "", None).changed;
    changed |= slider_row(ui, "Top", &mut cr.top, 0.0, 0.95, 0.0, 3, "", None).changed;
    changed |= slider_row(ui, "Bottom", &mut cr.bottom, 0.0, 0.95, 0.0, 3, "", None).changed;
    if changed {
        cr.right = cr.right.min(0.98 - cr.left);
        cr.bottom = cr.bottom.min(0.98 - cr.top);
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.crop = cr;
            }
        });
    }
}

fn clip_filter_section(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    let Some(f) = &c.filter else { return };
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{}: {}", tr("Filter"), catalog::filter(&f.id).map(|d| tr(d.name)).unwrap_or(""))).font(bold(13.0)),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_btn(ui, ph::TRASH, tr("Remove filter"), true, false).clicked() {
                app.edit(|p| {
                    if let Some(c) = p.clip_mut(id) {
                        c.filter = None;
                        c.kf.clear(Prop::FilterIntensity);
                    }
                });
            }
        });
    });
    prop_slider(app, ui, id, "Intensity", &[Prop::FilterIntensity], 100.0, 0.0, 100.0, 100.0, 0, "%");
}

fn chroma_section(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    let mut ck = c.chroma;
    let mut changed = section(ui, "Chroma key", Some(&mut ck.enabled));
    if ck.enabled {
        label_row(ui, "Color", |ui| {
            let mut col = [ck.color[0], ck.color[1], ck.color[2], 255];
            if color_button(ui, egui::Id::new(("chroma", id)), &mut col, false) {
                ck.color = [col[0], col[1], col[2]];
                changed = true;
            }
            for (name, rgb) in [("Green", [0u8, 255u8, 0u8]), ("Blue", [0, 70, 255])] {
                if ui.small_button(tr(name)).clicked() {
                    ck.color = rgb;
                    changed = true;
                }
            }
        });
        let mut i = ck.intensity * 100.0;
        if slider_row(ui, "Intensity", &mut i, 0.0, 100.0, 35.0, 0, "", None).changed {
            ck.intensity = i / 100.0;
            changed = true;
        }
        let mut s = ck.shadow * 100.0;
        if slider_row(ui, "Shadow", &mut s, 0.0, 100.0, 20.0, 0, "", None).changed {
            ck.shadow = s / 100.0;
            changed = true;
        }
    }
    if changed {
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.chroma = ck;
            }
        });
    }
    ui.add_space(6.0);
    ui.label(egui::RichText::new(tr("Removes a solid background colour (green screen).")).size(11.0).color(TEXT3()));
}

fn mask_section(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    section(ui, "Mask", None);
    let kinds: Vec<(Option<MaskKind>, &str, &str)> = vec![
        (None, "None", ph::PROHIBIT),
        (Some(MaskKind::Linear), "Linear", ph::MINUS),
        (Some(MaskKind::Mirror), "Mirror", ph::ARROWS_IN_LINE_HORIZONTAL),
        (Some(MaskKind::Circle), "Circle", ph::CIRCLE),
        (Some(MaskKind::Rectangle), "Rectangle", ph::RECTANGLE),
        (Some(MaskKind::Heart), "Heart", ph::HEART),
        (Some(MaskKind::Star), "Star", ph::STAR),
    ];
    let cur = c.mask.map(|m| m.kind);
    let w = ((ui.available_width() - 6.0 * 3.0) / 4.0).floor();
    for row in kinds.chunks(4) {
        ui.horizontal(|ui| {
            for (k, name, icon) in row {
                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 52.0), Sense::click());
                let sel = cur == *k;
                ui.painter().rect_filled(
                    r,
                    cr(6),
                    if sel {
                        ACCENT_BG()
                    } else if resp.hovered() {
                        PANEL3()
                    } else {
                        PANEL2()
                    },
                );
                if sel {
                    ui.painter().rect_stroke(r, cr(6), Stroke::new(1.0_f32, ACCENT()), StrokeKind::Inside);
                }
                ui.painter().text(
                    r.center() - Vec2::new(0.0, 8.0),
                    Align2::CENTER_CENTER,
                    *icon,
                    prop(17.0),
                    if sel { ACCENT() } else { TEXT() },
                );
                ui.painter().text(r.center() + Vec2::new(0.0, 13.0), Align2::CENTER_CENTER, tr(name), prop(10.5), TEXT2());
                if resp.clicked() {
                    let k = *k;
                    app.edit(|p| {
                        if let Some(c) = p.clip_mut(id) {
                            c.mask = k.map(|k| {
                                let mut m = c.mask.unwrap_or_default();
                                m.kind = k;
                                if k == MaskKind::Linear || k == MaskKind::Mirror {
                                    m.h = m.h.min(0.6);
                                }
                                m
                            });
                            if k.is_none() {
                                for pr in Prop::MASK {
                                    c.kf.clear(pr);
                                }
                            }
                        }
                    });
                }
            }
        });
    }
    let Some(m) = c.mask else { return };
    ui.add_space(4.0);
    let mut inv = m.invert;
    ui.horizontal(|ui| {
        ui.add_space(96.0);
        if toggle(ui, &mut inv).changed() {
            app.edit(|p| {
                if let Some(c) = p.clip_mut(id) {
                    if let Some(m) = c.mask.as_mut() {
                        m.invert = inv;
                    }
                }
            });
        }
        ui.label(egui::RichText::new(tr("Invert")).size(12.0).color(TEXT2()));
    });
    prop_slider(app, ui, id, "Position X", &[Prop::MaskX], 100.0, -100.0, 100.0, 0.0, 1, "%");
    prop_slider(app, ui, id, "Position Y", &[Prop::MaskY], 100.0, -100.0, 100.0, 0.0, 1, "%");
    if m.kind != MaskKind::Linear {
        if m.kind != MaskKind::Mirror {
            prop_slider(app, ui, id, "Width", &[Prop::MaskW], 100.0, 1.0, 200.0, 50.0, 0, "%");
        }
        prop_slider(app, ui, id, "Height", &[Prop::MaskH], 100.0, 1.0, 200.0, 50.0, 0, "%");
    }
    prop_slider(app, ui, id, "Rotate", &[Prop::MaskRotation], 1.0, -180.0, 180.0, 0.0, 0, "°");
    prop_slider(app, ui, id, "Feather", &[Prop::MaskFeather], 100.0, 0.0, 100.0, 5.0, 0, "");
    if m.kind == MaskKind::Rectangle {
        let mut r = m.round * 100.0;
        if slider_row(ui, "Round corners", &mut r, 0.0, 100.0, 0.0, 0, "", None).changed {
            app.edit(|p| {
                if let Some(c) = p.clip_mut(id) {
                    if let Some(m) = c.mask.as_mut() {
                        m.round = r / 100.0;
                    }
                }
            });
        }
    }
}

// ---------------------------------------------------------------- text / sticker

fn text_tab(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    let ClipKind::Text(td) = &c.kind else { return };
    let mut td = td.clone();
    let mut changed = false;
    if td.is_caption() {
        changed |= super::captions::inspector_section(app, ui, c, &mut td);
    }
    let focus = ui.memory_mut(|m| m.data.remove_temp::<bool>(egui::Id::new("focus-text")).unwrap_or(false));
    let r = ui.add(egui::TextEdit::multiline(&mut td.text).desired_rows(3).desired_width(ui.available_width()).hint_text(tr("Enter text")));
    if focus {
        r.request_focus();
    }
    changed |= r.changed();
    ui.add_space(4.0);
    // font: favourites, recently used and a preview of each font
    label_row(ui, "Font", |ui| {
        changed |= super::fonts::font_picker(app, ui, id, &mut td);
    });
    changed |= slider_row(ui, "Size", &mut td.size, 1.0, 120.0, 15.0, 1, "", None).changed;
    label_row(ui, "Style", |ui| {
        if ui.add(egui::Button::new(egui::RichText::new("B").strong()).selected(td.bold).min_size(Vec2::new(28.0, 22.0))).clicked() {
            td.bold = !td.bold;
            changed = true;
        }
        if ui.add(egui::Button::new(egui::RichText::new("I").italics()).selected(td.italic).min_size(Vec2::new(28.0, 22.0))).clicked() {
            td.italic = !td.italic;
            changed = true;
        }
        if ui.add(egui::Button::new(egui::RichText::new("U").underline()).selected(td.underline).min_size(Vec2::new(28.0, 22.0))).clicked()
        {
            td.underline = !td.underline;
            changed = true;
        }
        ui.add_space(8.0);
        for (i, icon) in [ph::TEXT_ALIGN_LEFT, ph::TEXT_ALIGN_CENTER, ph::TEXT_ALIGN_RIGHT].iter().enumerate() {
            if ui.add(egui::Button::new(*icon).selected(td.align as usize == i).min_size(Vec2::new(26.0, 22.0))).clicked() {
                td.align = i as u8;
                changed = true;
            }
        }
    });
    label_row(ui, "Color", |ui| {
        changed |= color_button(ui, egui::Id::new(("tcol", id)), &mut td.color, true);
    });
    changed |= slider_row(ui, "Character", &mut td.letter_spacing, -2.0, 10.0, 0.0, 1, "", None).changed;
    changed |= slider_row(ui, "Line", &mut td.line_spacing, -1.0, 4.0, 0.0, 1, "", None).changed;
    separator(ui);
    changed |= section(ui, "Stroke", Some(&mut td.stroke));
    if td.stroke {
        label_row(ui, "Color", |ui| {
            changed |= color_button(ui, egui::Id::new(("tstroke", id)), &mut td.stroke_color, true);
        });
        let mut w = td.stroke_width * 100.0;
        if slider_row(ui, "Width", &mut w, 1.0, 100.0, 40.0, 0, "", None).changed {
            td.stroke_width = w / 100.0;
            changed = true;
        }
    }
    separator(ui);
    changed |= section(ui, "Background", Some(&mut td.background));
    if td.background {
        label_row(ui, "Color", |ui| {
            changed |= color_button(ui, egui::Id::new(("tbg", id)), &mut td.bg_color, true);
        });
        let mut r = td.bg_round * 100.0;
        if slider_row(ui, "Round corners", &mut r, 0.0, 100.0, 30.0, 0, "", None).changed {
            td.bg_round = r / 100.0;
            changed = true;
        }
    }
    separator(ui);
    changed |= section(ui, "Shadow", Some(&mut td.shadow));
    if td.shadow {
        label_row(ui, "Color", |ui| {
            changed |= color_button(ui, egui::Id::new(("tsh", id)), &mut td.shadow_color, true);
        });
        let mut b = td.shadow_blur * 100.0;
        if slider_row(ui, "Blur", &mut b, 0.0, 100.0, 30.0, 0, "", None).changed {
            td.shadow_blur = b / 100.0;
            changed = true;
        }
        let mut d = td.shadow_distance * 100.0;
        if slider_row(ui, "Distance", &mut d, 0.0, 100.0, 30.0, 0, "", None).changed {
            td.shadow_distance = d / 100.0;
            changed = true;
        }
        changed |= slider_row(ui, "Angle", &mut td.shadow_angle, -180.0, 180.0, -45.0, 0, "°", None).changed;
    }
    if changed {
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.kind = ClipKind::Text(td.clone());
            }
        });
    }
    separator(ui);
    transform_section(app, ui, c);
    separator(ui);
    blend_section(app, ui, c);
    separator(ui);
    motion_blur_section(app, ui, c);
}

fn sticker_tab(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    let ClipKind::Sticker(sd) = &c.kind else { return };
    let is_emoji = catalog::sticker(&sd.id).and_then(|s| s.emoji).is_some();
    if !is_emoji {
        let mut col = sd.color;
        label_row(ui, "Color", |ui| {
            if color_button(ui, egui::Id::new(("stcol", id)), &mut col, true) {
                app.edit(|p| {
                    if let Some(c) = p.clip_mut(id) {
                        if let ClipKind::Sticker(s) = &mut c.kind {
                            s.color = col;
                        }
                    }
                });
            }
        });
        separator(ui);
    }
    transform_section(app, ui, c);
    separator(ui);
    blend_section(app, ui, c);
    separator(ui);
    motion_blur_section(app, ui, c);
}

// ---------------------------------------------------------------- effect / filter clips

fn effect_tab(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    let ClipKind::Effect(e) = &c.kind else { return };
    let def = catalog::effect(&e.id);
    ui.label(egui::RichText::new(def.map(|d| tr(d.name)).unwrap_or(&e.id)).font(bold(15.0)).color(Color32::WHITE));
    ui.add_space(6.0);
    let params: Vec<(usize, &'static str, f64)> =
        def.map(|d| d.params.iter().enumerate().map(|(i, p)| (i, p.name, p.default)).collect()).unwrap_or_default();
    if params.is_empty() {
        ui.label(egui::RichText::new(tr("This effect has no settings.")).color(TEXT3()));
    }
    for (i, name, default) in params {
        if i == 0 {
            prop_slider(app, ui, id, name, &[Prop::EffectAmount], 100.0, 0.0, 100.0, default * 100.0, 0, "");
        } else {
            let mut v = e.params[i] * 100.0;
            if slider_row(ui, name, &mut v, 0.0, 100.0, default * 100.0, 0, "", None).changed {
                app.edit(|p| {
                    if let Some(c) = p.clip_mut(id) {
                        if let ClipKind::Effect(e) = &mut c.kind {
                            e.params[i] = v / 100.0;
                        }
                    }
                });
            }
        }
    }
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tr("Effects apply to every track below this one.")).size(11.0).color(TEXT3()));
}

fn filter_clip_tab(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    let ClipKind::Filter(f) = &c.kind else { return };
    let mut fid = f.id.clone();
    let opts: Vec<(usize, String)> = catalog::FILTERS.iter().enumerate().map(|(i, d)| (i, tr(d.name).to_string())).collect();
    let mut idx = catalog::FILTERS.iter().position(|d| d.id == fid).unwrap_or(0);
    if combo_row(ui, "Filter", &mut idx, &opts) {
        fid = catalog::FILTERS[idx].id.to_string();
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                if let ClipKind::Filter(f) = &mut c.kind {
                    f.id = fid.clone();
                }
            }
        });
    }
    prop_slider(app, ui, id, "Intensity", &[Prop::EffectAmount], 100.0, 0.0, 100.0, 100.0, 0, "%");
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tr("Filter clips grade every track below them.")).size(11.0).color(TEXT3()));
}

// ---------------------------------------------------------------- audio

fn audio_tab(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    section(ui, "Basic", None);
    let t = app.playhead;
    let lin = c.prop(Prop::Volume, t);
    let mut db = if lin <= 0.0 { -60.0 } else { (20.0 * lin.log10()).clamp(-60.0, 20.0) };
    let kf = kf_state(c, &[Prop::Volume], t);
    let out = slider_row(ui, "Volume", &mut db, -60.0, 20.0, 0.0, 1, " dB", Some(kf));
    if out.changed {
        let v = if db <= -59.9 { 0.0 } else { 10f64.powf(db / 20.0) };
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.set_prop_at(Prop::Volume, t, v);
            }
        });
    }
    if out.kf_clicked {
        toggle_kf(app, id, &[Prop::Volume]);
    }
    let max_fade = util::us_to_s(c.duration) / 2.0;
    let mut fi = util::us_to_s(c.fade_in);
    if slider_row(ui, "Fade in", &mut fi, 0.0, max_fade.max(0.1), 0.0, 1, " s", None).changed {
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.fade_in = util::s_to_us(fi);
            }
        });
    }
    let mut fo = util::us_to_s(c.fade_out);
    if slider_row(ui, "Fade out", &mut fo, 0.0, max_fade.max(0.1), 0.0, 1, " s", None).changed {
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.fade_out = util::s_to_us(fo);
            }
        });
    }
    // ---- noise reduction
    separator(ui);
    let mut on = c.denoise > 0.0;
    if section(ui, "Reduce noise", Some(&mut on)) {
        app.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.denoise = if on { 0.5 } else { 0.0 };
            }
        });
    }
    if c.denoise > 0.0 {
        let mut k = c.denoise * 100.0;
        let out = slider_row(ui, "Strength", &mut k, 10.0, 100.0, 50.0, 0, "%", None);
        // whole tens: every strength is a filtered copy of the audio
        let k = ((k / 10.0).round() * 10.0).clamp(10.0, 100.0) / 100.0;
        if out.changed && (k - c.denoise).abs() > 1e-6 {
            app.edit(|p| {
                if let Some(c) = p.clip_mut(id) {
                    c.denoise = k;
                }
            });
        }
        ui.label(egui::RichText::new(tr("Removes steady background noise: hiss, hum, fans, wind.")).size(11.0).color(TEXT3()));
    }
    // ---- silence removal
    separator(ui);
    section(ui, "Remove silences", None);
    let mut db = app.settings.silence_db as f64;
    let mut changed = slider_row(ui, "Below", &mut db, -60.0, -15.0, -40.0, 0, " dB", None).changed;
    let mut min = app.settings.silence_min;
    changed |= slider_row(ui, "Min. length", &mut min, 0.1, 3.0, 0.5, 1, " s", None).changed;
    let mut pad = app.settings.silence_pad;
    changed |= slider_row(ui, "Margin", &mut pad, 0.0, 0.5, 0.15, 2, " s", None).changed;
    if changed {
        app.settings.silence_db = db as f32;
        app.settings.silence_min = min;
        app.settings.silence_pad = pad;
        app.settings.save();
    }
    let found = app.silences_of(id);
    ui.horizontal(|ui| {
        let (txt, n) = match &found {
            None => (tr("Reading the audio…").to_string(), 0),
            Some(v) => {
                let total: Us = v.iter().map(|(a, b)| b - a).sum();
                (format!("{} {}  •  {:.1} s", v.len(), tr("silent parts"), util::us_to_s(total)), v.len())
            }
        };
        ui.label(egui::RichText::new(txt).color(TEXT2()).size(11.5));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(n > 0, egui::Button::new(format!("{}  {}", ph::SCISSORS, tr("Remove")))).clicked() {
                app.remove_silences(id);
            }
        });
    });
    if found.is_none() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
    }
    separator(ui);
    section(ui, "Voice effects", None);
    let cur = c.voice.as_ref().map(|v| v.id.clone()).unwrap_or_default();
    let mut items: Vec<(&str, &str)> = vec![("", "None")];
    items.extend(catalog::VOICES.iter().map(|v| (v.id, v.name)));
    let w = ((ui.available_width() - 6.0 * 2.0) / 3.0).floor();
    for row in items.chunks(3) {
        ui.horizontal(|ui| {
            for (vid, name) in row {
                let sel = cur == *vid;
                let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 34.0), Sense::click());
                ui.painter().rect_filled(
                    r,
                    cr(6),
                    if sel {
                        ACCENT_BG()
                    } else if resp.hovered() {
                        PANEL3()
                    } else {
                        PANEL2()
                    },
                );
                if sel {
                    ui.painter().rect_stroke(r, cr(6), Stroke::new(1.0_f32, ACCENT()), StrokeKind::Inside);
                }
                ui.painter().text(r.center(), Align2::CENTER_CENTER, tr(name), prop(11.5), if sel { ACCENT() } else { TEXT() });
                if resp.clicked() {
                    let v = if vid.is_empty() { None } else { Some(VoiceFx { id: vid.to_string() }) };
                    app.edit(|p| {
                        if let Some(c) = p.clip_mut(id) {
                            c.voice = v;
                        }
                    });
                }
            }
        });
    }
    if app.project.track_of(id).map(|t| t.kind == TrackKind::Video).unwrap_or(false) {
        separator(ui);
        if ui
            .button(format!(
                "{}  {}",
                ph::WAVEFORM,
                if c.audio_detached { tr("Restore audio") } else { tr("Extract audio (Ctrl+Shift+S)") }
            ))
            .clicked()
        {
            app.edit(|p| p.toggle_extract_audio(id));
        }
    }
}

fn speed_tab(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    let mut mode = if c.curve.is_some() { 1 } else { 0 };
    if segmented(ui, &["Normal", "Curve"], &mut mode) {
        if mode == 0 {
            app.edit(|p| p.set_curve(id, None));
        } else {
            let cv = &catalog::SPEED_CURVES[0];
            let curve = SpeedCurve { name: cv.id.into(), points: cv.points.to_vec() };
            app.edit(|p| p.set_curve(id, Some(curve)));
        }
    }
    ui.add_space(8.0);
    if c.curve.is_none() {
        let mut sp = c.speed;
        if speed_row(ui, "Speed", &mut sp) {
            let sp = (sp * 100.0).round() / 100.0;
            app.edit(|p| p.set_speed(id, sp));
        }
        label_row(ui, "Duration", |ui| {
            ui.label(
                egui::RichText::new(format!("{}  {}  {}", util::short_dur(c.src_len()), ph::ARROW_RIGHT, util::short_dur(c.duration)))
                    .color(TEXT2())
                    .size(11.5),
            );
        });
        ui.horizontal(|ui| {
            for s in [0.5, 1.0, 1.5, 2.0, 4.0] {
                if ui.add(egui::Button::new(format!("{}x", s)).selected((c.speed - s).abs() < 1e-3)).clicked() {
                    app.edit(|p| p.set_speed(id, s));
                }
            }
        });
        ui.add_space(4.0);
        let mut cp = c.change_pitch;
        ui.horizontal(|ui| {
            if toggle(ui, &mut cp).changed() {
                app.edit(|p| {
                    if let Some(c) = p.clip_mut(id) {
                        c.change_pitch = cp;
                    }
                });
            }
            ui.label(tr("Change pitch"));
        });
    } else {
        let cur = c.curve.as_ref().map(|cv| cv.name.clone()).unwrap_or_default();
        let w = ((ui.available_width() - 6.0) / 2.0).floor();
        let curves: Vec<&catalog::CurveDef> = catalog::SPEED_CURVES.iter().collect();
        for row in curves.chunks(2) {
            ui.horizontal(|ui| {
                for cv in row {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 62.0), Sense::click());
                    let sel = cur == cv.id;
                    ui.painter().rect_filled(
                        r,
                        cr(6),
                        if sel {
                            ACCENT_BG()
                        } else if resp.hovered() {
                            PANEL3()
                        } else {
                            PANEL2()
                        },
                    );
                    if sel {
                        ui.painter().rect_stroke(r, cr(6), Stroke::new(1.0_f32, ACCENT()), StrokeKind::Inside);
                    }
                    // mini curve plot
                    let plot = Rect::from_min_max(r.min + Vec2::new(8.0, 8.0), Pos2::new(r.right() - 8.0, r.bottom() - 22.0));
                    let curve = SpeedCurve { name: String::new(), points: cv.points.to_vec() };
                    let pts: Vec<Pos2> = (0..=40)
                        .map(|i| {
                            let x = i as f64 / 40.0;
                            let s = curve_speed(&curve, x);
                            let y = (s.log10() + 1.0) / 2.0;
                            Pos2::new(plot.left() + x as f32 * plot.width(), plot.bottom() - y as f32 * plot.height())
                        })
                        .collect();
                    ui.painter().add(egui::Shape::line(pts, Stroke::new(1.5_f32, if sel { ACCENT() } else { TEXT2() })));
                    ui.painter().text(Pos2::new(r.center().x, r.bottom() - 10.0), Align2::CENTER_CENTER, tr(cv.name), prop(11.0), TEXT());
                    if resp.clicked() {
                        let curve = SpeedCurve { name: cv.id.into(), points: cv.points.to_vec() };
                        app.edit(|p| p.set_curve(id, Some(curve)));
                    }
                }
            });
        }
        ui.label(egui::RichText::new(tr("Curves speed up and slow down within the clip (velocity edits).")).size(11.0).color(TEXT3()));
    }
}

// ---------------------------------------------------------------- animation

fn animation_tab(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    let kid = egui::Id::new("anim-kind");
    let mut k: usize = ui.memory(|m| m.data.get_temp(kid)).unwrap_or(0);
    if segmented(ui, &["In", "Out", "Combo"], &mut k) {
        ui.memory_mut(|m| m.data.insert_temp(kid, k));
    }
    let kind = [AnimKind::In, AnimKind::Out, AnimKind::Combo][k.min(2)];
    let is_text = matches!(c.kind, ClipKind::Text(_));
    let cur = match kind {
        AnimKind::In => c.anim_in.clone(),
        AnimKind::Out => c.anim_out.clone(),
        AnimKind::Combo => c.anim_combo.clone(),
    };
    ui.add_space(6.0);
    let items: Vec<Option<&'static catalog::AnimDef>> =
        std::iter::once(None).chain(catalog::ANIMATIONS.iter().filter(|a| a.kind == kind && (!a.text_only || is_text)).map(Some)).collect();
    let cols = 3usize;
    let tw = ((ui.available_width() - 6.0 * (cols as f32 - 1.0)) / cols as f32).floor();
    let th = tw * 9.0 / 16.0;
    let mut budget = 4u32;
    for row in items.chunks(cols) {
        ui.horizontal(|ui| {
            for it in row {
                let sel = match (it, &cur) {
                    (None, None) => true,
                    (Some(d), Some(a)) => d.id == a.id,
                    _ => false,
                };
                match it {
                    None => {
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(tw, th + 18.0), Sense::click());
                        let img = Rect::from_min_size(r.min, Vec2::new(tw, th));
                        ui.painter().rect_filled(img, cr(6), PANEL2());
                        ui.painter().text(img.center(), Align2::CENTER_CENTER, ph::PROHIBIT, prop(20.0), TEXT3());
                        if sel {
                            ui.painter().rect_stroke(img, cr(6), Stroke::new(2.0_f32, ACCENT()), StrokeKind::Inside);
                        }
                        ui.painter().text(
                            Pos2::new(img.center().x, img.bottom() + 9.0),
                            Align2::CENTER_CENTER,
                            tr("None"),
                            prop(11.0),
                            TEXT2(),
                        );
                        if resp.clicked() {
                            app.edit(|p| {
                                if let Some(c) = p.clip_mut(id) {
                                    match kind {
                                        AnimKind::In => c.anim_in = None,
                                        AnimKind::Out => c.anim_out = None,
                                        AnimKind::Combo => c.anim_combo = None,
                                    }
                                }
                            });
                        }
                    }
                    Some(d) => {
                        let key = format!("an:{}", d.id);
                        let hov_id = egui::Id::new("anim-hover");
                        let hovered_last = ui.ctx().memory(|m| m.data.get_temp::<String>(hov_id)).map(|k| k == key).unwrap_or(false);
                        let tex = if hovered_last { app.tile_anim_texture(&key) } else { app.tile_texture(ui.ctx(), &key, &mut budget) };
                        let resp = tile(ui, Vec2::new(tw, th), tex, tr(d.name), sel, None, ph::FILM_STRIP);
                        if resp.hovered() {
                            ui.ctx().memory_mut(|m| m.data.insert_temp(hov_id, key.clone()));
                            ui.ctx().request_repaint();
                        } else if hovered_last {
                            ui.ctx().memory_mut(|m| m.data.remove::<String>(hov_id));
                        }
                        if resp.clicked() {
                            let au = AnimUse { id: d.id.into(), dur: d.dur.min(c.duration) };
                            app.edit(|p| {
                                if let Some(c) = p.clip_mut(id) {
                                    match kind {
                                        AnimKind::In => c.anim_in = Some(au),
                                        AnimKind::Out => c.anim_out = Some(au),
                                        AnimKind::Combo => c.anim_combo = Some(au),
                                    }
                                }
                            });
                            // jump to show it
                            let t = match kind {
                                AnimKind::In => c.start,
                                AnimKind::Out => (c.end() - d.dur.min(c.duration)).max(c.start),
                                AnimKind::Combo => c.start,
                            };
                            app.seek(t);
                        }
                    }
                }
            }
        });
        ui.add_space(4.0);
    }
    if let Some(a) = cur {
        separator(ui);
        let mut d = util::us_to_s(a.dur);
        let max = util::us_to_s(c.duration).max(0.1);
        let label = if kind == AnimKind::Combo { "Period" } else { "Duration" };
        if slider_row(ui, label, &mut d, 0.1, if kind == AnimKind::Combo { 10.0 } else { max }, 0.5, 1, " s", None).changed {
            let nd = util::s_to_us(d);
            app.edit(|p| {
                if let Some(c) = p.clip_mut(id) {
                    let slot = match kind {
                        AnimKind::In => &mut c.anim_in,
                        AnimKind::Out => &mut c.anim_out,
                        AnimKind::Combo => &mut c.anim_combo,
                    };
                    if let Some(a) = slot.as_mut() {
                        a.dur = nd;
                    }
                }
            });
        }
    }
}

// ---------------------------------------------------------------- adjust

fn adjust_tab(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip) {
    let id = c.id;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("Adjust")).font(bold(13.0)));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if icon_btn(ui, ph::ARROW_COUNTER_CLOCKWISE, tr("Reset"), true, false).clicked() {
                app.edit(|p| {
                    if let Some(c) = p.clip_mut(id) {
                        c.adjust = Adjust::default();
                        for pr in Prop::ADJUST {
                            c.kf.clear(pr);
                        }
                    }
                });
            }
            let kfs = kf_state(c, &Prop::ADJUST, app.playhead);
            if diamond(ui, kfs, tr("Keyframe all adjustments")).clicked() {
                toggle_kf(app, id, &Prop::ADJUST);
            }
        });
    });
    let groups: [(&str, &[(Prop, bool)]); 3] = [
        ("Color", &[(Prop::Temperature, true), (Prop::Tint, true), (Prop::Saturation, true), (Prop::Hue, true)]),
        (
            "Lightness",
            &[
                (Prop::Brightness, true),
                (Prop::Exposure, true),
                (Prop::Contrast, true),
                (Prop::Highlights, true),
                (Prop::Shadows, true),
                (Prop::Whites, true),
                (Prop::Blacks, true),
            ],
        ),
        ("Effects", &[(Prop::Sharpen, false), (Prop::Clarity, false), (Prop::Fade, false), (Prop::Vignette, true), (Prop::Grain, false)]),
    ];
    for (title, props) in groups {
        section(ui, title, None);
        for (p, bipolar) in props {
            let min = if *bipolar { -100.0 } else { 0.0 };
            prop_slider(app, ui, id, p.label(), &[*p], 100.0, min, 100.0, 0.0, 0, "");
        }
    }
    ui.add_space(6.0);
}
