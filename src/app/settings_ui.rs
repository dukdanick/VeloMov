//! Settings window, in tabs: appearance (theme, accent, gradient, shape), layout (where the
//! panels go, interface size), general, performance, captions and storage.

use super::i18n::tr;
use super::theme::{self, *};
use super::widgets::*;
use super::VeloApp;
use crate::util;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as ph;

const CACHE_DAYS: [u32; 6] = [0, 1, 3, 7, 14, 30];
/// Interface sizes offered (zoom factor).
const SCALES: [f32; 8] = [0.8, 0.9, 1.0, 1.1, 1.2, 1.25, 1.35, 1.5];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tab {
    Appearance,
    Layout,
    General,
    Performance,
    Captions,
    Storage,
}

impl Tab {
    pub const ALL: [Tab; 6] = [Tab::Appearance, Tab::Layout, Tab::General, Tab::Performance, Tab::Captions, Tab::Storage];

    fn icon(self) -> &'static str {
        match self {
            Tab::Appearance => ph::PALETTE,
            Tab::Layout => ph::LAYOUT,
            Tab::General => ph::GEAR,
            Tab::Performance => ph::GAUGE,
            Tab::Captions => ph::SUBTITLES,
            Tab::Storage => ph::HARD_DRIVES,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tab::Appearance => "Appearance",
            Tab::Layout => "Layout",
            Tab::General => "General",
            Tab::Performance => "Performance",
            Tab::Captions => "Captions",
            Tab::Storage => "Storage",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Tab::Appearance => "Theme, colours, gradient and shape",
            Tab::Layout => "Where things go and how big they are",
            Tab::General => "Language, projects and fonts",
            Tab::Performance => "Preview and decoding",
            Tab::Captions => "Speech recognition",
            Tab::Storage => "Cache and disk space",
        }
    }
}

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

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(tr(text)).color(TEXT3()).size(11.0));
}

/// "Label   (toggle)" row; returns true when it was switched.
fn toggle_row(ui: &mut egui::Ui, label: &str, on: &mut bool) -> bool {
    let mut changed = false;
    row(ui, label, |ui| {
        changed = toggle(ui, on).changed();
    });
    changed
}

/// "Label   [a | b | c]" row; returns true when the choice changed.
fn seg_row(ui: &mut egui::Ui, label: &str, opts: &[&str], value: &mut u8) -> bool {
    let mut sel = *value as usize;
    let mut changed = false;
    row(ui, label, |ui| {
        let w = ui.available_width().min(80.0 * opts.len() as f32 + 20.0);
        ui.allocate_ui(Vec2::new(w, 26.0), |ui| {
            changed = segmented(ui, opts, &mut sel);
        });
    });
    if changed {
        *value = sel as u8;
    }
    changed
}

/// `seg_row` for a two-way choice kept in a bool (false = first option).
fn seg_bool(ui: &mut egui::Ui, label: &str, opts: [&str; 2], value: &mut bool) -> bool {
    let mut v = *value as u8;
    let changed = seg_row(ui, label, &opts, &mut v);
    *value = v != 0;
    changed
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

    /// Apply new interface preferences right away and remember them.
    pub fn set_ui(&mut self, ctx: &egui::Context, prefs: theme::UiPrefs) {
        self.settings.ui = prefs;
        theme::set_ui_prefs(prefs);
        theme::apply_visuals(ctx);
        theme::apply_scale(ctx);
        self.settings.save();
    }
}

