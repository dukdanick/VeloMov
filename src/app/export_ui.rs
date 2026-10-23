//! Top bar, export dialog/progress and the about window.

use super::i18n::tr;
use super::theme::*;
use super::widgets::*;
use super::{FilePurpose, VeloApp};
use crate::export::{self, AudioFmt, Codec, Quality};
use crate::util;
use eframe::egui::{self, Align2, Color32, Pos2, Sense, Vec2};
use egui_phosphor::regular as ph;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

pub struct ExportDialog {
    pub name: String,
    pub dir: PathBuf,
    pub res: u32,
    pub quality: Quality,
    pub codec: Codec,
    pub fps: f64,
    pub video: bool,
    pub audio: bool,
    pub audio_fmt: AudioFmt,
    pub use_range: bool,
}

impl ExportDialog {
    pub fn new(app: &VeloApp) -> Self {
        let s = &app.settings.export;
        let dir = s.path.parent().map(|p| p.to_path_buf()).filter(|p| p.is_dir()).unwrap_or_else(util::videos_dir);
        ExportDialog {
            name: sanitize(&app.project.name),
            dir,
            // the project's own resolution (nearest export option)
            res: [480u32, 720, 1080, 1440, 2160]
                .into_iter()
                .min_by_key(|r| (*r as i64 - app.project.short_side() as i64).abs())
                .unwrap_or(s.res),
            quality: s.quality,
            codec: if s.codec.available() { s.codec } else { Codec::H264 },
            fps: app.project.fps,
            video: true,
            audio: true,
            audio_fmt: s.audio_fmt,
            use_range: app.tl.in_out.0.is_some() && app.tl.in_out.1.is_some(),
        }
    }
    fn path(&self) -> PathBuf {
        let ext = if self.video { self.codec.ext() } else { self.audio_fmt.ext() };
        self.dir.join(format!("{}.{}", if self.name.trim().is_empty() { "velomov" } else { self.name.trim() }, ext))
    }
}

pub fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c == '/' || c == '\\' || c == ':' { '-' } else { c }).collect()
}

