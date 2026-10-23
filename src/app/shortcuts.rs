//! Keyboard shortcuts and the
//! shortcut reference window.

use super::i18n::tr;
use super::theme::*;
use super::{LeftTab, RightTab, VeloApp};
use crate::util::SEC;
use eframe::egui::{self, Key, Modifiers};

#[derive(Clone, Copy)]
struct M {
    ctrl: bool,
    shift: bool,
    alt: bool,
}

fn mods(m: Modifiers) -> M {
    M { ctrl: m.ctrl || m.command, shift: m.shift, alt: m.alt }
}

/// Shortcut table used by the help window: (keys, action, standard key?)
pub const TABLE: &[(&str, &str, &[(&str, &str, bool)])] = &[
    (
        "Playback",
        "",
        &[
            ("Space", "Play / pause", true),
            ("J / K / L", "Shuttle backward / pause / forward", false),
            ("← / →", "Previous / next frame", true),
            ("Shift + ← / →", "Jump 10 frames", true),
            ("↑ / ↓", "Previous / next cut", true),
            ("Home / End", "Go to start / end", true),
            ("Ctrl+Shift+F", "Full screen preview", true),
            ("Esc", "Exit full screen / deselect", true),
        ],
    ),
    (
        "Edit",
        "",
        &[
            ("Ctrl+B", "Split selected clip at playhead", true),
            ("Ctrl+Shift+B", "Split all tracks at playhead", true),
            ("Q", "Delete left of playhead", true),
            ("W", "Delete right of playhead", true),
            ("Delete / Backspace", "Delete", true),
            ("Shift+Delete", "Ripple delete (close gap)", false),
            ("A / B", "Select mode / split mode", true),
            ("[ / ]", "Select clips left / right of playhead", true),
            ("V", "Enable / disable clip", true),
            ("Ctrl+G / Ctrl+Shift+G", "Group / ungroup", true),
            ("Ctrl+D", "Duplicate", false),
            ("F", "Freeze frame", false),
            ("Ctrl+Shift+R", "Reverse", false),
            ("R", "Rotate 90°", false),
            ("Alt + ← / →", "Nudge clip by one frame", false),
            ("Ctrl+Alt+C / V", "Copy / paste attributes", false),
        ],
    ),
    (
        "Keyframes & speed",
        "",
        &[
            ("Alt+K", "Add / remove keyframe", true),
            ("Alt+[ / Alt+]", "Previous / next keyframe", false),
            ("Ctrl+R", "Speed panel", true),
            ("Shift+B", "Curve speed", true),
            ("Ctrl+. / Ctrl+,", "Volume up / down", true),
            ("Ctrl+Shift+S", "Extract / restore audio", true),
            ("Shift+M", "Mute clip", false),
        ],
    ),
    (
        "Timeline",
        "",
        &[
            ("P", "Main track magnet", true),
            ("N", "Auto snapping", true),
            ("~", "Audio/video linkage", true),
            ("S", "Preview axis", true),
            ("Ctrl+= / Ctrl+-", "Zoom timeline", true),
            ("Ctrl+Scroll", "Zoom timeline", true),
            ("Shift+Z", "Fit timeline", true),
            ("M", "Add / remove marker", false),
            ("I / O", "Mark in / out (export range)", true),
            ("Alt+X", "Clear in / out", true),
        ],
    ),
    (
        "Project",
        "",
        &[
            ("Ctrl+N", "New project", true),
            ("Ctrl+I", "Import media", true),
            ("Ctrl+S", "Save", false),
            ("Ctrl+Alt+S", "Save as…", false),
            ("Ctrl+E", "Export", true),
            ("Ctrl+Z / Ctrl+Shift+Z", "Undo / redo", true),
            ("Ctrl+C / X / V", "Copy / cut / paste", true),
            ("Ctrl+A", "Select all", false),
            ("Ctrl+T", "Add text", false),
            ("Tab", "Next library tab", true),
            ("Ctrl+Alt+Q", "Back to home", true),
            ("? or F1", "Shortcut list", false),
        ],
    ),
];