pub fn window(app: &mut VeloApp, ctx: &egui::Context) {
    if !app.show_settings {
        return;
    }
    let mut close = false;
    let screen = ctx.screen_rect();
    let width = (screen.width() - 80.0).clamp(560.0, 860.0);
    let body_h = (screen.height() - 150.0).clamp(300.0, 640.0);
    let modal = egui::Modal::new(egui::Id::new("settings-modal")).show(ctx, |ui| {
        ui.set_width(width);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr("Settings")).font(bold(17.0)).color(Color32::WHITE));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_btn(ui, ph::X, tr("Close"), true, false).clicked() {
                    close = true;
                }
            });
        });
        ui.add_space(6.0);
        ui.horizontal_top(|ui| {
            // the tabs, down the left side
            let rail_w = 190.0;
            let (rail, _) = ui.allocate_exact_size(Vec2::new(rail_w, body_h), Sense::hover());
            ui.painter().rect_filled(rail, cr(CARD_R()), BG());
            let mut rui = ui.new_child(egui::UiBuilder::new().max_rect(rail.shrink(6.0)).layout(egui::Layout::top_down(egui::Align::Min)));
            rui.spacing_mut().item_spacing.y = 2.0;
            for tab in Tab::ALL {
                if tab_button(&mut rui, tab, app.settings_tab == tab, rail_w - 12.0).clicked() {
                    app.settings_tab = tab;
                    if tab == Tab::Storage {
                        app.cache_size = Some(util::dir_size(&util::cache_dir()));
                    }
                }
            }
            ui.add_space(16.0);
            ui.vertical(|ui| {
                let tab = app.settings_tab;
                ui.label(egui::RichText::new(tr(tab.name())).font(bold(16.0)).color(Color32::WHITE));
                hint(ui, tab.hint());
                separator(ui);
                let h = body_h - 52.0;
                egui::ScrollArea::vertical()
                    .id_salt(("settings-tab", tab as u8))
                    .drag_to_scroll(false)
                    .max_height(h)
                    .min_scrolled_height(h)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(ui.available_width() - 12.0);
                        match tab {
                            Tab::Appearance => appearance(app, ui),
                            Tab::Layout => layout(app, ui),
                            Tab::General => general(app, ui),
                            Tab::Performance => performance(app, ui),
                            Tab::Captions => super::captions::settings_section(app, ui),
                            Tab::Storage => storage(app, ui),
                        }
                        ui.add_space(12.0);
                    });
            });
        });
    });
    if modal.should_close() || close {
        app.show_settings = false;
    }
}

fn tab_button(ui: &mut egui::Ui, tab: Tab, active: bool, w: f32) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 38.0), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(r, cr(8), ACCENT_BG());
        p.rect_filled(Rect::from_min_size(Pos2::new(r.left(), r.top() + 10.0), Vec2::new(3.0, r.height() - 20.0)), cr(2), ACCENT());
    } else if resp.hovered() {
        p.rect_filled(r, cr(8), PANEL2());
    }
    p.text(
        Pos2::new(r.left() + 16.0, r.center().y),
        egui::Align2::LEFT_CENTER,
        tab.icon(),
        prop(16.0),
        if active { ACCENT() } else { TEXT2() },
    );
    let col = if active { Color32::WHITE } else { TEXT() };
    let g = elided(ui, tr(tab.name()), prop(13.0), col, r.width() - 48.0);
    ui.painter().galley(Pos2::new(r.left() + 40.0, r.center().y - g.size().y / 2.0), g, col);
    resp
}

// ---------------------------------------------------------------- appearance

fn appearance(app: &mut VeloApp, ui: &mut egui::Ui) {
    heading(ui, ph::PALETTE, "Theme");
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
    separator(ui);
    gradient(app, ui);
    separator(ui);
    shape(app, ui);
    ui.add_space(10.0);
    if ui.button(format!("{}  {}", ph::ARROW_COUNTER_CLOCKWISE, tr("Restore defaults"))).clicked() {
        app.set_theme(&ctx, theme::DEFAULT_THEME, theme::DEFAULT_ACCENT);
        let d = theme::DEFAULT_UI;
        let prefs = theme::UiPrefs {
            grad_mode: d.grad_mode,
            grad_hue: d.grad_hue,
            grad_a: d.grad_a,
            grad_b: d.grad_b,
            grad_dir: d.grad_dir,
            corner: d.corner,
            gap: d.gap,
            borders: d.borders,
            contrast: d.contrast,
            ..app.settings.ui
        };
        app.set_ui(&ctx, prefs);
    }
}

