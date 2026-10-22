//! Auto captions: the "Captions" library tab (speech recognition with Whisper and the
//! animated caption styles), the caption section of the text inspector and the
//! speech-model settings.

use super::i18n::tr;
use super::theme::*;
use super::widgets::*;
use super::VeloApp;
use crate::media::transcribe::{self, Download, Job};
use crate::model::catalog::{self, CaptionStyle};
use crate::model::*;
use crate::util::{self, Us};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use egui_phosphor::regular as ph;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

/// Caption length choices: (label, max characters; 0 = by the canvas shape).
const LENGTHS: [(&str, usize); 5] = [("Auto", 0), ("One word", 1), ("Short", 16), ("Medium", 26), ("Long", 40)];

pub struct CaptionsState {
    pub style: String,
    /// Index into LENGTHS.
    pub length: usize,
    /// 0 = whole timeline, 1 = selected clips.
    pub source: usize,
    pub replace: bool,
    pub job: Option<(Arc<Job>, (Us, Us))>,
    pub download: Option<Arc<Download>>,
    /// Cached `transcribe::find_whisper()` (looked up when the tab or settings open).
    pub engine: Option<Option<std::path::PathBuf>>,
}

impl Default for CaptionsState {
    fn default() -> Self {
        CaptionsState { style: "karaoke".into(), length: 0, source: 0, replace: true, job: None, download: None, engine: None }
    }
}

impl CaptionsState {
    fn engine(&mut self) -> Option<std::path::PathBuf> {
        self.engine.get_or_insert_with(transcribe::find_whisper).clone()
    }
}

fn is_auto_caption(c: &Clip) -> bool {
    matches!(&c.kind, ClipKind::Text(td) if !td.words.is_empty())
}

impl VeloApp {
    /// Start recognising speech for the whole timeline or the selected clips.
    pub fn start_captions(&mut self) {
        if self.captions.job.is_some() {
            return;
        }
        let only = if self.captions.source == 1 && !self.sel.clips.is_empty() { Some(self.sel.clips.clone()) } else { None };
        let range = match &only {
            Some(ids) => {
                let cs: Vec<&Clip> = ids.iter().filter_map(|id| self.project.clip(*id)).collect();
                (cs.iter().map(|c| c.start).min().unwrap_or(0), cs.iter().map(|c| c.end()).max().unwrap_or(0))
            }
            None => (0, self.project.duration()),
        };
        if range.1 <= range.0 {
            self.toast_err(tr("Add a video or audio with speech to the timeline first"));
            return;
        }
        let req = transcribe::Request {
            project: self.project.clone(),
            range,
            only,
            lang: self.settings.caption_lang.clone(),
            model: self.settings.whisper_model.clone(),
            threads: self.settings.whisper_threads,
            gpu: self.settings.whisper_gpu,
        };
        self.captions.job = Some((transcribe::spawn(req), range));
    }

