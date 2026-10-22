//! Left panel: media, audio, text, stickers, effects, transitions, filters, adjustment.

use super::i18n::tr;
use super::theme::*;
use super::widgets::*;
use super::{DragItem, FilePurpose, LeftTab, VeloApp};
use crate::model::catalog::{self, ECat, FCat, TCat};
use crate::model::edit::Target as Place;
use crate::model::{Adjust, MediaKind};
use crate::util;
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as ph;

const A0: Adjust = Adjust {
    brightness: 0.0,
    contrast: 0.0,
    saturation: 0.0,
    exposure: 0.0,
    temperature: 0.0,
    tint: 0.0,
    highlights: 0.0,
    shadows: 0.0,
    whites: 0.0,
    blacks: 0.0,
    sharpen: 0.0,
    clarity: 0.0,
    fade: 0.0,
    vignette: 0.0,
    grain: 0.0,
    hue: 0.0,
};

pub const ADJUST_PRESETS: &[(&str, Adjust)] = &[
    ("Adjustment layer", A0),
    ("Punchy", Adjust { contrast: 0.25, saturation: 0.25, sharpen: 0.3, clarity: 0.2, ..A0 }),
    ("Warm glow", Adjust { temperature: 0.35, highlights: -0.15, brightness: 0.08, ..A0 }),
    ("Cool tone", Adjust { temperature: -0.35, tint: 0.05, contrast: 0.1, ..A0 }),
    ("Soft matte", Adjust { fade: 0.35, contrast: -0.15, saturation: -0.1, ..A0 }),
    ("Moody dark", Adjust { exposure: -0.2, contrast: 0.3, saturation: -0.25, vignette: 0.45, ..A0 }),
    ("Bright & airy", Adjust { exposure: 0.25, shadows: 0.35, highlights: -0.25, saturation: 0.05, ..A0 }),
    ("Film grain", Adjust { grain: 0.5, fade: 0.15, vignette: 0.25, ..A0 }),
];

fn tab_info(t: LeftTab) -> (&'static str, &'static str) {
    match t {
        LeftTab::Media => (ph::FILM_STRIP, "Media"),
        LeftTab::Audio => (ph::MUSIC_NOTES, "Audio"),
        LeftTab::Text => (ph::TEXT_T, "Text"),
        LeftTab::Captions => (ph::SUBTITLES, "Captions"),
        LeftTab::Stickers => (ph::STICKER, "Stickers"),
        LeftTab::Effects => (ph::SPARKLE, "Effects"),
        LeftTab::Transitions => (ph::SWAP, "Transitions"),
        LeftTab::Filters => (ph::CIRCLE_HALF, "Filters"),
        LeftTab::Adjust => (ph::SLIDERS_HORIZONTAL, "Adjustment"),
    }
}

fn categories(t: LeftTab) -> Vec<&'static str> {
    match t {
        LeftTab::Media => vec!["Import", "Samples"],
        LeftTab::Audio => vec!["Sound effects", "Imported"],
        LeftTab::Text => vec!["Add text", "Styles"],
        LeftTab::Captions => vec!["Auto captions", "Styles"],
        LeftTab::Stickers => vec!["Shapes", "Emoji"],
        LeftTab::Effects => ECat::ALL.iter().map(|c| c.label()).collect(),
        LeftTab::Transitions => TCat::ALL.iter().map(|c| c.label()).collect(),
        LeftTab::Filters => FCat::ALL.iter().map(|c| c.label()).collect(),
        LeftTab::Adjust => vec!["Adjustment"],
    }
}