pub fn topbar(app: &mut VeloApp, ui: &mut egui::Ui) {
    let r = ui.max_rect();
    ui.horizontal_centered(|ui| {
        // logo + home
        let (lr, lresp) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::click());
        logo(ui, &mut app.brand, lr);
        if lresp.hovered() {
            ui.painter().rect_stroke(
                lr.expand(1.5),
                cr(9),
                egui::Stroke::new(1.0_f32, Color32::from_white_alpha(70)),
                egui::StrokeKind::Outside,
            );
        }
        if lresp.on_hover_text(tr("Back to home")).clicked() {
            app.back_home();
            return;
        }
        ui.add_space(6.0);
        ui.menu_button(format!("{} {}", tr("Menu"), ph::CARET_DOWN), |ui| {
            ui.set_min_width(240.0);
            let item = |ui: &mut egui::Ui, icon: &str, label: &str, key: &str| -> bool {
                ui.add(egui::Button::new(format!("{}  {}", icon, tr(label))).shortcut_text(key)).clicked()
            };
            if item(ui, ph::PLUS, "New project", "Ctrl+N") {
                app.open_new_project(false);
                ui.close_menu();
            }
            if item(ui, ph::FOLDER_OPEN, "Open project…", "Ctrl+O") {
                app.pick_files(FilePurpose::OpenProject);
                ui.close_menu();
            }
            if item(ui, ph::FLOPPY_DISK, "Save", "Ctrl+S") {
                app.save_project(false);
                ui.close_menu();
            }
            if item(ui, ph::FLOPPY_DISK_BACK, "Save as…", "Ctrl+Alt+S") {
                app.save_project_as();
                ui.close_menu();
            }
            if item(ui, ph::DOWNLOAD_SIMPLE, "Import media…", "Ctrl+I") {
                app.pick_files(FilePurpose::Import);
                ui.close_menu();
            }
            if item(ui, ph::EXPORT, "Export…", "Ctrl+E") {
                app.open_export();
                ui.close_menu();
            }
            ui.separator();
            if item(ui, ph::ARROW_COUNTER_CLOCKWISE, "Undo", "Ctrl+Z") {
                app.undo();
                ui.close_menu();
            }
            if item(ui, ph::ARROW_CLOCKWISE, "Redo", "Ctrl+Shift+Z") {
                app.redo();
                ui.close_menu();
            }
            ui.separator();
            if item(ui, ph::KEYBOARD, "Keyboard shortcuts", "?") {
                app.show_shortcuts = true;
                ui.close_menu();
            }
            if item(ui, ph::GEAR, "Settings", "") {
                app.open_settings();
                ui.close_menu();
            }
            if item(ui, ph::INFO, "About VeloMov", "") {
                app.show_about = true;
                ui.close_menu();
            }
            ui.separator();
            if item(ui, ph::HOUSE, "Back to home", "Ctrl+Alt+Q") {
                app.back_home();
                ui.close_menu();
            }
        });
        // centre: project name
        let name = app.project.name.clone();
        let status = if app.dirty { tr("Editing") } else { tr("Saved") };
        let center = Pos2::new(r.center().x, r.center().y);
        if let Some(mut edit) = app.rename_project.take() {
            let rr = egui::Rect::from_center_size(center, Vec2::new(240.0, 24.0));
            let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(rr));
            let resp = cui.add(egui::TextEdit::singleline(&mut edit).desired_width(240.0).horizontal_align(egui::Align::Center));
            resp.request_focus();
            if resp.lost_focus() || cui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let n = edit.trim().to_string();
                if !n.is_empty() {
                    app.edit(|p| p.name = n);
                }
            } else {
                app.rename_project = Some(edit);
            }
        } else {
            let g = ui.fonts(|f| f.layout_no_wrap(name.clone(), bold(13.0), Color32::WHITE));
            let nr = egui::Rect::from_center_size(center - Vec2::new(0.0, 6.0), g.size() + Vec2::new(16.0, 4.0));
            let resp = ui.interact(nr, ui.id().with("pname"), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(nr, cr(5), PANEL2());
            }
            ui.painter().galley(nr.min + Vec2::new(8.0, 2.0), g, Color32::WHITE);
            ui.painter().text(center + Vec2::new(0.0, 10.0), Align2::CENTER_CENTER, status, prop(10.0), TEXT3());
            if resp.on_hover_text(tr("Click to rename")).clicked() {
                app.rename_project = Some(name);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let busy = app.export_job.as_ref().map(|j| !j.done.load(Ordering::Relaxed)).unwrap_or(false);
            let label = if busy {
                format!("{} {:.0}%", tr("Exporting"), app.export_job.as_ref().unwrap().frac() * 100.0)
            } else {
                tr("Export").to_string()
            };
            if grad_pill(ui, &mut app.brand, ph::EXPORT, &label).on_hover_text("Ctrl+E").clicked() {
                if busy {
                    app.export_dialog = Some(ExportDialog::new(app));
                } else {
                    app.open_export();
                }
            }
            ui.add_space(6.0);
            if icon_btn(ui, ph::KEYBOARD, &format!("{} (?)", tr("Keyboard shortcuts")), true, app.show_shortcuts).clicked() {
                app.show_shortcuts = !app.show_shortcuts;
            }
            if app.pending_probes > 0 || app.audio.busy() || !app.proxy_jobs.is_empty() {
                ui.spinner();
            }
        });
    });
}

