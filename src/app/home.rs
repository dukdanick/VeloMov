//! Home screen: create / open projects.

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
    let mut v = vec![];
    if let Ok(rd) = std::fs::read_dir(util::projects_dir()) {
        for e in rd.flatten() {
            let p = e.path().join("project.vcut");
            if !p.exists() {
                continue;
            }
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
    }
    v.sort_by_key(|p| -p.modified);
    v
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
    if diff < 7 * 86400 {
        return format!("{} {}", diff / 86400, tr("days ago"));
    }
    let (y, m, d) = civil(ms / 86_400_000);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

fn hero_card(ui: &mut egui::Ui, size: Vec2, icon: &str, title: &str, subtitle: &str, grad: Option<egui::TextureId>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    let primary = grad.is_some();
    match grad {
        Some(t) => {
            // brand gradient, like the Export button
            p.add(
                egui::epaint::RectShape::filled(rect, cr(14), Color32::WHITE)
                    .with_texture(t, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))),
            );
            if resp.hovered() {
                p.rect_filled(rect, cr(14), Color32::from_white_alpha(22));
            }
        }
        None => {
            p.rect_filled(rect, cr(14), if resp.hovered() { PANEL3() } else { PANEL2() });
        }
    }
    let ic = Pos2::new(rect.left() + 46.0, rect.center().y);
    p.circle_filled(ic, 24.0, if primary { Color32::from_white_alpha(46) } else { PANEL3() });
    p.text(ic, Align2::CENTER_CENTER, icon, prop(24.0), if primary { ON_GRAD() } else { TEXT() });
    p.text(
        Pos2::new(rect.left() + 84.0, rect.center().y - 9.0),
        Align2::LEFT_CENTER,
        title,
        bold(16.0),
        if primary { ON_GRAD() } else { Color32::WHITE },
    );
    p.text(
        Pos2::new(rect.left() + 84.0, rect.center().y + 12.0),
        Align2::LEFT_CENTER,
        subtitle,
        prop(12.0),
        if primary { ON_GRAD().gamma_multiply(0.85) } else { TEXT2() },
    );
    resp
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
    egui::SidePanel::left("home-nav")
        .exact_width(214.0)
        .show_separator_line(false)
        .frame(egui::Frame::new().fill(BG()).inner_margin(egui::Margin::symmetric(14, 16)))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
                logo(ui, &mut app.brand, r);
                ui.add_space(2.0);
                ui.label(egui::RichText::new("VeloMov").font(bold(19.0)).color(Color32::WHITE));
            });
            ui.add_space(22.0);
            let nav = |ui: &mut egui::Ui, icon: &str, label: &str, active: bool| -> egui::Response {
                let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
                if active || resp.hovered() {
                    ui.painter().rect_filled(r, cr(8), if active { PANEL2() } else { PANEL() });
                }
                ui.painter().text(
                    Pos2::new(r.left() + 12.0, r.center().y),
                    Align2::LEFT_CENTER,
                    icon,
                    prop(16.0),
                    if active { ACCENT() } else { TEXT2() },
                );
                ui.painter().text(
                    Pos2::new(r.left() + 38.0, r.center().y),
                    Align2::LEFT_CENTER,
                    tr(label),
                    prop(13.0),
                    if active { Color32::WHITE } else { TEXT() },
                );
                resp
            };
            nav(ui, ph::HOUSE, "Home", true);
            if nav(ui, ph::FOLDER_OPEN, "Open project", false).clicked() {
                app.pick_files(FilePurpose::OpenProject);
            }
            if nav(ui, ph::GEAR, "Settings", app.show_settings).clicked() {
                app.open_settings();
            }
            if nav(ui, ph::INFO, "About", false).clicked() {
                app.show_about = true;
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                ui.label(egui::RichText::new(format!("v{}  •  GPL-3.0", env!("CARGO_PKG_VERSION"))).size(11.0).color(TEXT3()));
                ui.label(egui::RichText::new(tr("Open source video editor for Linux and Windows")).size(11.0).color(TEXT3()));
            });
        });
    let content = egui::Frame::new()
        .fill(PANEL())
        .corner_radius(CARD_R + 2)
        .outer_margin(egui::Margin { left: 0, right: 8, top: 8, bottom: 8 })
        .inner_margin(egui::Margin::symmetric(28, 22));
    egui::CentralPanel::default().frame(content).show(ctx, |ui| {
        ui.label(egui::RichText::new(tr("Start creating")).font(bold(22.0)).color(Color32::WHITE));
        ui.add_space(14.0);
        let avail = ui.available_width();
        let cw = ((avail - 24.0) / 3.0).clamp(220.0, 420.0);
        ui.horizontal(|ui| {
            let grad = Some(app.brand.grad(ui.ctx()));
            if hero_card(ui, Vec2::new(cw, 96.0), ph::PLUS, tr("Create project"), tr("Start a new edit"), grad).clicked() {
                app.open_new_project(false);
            }
            ui.add_space(8.0);
            if hero_card(ui, Vec2::new(cw, 96.0), ph::DOWNLOAD_SIMPLE, tr("Import media"), tr("New project from files"), None).clicked() {
                app.open_new_project(true);
            }
            ui.add_space(8.0);
            let busy = app.home.demo_rx.is_some();
            let sub = if busy { tr("Generating demo media…") } else { tr("Transitions, keyframes, effects") };
            if hero_card(ui, Vec2::new(cw, 96.0), ph::SPARKLE, tr("Demo project"), sub, None).clicked() && !busy {
                let (tx, rx) = std::sync::mpsc::channel();
                app.home.demo_rx = Some(rx);
                std::thread::spawn(move || {
                    let dir = util::data_dir().join("demo-media");
                    let r = crate::demo::make_demo(&dir).and_then(|mut p| {
                        p.name = "VeloMov demo".into();
                        let path = util::projects_dir().join(&p.id).join("project.vcut");
                        p.save(&path)?;
                        Ok(path)
                    });
                    let _ = tx.send(r.map_err(|e| e.to_string()));
                });
            }
        });
        ui.add_space(26.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{} ({})", tr("Projects"), app.home.projects.len())).font(bold(16.0)).color(Color32::WHITE),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut app.home.search)
                        .hint_text(format!("{}  {}", ph::MAGNIFYING_GLASS_PLUS, tr("Search projects")))
                        .desired_width(220.0),
                );
                if !app.home.projects.is_empty() && !app.home.select_mode && app.home.selected.is_empty() {
                    if ui.button(format!("{}  {}", ph::CHECK_SQUARE, tr("Select"))).clicked() {
                        app.home.select_mode = true;
                    }
                }
            });
        });
        selection_bar(app, ui);
        ui.add_space(10.0);
        if app.home.projects.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(ph::FILM_STRIP).size(40.0).color(TEXT3()));
                ui.label(egui::RichText::new(tr("No projects yet. Create one or drop video files here.")).color(TEXT3()));
            });
            return;
        }
        let search = app.home.search.to_lowercase();
        let list: Vec<ProjectSummary> =
            app.home.projects.iter().filter(|p| search.is_empty() || p.name.to_lowercase().contains(&search)).cloned().collect();
        egui::ScrollArea::vertical().drag_to_scroll(false).auto_shrink([false, false]).show(ui, |ui| {
            let card_w = 214.0;
            let cols = ((ui.available_width() + 14.0) / (card_w + 14.0)).floor().max(1.0) as usize;
            for row in list.chunks(cols) {
                ui.horizontal(|ui| {
                    for p in row {
                        project_card(app, ui, p, card_w);
                        ui.add_space(14.0);
                    }
                });
                ui.add_space(12.0);
            }
        });
    });
    // dialogs
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
                        if let Some(dir) = path.parent() {
                            let _ = std::fs::remove_dir_all(dir);
                        }
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
    super::export_ui::about_window(app, ctx);
}