pub fn ui(app: &mut VeloApp, ui: &mut egui::Ui) {
    // ---- tab strip
    let full = ui.available_rect_before_wrap();
    let strip_h = 54.0;
    let strip = Rect::from_min_size(full.min, Vec2::new(full.width(), strip_h));
    let n = LeftTab::ALL.len() as f32;
    let inner = strip.shrink2(Vec2::new(6.0, 0.0));
    let tw = inner.width() / n;
    for (i, t) in LeftTab::ALL.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(inner.left() + i as f32 * tw, strip.top() + 5.0), Vec2::new(tw, strip_h - 9.0));
        let resp = ui.interact(r, ui.id().with(("ltab", i)), Sense::click());
        let active = app.left_tab == *t;
        let (icon, label) = tab_info(*t);
        if resp.hovered() && !active {
            ui.painter().rect_filled(r.shrink2(Vec2::new(2.0, 0.0)), cr(8), Color32::from_white_alpha(6));
        }
        let col = if active {
            ACCENT()
        } else if resp.hovered() {
            Color32::WHITE
        } else {
            TEXT2()
        };
        ui.painter().text(Pos2::new(r.center().x, r.top() + 14.0), Align2::CENTER_CENTER, icon, prop(18.0), col);
        let g = elided(ui, tr(label), prop(10.5), if active { Color32::WHITE } else { col }, tw + 2.0);
        ui.painter().galley(Pos2::new(r.center().x - g.size().x / 2.0, r.top() + 27.0), g, col);
        if active {
            let bar = Rect::from_center_size(Pos2::new(r.center().x, strip.bottom() - 1.5), Vec2::new(18.0, 3.0));
            ui.painter().rect_filled(bar, cr(2), ACCENT());
        }
        if resp.clicked() {
            app.left_tab = *t;
        }
    }
    ui.painter().line_segment(
        [Pos2::new(strip.left() + 10.0, strip.bottom()), Pos2::new(strip.right() - 10.0, strip.bottom())],
        Stroke::new(1.0_f32, BORDER()),
    );
    ui.allocate_rect(strip, Sense::hover());
    // ---- body
    let body = Rect::from_min_max(Pos2::new(full.left(), strip.bottom()), full.max);
    let cat_w = 96.0;
    let cats = categories(app.left_tab);
    let cur = *app.left_cat.get(&app.left_tab).unwrap_or(&0);
    let cur = cur.min(cats.len().saturating_sub(1));
    let side = Rect::from_min_size(body.min, Vec2::new(cat_w, body.height()));
    ui.painter().line_segment(
        [Pos2::new(side.right(), side.top() + 10.0), Pos2::new(side.right(), side.bottom() - 10.0)],
        Stroke::new(1.0_f32, BORDER()),
    );
    let mut y = side.top() + 10.0;
    for (i, c) in cats.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(side.left() + 8.0, y), Vec2::new(cat_w - 16.0, 28.0));
        let resp = ui.interact(r, ui.id().with(("lcat", app.left_tab as u8, i)), Sense::click());
        let active = i == cur;
        if active {
            ui.painter().rect_filled(r, cr(7), PANEL3());
        } else if resp.hovered() {
            ui.painter().rect_filled(r, cr(7), PANEL2());
        }
        let col = if active {
            Color32::WHITE
        } else if resp.hovered() {
            TEXT()
        } else {
            TEXT2()
        };
        let g = elided(ui, tr(c), if active { bold(12.0) } else { prop(12.0) }, col, r.width() - 14.0);
        ui.painter().galley(Pos2::new(r.left() + 9.0, r.center().y - g.size().y / 2.0), g, col);
        if resp.clicked() {
            app.left_cat.insert(app.left_tab, i);
        }
        y += 31.0;
    }
    let content = Rect::from_min_max(Pos2::new(side.right() + 1.0, body.top()), body.max).shrink2(Vec2::new(10.0, 10.0));
    let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(content).layout(egui::Layout::top_down(egui::Align::Min)));
    let tab = app.left_tab;
    egui::ScrollArea::vertical().drag_to_scroll(false).id_salt(("lib-scroll", tab as u8, cur)).auto_shrink([false, false]).show(
        &mut cui,
        |ui| {
            let mut budget = 3u32;
            match tab {
                LeftTab::Media => media_tab(app, ui, cur),
                LeftTab::Audio => audio_tab(app, ui, cur),
                LeftTab::Text => text_tab(app, ui, cur, &mut budget),
                LeftTab::Captions => super::captions::library_tab(app, ui, cur),
                LeftTab::Stickers => sticker_tab(app, ui, cur, &mut budget),
                LeftTab::Effects => effect_tab(app, ui, cur, &mut budget),
                LeftTab::Transitions => transition_tab(app, ui, cur, &mut budget),
                LeftTab::Filters => filter_tab(app, ui, cur, &mut budget),
                LeftTab::Adjust => adjust_tab(app, ui),
            }
        },
    );
    ui.allocate_rect(body, Sense::hover());
}