    /// Finished recognition and model downloads (called every frame of the editor).
    pub fn poll_captions(&mut self, ctx: &egui::Context) {
        if let Some(d) = self.captions.download.clone() {
            if d.done.load(Ordering::Relaxed) {
                let err = d.error.lock().unwrap().take();
                if let Some(e) = err {
                    self.toast_err(&format!("{}: {}", tr("Download failed"), e));
                } else if !d.cancel.load(Ordering::Relaxed) {
                    self.toast(tr("Model downloaded"));
                }
                self.captions.download = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(150));
            }
        }
        let Some((job, range)) = self.captions.job.clone() else { return };
        if !job.done.load(Ordering::Relaxed) {
            ctx.request_repaint_after(Duration::from_millis(120));
            return;
        }
        self.captions.job = None;
        let err = job.error.lock().unwrap().take();
        if let Some(e) = err {
            self.toast_err(&format!("{}: {}", tr("Captions failed"), e));
            return;
        }
        let res = job.result.lock().unwrap().take();
        let Some((words, lang)) = res else { return };
        if words.is_empty() {
            self.toast_err(tr("No speech was found"));
            return;
        }
        let n = self.add_captions(&words, range);
        let lang_name = transcribe::LANGUAGES.iter().find(|l| l.0 == lang).map(|l| l.1).unwrap_or(&lang);
        self.toast(&format!("{} {} ({})", n, tr("captions created"), lang_name));
    }

    /// Turn recognised words into caption clips on a new text track; returns how many.
    pub fn add_captions(&mut self, words: &[Word], range: (Us, Us)) -> usize {
        let max_chars = LENGTHS[self.captions.length.min(LENGTHS.len() - 1)].1;
        let (style, replace) = (self.captions.style.clone(), self.captions.replace);
        let (n, first) = self.edit(|p| transcribe::insert_captions(p, words, range, &style, max_chars, replace));
        self.flush_edit();
        if let Some(id) = first {
            self.select_only(id);
            self.right_tab = super::RightTab::Video;
        }
        n
    }

    /// Give `targets` (or every auto caption when empty) a caption style.
    pub fn apply_caption_style(&mut self, style: &'static CaptionStyle, targets: &[u64]) -> usize {
        self.captions.style = style.id.into();
        let all = targets.is_empty();
        let ids: Vec<u64> = self
            .project
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .filter(|c| if all { is_auto_caption(c) } else { targets.contains(&c.id) && matches!(c.kind, ClipKind::Text(_)) })
            .map(|c| c.id)
            .collect();
        if ids.is_empty() {
            return 0;
        }
        self.edit(|p| {
            for id in &ids {
                if let Some(c) = p.clip_mut(*id) {
                    if let ClipKind::Text(td) = &mut c.kind {
                        style.apply(td);
                    }
                }
            }
        });
        self.flush_edit();
        ids.len()
    }

    /// Copy the look of caption `src` (style, colours, font, size, position) to every auto caption.
    fn caption_look_to_all(&mut self, src: &Clip) {
        let ClipKind::Text(s) = &src.kind else { return };
        let (s, tf) = (s.clone(), src.tf);
        self.edit(|p| {
            for t in p.tracks.iter_mut() {
                for c in t.clips.iter_mut().filter(|c| is_auto_caption(c) && c.id != src.id) {
                    if let ClipKind::Text(td) = &mut c.kind {
                        let (text, words) = (std::mem::take(&mut td.text), std::mem::take(&mut td.words));
                        *td = TextData { text, words, ..s.clone() };
                    }
                    c.tf.x = tf.x;
                    c.tf.y = tf.y;
                    c.tf.scale = tf.scale;
                }
            }
        });
        self.flush_edit();
    }
}

// ------------------------------------------------------------------ library tab

pub fn library_tab(app: &mut VeloApp, ui: &mut egui::Ui, cat: usize) {
    if cat == 0 {
        auto_tab(app, ui);
    } else {
        styles_tab(app, ui);
    }
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.0).color(TEXT3()));
}

fn field(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        let (lr, _) = ui.allocate_exact_size(Vec2::new(118.0, 22.0), Sense::hover());
        let g = elided(ui, tr(label), prop(12.0), TEXT2(), 114.0);
        ui.painter().galley(Pos2::new(lr.left(), lr.center().y - g.size().y / 2.0), g, TEXT2());
        add(ui);
    });
}