pub fn dialogs(app: &mut VeloApp, ctx: &egui::Context) {
    about_window(app, ctx);
    if app.export_dialog.is_none() {
        return;
    }
    let job = app.export_job.clone();
    let running = job.as_ref().map(|j| !j.done.load(Ordering::Relaxed)).unwrap_or(false);
    let mut close = false;
    let mut start = false;
    let modal = egui::Modal::new(egui::Id::new("export-modal")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("Export")).font(bold(17.0)).color(Color32::WHITE));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !running && icon_btn(ui, ph::X, tr("Close"), true, false).clicked() {
                    close = true;
                }
            });
        });
        ui.add_space(6.0);
        if let Some(j) = &job {
            if running || j.output.lock().unwrap().is_some() || j.error.lock().unwrap().is_some() {
                progress_ui(app, ui, j, &mut close);
                return;
            }
        }
        let d = app.export_dialog.as_mut().unwrap();
        ui.horizontal(|ui| {
            // cover preview
            let (cr_, _) = ui.allocate_exact_size(Vec2::new(176.0, 176.0 * 9.0 / 16.0), Sense::hover());
            ui.painter().rect_filled(cr_, cr(6), Color32::BLACK);
            if let Some(p) = &app.preview {
                let a = p.w as f32 / p.h.max(1) as f32;
                let sz = if a > 16.0 / 9.0 { Vec2::new(cr_.width(), cr_.width() / a) } else { Vec2::new(cr_.height() * a, cr_.height()) };
                ui.painter().image(
                    p.id,
                    egui::Rect::from_center_size(cr_.center(), sz),
                    egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            ui.add_space(10.0);
            ui.vertical(|ui| {
                label_row(ui, "Name", |ui| {
                    ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(250.0));
                });
                label_row(ui, "Export to", |ui| {
                    let s = d.dir.to_string_lossy().to_string();
                    // the home folder as "~" (like file managers), then the end of long paths
                    let home = util::home_dir();
                    let s = match d.dir.strip_prefix(&home) {
                        Ok(rest) if home.components().count() > 1 => {
                            let sep = std::path::MAIN_SEPARATOR;
                            if rest.as_os_str().is_empty() { "~".to_string() } else { format!("~{}{}", sep, rest.to_string_lossy()) }
                        }
                        _ => s,
                    };
                    let shown = if s.chars().count() > 34 {
                        format!("…{}", s.chars().rev().take(33).collect::<Vec<_>>().into_iter().rev().collect::<String>())
                    } else {
                        s.clone()
                    };
                    ui.add_sized(Vec2::new(220.0, 18.0), egui::Label::new(egui::RichText::new(shown).color(TEXT2()).size(11.5)).truncate())
                        .on_hover_text(&s);
                    if ui.small_button(ph::FOLDER_OPEN).clicked() {
                        app.pick_files_later = true;
                    }
                });
                label_row(ui, "Duration", |ui| {
                    ui.label(util::timecode(app.project.duration(), app.project.fps));
                });
            });
        });
        let d = app.export_dialog.as_mut().unwrap();
        ui.add_space(8.0);
        separator(ui);
        ui.horizontal(|ui| {
            ui.checkbox(&mut d.video, egui::RichText::new(tr("Video")).font(bold(13.5)));
        });
        if d.video {
            let res_opts: Vec<(u32, String)> =
                vec![(480, "480p".into()), (720, "720p".into()), (1080, "1080p".into()), (1440, "2K".into()), (2160, "4K".into())];
            combo_row(ui, "Resolution", &mut d.res, &res_opts);
            let q_opts: Vec<(Quality, String)> =
                [Quality::Lower, Quality::Recommended, Quality::Higher].iter().map(|q| (*q, tr(q.label()).to_string())).collect();
            combo_row(ui, "Bit rate", &mut d.quality, &q_opts);
            let c_opts: Vec<(Codec, String)> =
                Codec::ALL.iter().filter(|c| c.available()).map(|c| (*c, tr(c.label()).to_string())).collect();
            combo_row(ui, "Codec", &mut d.codec, &c_opts);
            label_row(ui, "Format", |ui| {
                ui.label(egui::RichText::new(d.codec.ext()).color(TEXT()));
            });
            let f_opts: Vec<(i32, String)> = [24, 25, 30, 50, 60].iter().map(|f| (*f, format!("{} fps", f))).collect();
            let mut fi = d.fps.round() as i32;
            if combo_row(ui, "Frame rate", &mut fi, &f_opts) {
                d.fps = fi as f64;
            }
            let (w, h) = export::output_size(&app.project, d.res);
            label_row(ui, "Size", |ui| {
                ui.label(egui::RichText::new(format!("{} × {}", w, h)).color(TEXT2()));
            });
        }
        separator(ui);
        ui.horizontal(|ui| {
            ui.checkbox(&mut d.audio, egui::RichText::new(tr("Audio")).font(bold(13.5)));
            if !d.video && d.audio {
                ui.label(egui::RichText::new(tr("(audio only)")).color(TEXT3()));
            }
        });
        if d.audio && !d.video {
            let a_opts: Vec<(AudioFmt, String)> = vec![
                (AudioFmt::Mp3, "MP3".into()),
                (AudioFmt::Aac, "AAC (m4a)".into()),
                (AudioFmt::Wav, "WAV".into()),
                (AudioFmt::Flac, "FLAC".into()),
            ];
            combo_row(ui, "Format", &mut d.audio_fmt, &a_opts);
        }
        if let (Some(i), Some(o)) = app.tl.in_out {
            if o > i {
                ui.checkbox(
                    &mut d.use_range,
                    format!(
                        "{} ({} – {})",
                        tr("Only the marked range (I/O)"),
                        util::timecode(i, app.project.fps),
                        util::timecode(o, app.project.fps)
                    ),
                );
            }
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let ok = d.video || d.audio;
                if ok {
                    if grad_pill(ui, &mut app.brand, ph::EXPORT, tr("Export")).clicked() {
                        start = true;
                    }
                } else {
                    ui.add_enabled(false, egui::Button::new(format!("{}  {}", ph::EXPORT, tr("Export"))).min_size(Vec2::new(110.0, 30.0)));
                }
                if ui.button(tr("Cancel")).clicked() {
                    close = true;
                }
            });
        });
    });
    if modal.should_close() && !running {
        close = true;
    }
    if app.pick_files_later {
        app.pick_files_later = false;
        app.pick_files(FilePurpose::ExportDir);
    }
    if start {
        let d = app.export_dialog.as_ref().unwrap();
        let path = d.path();
        let range = if d.use_range {
            match app.tl.in_out {
                (Some(i), Some(o)) if o > i => Some((i, o)),
                _ => None,
            }
        } else {
            None
        };
        let s = export::Settings {
            path: path.clone(),
            res: d.res,
            fps: d.fps,
            codec: d.codec,
            quality: d.quality,
            video: d.video,
            audio: d.audio,
            audio_fmt: d.audio_fmt,
            range,
        };
        app.settings.export = s.clone();
        app.settings.save();
        let dev = Some((app.rs.device.clone(), app.rs.queue.clone()));
        app.export_job = Some(export::spawn(app.project.clone(), s, dev));
    }
    if close {
        app.export_dialog = None;
        if !running {
            app.export_job = None;
        }
    }
    if running {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

fn progress_ui(app: &mut VeloApp, ui: &mut egui::Ui, j: &export::Progress, close: &mut bool) {
    let done = j.done.load(Ordering::Relaxed);
    let err = j.error.lock().unwrap().clone();
    let out = j.output.lock().unwrap().clone();
    if let Some(e) = err {
        ui.label(egui::RichText::new(format!("{}  {}", ph::WARNING_CIRCLE, tr("Export failed"))).font(bold(14.0)).color(DANGER));
        ui.label(egui::RichText::new(e).color(TEXT2()).size(11.5));
        ui.add_space(8.0);
        if ui.button(tr("Close")).clicked() {
            *close = true;
            app.export_job = None;
        }
        return;
    }
    if done {
        if let Some(p) = out {
            ui.label(egui::RichText::new(format!("{}  {}", ph::CHECK_CIRCLE, tr("Export complete"))).font(bold(15.0)).color(ACCENT()));
            ui.label(egui::RichText::new(p.to_string_lossy()).color(TEXT2()).size(11.5));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if pill(ui, ph::PLAY, tr("Play"), ACCENT(), ON_ACCENT()).clicked() {
                    crate::util::open_path(&p);
                }
                if pill(ui, ph::FOLDER_OPEN, tr("Open folder"), PANEL3(), Color32::WHITE).clicked() {
                    if let Some(d) = p.parent() {
                        crate::util::open_path(d);
                    }
                }
                if ui.button(tr("Done")).clicked() {
                    *close = true;
                    app.export_job = None;
                }
            });
        } else {
            ui.label(tr("Export cancelled"));
            if ui.button(tr("Close")).clicked() {
                *close = true;
                app.export_job = None;
            }
        }
        return;
    }
    let f = j.frac();
    let stage = j.stage.lock().unwrap().clone();
    ui.label(egui::RichText::new(format!("{}…", tr(&stage))).color(TEXT()));
    ui.add(egui::ProgressBar::new(f).show_percentage().desired_height(14.0));
    let fps = f32::from_bits(j.fps.load(Ordering::Relaxed));
    let eta = j.eta_s().map(|e| format!("  •  {} {:.0}s", tr("remaining"), e)).unwrap_or_default();
    ui.label(egui::RichText::new(format!("{:.1} fps{}", fps, eta)).color(TEXT3()).size(11.5));
    ui.add_space(8.0);
    if ui.button(tr("Cancel")).clicked() {
        j.cancel.store(true, Ordering::Relaxed);
    }
}

