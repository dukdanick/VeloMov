//! "New project" window: name, aspect ratio, resolution, frame rate and canvas colour,
//! chosen before editing starts (everything can still be changed later in Details).

use super::i18n::tr;
use super::theme::*;
use super::widgets::*;
use super::VeloApp;
use crate::model::{canvas_for_aspect, Ratio};
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as ph;

pub const RESOLUTIONS: [(u32, &str); 5] = [(480, "480p"), (720, "720p"), (1080, "1080p"), (1440, "2K"), (2160, "4K")];
pub const FRAME_RATES: [f64; 5] = [24.0, 25.0, 30.0, 50.0, 60.0];

/// Ratios offered when creating a project (Custom last).
const RATIOS: [Ratio; 12] = [
    Ratio::Original,
    Ratio::R16x9,
    Ratio::R9x16,
    Ratio::R1x1,
    Ratio::R4x5,
    Ratio::R4x3,
    Ratio::R3x4,
    Ratio::R21x9,
    Ratio::R235x1,
    Ratio::R2x1,
    Ratio::R185x1,
    Ratio::Custom,
];

pub struct NewProject {
    pub name: String,
    /// Shown when the name is left empty.
    pub default_name: String,
    pub ratio: Ratio,
    pub short: u32,
    pub custom: (u32, u32),
    pub fps: f64,
    pub bg: [u8; 4],
    /// Open the import dialog right after creating it.
    pub then_import: bool,
    focus: bool,
}

impl NewProject {
    pub fn new(default_name: String, then_import: bool) -> Self {
        NewProject {
            name: String::new(),
            default_name,
            ratio: Ratio::Original,
            short: 1080,
            custom: (1920, 1080),
            fps: 30.0,
            bg: [0, 0, 0, 255],
            then_import,
            focus: true,
        }
    }

    /// Canvas size this choice gives (`None` = follows the first video).
    pub fn size(&self) -> Option<(u32, u32)> {
        match self.ratio {
            Ratio::Original => None,
            Ratio::Custom => Some(self.custom),
            r => Some(canvas_for_aspect(r.aspect().unwrap_or(16.0 / 9.0), self.short)),
        }
    }
}

/// Small rectangle with the shape of the ratio.
fn ratio_icon(p: &egui::Painter, c: Pos2, ratio: Ratio, col: Color32) {
    let a = match ratio {
        Ratio::Original | Ratio::Custom => None,
        r => r.aspect(),
    };
    let max = 26.0;
    match a {
        Some(a) => {
            let (w, h) = if a >= 1.0 { (max, max / a as f32) } else { (max * a as f32, max) };
            p.rect_stroke(Rect::from_center_size(c, Vec2::new(w, h)), cr(3), Stroke::new(1.5_f32, col), StrokeKind::Inside);
        }
        None => {
            let icon = if ratio == Ratio::Custom { ph::PENCIL_SIMPLE } else { ph::FILM_STRIP };
            p.text(c, Align2::CENTER_CENTER, icon, prop(20.0), col);
        }
    }
}

fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tr(text)).color(TEXT2()).size(11.5));
    ui.add_space(2.0);
}