fn gradient(app: &mut VeloApp, ui: &mut egui::Ui) {
    heading(ui, ph::GRADIENT, "Gradient");
    hint(ui, "Used on the Export button, the home banner and other highlights.");
    ui.add_space(6.0);
    let ctx = ui.ctx().clone();
    let mut u = app.settings.ui;
    // preview: a bar filled with the gradient and a sample button
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 130.0).clamp(120.0, 420.0);
        let (r, _) = ui.allocate_exact_size(Vec2::new(w, 34.0), Sense::hover());
        let tex = app.brand.grad(&ctx);
        ui.painter().add(
            egui::epaint::RectShape::filled(r, cr(CARD_R()), Color32::WHITE)
                .with_texture(tex, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))),
        );
        ui.add_space(10.0);
        let _ = grad_pill(ui, &mut app.brand, ph::EXPORT, tr("Export"));
    });
    ui.add_space(6.0);
    let mut changed = seg_row(ui, "Style", &["Automatic", "Two colours", "Off"], &mut u.grad_mode);
    match u.grad_mode {
        0 => {
            row(ui, "Second colour", |ui| {
                changed |= ui
                    .add(egui::Slider::new(&mut u.grad_hue, -180.0..=180.0).step_by(5.0).fixed_decimals(0).suffix("°"))
                    .on_hover_text(tr("How far the second colour is from the accent on the colour wheel"))
                    .changed();
            });
        }
        1 => {
            row(ui, "Colours", |ui| {
                changed |= ui.color_edit_button_srgb(&mut u.grad_a).changed();
                ui.label(egui::RichText::new(ph::ARROW_RIGHT).color(TEXT3()));
                changed |= ui.color_edit_button_srgb(&mut u.grad_b).changed();
                ui.add_space(6.0);
                if ui.small_button(format!("{}  {}", ph::ARROWS_LEFT_RIGHT, tr("Swap"))).clicked() {
                    std::mem::swap(&mut u.grad_a, &mut u.grad_b);
                    changed = true;
                }
            });
        }
        _ => hint(ui, "The gradient is off: highlights use only the accent colour."),
    }
    if u.grad_mode == 1 && app.settings.ui.grad_mode != 1 {
        // switching to custom: start from the gradient that was showing
        let (a, b) = (GRAD_A(), GRAD_B());
        u.grad_a = [a.r(), a.g(), a.b()];
        u.grad_b = [b.r(), b.g(), b.b()];
    }
    if u.grad_mode != 2 {
        changed |= seg_row(ui, "Direction", &["Horizontal", "Diagonal", "Vertical"], &mut u.grad_dir);
    }
    if changed {
        app.set_ui(&ctx, u);
    }
}

fn shape(app: &mut VeloApp, ui: &mut egui::Ui) {
    heading(ui, ph::BOUNDING_BOX, "Shape and contrast");
    let ctx = ui.ctx().clone();
    let mut u = app.settings.ui;
    let mut changed = false;
    let mut corner = u.corner as f32;
    row(ui, "Rounded corners", |ui| {
        if ui.add(egui::Slider::new(&mut corner, 0.0..=20.0).step_by(1.0).fixed_decimals(0)).changed() {
            u.corner = corner as u8;
            changed = true;
        }
    });
    let mut gap = u.gap as f32;
    row(ui, "Space between panels", |ui| {
        if ui.add(egui::Slider::new(&mut gap, 0.0..=12.0).step_by(1.0).fixed_decimals(0)).changed() {
            u.gap = gap as u8;
            changed = true;
        }
    });
    changed |= toggle_row(ui, "Panel borders", &mut u.borders);
    changed |= toggle_row(ui, "High-contrast text", &mut u.contrast);
    if changed {
        app.set_ui(&ctx, u);
    }
}

// ---------------------------------------------------------------- layout