pub fn about_window(app: &mut VeloApp, ctx: &egui::Context) {
    if !app.show_about {
        return;
    }
    let mut open = true;
    egui::Window::new(tr("About VeloMov")).open(&mut open).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.horizontal(|ui| {
            let (lr, _) = ui.allocate_exact_size(Vec2::splat(48.0), Sense::hover());
            logo(ui, &mut app.brand, lr);
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("VeloMov").font(bold(20.0)));
                ui.label(egui::RichText::new(format!("{} {}", tr("Version"), env!("CARGO_PKG_VERSION"))).color(TEXT2()));
            });
        });
        ui.add_space(6.0);
        ui.label(tr("A lightweight video editor for Linux and Windows. GPU compositing (wgpu), FFmpeg decoding/encoding, keyframes, transitions with real motion blur, effects, filters, animated auto captions and keyboard shortcuts."));
        ui.add_space(6.0);
        ui.label(egui::RichText::new(tr("Free software under the GNU GPL v3 or later.")).color(TEXT3()).size(11.0));
        let dev = app.player.device_name.clone();
        ui.label(egui::RichText::new(format!("{}: {}", tr("Audio device"), if dev.is_empty() { "—".into() } else { dev })).color(TEXT3()).size(11.0));
        let info = app.rs.adapter.get_info();
        ui.label(egui::RichText::new(format!("GPU: {} ({:?})", info.name, info.backend)).color(TEXT3()).size(11.0));
    });
    if !open {
        app.show_about = false;
    }
}