pub fn window(app: &mut VeloApp, ctx: &egui::Context) {
    let Some(mut d) = app.new_dialog.take() else { return };
    let mut create = false;
    let mut cancel = false;
    let modal = egui::Modal::new(egui::Id::new("new-project-modal")).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("New project")).font(bold(17.0)).color(Color32::WHITE));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_btn(ui, ph::X, tr("Close"), true, false).clicked() {
                    cancel = true;
                }
            });
        });
        // name
        section_label(ui, "Name");
        let resp = ui.add(egui::TextEdit::singleline(&mut d.name).hint_text(&d.default_name).desired_width(f32::INFINITY));
        if d.focus {
            resp.request_focus();
            d.focus = false;
        }
        // aspect ratio
        section_label(ui, "Aspect ratio");
        let cols = 6;
        let gap = 6.0;
        let cw = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).floor();
        for row in RATIOS.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for r in row {
                    let (rect, rr) = ui.allocate_exact_size(Vec2::new(cw, 66.0), Sense::click());
                    let on = d.ratio == *r;
                    let p = ui.painter();
                    p.rect_filled(
                        rect,
                        cr(8),
                        if on {
                            ACCENT_BG()
                        } else if rr.hovered() {
                            PANEL3()
                        } else {
                            PANEL2()
                        },
                    );
                    if on {
                        p.rect_stroke(rect, cr(8), Stroke::new(1.5_f32, ACCENT()), StrokeKind::Inside);
                    }
                    let col = if on { ACCENT() } else { TEXT() };
                    ratio_icon(p, Pos2::new(rect.center().x, rect.top() + 24.0), *r, col);
                    let label = match r {
                        Ratio::Original => tr("Auto"),
                        Ratio::Custom => tr("Custom"),
                        r => r.label(),
                    };
                    p.text(Pos2::new(rect.center().x, rect.bottom() - 12.0), Align2::CENTER_CENTER, label, prop(11.5), col);
                    let tip = match r {
                        Ratio::Original => tr("Same shape as the first video you add").to_string(),
                        Ratio::Custom => tr("Any width and height").to_string(),
                        r if !r.hint().is_empty() => r.hint().to_string(),
                        _ => String::new(),
                    };
                    let rr = if tip.is_empty() { rr } else { rr.on_hover_text(tip) };
                    if rr.clicked() {
                        d.ratio = *r;
                    }
                }
            });
            ui.add_space(gap);
        }
        // resolution
        section_label(ui, "Resolution");
        if d.ratio == Ratio::Custom {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut d.custom.0).range(16..=8192).speed(4).suffix(" px"));
                ui.label("×");
                ui.add(egui::DragValue::new(&mut d.custom.1).range(16..=8192).speed(4).suffix(" px"));
                ui.add_space(8.0);
                if ui.small_button(format!("{}  {}", ph::ARROWS_LEFT_RIGHT, tr("Swap"))).clicked() {
                    d.custom = (d.custom.1, d.custom.0);
                }
            });
            d.custom = ((d.custom.0 / 2) * 2, (d.custom.1 / 2) * 2);
        } else {
            let mut i = RESOLUTIONS.iter().position(|r| r.0 == d.short).unwrap_or(2);
            let names: Vec<&str> = RESOLUTIONS.iter().map(|r| r.1).collect();
            if segmented(ui, &names, &mut i) {
                d.short = RESOLUTIONS[i].0;
            }
        }
        // frame rate
        section_label(ui, "Frame rate");
        let mut fi = FRAME_RATES.iter().position(|f| *f == d.fps).unwrap_or(2);
        let names: Vec<String> = FRAME_RATES.iter().map(|f| format!("{} fps", f)).collect();
        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        if segmented(ui, &refs, &mut fi) {
            d.fps = FRAME_RATES[fi];
        }
        // canvas
        section_label(ui, "Background");
        ui.horizontal(|ui| {
            color_button(ui, egui::Id::new("new-bg"), &mut d.bg, false);
            ui.label(egui::RichText::new(tr("Shown around videos that don't fill the frame")).color(TEXT3()).size(11.0));
        });
        // summary + buttons
        ui.add_space(12.0);
        separator(ui);
        ui.horizontal(|ui| {
            let summary = match d.size() {
                Some((w, h)) => format!("{} × {}  •  {} fps", w, h, d.fps),
                None => format!("{} ({}p)  •  {} fps", tr("Follows the first video"), d.short, d.fps),
            };
            ui.label(egui::RichText::new(summary).color(TEXT2()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if grad_pill(ui, &mut app.brand, ph::PLUS, tr("Create project")).clicked() {
                    create = true;
                }
                if ui.button(tr("Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            create = true;
        }
    });
    if modal.should_close() {
        cancel = true;
    }
    if create {
        app.create_project(d);
    } else if !cancel {
        app.new_dialog = Some(d);
    }
}