/// Little drawing of the editor with the chosen layout.
fn layout_picture(ui: &mut egui::Ui, u: &theme::UiPrefs) {
    let (r, _) = ui.allocate_exact_size(Vec2::new(200.0, 118.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, cr(8), BG());
    p.rect_stroke(r, cr(8), Stroke::new(1.0_f32, BORDER()), StrokeKind::Inside);
    let inner = r.shrink(6.0);
    let top = Rect::from_min_size(inner.min, Vec2::new(inner.width(), 8.0));
    p.rect_filled(top.shrink2(Vec2::new(0.0, 2.0)), cr(2), PANEL());
    let body = Rect::from_min_max(Pos2::new(inner.left(), top.bottom() + 3.0), inner.max);
    let tl_h = body.height() * 0.38;
    let (tl, upper) = if u.timeline_top {
        (
            Rect::from_min_size(body.min, Vec2::new(body.width(), tl_h)),
            Rect::from_min_max(Pos2::new(body.left(), body.top() + tl_h + 3.0), body.max),
        )
    } else {
        (
            Rect::from_min_max(Pos2::new(body.left(), body.bottom() - tl_h), body.max),
            Rect::from_min_max(body.min, Pos2::new(body.right(), body.bottom() - tl_h - 3.0)),
        )
    };
    p.rect_filled(tl, cr(4), TIMELINE_BG());
    for i in 0..3 {
        let y = tl.top() + 5.0 + i as f32 * (tl.height() - 8.0) / 3.0;
        let x0 = tl.left() + 8.0 + i as f32 * 14.0;
        let c = if i == 2 { CLIP_VIDEO } else { PANEL3() };
        p.rect_filled(Rect::from_min_size(Pos2::new(x0, y), Vec2::new(tl.width() * 0.45, 5.0)), cr(2), c);
    }
    let lib_w = upper.width() * 0.3;
    let insp_w = upper.width() * 0.25;
    let (lib, insp) = if u.library_right {
        (
            Rect::from_min_max(Pos2::new(upper.right() - lib_w, upper.top()), upper.max),
            Rect::from_min_size(upper.min, Vec2::new(insp_w, upper.height())),
        )
    } else {
        (
            Rect::from_min_size(upper.min, Vec2::new(lib_w, upper.height())),
            Rect::from_min_max(Pos2::new(upper.right() - insp_w, upper.top()), upper.max),
        )
    };
    let (l, rgt) = if u.library_right { (insp.right(), lib.left()) } else { (lib.right(), insp.left()) };
    let player = Rect::from_min_max(Pos2::new(l + 3.0, upper.top()), Pos2::new(rgt - 3.0, upper.bottom()));
    p.rect_filled(lib, cr(4), PANEL2());
    p.rect_filled(insp, cr(4), PANEL());
    p.rect_filled(player, cr(4), PANEL());
    p.rect_filled(player.shrink2(Vec2::new(8.0, 7.0)), cr(2), Color32::from_gray(8));
    // tiles in the library, sliders in the inspector
    let tw = (lib.width() - 12.0) / 2.0;
    for i in 0..4 {
        let (cx, cy) = ((i % 2) as f32, (i / 2) as f32);
        let tr = Rect::from_min_size(Pos2::new(lib.left() + 4.0 + cx * (tw + 4.0), lib.top() + 5.0 + cy * 12.0), Vec2::new(tw, 9.0));
        p.rect_filled(tr, cr(2), ACCENT().gamma_multiply(0.45));
    }
    for i in 0..3 {
        let y = insp.top() + 7.0 + i as f32 * 8.0;
        p.rect_filled(Rect::from_min_size(Pos2::new(insp.left() + 4.0, y), Vec2::new(insp.width() - 8.0, 3.0)), cr(1), PANEL3());
    }
}

fn layout(app: &mut VeloApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let mut u = app.settings.ui;
    let mut changed = false;
    heading(ui, ph::MAGNIFYING_GLASS_PLUS, "Interface size");
    let pct = |z: f32| format!("{:.0}%", z * 100.0);
    row(ui, "Size", |ui| {
        if ui.small_button(ph::MINUS).on_hover_text(tr("Smaller")).clicked() {
            u.scale = SCALES.iter().rev().find(|z| **z < u.scale - 0.001).copied().unwrap_or(SCALES[0]);
            changed = true;
        }
        egui::ComboBox::from_id_salt("ui-scale").selected_text(pct(u.scale)).width(90.0).show_ui(ui, |ui| {
            for z in SCALES {
                if ui.selectable_label((u.scale - z).abs() < 0.001, pct(z)).clicked() {
                    u.scale = z;
                    changed = true;
                }
            }
        });
        if ui.small_button(ph::PLUS).on_hover_text(tr("Bigger")).clicked() {
            u.scale = SCALES.iter().find(|z| **z > u.scale + 0.001).copied().unwrap_or(SCALES[SCALES.len() - 1]);
            changed = true;
        }
    });
    hint(ui, "Makes every button and text bigger or smaller (good for high-resolution screens).");
    separator(ui);
    heading(ui, ph::LAYOUT, "Editor");
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_max_width((ui.available_width() - 214.0).max(280.0));
            changed |= seg_bool(ui, "Media library", ["Left", "Right"], &mut u.library_right);
            changed |= seg_bool(ui, "Timeline", ["Bottom", "Top"], &mut u.timeline_top);
            changed |= seg_row(ui, "Track height", &["Compact", "Normal", "Large"], &mut u.track_size);
            hint(ui, "The inspector goes on the other side of the media library.");
        });
        ui.add_space(10.0);
        layout_picture(ui, &u);
    });
    separator(ui);
    heading(ui, ph::HOUSE, "Home screen");
    changed |= seg_row(ui, "Navigation", &["Top bar", "Left side", "Right side"], &mut u.home_nav);
    changed |= toggle_row(ui, "Welcome banner", &mut u.home_hero);
    changed |= seg_row(ui, "Projects", &["Grid", "List"], &mut u.home_view);
    if u.home_view == 0 {
        changed |= seg_row(ui, "Card size", &["Small", "Normal", "Large"], &mut u.home_card);
    }
    separator(ui);
    heading(ui, ph::SPARKLE, "Behaviour");
    changed |= seg_bool(ui, "Notifications", ["Top", "Bottom"], &mut u.toasts_bottom);
    changed |= toggle_row(ui, "Animations", &mut u.animations);
    hint(ui, "Smooth transitions in menus and the moving background of the home screen.");
    ui.add_space(10.0);
    if ui.button(format!("{}  {}", ph::ARROW_COUNTER_CLOCKWISE, tr("Restore defaults"))).clicked() {
        let d = theme::DEFAULT_UI;
        u = theme::UiPrefs {
            scale: d.scale,
            library_right: d.library_right,
            timeline_top: d.timeline_top,
            track_size: d.track_size,
            home_nav: d.home_nav,
            home_hero: d.home_hero,
            home_view: d.home_view,
            home_card: d.home_card,
            toasts_bottom: d.toasts_bottom,
            animations: d.animations,
            ..u
        };
        changed = true;
    }
    if changed {
        app.set_ui(&ctx, u);
    }
}