fn grid_cols(ui: &egui::Ui, min_w: f32) -> (usize, f32) {
    let w = ui.available_width();
    let cols = ((w + 8.0) / (min_w + 8.0)).floor().max(1.0) as usize;
    let tw = (w - 8.0 * (cols as f32 - 1.0)) / cols as f32;
    (cols, tw.floor())
}

/// Lay out tiles in a grid. `f` draws one tile and gets (ui, index, tile width).
fn grid<T>(ui: &mut egui::Ui, items: &[T], min_w: f32, mut f: impl FnMut(&mut egui::Ui, &T, f32)) {
    let (cols, tw) = grid_cols(ui, min_w);
    for row in items.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for it in row {
                f(ui, it, tw);
            }
        });
        ui.add_space(4.0);
    }
}

fn plus_hit(resp: &egui::Response, rect_size: Vec2) -> bool {
    if let Some(p) = resp.interact_pointer_pos() {
        let c = resp.rect.min + Vec2::new(rect_size.x - 13.0, rect_size.y - 13.0);
        return p.distance(c) < 12.0;
    }
    false
}

fn media_tab(app: &mut VeloApp, ui: &mut egui::Ui, cat: usize) {
    if cat == 1 {
        samples(app, ui);
        return;
    }
    // import tile
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 64.0), Sense::click());
    let hov = resp.hovered();
    ui.painter().rect_filled(r, cr(8), if hov { PANEL3() } else { PANEL2() });
    ui.painter().rect_stroke(r, cr(8), Stroke::new(1.0_f32, if hov { ACCENT() } else { BORDER() }), StrokeKind::Inside);
    ui.painter().text(Pos2::new(r.center().x - 60.0, r.center().y), Align2::CENTER_CENTER, ph::DOWNLOAD_SIMPLE, prop(20.0), ACCENT());
    ui.painter().text(Pos2::new(r.center().x - 40.0, r.center().y - 8.0), Align2::LEFT_CENTER, tr("Import"), bold(13.5), Color32::WHITE);
    ui.painter().text(
        Pos2::new(r.center().x - 40.0, r.center().y + 10.0),
        Align2::LEFT_CENTER,
        tr("Videos, photos, audio  (Ctrl+I)"),
        prop(11.0),
        TEXT2(),
    );
    if resp.clicked() {
        app.pick_files(FilePurpose::Import);
    }
    ui.add_space(8.0);
    if app.pending_probes > 0 {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(egui::RichText::new(tr("Importing…")).color(TEXT2()));
        });
    }
    let media: Vec<(u64, String, MediaKind, i64, bool, String)> = app
        .project
        .media
        .iter()
        .filter(|m| !m.builtin)
        .map(|m| (m.id, m.name.clone(), m.kind, m.duration, std::path::Path::new(&m.path).exists(), m.path.clone()))
        .collect();
    if media.is_empty() {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(ph::FILM_STRIP).size(30.0).color(TEXT3()));
            ui.label(egui::RichText::new(tr("Drag & drop videos, photos and music here")).color(TEXT3()));
        });
        return;
    }
    let used: std::collections::HashSet<u64> =
        app.project.tracks.iter().flat_map(|t| t.clips.iter()).filter_map(|c| c.media_id()).collect();
    let show_proxies = app.settings.preview_quality >= 1;
    grid(ui, &media, 108.0, |ui, (id, name, kind, dur, exists, path), tw| {
        let th = tw * 9.0 / 16.0;
        let tex = app.thumb_tex.get(&(*id, u32::MAX)).map(|t| t.id());
        let badge = if *kind != MediaKind::Image { Some(util::timecode(*dur, 30.0)[3..8].to_string()) } else { None };
        let ph_icon = if !exists {
            ph::QUESTION
        } else if *kind == MediaKind::Image {
            ph::IMAGE
        } else if *kind == MediaKind::Audio {
            ph::MUSIC_NOTES
        } else {
            ph::VIDEO
        };
        let resp = tile(ui, Vec2::new(tw, th), tex, name, false, badge.as_deref(), ph_icon);
        if used.contains(id) {
            let c = resp.rect.min + Vec2::new(10.0, 10.0);
            ui.painter().circle_filled(c, 6.0, ACCENT_DARK());
            ui.painter().text(c, Align2::CENTER_CENTER, ph::CHECK, prop(8.5), Color32::WHITE);
        }
        // proxy state (light copy used by the preview)
        if show_proxies {
            use crate::media::proxy::ProxyState;
            let (text, tip) = match app.proxies.state(path) {
                Some(ProxyState::Queued) => (format!("{} …", ph::LIGHTNING), "Waiting to optimize for preview"),
                Some(ProxyState::Running(f)) => (format!("{} {:.0}%", ph::LIGHTNING, f * 100.0), "Optimizing for preview"),
                Some(ProxyState::Ready(_)) => (ph::LIGHTNING.to_string(), "Optimized for preview (proxy)"),
                _ => (String::new(), ""),
            };
            if !text.is_empty() {
                let g = ui.fonts(|f| f.layout_no_wrap(text, prop(10.0), Color32::WHITE));
                let br = Rect::from_min_size(
                    Pos2::new(resp.rect.left() + tw - g.size().x - 12.0, resp.rect.top() + 4.0),
                    g.size() + Vec2::new(8.0, 2.0),
                );
                let ready = matches!(app.proxies.state(path), Some(ProxyState::Ready(_)));
                ui.painter().rect_filled(br, cr(4), if ready { ACCENT_BG() } else { Color32::from_black_alpha(170) });
                ui.painter().galley(br.min + Vec2::new(4.0, 1.0), g, if ready { ACCENT() } else { Color32::WHITE });
                ui.interact(br, ui.id().with(("proxy", *id)), Sense::hover()).on_hover_text(tr(tip));
            }
        }
        if resp.drag_started() {
            app.drag = Some(DragItem::Media(*id));
        }
        let is_audio = *kind == MediaKind::Audio;
        if resp.double_clicked() || (resp.clicked() && plus_hit(&resp, Vec2::new(tw, th))) {
            let t = app.playhead;
            app.place_media(*id, if is_audio { Place::Overlay(t) } else { Place::Main(t) });
        }
        let id = *id;
        resp.on_hover_text(name.as_str()).context_menu(|ui| {
            if is_audio {
                if ui.button(format!("{}  {}", ph::PLUS, tr("Add to timeline"))).clicked() {
                    let t = app.playhead;
                    app.place_media(id, Place::Overlay(t));
                    ui.close_menu();
                }
            } else if ui.button(format!("{}  {}", ph::PLUS, tr("Add to main track"))).clicked() {
                let t = app.playhead;
                app.place_media(id, Place::Main(t));
                ui.close_menu();
            }
            if !is_audio && ui.button(format!("{}  {}", ph::SQUARES_FOUR, tr("Add as overlay"))).clicked() {
                let t = app.playhead;
                app.place_media(id, Place::Overlay(t));
                ui.close_menu();
            }
            if ui.button(format!("{}  {}", ph::LINK, tr("Relink media…"))).clicked() {
                app.pick_files(FilePurpose::Relink(id));
                ui.close_menu();
            }
            ui.separator();
            let in_use = app.project.tracks.iter().flat_map(|t| t.clips.iter()).any(|c| c.media_id() == Some(id));
            if ui.add_enabled(!in_use, egui::Button::new(format!("{}  {}", ph::TRASH, tr("Remove from project")))).clicked() {
                app.edit(|p| p.media.retain(|m| m.id != id));
                ui.close_menu();
            }
        });
    });
}