fn auto_tab(app: &mut VeloApp, ui: &mut egui::Ui) {
    ui.label(egui::RichText::new(tr("Auto captions")).font(bold(15.0)).color(Color32::WHITE));
    ui.add_space(2.0);
    hint(ui, tr("Recognises the speech in your video and turns it into animated captions. It runs on this computer: nothing is uploaded."));
    ui.add_space(8.0);
    let engine = app.captions.engine();
    if engine.is_none() {
        egui::Frame::new().fill(Color32::from_rgb(0x4a, 0x1d, 0x24)).corner_radius(cr(8)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.label(egui::RichText::new(format!("{}  {}", ph::WARNING, tr("Speech recognition is not installed"))).font(bold(12.5)).color(Color32::WHITE));
            ui.label(
                egui::RichText::new(tr("VeloMov needs whisper-cli (whisper.cpp). Run the installer again, or put whisper-cli in your PATH."))
                    .size(11.0)
                    .color(TEXT()),
            );
            if ui.small_button(tr("Check again")).clicked() {
                app.captions.engine = None;
            }
        });
        ui.add_space(8.0);
    }
    let w = ui.available_width() - 128.0;
    field(ui, "Spoken language", |ui| {
        let cur = transcribe::LANGUAGES.iter().find(|l| l.0 == app.settings.caption_lang).map(|l| l.1).unwrap_or("auto");
        egui::ComboBox::from_id_salt("cap-lang").width(w).selected_text(tr(cur)).height(300.0).show_ui(ui, |ui| {
            for (code, name) in transcribe::LANGUAGES {
                if ui.selectable_label(app.settings.caption_lang == *code, tr(name)).clicked() {
                    app.settings.caption_lang = code.to_string();
                    app.settings.save();
                }
            }
        });
    });
    let has_sel = !app.sel.clips.is_empty();
    if !has_sel {
        app.captions.source = 0;
    }
    field(ui, "Audio", |ui| {
        let opts = [tr("Whole timeline"), tr("Selected clips")];
        egui::ComboBox::from_id_salt("cap-src").width(w).selected_text(opts[app.captions.source.min(1)]).show_ui(ui, |ui| {
            ui.selectable_value(&mut app.captions.source, 0, opts[0]);
            ui.add_enabled_ui(has_sel, |ui| {
                ui.selectable_value(&mut app.captions.source, 1, opts[1]).on_disabled_hover_text(tr("Select clips on the timeline first"));
            });
        });
    });
    field(ui, "Caption length", |ui| {
        let cur = LENGTHS[app.captions.length.min(LENGTHS.len() - 1)].0;
        egui::ComboBox::from_id_salt("cap-len").width(w).selected_text(tr(cur)).show_ui(ui, |ui| {
            for (i, (name, _)) in LENGTHS.iter().enumerate() {
                ui.selectable_value(&mut app.captions.length, i, tr(name));
            }
        });
    });
    field(ui, "Style", |ui| {
        let cur = catalog::caption_style(&app.captions.style).map(|s| s.name).unwrap_or("Karaoke");
        egui::ComboBox::from_id_salt("cap-style").width(w).selected_text(tr(cur)).height(320.0).show_ui(ui, |ui| {
            for s in catalog::CAPTION_STYLES {
                if ui.selectable_label(app.captions.style == s.id, tr(s.name)).on_hover_text(tr(s.desc)).clicked() {
                    app.captions.style = s.id.into();
                }
            }
        });
    });
    ui.checkbox(&mut app.captions.replace, tr("Replace earlier auto captions"));
    ui.add_space(6.0);
    // model line
    let m = transcribe::model(&app.settings.whisper_model);
    ui.horizontal(|ui| {
        let ready = transcribe::model_ready(m.id);
        let state = if ready {
            format!("{} {}", ph::CHECK, tr("ready"))
        } else {
            format!("{} {} {}", ph::DOWNLOAD_SIMPLE, settings_bytes(m.bytes), tr("download on first use"))
        };
        ui.label(egui::RichText::new(format!("{} {} · {}", tr("Model:"), m.name, state)).size(11.5).color(TEXT2()));
        if ui.small_button(tr("Change")).on_hover_text(tr("Choose the speech model in Settings")).clicked() {
            app.open_settings();
        }
    });
    ui.add_space(8.0);
    if let Some((job, _)) = app.captions.job.clone() {
        let f = job.frac();
        ui.label(egui::RichText::new(format!("{}  {:.0}%", tr(&job.stage()), f * 100.0)).color(TEXT()));
        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 6.0), Sense::hover());
        ui.painter().rect_filled(r, cr(3), PANEL3());
        ui.painter().rect_filled(Rect::from_min_size(r.min, Vec2::new(r.width() * f, r.height())), cr(3), ACCENT());
        ui.add_space(6.0);
        if ui.button(format!("{}  {}", ph::X, tr("Cancel"))).clicked() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    } else {
        let enabled = engine.is_some() && app.project.duration() > 0;
        let resp = ui.add_enabled_ui(enabled, |ui| pill(ui, ph::SUBTITLES, tr("Generate captions"), ACCENT(), ON_ACCENT())).inner;
        if resp.clicked() {
            app.start_captions();
        }
    }
    ui.add_space(12.0);
    hint(ui, tr("Tip: fix a misheard word by editing the caption text; the timing of the words is kept. Change the look of all captions in the Styles tab."));
}

fn settings_bytes(b: u64) -> String {
    super::settings_ui::fmt_bytes(b)
}

const SAMPLE: &str = "Captions that move with the voice";

