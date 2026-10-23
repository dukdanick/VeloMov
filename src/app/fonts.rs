//! Text font picker: every font drawn in itself, a big preview of the clip's text,
//! favourite fonts (star) and the last ones used.

use super::i18n::tr;
use super::theme::*;
use super::widgets::*;
use super::VeloApp;
use crate::engine::raster::Raster;
use crate::model::TextData;
use eframe::egui::{self, Align2, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as ph;
use std::collections::HashMap;

/// How many recently used fonts are remembered.
pub const MAX_RECENT: usize = 8;
/// New row previews drawn per frame (the rest come in the next frames).
const PER_FRAME: usize = 12;
const ROW_H: f32 = 30.0;
const LIST_H: f32 = 300.0;

/// Font previews drawn with the same text engine as the video, kept as textures.
#[derive(Default)]
pub struct FontPreviews {
    raster: Option<Raster>,
    rows: HashMap<String, egui::TextureHandle>,
    /// (key of family/text/style, texture) of the big preview.
    big: Option<(u64, egui::TextureHandle)>,
    budget: usize,
}

impl FontPreviews {
    fn draw(&mut self, ctx: &egui::Context, name: &str, family: &str, text: &str, px: f32, bold: bool, italic: bool) -> egui::TextureHandle {
        let r = self.raster.get_or_insert_with(Raster::new);
        let img = r.font_sample(family, text, px, bold, italic);
        let ci = egui::ColorImage::from_rgba_premultiplied([img.w as usize, img.h as usize], &img.data);
        ctx.load_texture(name, ci, egui::TextureOptions::LINEAR)
    }

    /// The family's name written in that family (None while waiting for its turn).
    fn row(&mut self, ctx: &egui::Context, family: &str) -> Option<egui::TextureHandle> {
        if let Some(t) = self.rows.get(family) {
            return Some(t.clone());
        }
        if self.budget == 0 {
            return None;
        }
        self.budget -= 1;
        if self.rows.len() > 1500 {
            self.rows.clear();
        }
        let px = 17.0 * ctx.pixels_per_point();
        let t = self.draw(ctx, &format!("font-row-{}", family), family, family, px, false, false);
        self.rows.insert(family.to_string(), t.clone());
        Some(t)
    }

    /// `text` in `family`, with the clip's bold/italic, for the preview box.
    fn big(&mut self, ctx: &egui::Context, family: &str, text: &str, bold: bool, italic: bool) -> egui::TextureHandle {
        let key = crate::util::hash_str(&format!("{}\u{1}{}\u{1}{}{}", family, text, bold, italic));
        if let Some((k, t)) = &self.big {
            if *k == key {
                return t.clone();
            }
        }
        let px = 34.0 * ctx.pixels_per_point();
        let t = self.draw(ctx, "font-big", family, text, px, bold, italic);
        self.big = Some((key, t.clone()));
        t
    }
}

impl VeloApp {
    /// Put `family` first in the recently used fonts.
    pub fn remember_font(&mut self, family: &str) {
        if family.is_empty() {
            return;
        }
        let r = &mut self.settings.recent_fonts;
        r.retain(|f| f != family);
        r.insert(0, family.to_string());
        r.truncate(MAX_RECENT);
        self.settings.save();
    }

    pub fn toggle_fav_font(&mut self, family: &str) {
        let f = &mut self.settings.fav_fonts;
        if let Some(i) = f.iter().position(|x| x == family) {
            f.remove(i);
        } else {
            f.push(family.to_string());
            f.sort_by_key(|a| a.to_lowercase());
        }
        self.settings.save();
    }
}

enum Row {
    Header(String),
    Default,
    Font(String),
    Empty(String),
}

/// First line of the clip's text (shortened) for the preview box.
fn sample_text(td: &TextData) -> String {
    let line = td.text.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.is_empty() {
        return "Aa Bb Cc 123".into();
    }
    let mut s: String = line.chars().take(28).collect();
    if line.chars().count() > 28 {
        s.push('…');
    }
    s
}

/// Font row of the inspector: a button that opens the picker. Returns true when the
/// clip's font changed.
pub fn font_picker(app: &mut VeloApp, ui: &mut egui::Ui, id: u64, td: &mut TextData) -> bool {
    if app.font_list.is_none() {
        app.font_list = Some(crate::engine::raster::font_families());
    }
    let mut changed = false;
    let popup_id = egui::Id::new(("font-picker", id));
    let open = ui.memory(|m| m.is_popup_open(popup_id));
    // the button: current font, written in itself when its preview is ready
    let w = (ui.available_width() - 6.0).max(120.0);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 26.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, cr(6), if resp.hovered() || open { PANEL3() } else { PANEL2() });
    if open {
        p.rect_stroke(rect, cr(6), Stroke::new(1.0_f32, ACCENT()), StrokeKind::Inside);
    }
    let cur = if td.font.is_empty() { tr("System default").to_string() } else { td.font.clone() };
    let fav = !td.font.is_empty() && app.settings.fav_fonts.contains(&td.font);
    let g = elided(ui, &cur, prop(12.5), TEXT(), w - 44.0);
    ui.painter().galley(Pos2::new(rect.left() + 9.0, rect.center().y - g.size().y / 2.0), g, TEXT());
    ui.painter().text(Pos2::new(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, ph::CARET_DOWN, prop(12.0), TEXT2());
    if fav {
        ui.painter().text(Pos2::new(rect.right() - 26.0, rect.center().y), Align2::RIGHT_CENTER, ph::STAR, fill(12.0), ACCENT());
    }
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    let ctx = ui.ctx().clone();
    app.fonts.budget = PER_FRAME;
    let filt_id = egui::Id::new("font-filter");
    let favs_only_id = egui::Id::new("font-favs-only");
    egui::popup_below_widget(ui, popup_id, &resp, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        let pw = w.max(330.0);
        ui.set_width(pw);
        let mut hovered: Option<String> = None;
        let prev_hover: Option<String> = ui.memory(|m| m.data.get_temp(egui::Id::new("font-hover")));
        // ---- preview of the clip's text in the hovered (or current) font
        let shown = prev_hover.clone().unwrap_or_else(|| td.font.clone());
        let (pr, _) = ui.allocate_exact_size(Vec2::new(pw, 78.0), Sense::hover());
        ui.painter().rect_filled(pr, cr(8), BG());
        let tex = app.fonts.big(&ctx, &shown, &sample_text(td), td.bold, td.italic);
        let ppp = ctx.pixels_per_point();
        let mut sz = tex.size_vec2() / ppp;
        let room = Vec2::new(pr.width() - 20.0, pr.height() - 26.0);
        let k = (room.x / sz.x).min(room.y / sz.y).min(1.0);
        sz *= k;
        let ir = Rect::from_center_size(Pos2::new(pr.center().x, pr.top() + 8.0 + room.y / 2.0), sz);
        ui.painter().image(tex.id(), ir, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        let label = if shown.is_empty() { tr("System default").to_string() } else { shown.clone() };
        let g = elided(ui, &label, prop(10.5), TEXT3(), pw - 20.0);
        ui.painter().galley(Pos2::new(pr.center().x - g.size().x / 2.0, pr.bottom() - g.size().y - 5.0), g, TEXT3());
        ui.add_space(4.0);
        // ---- search + favourites filter
        let mut filt: String = ui.memory(|m| m.data.get_temp(filt_id)).unwrap_or_default();
        let mut favs_only: bool = ui.memory(|m| m.data.get_temp(favs_only_id)).unwrap_or(false);
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut filt)
                    .hint_text(format!("{}  {}", ph::MAGNIFYING_GLASS, tr("Search fonts")))
                    .desired_width(pw - 44.0),
            );
            if r.changed() {
                ui.memory_mut(|m| m.data.insert_temp(filt_id, filt.clone()));
            }
            let star = if favs_only { fill(15.0) } else { prop(15.0) };
            let (sr, sresp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
            if favs_only || sresp.hovered() {
                ui.painter().rect_filled(sr, cr(6), if favs_only { ACCENT_BG() } else { PANEL3() });
            }
            ui.painter().text(sr.center(), Align2::CENTER_CENTER, ph::STAR, star, if favs_only { ACCENT() } else { TEXT2() });
            if sresp.on_hover_text(tr("Only favourite fonts")).clicked() {
                favs_only = !favs_only;
                ui.memory_mut(|m| m.data.insert_temp(favs_only_id, favs_only));
            }
        });
        ui.add_space(2.0);
        // ---- rows: favourites, recently used, then every font
        let fl = filt.trim().to_lowercase();
        let matches = |f: &str| fl.is_empty() || f.to_lowercase().contains(&fl);
        let all = app.font_list.clone().unwrap_or_default();
        let installed = |f: &String| all.binary_search(f).is_ok();
        let mut rows = vec![];
        let favs: Vec<String> = app.settings.fav_fonts.iter().filter(|f| matches(f) && installed(f)).cloned().collect();
        if favs_only {
            if favs.is_empty() {
                rows.push(Row::Empty(tr("No favourite fonts yet: click the star next to a font.").into()));
            }
            rows.extend(favs.into_iter().map(Row::Font));
        } else {
            if !favs.is_empty() {
                rows.push(Row::Header(format!("{}  {}", ph::STAR, tr("Favourites"))));
                rows.extend(favs.into_iter().map(Row::Font));
            }
            let recent: Vec<String> = app.settings.recent_fonts.iter().filter(|f| matches(f) && installed(f)).cloned().collect();
            if !recent.is_empty() {
                rows.push(Row::Header(format!("{}  {}", ph::CLOCK_COUNTER_CLOCKWISE, tr("Recently used"))));
                rows.extend(recent.into_iter().map(Row::Font));
            }
            let list: Vec<&String> = all.iter().filter(|f| matches(f)).collect();
            rows.push(Row::Header(format!("{}  {} ({})", ph::TEXT_AA, tr("All fonts"), list.len())));
            if fl.is_empty() {
                rows.push(Row::Default);
            }
            if list.is_empty() {
                rows.push(Row::Empty(tr("No font matches the search.").into()));
            }
            rows.extend(list.into_iter().map(|f| Row::Font(f.clone())));
        }
        let mut pick: Option<String> = None;
        let mut star: Option<String> = None;
        egui::ScrollArea::vertical().id_salt(("font-rows", id)).max_height(LIST_H).auto_shrink([false, true]).show_rows(
            ui,
            ROW_H,
            rows.len(),
            |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                // rows stop short of the (floating) scroll bar so the stars stay clickable
                let row_w = pw - 14.0;
                for (i, row) in rows[range.clone()].iter().enumerate() {
                    let (r, rresp) = ui.allocate_exact_size(Vec2::new(row_w, ROW_H), Sense::click());
                    match row {
                        Row::Header(t) => {
                            ui.painter().text(Pos2::new(r.left() + 6.0, r.bottom() - 9.0), Align2::LEFT_CENTER, t, bold(11.0), TEXT2());
                        }
                        Row::Empty(t) => {
                            let g = elided(ui, t, prop(11.5), TEXT3(), row_w - 16.0);
                            ui.painter().galley(Pos2::new(r.left() + 8.0, r.center().y - g.size().y / 2.0), g, TEXT3());
                        }
                        Row::Default | Row::Font(_) => {
                            let fam = if let Row::Font(f) = row { f.as_str() } else { "" };
                            let selected = td.font == fam;
                            if selected {
                                ui.painter().rect_filled(r, cr(6), ACCENT_BG());
                            } else if rresp.hovered() {
                                ui.painter().rect_filled(r, cr(6), PANEL3());
                            }
                            if rresp.hovered() {
                                hovered = Some(fam.to_string());
                            }
                            let text_x = r.left() + 10.0;
                            let max_w = row_w - 58.0;
                            if fam.is_empty() {
                                let g = elided(ui, tr("System default"), prop(13.0), TEXT(), max_w);
                                ui.painter().galley(Pos2::new(text_x, r.center().y - g.size().y / 2.0), g, TEXT());
                            } else if let Some(t) = app.fonts.row(&ctx, fam) {
                                // the name drawn in its own font (cut at the row's width)
                                let sz = t.size_vec2() / ppp;
                                let shown_w = sz.x.min(max_w);
                                let dst = Rect::from_min_size(Pos2::new(text_x - 4.0, r.center().y - sz.y / 2.0), Vec2::new(shown_w, sz.y));
                                let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(shown_w / sz.x, 1.0));
                                ui.painter().with_clip_rect(r).image(t.id(), dst, uv, if selected { Color32::WHITE } else { TEXT() });
                            } else {
                                let g = elided(ui, fam, prop(12.5), TEXT3(), max_w);
                                ui.painter().galley(Pos2::new(text_x, r.center().y - g.size().y / 2.0), g, TEXT3());
                            }
                            if selected {
                                ui.painter().text(Pos2::new(r.right() - 36.0, r.center().y), Align2::CENTER_CENTER, ph::CHECK, prop(13.0), ACCENT());
                            }
                            let mut clicked_star = false;
                            if !fam.is_empty() {
                                let is_fav = app.settings.fav_fonts.iter().any(|f| f == fam);
                                let sr = Rect::from_center_size(Pos2::new(r.right() - 14.0, r.center().y), Vec2::splat(24.0));
                                let sresp = ui.interact(sr, ui.id().with(("font-star", range.start + i)), Sense::click());
                                if is_fav || rresp.hovered() || sresp.hovered() {
                                    let col = if is_fav {
                                        ACCENT()
                                    } else if sresp.hovered() {
                                        Color32::WHITE
                                    } else {
                                        TEXT3()
                                    };
                                    let font = if is_fav { fill(14.0) } else { prop(14.0) };
                                    ui.painter().text(sr.center(), Align2::CENTER_CENTER, ph::STAR, font, col);
                                }
                                let tip = if is_fav { tr("Remove from favourites") } else { tr("Add to favourites") };
                                if sresp.on_hover_text(tip).clicked() {
                                    star = Some(fam.to_string());
                                    clicked_star = true;
                                }
                            }
                            if rresp.clicked() && !clicked_star {
                                pick = Some(fam.to_string());
                            }
                        }
                    }
                }
            },
        );
        ui.memory_mut(|m| match &hovered {
            Some(h) => m.data.insert_temp(egui::Id::new("font-hover"), h.clone()),
            None => m.data.remove::<String>(egui::Id::new("font-hover")),
        });
        if hovered != prev_hover {
            ctx.request_repaint();
        }
        if let Some(f) = star {
            app.toggle_fav_font(&f);
        }
        if let Some(f) = pick {
            if td.font != f {
                td.font = f.clone();
                changed = true;
            }
            app.remember_font(&f);
            ui.memory_mut(|m| m.close_popup());
        }
    });
    if !ui.memory(|m| m.is_popup_open(popup_id)) && open {
        ui.memory_mut(|m| m.data.remove::<String>(egui::Id::new("font-hover")));
    }
    if app.fonts.budget == 0 {
        // more previews to draw
        ctx.request_repaint();
    }
    changed
}