fn samples(app: &mut VeloApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new(tr("Built-in pictures and colour backgrounds")).color(TEXT2()).size(11.5));
    ui.add_space(6.0);
    let items: Vec<(&str, u32)> = vec![("Sunset", 0), ("City night", 1)];
    grid(ui, &items, 108.0, |ui, (name, kind), tw| {
        let th = tw * 9.0 / 16.0;
        let key = format!("sample-{}", kind);
        if !app.tile_tex.contains_key(&key) {
            let img = crate::demo::sample_image(*kind, 320, 180);
            let ci = egui::ColorImage::from_rgba_unmultiplied([320, 180], img.as_raw());
            let t = ui.ctx().load_texture(key.clone(), ci, egui::TextureOptions::LINEAR);
            app.tile_tex.insert(key.clone(), t);
        }
        let tex = app.tile_tex.get(&key).map(|t| t.id());
        let resp = tile(ui, Vec2::new(tw, th), tex, tr(name), false, None, "");
        if resp.clicked() || resp.drag_started() {
            let path = util::data_dir().join(format!("sample-{}.png", kind));
            if !path.exists() {
                let _ = crate::demo::sample_image(*kind, 1920, 1080).save(&path);
            }
            let ps = path.to_string_lossy().to_string();
            let existing = app.project.media.iter().find(|m| m.path == ps).map(|m| m.id);
            let id = match existing {
                Some(id) => Some(id),
                None => crate::media::probe(&ps).ok().map(|m| {
                    let id = m.id;
                    let (path, kind, aspect) = (m.path.clone(), m.kind, m.width as f64 / m.height.max(1) as f64);
                    app.edit(|p| p.media.push(m));
                    app.request_thumbs(id, &path, kind, 0.0, aspect);
                    id
                }),
            };
            if let Some(id) = id {
                if resp.drag_started() {
                    app.drag = Some(DragItem::Media(id));
                } else {
                    let t = app.playhead;
                    app.place_media(id, Place::Main(t));
                }
            }
        }
    });
    ui.add_space(8.0);
    ui.label(egui::RichText::new(tr("Canvas colour")).color(TEXT2()).size(11.5));
    let mut c = app.project.bg_color;
    if color_button(ui, egui::Id::new("canvas-bg"), &mut c, false) {
        app.edit(|p| p.bg_color = c);
    }
}

