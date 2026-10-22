//! Settings window: appearance (theme, accent colour), general, performance and storage.

use super::i18n::tr;
use super::theme::{self, *};
use super::widgets::*;
use super::VeloApp;
use crate::util;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as ph;

const CACHE_DAYS: [u32; 6] = [0, 1, 3, 7, 14, 30];

fn cache_days_label(d: u32) -> String {
    match d {
        0 => tr("Never").to_string(),
        1 => tr("After 1 day unused").to_string(),
        d => format!("{} {} {}", tr("After"), d, tr("days unused")),
    }
}

pub fn fmt_bytes(b: u64) -> String {
    let b = b as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else {
        format!("{:.0} KB", b / 1e3)
    }
}

/// "Label   [control]" with a label column wide enough for Portuguese.
fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        let (lr, _) = ui.allocate_exact_size(Vec2::new(170.0, 22.0), Sense::hover());
        let g = elided(ui, tr(label), prop(12.0), TEXT2(), 166.0);
        ui.painter().galley(Pos2::new(lr.left(), lr.center().y - g.size().y / 2.0), g, TEXT2());
        add(ui);
    });
}

fn heading(ui: &mut egui::Ui, icon: &str, title: &str) {
    ui.add_space(10.0);
    ui.label(egui::RichText::new(format!("{}  {}", icon, tr(title))).font(bold(14.0)).color(Color32::WHITE));
    ui.add_space(4.0);
}

impl VeloApp {
    pub fn open_settings(&mut self) {
        self.show_settings = true;
        self.captions.engine = None;
        self.cache_size = Some(util::dir_size(&util::cache_dir()));
    }

    /// Apply a new theme / accent colour right away and remember it.
    pub fn set_theme(&mut self, ctx: &egui::Context, theme_id: &str, accent: [u8; 3]) {
        self.settings.theme = theme_id.to_string();
        self.settings.accent = accent;
        theme::set_palette(theme_id, accent);
        theme::apply_visuals(ctx);
        self.settings.save();
    }
}

pub fn window(app: &mut VeloApp, ctx: &egui::Context) {
    if !app.show_settings {
        return;
    }
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("settings-modal")).show(ctx, |ui| {
        ui.set_width(540.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("Settings")).font(bold(17.0)).color(Color32::WHITE));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_btn(ui, ph::X, tr("Close"), true, false).clicked() {
                    close = true;
                }
            });
        });
        let max_h = (ctx.screen_rect().height() - 140.0).max(240.0);
        egui::ScrollArea::vertical()
            .drag_to_scroll(false)
            .max_height(max_h)
            .min_scrolled_height(max_h.min(600.0))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                appearance(app, ui);
                separator(ui);
                general(app, ui);
                separator(ui);
                performance(app, ui);
                separator(ui);
                super::captions::settings_section(app, ui);
                separator(ui);
                storage(app, ui);
            });
    });
    if modal.should_close() || close {
        app.show_settings = false;
    }
}

fn appearance(app: &mut VeloApp, ui: &mut egui::Ui) {
    heading(ui, ph::PALETTE, "Appearance");
    ui.label(egui::RichText::new(tr("Theme")).color(TEXT2()).size(11.5));
    let ctx = ui.ctx().clone();
    // theme cards: a tiny drawing of the editor in each theme
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
        for (id, name, c) in THEMES {
            let col = |i: usize| Color32::from_rgb((c[i] >> 16) as u8, (c[i] >> 8) as u8, c[i] as u8);
            let (r, resp) = ui.allocate_exact_size(Vec2::new(96.0, 78.0), Sense::click());
            let pic = Rect::from_min_size(r.min, Vec2::new(96.0, 56.0));
            let p = ui.painter();
            p.rect_filled(pic, cr(8), col(0));
            // library | player | inspector and the timeline, as rounded cards
            let card = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(pic.min + Vec2::new(x0, y0), pic.min + Vec2::new(x1, y1));
            p.rect_filled(card(4.0, 4.0, 30.0, 32.0), cr(3), col(1));
            p.rect_filled(card(33.0, 4.0, 63.0, 32.0), cr(3), col(1));
            p.rect_filled(card(66.0, 4.0, 92.0, 32.0), cr(3), col(1));
            p.rect_filled(card(4.0, 35.0, 92.0, 52.0), cr(3), col(5));
            p.rect_filled(card(8.0, 40.0, 50.0, 46.0), cr(2), col(7));
            p.rect_filled(card(38.0, 9.0, 58.0, 25.0), cr(2), Color32::from_gray(10));
            p.rect_filled(card(8.0, 8.0, 18.0, 11.0), cr(1), ACCENT());
            let on = app.settings.theme == *id;
            let stroke = if on {
                Stroke::new(2.0_f32, ACCENT())
            } else if resp.hovered() {
                Stroke::new(1.0_f32, Color32::from_white_alpha(90))
            } else {
                Stroke::new(1.0_f32, Color32::from_white_alpha(25))
            };
            p.rect_stroke(pic, cr(8), stroke, StrokeKind::Inside);
            let g = elided(ui, tr(name), prop(11.5), if on { Color32::WHITE } else { TEXT2() }, 96.0);
            ui.painter().galley(Pos2::new(r.center().x - g.size().x / 2.0, pic.bottom() + 5.0), g, TEXT2());
            if resp.clicked() && !on {
                let accent = app.settings.accent;
                app.set_theme(&ctx, id, accent);
            }
        }
    });
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tr("Accent colour")).color(TEXT2()).size(11.5));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (name, c) in ACCENTS {
            let (r, resp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
            let on = app.settings.accent == *c;
            ui.painter().circle_filled(r.center(), 11.0, Color32::from_rgb(c[0], c[1], c[2]));
            if on {
                ui.painter().circle_stroke(r.center(), 13.0, Stroke::new(2.0_f32, Color32::WHITE));
            } else if resp.hovered() {
                ui.painter().circle_stroke(r.center(), 13.0, Stroke::new(1.0_f32, Color32::from_white_alpha(120)));
            }
            if resp.on_hover_text(tr(name)).clicked() && !on {
                let t = app.settings.theme.clone();
                app.set_theme(&ctx, &t, *c);
            }
        }
        ui.add_space(4.0);
        // any colour
        let mut c = app.settings.accent;
        let custom = !ACCENTS.iter().any(|a| a.1 == c);
        let resp = ui.color_edit_button_srgb(&mut c).on_hover_text(tr("Custom"));
        if custom {
            ui.painter().rect_stroke(resp.rect.expand(2.0), cr(5), Stroke::new(2.0_f32, Color32::WHITE), StrokeKind::Outside);
        }
        if c != app.settings.accent {
            let t = app.settings.theme.clone();
            app.set_theme(&ctx, &t, c);
        }
        ui.label(egui::RichText::new(tr("Custom")).color(TEXT3()).size(11.0));
    });
    ui.add_space(6.0);
    if ui.small_button(format!("{}  {}", ph::ARROW_COUNTER_CLOCKWISE, tr("Restore defaults"))).clicked() {
        app.set_theme(&ctx, theme::DEFAULT_THEME, theme::DEFAULT_ACCENT);
    }
}