fn project_card(app: &mut VeloApp, ui: &mut egui::Ui, p: &ProjectSummary, w: f32) {
    let h = w * 9.0 / 16.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h + 44.0), Sense::click());
    let img = Rect::from_min_size(rect.min, Vec2::new(w, h));
    let painter = ui.painter();
    painter.rect_filled(img, cr(10), PANEL2());
    // cover
    let cover = app.home.covers.entry(p.path.clone()).or_insert_with(|| {
        let cp = p.path.parent()?.join("cover.png");
        let im = image::open(cp).ok()?.to_rgba8();
        let ci = egui::ColorImage::from_rgba_unmultiplied([im.width() as usize, im.height() as usize], im.as_raw());
        Some(ui.ctx().load_texture(format!("cover-{}", p.path.display()), ci, egui::TextureOptions::LINEAR))
    });
    if let Some(t) = cover {
        let sz = t.size_vec2();
        let k = (img.width() / sz.x).min(img.height() / sz.y);
        let r = Rect::from_center_size(img.center(), sz * k);
        painter.add(
            egui::epaint::RectShape::filled(r, cr(10), Color32::WHITE)
                .with_texture(t.id(), Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))),
        );
    } else {
        painter.text(img.center(), Align2::CENTER_CENTER, ph::FILM_STRIP, prop(28.0), TEXT3());
    }
    let selected = app.home.selected.contains(&p.path);
    let selecting = app.home.select_mode || !app.home.selected.is_empty();
    if selected {
        painter.rect_filled(img, cr(10), ACCENT().gamma_multiply(0.12));
        painter.rect_stroke(img, cr(10), Stroke::new(2.5_f32, ACCENT()), StrokeKind::Inside);
    } else if resp.hovered() {
        painter.rect_stroke(img, cr(10), Stroke::new(1.5_f32, ACCENT()), StrokeKind::Inside);
    }
    // round check in the corner (shown while selecting, or on hover)
    let check = Pos2::new(img.left() + 16.0, img.top() + 16.0);
    let check_hit = ui.input(|i| i.pointer.hover_pos()).map(|h| h.distance(check) < 13.0).unwrap_or(false);
    if selecting || resp.hovered() {
        let p = ui.painter();
        if selected {
            p.circle_filled(check, 10.0, ACCENT());
            p.text(check, Align2::CENTER_CENTER, ph::CHECK, bold(12.0), ON_ACCENT());
        } else {
            p.circle_filled(check, 10.0, Color32::from_black_alpha(if check_hit { 200 } else { 140 }));
            p.circle_stroke(check, 10.0, Stroke::new(1.5_f32, Color32::from_white_alpha(220)));
        }
    }
    let dur = util::timecode(p.duration, 30.0);
    let g = ui.fonts(|f| f.layout_no_wrap(dur[3..8].to_string(), prop(10.5), Color32::WHITE));
    let br =
        Rect::from_min_size(Pos2::new(img.right() - g.size().x - 12.0, img.bottom() - g.size().y - 8.0), g.size() + Vec2::new(6.0, 2.0));
    painter.rect_filled(br, cr(3), Color32::from_black_alpha(170));
    painter.galley(br.min + Vec2::new(3.0, 1.0), g, Color32::WHITE);
    let name = elided(ui, &p.name, prop(13.0), Color32::WHITE, w - 30.0);
    ui.painter().galley(Pos2::new(rect.left() + 2.0, img.bottom() + 6.0), name, Color32::WHITE);
    ui.painter().text(
        Pos2::new(rect.left() + 2.0, img.bottom() + 32.0),
        Align2::LEFT_CENTER,
        format!("{}  •  {}", fmt_date(p.modified), p.ratio),
        prop(11.0),
        TEXT3(),
    );
    if resp.clicked() {
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
    let path = p.path.clone();
    let name = p.name.clone();
    resp.context_menu(|ui| {
        if ui.button(format!("{}  {}", ph::FOLDER_OPEN, tr("Open"))).clicked() {
            if let Err(e) = app.open_project(&path) {
                app.toast_err(&e.to_string());
            }
            ui.close_menu();
        }
        if ui.button(format!("{}  {}", ph::TAG, tr("Rename"))).clicked() {
            app.home.renaming = Some((path.clone(), name.clone()));
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
    });
}

/// Copy a project under a new id ("<name> copy"), cover included.
pub fn duplicate_project(path: &std::path::Path) {
    if let Ok(mut pj) = Project::load(path) {
        pj.id = format!("{:x}", util::new_id());
        pj.name = format!("{} {}", pj.name, tr("copy"));
        let np = util::projects_dir().join(&pj.id).join("project.vcut");
        let _ = pj.save(&np);
        if let (Some(a), Some(b)) = (path.parent(), np.parent()) {
            let _ = std::fs::copy(a.join("cover.png"), b.join("cover.png"));
        }
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