fn audio_tab(app: &mut VeloApp, ui: &mut egui::Ui, cat: usize) {
    if cat == 0 {
        for s in catalog::SFX {
            let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 44.0), Sense::click_and_drag());
            let hov = resp.hovered();
            ui.painter().rect_filled(r, cr(8), if hov { PANEL3() } else { PANEL2() });
            let play = Rect::from_center_size(Pos2::new(r.left() + 22.0, r.center().y), Vec2::splat(30.0));
            ui.painter().rect_filled(play, cr(6), CLIP_AUDIO);
            ui.painter().text(play.center(), Align2::CENTER_CENTER, ph::PLAY, fill(13.0), WAVE);
            ui.painter().text(Pos2::new(r.left() + 46.0, r.center().y - 7.0), Align2::LEFT_CENTER, tr(s.name), prop(12.5), Color32::WHITE);
            ui.painter().text(
                Pos2::new(r.left() + 46.0, r.center().y + 9.0),
                Align2::LEFT_CENTER,
                format!("{:.1}s  •  {}", util::us_to_s(s.dur), tr("VeloMov sound")),
                prop(10.5),
                TEXT3(),
            );
            let add = Rect::from_center_size(Pos2::new(r.right() - 20.0, r.center().y), Vec2::splat(24.0));
            if hov {
                ui.painter().circle_filled(add.center(), 11.0, ACCENT());
                ui.painter().text(add.center(), Align2::CENTER_CENTER, ph::PLUS, prop(12.0), ON_ACCENT());
            }
            if resp.drag_started() {
                app.drag = Some(DragItem::Sfx(s.id));
            }
            if resp.clicked() {
                let p = resp.interact_pointer_pos().unwrap_or_default();
                if add.expand(4.0).contains(p) {
                    if let Some(mid) = app.add_builtin_sfx(s.id) {
                        let t = app.playhead;
                        app.place_media(mid, Place::Overlay(t));
                    }
                } else {
                    app.preview_sfx(s.id);
                }
            }
            ui.add_space(2.0);
        }
        return;
    }
    let media: Vec<(u64, String, i64)> =
        app.project.media.iter().filter(|m| m.kind == MediaKind::Audio && !m.builtin).map(|m| (m.id, m.name.clone(), m.duration)).collect();
    if ui
        .add_sized(Vec2::new(ui.available_width(), 30.0), egui::Button::new(format!("{}  {}", ph::DOWNLOAD_SIMPLE, tr("Import audio"))))
        .clicked()
    {
        app.pick_files(FilePurpose::Import);
    }
    ui.add_space(6.0);
    if media.is_empty() {
        ui.label(egui::RichText::new(tr("Imported music and voice-overs appear here.")).color(TEXT3()));
    }
    for (id, name, dur) in media {
        let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click_and_drag());
        ui.painter().rect_filled(r, cr(8), if resp.hovered() { PANEL3() } else { PANEL2() });
        ui.painter().text(Pos2::new(r.left() + 14.0, r.center().y), Align2::LEFT_CENTER, ph::MUSIC_NOTE, prop(15.0), WAVE);
        let g = elided(ui, &name, prop(12.5), Color32::WHITE, r.width() - 110.0);
        ui.painter().galley(Pos2::new(r.left() + 36.0, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
        ui.painter().text(Pos2::new(r.right() - 12.0, r.center().y), Align2::RIGHT_CENTER, util::short_dur(dur), prop(11.0), TEXT2());
        if resp.drag_started() {
            app.drag = Some(DragItem::Media(id));
        }
        if resp.double_clicked() {
            let t = app.playhead;
            app.place_media(id, Place::Overlay(t));
        }
        ui.add_space(2.0);
    }
}