// ---------------------------------------------------------------- general

fn general(app: &mut VeloApp, ui: &mut egui::Ui) {
    heading(ui, ph::GLOBE, "Language");
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
    separator(ui);
    heading(ui, ph::FILM_SLATE, "Projects");
    row(ui, "Photo duration", |ui| {
        if ui.add(egui::DragValue::new(&mut app.settings.image_dur).range(0.5..=60.0).speed(0.1).suffix(" s")).changed() {
            app.settings.save();
        }
    });
    if toggle_row(ui, "Auto-save projects", &mut app.settings.autosave) {
        app.settings.save();
    }
    separator(ui);
    heading(ui, ph::TEXT_AA, "Text fonts");
    hint(ui, "Mark fonts with the star in the font list to keep them at the top. The last fonts you used show up there too.");
    ui.add_space(4.0);
    let favs = app.settings.fav_fonts.clone();
    row(ui, "Favourite fonts", |ui| {
        ui.label(egui::RichText::new(favs.len().to_string()).color(TEXT()));
        ui.add_space(8.0);
        if ui.add_enabled(!favs.is_empty(), egui::Button::new(tr("Clear list")).small()).clicked() {
            app.settings.fav_fonts.clear();
            app.settings.save();
        }
    });
    if !favs.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.add_space(174.0);
            for f in &favs {
                let r = ui.add(egui::Button::new(egui::RichText::new(format!("{}  {}", f, ph::X)).size(11.5)).small());
                if r.on_hover_text(tr("Remove from favourites")).clicked() {
                    app.toggle_fav_font(f);
                }
            }
        });
    }
    let recent = app.settings.recent_fonts.len();
    row(ui, "Recently used", |ui| {
        ui.label(egui::RichText::new(format!("{} / {}", recent, super::fonts::MAX_RECENT)).color(TEXT()));
        ui.add_space(8.0);
        if ui.add_enabled(recent > 0, egui::Button::new(tr("Clear list")).small()).clicked() {
            app.settings.recent_fonts.clear();
            app.settings.save();
        }
    });
}

// ---------------------------------------------------------------- performance

fn performance(app: &mut VeloApp, ui: &mut egui::Ui) {
    heading(ui, ph::MONITOR_PLAY, "Preview");
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
    if ui
        .checkbox(&mut app.settings.auto_proxy, tr("Optimize heavy media (proxies)"))
        .on_hover_text(tr("Makes light copies of 4K / HEVC / 60 fps videos for a smooth preview. Exports use the originals."))
        .changed()
    {
        app.settings.save();
        app.request_proxies();
    }
    hint(ui, "Makes light copies of 4K / HEVC / 60 fps videos for a smooth preview. Exports use the originals.");
    separator(ui);
    heading(ui, ph::CPU, "Decoding");
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
}

// ---------------------------------------------------------------- storage

fn storage(app: &mut VeloApp, ui: &mut egui::Ui) {
    heading(ui, ph::HARD_DRIVES, "Cache");
    hint(ui, "The cache holds extracted audio, proxies, reversed clips and thumbnails. They are made again when needed.");
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
        hint(ui, "Checked every time VeloMov starts.");
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