fn general(app: &mut VeloApp, ui: &mut egui::Ui) {
    heading(ui, ph::GEAR, "General");
    let mut sel = match app.settings.lang.as_str() {
        "en" => 1usize,
        "pt" => 2,
        _ => 0,
    };
    let opts = [tr("System").to_string(), "English".to_string(), "Português".to_string()];
    row(ui, "Language", |ui| {
        egui::ComboBox::from_id_salt("lang").selected_text(opts[sel].clone()).width(200.0).show_ui(ui, |ui| {
            for (i, o) in opts.iter().enumerate() {
                if ui.selectable_value(&mut sel, i, o).clicked() {
                    app.set_lang(["auto", "en", "pt"][i]);
                }
            }
        });
    });
    row(ui, "Photo duration", |ui| {
        if ui.add(egui::DragValue::new(&mut app.settings.image_dur).range(0.5..=60.0).speed(0.1).suffix(" s")).changed() {
            app.settings.save();
        }
    });
    if ui.checkbox(&mut app.settings.autosave, tr("Auto-save projects")).changed() {
        app.settings.save();
    }
}

fn performance(app: &mut VeloApp, ui: &mut egui::Ui) {
    heading(ui, ph::GAUGE, "Performance");
    let q = app.settings.preview_quality.min(2) as usize;
    row(ui, "Preview quality", |ui| {
        egui::ComboBox::from_id_salt("pquality").selected_text(tr(super::player::QUALITY_NAMES[q])).width(200.0).show_ui(ui, |ui| {
            for (i, n) in super::player::QUALITY_NAMES.iter().enumerate() {
                if ui.selectable_label(i == q, tr(n)).clicked() && i != q {
                    app.set_preview_quality(i as u8);
                }
            }
        });
    });
    let hw_ok = cfg!(windows) || crate::media::video::hw_available();
    let hw_label = if cfg!(windows) { tr("Hardware decoding (GPU)") } else { tr("Hardware decoding (VA-API)") };
    let hw = ui.add_enabled(hw_ok, egui::Checkbox::new(&mut app.settings.hw_decode, hw_label));
    if hw.changed() {
        crate::media::video::set_hwaccel(app.settings.hw_decode);
        app.frames.video.stop_all();
        app.render_pending = true;
        app.settings.save();
    }
    if !hw_ok {
        hw.on_disabled_hover_text(tr("No VA-API device found (needs Mesa/VA-API drivers)"));
    }
    if ui
        .checkbox(&mut app.settings.auto_proxy, tr("Optimize heavy media (proxies)"))
        .on_hover_text(tr("Makes light copies of 4K / HEVC / 60 fps videos for a smooth preview. Exports use the originals."))
        .changed()
    {
        app.settings.save();
        app.request_proxies();
    }
}

fn storage(app: &mut VeloApp, ui: &mut egui::Ui) {
    heading(ui, ph::HARD_DRIVES, "Storage");
    ui.label(
        egui::RichText::new(tr(
            "The cache holds extracted audio, proxies, reversed clips and thumbnails. They are made again when needed.",
        ))
        .color(TEXT3())
        .size(11.0),
    );
    ui.add_space(4.0);
    row(ui, "Clean automatically", |ui| {
        let cur = app.settings.cache_days;
        egui::ComboBox::from_id_salt("cache-days").selected_text(cache_days_label(cur)).width(200.0).show_ui(ui, |ui| {
            for d in CACHE_DAYS {
                if ui.selectable_label(d == cur, cache_days_label(d)).clicked() && d != cur {
                    app.settings.cache_days = d;
                    app.settings.save();
                }
            }
        });
    });
    if app.settings.cache_days > 0 {
        ui.label(egui::RichText::new(tr("Checked every time VeloMov starts.")).color(TEXT3()).size(11.0));
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let size = app.cache_size.map(fmt_bytes).unwrap_or_else(|| "…".into());
        ui.label(egui::RichText::new(format!("{}: {}", tr("Cache size"), size)).color(TEXT()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(format!("{}  {}", ph::TRASH, tr("Clear cache now"))).clicked() {
                app.clear_cache();
            }
        });
    });
    let dir = util::cache_dir();
    ui.label(egui::RichText::new(dir.to_string_lossy()).color(TEXT3()).size(10.5));
}