fn text_tab(app: &mut VeloApp, ui: &mut egui::Ui, cat: usize, budget: &mut u32) {
    if cat == 0 {
        let w = ui.available_width();
        let (r, resp) = ui.allocate_exact_size(Vec2::new(w, 72.0), Sense::click_and_drag());
        ui.painter().rect_filled(r, cr(8), if resp.hovered() { PANEL3() } else { PANEL2() });
        ui.painter().text(r.center() - Vec2::new(0.0, 8.0), Align2::CENTER_CENTER, tr("Default text"), bold(17.0), Color32::WHITE);
        ui.painter().text(
            r.center() + Vec2::new(0.0, 16.0),
            Align2::CENTER_CENTER,
            format!("{}  {}", ph::PLUS, tr("Add to timeline  (Ctrl+T)")),
            prop(11.0),
            TEXT2(),
        );
        if resp.clicked() {
            app.add_text("default", None);
        }
        if resp.drag_started() {
            app.drag = Some(DragItem::Text("default"));
        }
        ui.add_space(10.0);
        ui.label(egui::RichText::new(tr("Tip: double-click text in the player to edit it in the Text panel.")).size(11.0).color(TEXT3()));
        return;
    }
    let items: Vec<&'static catalog::TextPreset> = catalog::TEXT_PRESETS.iter().collect();
    grid(ui, &items, 96.0, |ui, pr, tw| {
        let th = tw * 9.0 / 16.0;
        let key = format!("txt:{}", pr.id);
        let tex = app.tile_texture(ui.ctx(), &key, budget);
        let resp = tile(ui, Vec2::new(tw, th), tex, tr(pr.name), false, None, ph::TEXT_T);
        if resp.drag_started() {
            app.drag = Some(DragItem::Text(pr.id));
        }
        if resp.clicked() {
            app.add_text(pr.id, None);
        }
    });
}