fn styles_tab(app: &mut VeloApp, ui: &mut egui::Ui) {
    let targets: Vec<u64> =
        app.sel.clips.iter().copied().filter(|id| matches!(app.project.clip(*id).map(|c| &c.kind), Some(ClipKind::Text(_)))).collect();
    let note = if targets.is_empty() {
        tr("Click a style to use it on all auto captions (and on new ones).")
    } else {
        tr("Click a style to use it on the selected text.")
    };
    hint(ui, note);
    ui.add_space(6.0);
    let w = ui.available_width();
    let cols = if w > 330.0 { 2 } else { 1 };
    let tw = ((w - 8.0 * (cols as f32 - 1.0)) / cols as f32).floor();
    let th = 70.0;
    let words: Vec<&str> = tr(SAMPLE).split_whitespace().collect();
    let time = ui.input(|i| i.time);
    for row in catalog::CAPTION_STYLES.chunks(cols) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            for s in row {
                let (r, resp) = ui.allocate_exact_size(Vec2::new(tw, th + 18.0), Sense::click());
                let pic = Rect::from_min_size(r.min, Vec2::new(tw, th));
                let on = app.captions.style == s.id;
                ui.painter().rect_filled(pic, cr(8), Color32::from_rgb(0x1c, 0x1d, 0x22));
                // animate the hovered tile only (idle editor = no repaints)
                let k = if resp.hovered() {
                    ui.ctx().request_repaint_after(Duration::from_millis(33));
                    (time / 0.45) % (words.len() as f64 + 2.0)
                } else {
                    1.4
                };
                draw_sample(ui, pic.shrink(6.0), s, &words, k as f32);
                let stroke = if on {
                    Stroke::new(2.0_f32, ACCENT())
                } else if resp.hovered() {
                    Stroke::new(1.0_f32, Color32::from_white_alpha(90))
                } else {
                    Stroke::new(1.0_f32, Color32::from_white_alpha(20))
                };
                ui.painter().rect_stroke(pic, cr(8), stroke, StrokeKind::Inside);
                let g = elided(ui, tr(s.name), prop(11.5), if on { Color32::WHITE } else { TEXT2() }, tw);
                ui.painter().galley(Pos2::new(r.left() + 2.0, pic.bottom() + 3.0), g, TEXT2());
                if resp.on_hover_text(tr(s.desc)).clicked() {
                    let n = app.apply_caption_style(s, &targets);
                    if n > 0 {
                        app.toast(&format!("{} · {}", tr(s.name), n));
                    }
                }
            }
        });
        ui.add_space(6.0);
    }
}

fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