pub fn handle(app: &mut VeloApp, ctx: &egui::Context) {
    if ctx.wants_keyboard_input() || app.export_dialog.is_some() || app.show_settings || app.new_dialog.is_some() {
        return;
    }
    let events: Vec<(Key, Modifiers)> = ctx.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => Some((*key, *modifiers)),
                _ => None,
            })
            .collect()
    });
    let text: Vec<String> =
        ctx.input(|i| i.events.iter().filter_map(|e| if let egui::Event::Text(t) = e { Some(t.clone()) } else { None }).collect());
    for t in text {
        if t == "?" {
            app.show_shortcuts = !app.show_shortcuts;
        }
        if t == "~" || t == "`" {
            let l = app.project.linkage;
            app.edit(|p| p.linkage = !l);
            app.toast(if l { tr("Linkage off") } else { tr("Linkage on") });
        }
    }
    for (key, m) in events {
        let m = mods(m);
        let fd = app.project.frame_dur();
        match (key, m.ctrl, m.shift, m.alt) {
            (Key::Space, false, false, false) => app.toggle_play(),
            (Key::J, false, false, false) => app.shuttle(-1),
            (Key::K, false, false, false) => app.shuttle(0),
            (Key::L, false, false, false) => app.shuttle(1),
            (Key::ArrowLeft, false, false, false) => {
                app.pause();
                app.seek(app.playhead - fd)
            }
            (Key::ArrowRight, false, false, false) => {
                app.pause();
                app.seek(app.playhead + fd)
            }
            (Key::ArrowLeft, false, true, false) => app.seek(app.playhead - fd * 10),
            (Key::ArrowRight, false, true, false) => app.seek(app.playhead + fd * 10),
            (Key::ArrowLeft, false, false, true) | (Key::ArrowRight, false, false, true) => {
                let d = if key == Key::ArrowLeft { -fd } else { fd };
                let ids = app.sel.clips.clone();
                for id in ids {
                    let Some(c) = app.project.clip(id).cloned() else { continue };
                    let ti = app.project.find_clip(id).map(|x| x.0);
                    app.edit(|p| p.move_clip(id, (c.start + d).max(0), ti, None));
                }
            }
            (Key::ArrowUp, false, false, false) => {
                if let Some(t) = app.project.edit_point(app.playhead, false) {
                    app.seek(t);
                }
            }
            (Key::ArrowDown, false, false, false) => {
                if let Some(t) = app.project.edit_point(app.playhead, true) {
                    app.seek(t);
                }
            }
            (Key::ArrowUp, true, false, false) | (Key::ArrowDown, true, false, false) => {
                if let Some((ti, _)) = app.sel.one().and_then(|id| app.project.find_clip(id)) {
                    let up = key == Key::ArrowUp;
                    app.edit(|p| p.move_track(ti, up));
                }
            }
            (Key::Home, false, false, false) => app.seek(0),
            (Key::End, false, false, false) => app.seek(app.project.duration()),
            (Key::B, true, false, false) => app.split(),
            (Key::B, true, true, false) => app.split_all(),
            (Key::B, false, false, false) => app.tl.split_mode = true,
            (Key::B, false, true, false) => {
                app.right_tab = RightTab::Speed;
                if let Some(id) = app.sel.one() {
                    if app.project.clip(id).map(|c| c.curve.is_none()).unwrap_or(false) {
                        let cv = &crate::model::catalog::SPEED_CURVES[0];
                        let curve = crate::model::SpeedCurve { name: cv.id.into(), points: cv.points.to_vec() };
                        app.edit(|p| p.set_curve(id, Some(curve)));
                    }
                }
            }
            (Key::A, false, false, false) => app.tl.split_mode = false,
            (Key::A, true, false, false) => app.select_all(),
            (Key::Q, false, false, false) => app.delete_left(),
            (Key::W, false, false, false) => app.delete_right(),
            (Key::Delete, false, false, false) | (Key::Backspace, false, false, false) => app.delete_selected(),
            (Key::Delete, false, true, false) | (Key::Backspace, false, true, false) => app.ripple_delete(),
            (Key::D, true, true, false) => app.ripple_delete(),
            (Key::D, true, false, false) => app.duplicate(),
            (Key::V, false, false, false) => app.toggle_enabled(),
            (Key::G, true, false, false) => {
                let ids = app.sel.clips.clone();
                app.edit(|p| p.group(&ids));
                app.toast(tr("Grouped"));
            }
            (Key::G, true, true, false) => {
                let ids = app.sel.clips.clone();
                app.edit(|p| p.ungroup(&ids));
                app.toast(tr("Ungrouped"));
            }
            (Key::G, false, _, true) => app.toast(tr("Compound clips are not supported yet")),
            (Key::OpenBracket, false, false, false) | (Key::CloseBracket, false, false, false) => {
                let t = app.playhead;
                let left = key == Key::OpenBracket;
                app.sel.clips = app
                    .project
                    .tracks
                    .iter()
                    .filter(|t| !t.locked)
                    .flat_map(|tr| tr.clips.iter())
                    .filter(|c| if left { c.start < t } else { c.end() > t })
                    .map(|c| c.id)
                    .collect();
                app.sel.transition = None;
            }
            (Key::OpenBracket, false, false, true) => app.jump_keyframe(false),
            (Key::CloseBracket, false, false, true) => app.jump_keyframe(true),
            (Key::F, false, false, false) => app.freeze(),
            (Key::F, true, true, false) => app.fullscreen = !app.fullscreen,
            (Key::Escape, _, _, _) => {
                if app.fullscreen {
                    app.fullscreen = false;
                } else if app.show_shortcuts {
                    app.show_shortcuts = false;
                } else {
                    app.sel = super::Selection::default();
                    app.junction = None;
                    app.tl.split_mode = false;
                }
            }
            (Key::R, false, false, false) => app.rotate90(),
            (Key::R, true, false, false) => app.right_tab = RightTab::Speed,
            (Key::R, true, true, false) => app.reverse(),
            (Key::K, false, false, true) => app.toggle_keyframe(),
            (Key::Period, true, false, false) => app.nudge_volume(1.0),
            (Key::Comma, true, false, false) => app.nudge_volume(-1.0),
            (Key::S, true, true, false) => {
                let ids = app.sel.clips.clone();
                app.edit(|p| {
                    for id in ids {
                        p.toggle_extract_audio(id);
                    }
                });
            }
            (Key::S, true, false, false) => app.save_project(false),
            (Key::S, true, false, true) => app.save_project_as(),
            (Key::S, false, false, false) => {
                let a = app.project.preview_axis;
                app.edit(|p| p.preview_axis = !a);
                app.toast(if a { tr("Preview axis off") } else { tr("Preview axis on") });
            }
            (Key::P, false, false, false) => {
                let m = app.project.magnet;
                app.edit(|p| {
                    p.magnet = !m;
                    p.normalize();
                });
                app.toast(if m { tr("Main track magnet off") } else { tr("Main track magnet on") });
            }
            (Key::N, false, false, false) => {
                let s = app.project.snapping;
                app.edit(|p| p.snapping = !s);
                app.toast(if s { tr("Snapping off") } else { tr("Snapping on") });
            }
            (Key::N, true, false, false) => {
                app.open_new_project(false);
            }
            (Key::I, true, false, false) => app.pick_files(super::FilePurpose::Import),
            (Key::I, false, false, false) => app.tl.in_out.0 = Some(app.playhead),
            (Key::O, false, false, false) => app.tl.in_out.1 = Some(app.playhead),
            (Key::O, true, false, false) => app.pick_files(super::FilePurpose::OpenProject),
            (Key::X, false, false, true) => app.tl.in_out = (None, None),
            (Key::Z, true, false, false) => app.undo(),
            (Key::Z, true, true, false) | (Key::Y, true, false, false) => app.redo(),
            (Key::Z, false, true, false) => app.tl.fit_request = true,
            (Key::C, true, false, false) => app.copy(),
            (Key::X, true, false, false) => app.cut(),
            (Key::V, true, false, false) => app.paste(),
            (Key::C, true, false, true) => {
                app.attr_clip = app.sel.one().and_then(|id| app.project.clip(id)).cloned();
                if app.attr_clip.is_some() {
                    app.toast(tr("Attributes copied"));
                }
            }
            (Key::V, true, false, true) => {
                if let Some(src) = app.attr_clip.clone() {
                    let ids = app.sel.clips.clone();
                    app.edit(|p| p.paste_attributes(&src, &ids));
                }
            }
            (Key::E, true, _, false) => app.open_export(),
            (Key::T, true, false, false) => app.add_text("default", None),
            (Key::M, false, false, false) => {
                let t = app.playhead;
                app.edit(|p| {
                    if let Some(i) = p.markers.iter().position(|m| (*m - t).abs() < 20_000) {
                        p.markers.remove(i);
                    } else {
                        p.markers.push(t);
                    }
                });
            }
            (Key::M, false, true, false) => app.toggle_mute_clip(),
            (Key::Equals, true, _, false) | (Key::Plus, true, _, false) => app.tl.pps = (app.tl.pps * 1.4).min(1500.0),
            (Key::Minus, true, _, false) => app.tl.pps = (app.tl.pps / 1.4).max(1.0),
            (Key::Tab, false, false, false) => {
                let i = LeftTab::ALL.iter().position(|t| *t == app.left_tab).unwrap_or(0);
                app.left_tab = LeftTab::ALL[(i + 1) % LeftTab::ALL.len()];
            }
            (Key::Q, true, false, true) => app.back_home(),
            (Key::Q, true, false, false) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            (Key::F1, _, _, _) => app.show_shortcuts = !app.show_shortcuts,
            _ => {}
        }
        let _ = SEC;
    }
}