fn sticker_tab(app: &mut VeloApp, ui: &mut egui::Ui, cat: usize, budget: &mut u32) {
    let items: Vec<&'static catalog::StickerDef> = catalog::STICKERS.iter().filter(|s| (cat == 1) == s.emoji.is_some()).collect();
    grid(ui, &items, 70.0, |ui, st, tw| {
        let key = format!("st:{}", st.id);
        let tex = app.tile_texture(ui.ctx(), &key, budget);
        // stickers are rendered on a 16:9 canvas: show the centre square
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(tw), Sense::click_and_drag());
        ui.painter().rect_filled(rect, cr(6), if resp.hovered() { PANEL3() } else { PANEL2() });
        if let Some(t) = tex {
            ui.painter().image(t, rect.shrink(2.0), Rect::from_min_max(Pos2::new(0.21875, 0.0), Pos2::new(0.78125, 1.0)), Color32::WHITE);
        }
        if resp.drag_started() {
            app.drag = Some(DragItem::Sticker(st.id));
        }
        if resp.clicked() {
            app.add_sticker(st.id, None);
        }
        resp.on_hover_text(tr(st.name));
    });
}

fn effect_tab(app: &mut VeloApp, ui: &mut egui::Ui, cat: usize, budget: &mut u32) {
    let c = ECat::ALL[cat.min(ECat::ALL.len() - 1)];
    let items: Vec<&'static catalog::EffectDef> =
        catalog::EFFECTS.iter().filter(|e| if c == ECat::Trending { e.trending } else { e.cat == c }).collect();
    grid(ui, &items, 100.0, |ui, e, tw| {
        let th = tw * 9.0 / 16.0;
        let key = format!("fx:{}", e.id);
        let hovered_last = ui.ctx().memory(|m| m.data.get_temp::<String>(egui::Id::new("tile-hover"))).map(|k| k == key).unwrap_or(false);
        let tex = if hovered_last { app.tile_anim_texture(&key) } else { app.tile_texture(ui.ctx(), &key, budget) };
        let resp = tile(ui, Vec2::new(tw, th), tex, tr(e.name), false, None, ph::SPARKLE);
        track_hover(ui, &resp, &key);
        if resp.drag_started() {
            app.drag = Some(DragItem::Effect(e.id));
        }
        if resp.clicked() {
            app.add_effect(e.id, None);
        }
    });
}

fn transition_tab(app: &mut VeloApp, ui: &mut egui::Ui, cat: usize, budget: &mut u32) {
    let c = TCat::ALL[cat.min(TCat::ALL.len() - 1)];
    let items: Vec<&'static catalog::TransitionDef> =
        catalog::TRANSITIONS.iter().filter(|t| if c == TCat::Trending { t.trending } else { t.cat == c }).collect();
    if app.project.tracks[app.project.main_track_index()].clips.len() < 2 {
        ui.label(
            egui::RichText::new(tr("Transitions go between two clips on the main track. Click to apply at the selected cut."))
                .size(11.0)
                .color(TEXT3()),
        );
        ui.add_space(4.0);
    }
    let current = app.sel.transition.and_then(|id| app.project.clip(id)).and_then(|c| c.transition.as_ref()).map(|t| t.id.clone());
    grid(ui, &items, 100.0, |ui, t, tw| {
        let th = tw * 9.0 / 16.0;
        let key = format!("tr:{}", t.id);
        let hovered_last = ui.ctx().memory(|m| m.data.get_temp::<String>(egui::Id::new("tile-hover"))).map(|k| k == key).unwrap_or(false);
        let tex = if hovered_last { app.tile_anim_texture(&key) } else { app.tile_texture(ui.ctx(), &key, budget) };
        let sel = current.as_deref() == Some(t.id);
        let resp = tile(ui, Vec2::new(tw, th), tex, tr(t.name), sel, None, ph::SWAP);
        track_hover(ui, &resp, &key);
        if resp.drag_started() {
            app.drag = Some(DragItem::Transition(t.id));
        }
        if resp.clicked() {
            app.apply_transition(t.id, None);
        }
    });
}