/// A quick egui drawing of a caption style: `k` is the word being spoken (fraction = progress).
fn draw_sample(ui: &egui::Ui, r: Rect, s: &CaptionStyle, words: &[&str], k: f32) {
    let p = ui.painter_at(r);
    let active = k.floor() as usize;
    let prog = k.fract();
    let size = 15.0;
    let font = |scale: f32| bold(size * scale);
    let galleys: Vec<_> = words.iter().map(|w| ui.fonts(|f| f.layout_no_wrap(w.to_string(), font(1.0), Color32::WHITE))).collect();
    let space = size * 0.3;
    // two lines at most
    let mut lines: Vec<Vec<usize>> = vec![vec![]];
    let mut lw = 0.0;
    for (i, g) in galleys.iter().enumerate() {
        if lw + g.size().x > r.width() - 12.0 && !lines.last().unwrap().is_empty() {
            lines.push(vec![]);
            lw = 0.0;
        }
        lw += g.size().x + space;
        lines.last_mut().unwrap().push(i);
    }
    let lh = size * 1.35;
    let total_h = lh * lines.len() as f32;
    let mut pos = vec![Pos2::ZERO; words.len()];
    for (li, line) in lines.iter().enumerate() {
        let w: f32 = line.iter().map(|i| galleys[*i].size().x).sum::<f32>() + space * (line.len() as f32 - 1.0);
        let mut x = r.center().x - w / 2.0;
        let y = r.center().y - total_h / 2.0 + li as f32 * lh;
        for i in line {
            pos[*i] = Pos2::new(x, y);
            x += galleys[*i].size().x + space;
        }
    }
    if s.bg.is_some() {
        let all = pos.iter().zip(&galleys).fold(Rect::NOTHING, |a, (p, g)| a.union(Rect::from_min_size(*p, g.size())));
        p.rect_filled(all.expand2(Vec2::new(8.0, 4.0)), cr(6), c32(s.bg.unwrap()));
    }
    let hl = c32(s.hl);
    for (i, w) in words.iter().enumerate() {
        let spoken = i < active || (i == active);
        let is_active = i == active;
        let mut col = c32(s.color);
        let mut scale = 1.0;
        let mut dy = 0.0;
        let mut alpha = 1.0f32;
        let rect = Rect::from_min_size(pos[i], galleys[i].size());
        match s.id {
            "karaoke" if spoken => col = hl,
            "highlight" | "glow" | "bubble" if is_active => col = hl,
            "pop" if is_active => {
                col = hl;
                scale = 1.18;
            }
            "bounce" => {
                if !spoken {
                    alpha = 0.0;
                } else if is_active {
                    col = hl;
                    dy = -4.0 * (1.0 - prog).powi(2);
                }
            }
            "reveal" if !spoken => alpha = 0.0,
            "dim" if !spoken => alpha = 0.35,
            "single" => {
                if !is_active {
                    continue;
                }
                scale = 1.5;
            }
            _ => {}
        }
        if is_active && s.id == "box" {
            p.rect_filled(rect.expand2(Vec2::new(4.0, 1.0)), cr(5), hl);
        }
        if is_active && s.id == "underline" {
            p.rect_filled(Rect::from_min_size(Pos2::new(rect.left(), rect.bottom() - 1.0), Vec2::new(rect.width(), 3.0)), cr(2), hl);
        }
        let g = ui.fonts(|f| f.layout_no_wrap(w.to_string(), font(scale), Color32::WHITE));
        let mut at = Pos2::new(rect.center().x - g.size().x / 2.0, rect.center().y - g.size().y / 2.0 + dy);
        if s.id == "single" {
            at = r.center() - g.size() / 2.0;
        }
        if is_active && s.id == "glow" {
            for d in [Vec2::new(-1.5, 0.0), Vec2::new(1.5, 0.0), Vec2::new(0.0, -1.5), Vec2::new(0.0, 1.5)] {
                p.galley(at + d * 1.5, g.clone(), hl.gamma_multiply(0.35));
            }
        }
        if let Some((sc, _)) = s.stroke {
            for d in [Vec2::new(-1.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(0.0, -1.0), Vec2::new(0.0, 1.0)] {
                p.galley_with_override_text_color(at + d, g.clone(), c32(sc).gamma_multiply(alpha));
            }
        }
        p.galley_with_override_text_color(at, g, col.gamma_multiply(alpha));
    }
}

// ------------------------------------------------------------------ inspector

/// Caption section at the top of the text inspector; returns true when `td` changed.
pub fn inspector_section(app: &mut VeloApp, ui: &mut egui::Ui, c: &Clip, td: &mut TextData) -> bool {
    let mut changed = false;
    section(ui, "Caption animation", None);
    label_row(ui, "Style", |ui| {
        let cur = catalog::caption_style(&td.caption).map(|s| tr(s.name)).unwrap_or(tr("None"));
        egui::ComboBox::from_id_salt(("cap-anim", c.id)).width(ui.available_width() - 6.0).selected_text(cur).height(320.0).show_ui(ui, |ui| {
            if ui.selectable_label(td.caption.is_empty(), tr("None")).clicked() {
                td.caption.clear();
                changed = true;
            }
            for s in catalog::CAPTION_STYLES {
                if ui.selectable_label(td.caption == s.id, tr(s.name)).on_hover_text(tr(s.desc)).clicked() {
                    // keep the user's colours: only switch the animation
                    td.caption = s.id.into();
                    changed = true;
                }
            }
        });
    });
    if !td.caption.is_empty() {
        label_row(ui, "Highlight", |ui| {
            changed |= color_button(ui, egui::Id::new(("cap-hl", c.id)), &mut td.hl_color, true);
        });
    }
    if !td.words.is_empty() {
        ui.horizontal(|ui| {
            if ui.button(format!("{}  {}", ph::COPY, tr("Apply look to all captions"))).on_hover_text(tr("Style, colours, font, size and position")).clicked() {
                let mut src = c.clone();
                src.kind = ClipKind::Text(td.clone());
                app.caption_look_to_all(&src);
                app.toast(tr("Applied to all captions"));
            }
        });
        let local = app.playhead - c.start + c.src_in;
        let spans = td.word_spans((0, c.src_in + c.duration));
        // the word being spoken, to help fixing it
        if let Some(sp) = spans.iter().rev().find(|s| s.2 <= local).filter(|_| c.contains(app.playhead)) {
            hint(ui, &format!("{} “{}”", tr("Now:"), &td.text[sp.0..sp.1]));
        }
    }
    separator(ui);
    changed
}

// ------------------------------------------------------------------ settings

pub fn settings_section(app: &mut VeloApp, ui: &mut egui::Ui) {
    ui.add_space(10.0);
    ui.label(egui::RichText::new(format!("{}  {}", ph::SUBTITLES, tr("Auto captions (speech recognition)"))).font(bold(14.0)).color(Color32::WHITE));
    ui.add_space(4.0);
    hint(
        ui,
        tr("Captions are made with Whisper, on this computer. Bigger models understand better but are slower and use more memory. A model is downloaded once, the first time it is used."),
    );
    ui.add_space(6.0);
    match app.captions.engine() {
        Some(p) => hint(ui, &format!("{} {} {}", ph::CHECK_CIRCLE, tr("Engine:"), p.display())),
        None => {
            ui.label(egui::RichText::new(format!("{} {}", ph::WARNING, tr("Speech recognition is not installed"))).color(Color32::from_rgb(255, 120, 120)));
        }
    }
    ui.add_space(6.0);
    let dl = app.captions.download.clone();
    for m in transcribe::MODELS {
        let on = app.settings.whisper_model == m.id;
        let ready = transcribe::model_ready(m.id);
        let frame = egui::Frame::new()
            .fill(if on { ACCENT_BG() } else { PANEL2() })
            .corner_radius(cr(8))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .stroke(if on { Stroke::new(1.0_f32, ACCENT()) } else { Stroke::NONE });
        let resp = frame
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(if on { ph::RADIO_BUTTON } else { ph::CIRCLE }).size(15.0).color(if on { ACCENT() } else { TEXT3() }));
                    ui.label(egui::RichText::new(m.name).font(bold(13.0)).color(Color32::WHITE));
                    ui.label(egui::RichText::new(fmt_size(m.bytes)).size(11.0).color(TEXT3()));
                    if m.id == transcribe::DEFAULT_MODEL {
                        ui.label(egui::RichText::new(tr("Recommended")).size(10.5).color(ACCENT()));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match &dl {
                            Some(d) if d.model == m.id => {
                                if ui.small_button(ph::X).on_hover_text(tr("Cancel")).clicked() {
                                    d.cancel.store(true, Ordering::Relaxed);
                                }
                                ui.label(egui::RichText::new(format!("{:.0}%", d.frac() * 100.0)).color(TEXT()));
                            }
                            _ if ready => {
                                if ui.small_button(ph::TRASH).on_hover_text(tr("Delete the downloaded model")).clicked() {
                                    let _ = std::fs::remove_file(transcribe::model_path(m.id));
                                }
                                ui.label(egui::RichText::new(format!("{} {}", ph::CHECK, tr("Downloaded"))).size(11.0).color(TEXT2()));
                            }
                            _ => {
                                let busy = dl.is_some();
                                if ui.add_enabled(!busy, egui::Button::new(format!("{} {}", ph::DOWNLOAD_SIMPLE, tr("Download"))).small()).clicked() {
                                    app.captions.download = Some(transcribe::download(m.id));
                                }
                            }
                        }
                    });
                });
                ui.label(egui::RichText::new(tr(m.desc)).size(11.0).color(TEXT2()));
            })
            .response;
        if ui.interact(resp.rect, egui::Id::new(("model-card", m.id)), Sense::click()).clicked() && !on {
            app.settings.whisper_model = m.id.to_string();
            app.settings.save();
        }
        ui.add_space(4.0);
    }
    ui.add_space(4.0);
    if ui
        .checkbox(&mut app.settings.whisper_gpu, tr("Use the graphics card (much faster)"))
        .on_hover_text(tr("Works when the speech engine was built with GPU support (Vulkan). Turn it off if captions fail."))
        .changed()
    {
        app.settings.save();
    }
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr("Processor threads")).color(TEXT2()));
        let mut t = app.settings.whisper_threads;
        let r = ui.add(egui::DragValue::new(&mut t).range(0..=64).custom_formatter(|v, _| if v < 0.5 { tr("Auto").to_string() } else { format!("{}", v as u32) }));
        if r.changed() {
            app.settings.whisper_threads = t;
            app.settings.save();
        }
    });
    ui.horizontal(|ui| {
        let dir = transcribe::models_dir();
        ui.label(egui::RichText::new(dir.to_string_lossy()).color(TEXT3()).size(10.5));
        if ui.small_button(ph::FOLDER_OPEN).on_hover_text(tr("Open folder")).clicked() {
            util::open_path(&dir);
        }
    });
}

fn fmt_size(b: u64) -> String {
    super::settings_ui::fmt_bytes(b)
}