pub fn help_window(app: &mut VeloApp, ctx: &egui::Context) {
    if !app.show_shortcuts {
        return;
    }
    let mut open = true;
    egui::Window::new(format!("{}  {}", egui_phosphor::regular::KEYBOARD, tr("Keyboard shortcuts")))
        .open(&mut open)
        .collapsible(false)
        .resizable(true)
        .default_size([760.0, 560.0])
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().drag_to_scroll(false).show(ui, |ui| {
                ui.columns(2, |cols| {
                    for (i, (group, _, rows)) in TABLE.iter().enumerate() {
                        let ui = &mut cols[if i < 2 { 0 } else { 1 }];
                        ui.label(egui::RichText::new(tr(group)).font(bold(13.5)).color(ACCENT()));
                        egui::Grid::new(("sc", i)).num_columns(2).spacing([14.0, 4.0]).show(ui, |ui| {
                            for (keys, action, _) in rows.iter() {
                                ui.label(egui::RichText::new(*keys).monospace().color(egui::Color32::WHITE));
                                ui.label(egui::RichText::new(tr(action)).color(TEXT2()));
                                ui.end_row();
                            }
                        });
                        ui.add_space(10.0);
                    }
                });
            });
        });
    if !open {
        app.show_shortcuts = false;
    }
}