fn filter_tab(app: &mut VeloApp, ui: &mut egui::Ui, cat: usize, budget: &mut u32) {
    let c = FCat::ALL[cat.min(FCat::ALL.len() - 1)];
    let items: Vec<&'static catalog::FilterDef> = catalog::FILTERS.iter().filter(|f| f.cat == c).collect();
    let current = app.sel.one().and_then(|id| app.project.clip(id)).and_then(|c| c.filter.as_ref()).map(|f| f.id.clone());
    grid(ui, &items, 92.0, |ui, f, tw| {
        let th = tw * 9.0 / 16.0;
        let key = format!("flt:{}", f.id);
        let tex = app.tile_texture(ui.ctx(), &key, budget);
        let resp = tile(ui, Vec2::new(tw, th), tex, tr(f.name), current.as_deref() == Some(f.id), None, ph::CIRCLE_HALF);
        if resp.drag_started() {
            app.drag = Some(DragItem::Filter(f.id));
        }
        if resp.clicked() {
            app.add_filter(f.id, None);
        }
    });
}

fn adjust_tab(app: &mut VeloApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new(tr("Adjustment layers grade everything below them on the timeline.")).size(11.0).color(TEXT3()));
    ui.add_space(6.0);
    for (i, (name, _)) in ADJUST_PRESETS.iter().enumerate() {
        let (r, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 38.0), Sense::click_and_drag());
        ui.painter().rect_filled(r, cr(8), if resp.hovered() { PANEL3() } else { PANEL2() });
        ui.painter().text(
            Pos2::new(r.left() + 14.0, r.center().y),
            Align2::LEFT_CENTER,
            ph::SLIDERS_HORIZONTAL,
            prop(15.0),
            Color32::from_rgb(110, 160, 255),
        );
        ui.painter().text(Pos2::new(r.left() + 38.0, r.center().y), Align2::LEFT_CENTER, tr(name), prop(12.5), Color32::WHITE);
        if resp.drag_started() {
            app.drag = Some(DragItem::Adjust(i));
        }
        if resp.clicked() {
            app.add_adjust_layer(i, None);
        }
        ui.add_space(2.0);
    }
}

fn track_hover(ui: &egui::Ui, resp: &egui::Response, key: &str) {
    let id = egui::Id::new("tile-hover");
    if resp.hovered() {
        ui.ctx().memory_mut(|m| m.data.insert_temp(id, key.to_string()));
        ui.ctx().request_repaint();
    } else {
        let cur = ui.ctx().memory(|m| m.data.get_temp::<String>(id));
        if cur.as_deref() == Some(key) {
            ui.ctx().memory_mut(|m| m.data.remove::<String>(id));
        }
    }
}

/// Floating label under the pointer while dragging a library item.
pub fn drag_ghost(app: &mut VeloApp, ctx: &egui::Context) {
    let Some(item) = app.drag.clone() else { return };
    let (pos, down) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.any_down()));
    if !down {
        app.drag = None;
        return;
    }
    let Some(pos) = pos else { return };
    let label = match &item {
        DragItem::Media(id) => app.project.media(*id).map(|m| m.name.clone()).unwrap_or_default(),
        DragItem::Sfx(s) => catalog::SFX.iter().find(|x| x.id == *s).map(|x| tr(x.name).to_string()).unwrap_or_default(),
        DragItem::Text(_) => tr("Text").to_string(),
        DragItem::Sticker(s) => tr(catalog::sticker_name(s)).to_string(),
        DragItem::Effect(e) => catalog::effect(e).map(|d| tr(d.name).to_string()).unwrap_or_default(),
        DragItem::Filter(f) => catalog::filter(f).map(|d| tr(d.name).to_string()).unwrap_or_default(),
        DragItem::Transition(t) => catalog::transition(t).map(|d| tr(d.name).to_string()).unwrap_or_default(),
        DragItem::Adjust(i) => tr(ADJUST_PRESETS[*i].0).to_string(),
    };
    egui::Area::new(egui::Id::new("drag-ghost"))
        .fixed_pos(pos + Vec2::new(12.0, 10.0))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new().fill(ACCENT_DARK()).corner_radius(cr(6)).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
                ui.label(egui::RichText::new(label).color(Color32::WHITE));
            });
        });
    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
}
