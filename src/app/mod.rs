//! The editor application (egui).

pub mod captions;
pub mod export_ui;
pub mod home;
pub mod i18n;
pub mod inspector;
pub mod library;
pub mod new_project;
pub mod player;
pub mod script;
pub mod settings_ui;
pub mod shortcuts;
pub mod theme;
pub mod timeline;
pub mod widgets;

use crate::engine::{gpu::Tex, Engine, Target};
use crate::export;
use crate::media::{self, audio::AudioLib, proxy::ProxyJob, thumbs::Thumbs};
use crate::model::edit::Target as Place;
use crate::model::*;
use crate::playback::Player;
use crate::preview::SampleFrames;
use crate::util::{self, Us, SEC};
use eframe::egui::{self, Color32};
use eframe::egui_wgpu;
use eframe::wgpu;
use i18n::tr;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Home,
    Editor,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum LeftTab {
    Media,
    Audio,
    Text,
    Captions,
    Stickers,
    Effects,
    Transitions,
    Filters,
    Adjust,
}

impl LeftTab {
    pub const ALL: [LeftTab; 9] = [
        LeftTab::Media,
        LeftTab::Audio,
        LeftTab::Text,
        LeftTab::Captions,
        LeftTab::Stickers,
        LeftTab::Effects,
        LeftTab::Transitions,
        LeftTab::Filters,
        LeftTab::Adjust,
    ];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RightTab {
    Video,
    Audio,
    Speed,
    Animation,
    Adjust,
}

#[derive(Default, Clone, Debug)]
pub struct Selection {
    pub clips: Vec<u64>,
    /// Transition selected: id of the clip that owns it (the left clip).
    pub transition: Option<u64>,
}

impl Selection {
    pub fn one(&self) -> Option<u64> {
        if self.clips.len() == 1 {
            Some(self.clips[0])
        } else {
            None
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DragItem {
    Media(u64),
    Sfx(&'static str),
    Text(&'static str),
    Sticker(&'static str),
    Effect(&'static str),
    Filter(&'static str),
    Transition(&'static str),
    Adjust(usize),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub lang: String,
    /// VA-API hardware decoding (falls back to software per file).
    pub hw_decode: bool,
    pub image_dur: f64,
    pub autosave: bool,
    pub export: export::Settings,
    pub master_volume: f32,
    /// Preview quality: 0 = high, 1 = medium (proxies), 2 = low (proxies, lighter rendering).
    pub preview_quality: u8,
    /// Make light proxies for heavy media (4K, HEVC, high frame rate) in the background.
    pub auto_proxy: bool,
    /// Colour theme id (see `theme::THEMES`) and accent colour.
    pub theme: String,
    pub accent: [u8; 3],
    /// Delete cache files unused for this many days (0 = never).
    pub cache_days: u32,
    /// "Remove silences": quieter than this (dBFS), at least this long (s), keeping this margin (s).
    pub silence_db: f32,
    pub silence_min: f64,
    pub silence_pad: f64,
    /// Auto captions: Whisper model id, spoken language ("auto" = detect), threads (0 = auto).
    pub whisper_model: String,
    pub caption_lang: String,
    pub whisper_threads: u32,
    pub whisper_gpu: bool,
    /// Format of this file (older files lack it: 0, and are migrated on load).
    #[serde(default)]
    pub version: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lang: "auto".into(),
            hw_decode: true,
            image_dur: 5.0,
            autosave: true,
            export: export::Settings::default(),
            master_volume: 1.0,
            preview_quality: 1,
            auto_proxy: true,
            theme: theme::DEFAULT_THEME.into(),
            accent: theme::DEFAULT_ACCENT,
            cache_days: 7,
            silence_db: -40.0,
            silence_min: 0.5,
            silence_pad: 0.15,
            whisper_model: crate::media::transcribe::DEFAULT_MODEL.into(),
            caption_lang: "auto".into(),
            whisper_threads: 0,
            whisper_gpu: true,
            version: Self::VERSION,
        }
    }
}

#[cfg(test)]
mod settings_tests {
    use super::Settings;

    #[test]
    fn old_settings_get_hw_decoding() {
        let old = r#"{"lang":"auto","hw_decode":false,"image_dur":5.0}"#;
        let s = Settings::migrate(serde_json::from_str(old).unwrap());
        assert!(s.hw_decode);
        assert_eq!(s.version, Settings::VERSION);
        assert_eq!(s.preview_quality, 1);
        // a current file keeps the user's choice
        let cur = serde_json::to_string(&Settings { hw_decode: false, ..Settings::default() }).unwrap();
        assert!(!Settings::migrate(serde_json::from_str(&cur).unwrap()).hw_decode);
    }
}

impl Settings {
    fn path() -> PathBuf {
        util::config_dir().join("settings.json")
    }
    const VERSION: u32 = 2;

    pub fn load() -> Settings {
        Self::migrate(std::fs::read_to_string(Self::path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default())
    }

    fn migrate(mut s: Settings) -> Settings {
        if s.version < 2 {
            // 0.1 saved hardware decoding off by default; it is now on (with automatic fallback)
            s.hw_decode = true;
            s.version = Self::VERSION;
        }
        s
    }
    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(Self::path(), s);
        }
    }
}

pub struct Display {
    pub tex: Tex,
    pub id: egui::TextureId,
    pub w: u32,
    pub h: u32,
}

/// Where a batch of imported files goes on the timeline (files keep their order).
#[derive(Clone, Copy, Debug)]
pub struct DropAt {
    pub batch: u64,
    /// Start of the videos/photos (`APPEND` = after the last clip of the main track).
    pub t: Us,
    /// Start of the audio files (they go side by side with the pictures, not after them).
    pub audio_t: Us,
    /// Track (display index) to use, `None` = main track for video, a free audio track for audio.
    pub track: Option<usize>,
}

impl DropAt {
    pub const APPEND: Us = i64::MAX / 2;
}

pub enum BgMsg {
    Probed { media: MediaAsset, place: Option<DropAt> },
    ProbeFailed(String, String),
    Files { purpose: FilePurpose, paths: Vec<PathBuf> },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilePurpose {
    Import,
    OpenProject,
    ExportDir,
    Relink(u64),
}

pub struct TimelineState {
    pub pps: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub split_mode: bool,
    pub drag: Option<timeline::TlDrag>,
    pub snap_line: Option<Us>,
    pub hover_time: Option<Us>,
    pub fit_request: bool,
    pub in_out: (Option<Us>, Option<Us>),
    pub width: f32,
    /// Geometry of the last drawn frame (used by UI test scripts).
    pub rows: Vec<(usize, egui::Rect)>,
    pub x0: f32,
    pub ruler_y: f32,
    /// Clip clicked last (start of a Shift+click range).
    pub anchor: Option<u64>,
    /// Clip to select alone if the press inside a multiple selection ends without dragging.
    pub reselect: Option<u64>,
}

impl Default for TimelineState {
    fn default() -> Self {
        TimelineState {
            pps: 60.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            split_mode: false,
            drag: None,
            snap_line: None,
            hover_time: None,
            fit_request: true,
            in_out: (None, None),
            width: 800.0,
            rows: vec![],
            x0: 0.0,
            ruler_y: 0.0,
            anchor: None,
            reselect: None,
        }
    }
}

pub struct VeloApp {
    pub rs: egui_wgpu::RenderState,
    pub engine: Engine,
    pub frames: media::Frames,
    pub audio: AudioLib,
    pub player: Player,
    pub thumbs: Thumbs,
    pub proxies: media::proxy::ProxyManager,
    proxy_version: u64,
    pub thumb_tex: HashMap<(u64, u32), egui::TextureHandle>,
    pub preview: Option<Display>,
    pub tile_tex: HashMap<String, egui::TextureHandle>,
    pub tile_anim: Option<(String, Display, Instant)>,
    pub tile_projects: HashMap<String, (Project, Us, Us)>,
    pub samples: SampleFrames,
    pub screen: Screen,
    pub project: Project,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    last_save: Instant,
    pub undo: Vec<Project>,
    pub redo: Vec<Project>,
    pre_edit: Option<Project>,
    pub rev: u64,
    pub sel: Selection,
    pub playhead: Us,
    pub playing: bool,
    pub shuttle: f64,
    shuttle_start: Option<(Instant, Us)>,
    last_render: Option<(u64, Us, u32, u32)>,
    pub render_pending: bool,
    plan_rev: u64,
    last_plan_build: Instant,
    pub left_tab: LeftTab,
    pub left_cat: HashMap<LeftTab, usize>,
    pub right_tab: RightTab,
    pub video_sub: usize,
    pub tl: TimelineState,
    pub drag: Option<DragItem>,
    pub clipboard: Vec<(TrackKind, bool, Clip)>,
    pub attr_clip: Option<Clip>,
    pub toasts: Vec<(String, Instant, bool)>,
    pub export_dialog: Option<export_ui::ExportDialog>,
    pub export_job: Option<Arc<export::Progress>>,
    pub proxy_jobs: Vec<Arc<ProxyJob>>,
    pub bg_tx: Sender<BgMsg>,
    bg_rx: Receiver<BgMsg>,
    pub pending_probes: usize,
    pub settings: Settings,
    pub show_shortcuts: bool,
    pub fullscreen: bool,
    pub home: home::HomeState,
    pub font_list: Option<Vec<String>>,
    pub junction: Option<u64>,
    pub sfx_preview: Option<Instant>,
    pub rename_project: Option<String>,
    pub preview_axis_time: Option<Us>,
    last_gc: Instant,
    pub show_about: bool,
    pub show_settings: bool,
    pub new_dialog: Option<new_project::NewProject>,
    silence_cache: Option<((u64, u64, u64, u64, u64), Vec<(Us, Us)>)>,
    /// Bytes in the cache folder (measured when the settings open).
    pub cache_size: Option<u64>,
    pub captions: captions::CaptionsState,
    pub pick_files_later: bool,
    pub pending_sfx: Option<(String, Instant)>,
    /// next free time per import batch (pictures and audio separately)
    batch_next: HashMap<(u64, bool), Us>,
    pub script: Option<script::Script>,
    last_title: String,
    quitting: bool,
    pub brand: widgets::BrandTex,
}

pub fn run(args: Vec<String>) -> anyhow::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("VeloMov")
        .with_app_id("velomov")
        .with_inner_size([1600.0, 900.0])
        .with_min_inner_size([1100.0, 680.0])
        .with_maximized(true);
    if let Ok(icon) = image::load_from_memory(include_bytes!("../../assets/velomov.png")) {
        let icon = icon.to_rgba8();
        let (w, h) = icon.dimensions();
        viewport = viewport.with_icon(egui::IconData { rgba: icon.into_raw(), width: w, height: h });
    }
    let force_wayland = args.iter().any(|a| a == "--wayland") || std::env::var_os("VELOMOV_WAYLAND").is_some();
    let use_x11 = !force_wayland && std::env::var_os("DISPLAY").is_some() && std::env::var_os("VELOMOV_NO_X11").is_none();
    let mut options = eframe::NativeOptions { viewport, renderer: eframe::Renderer::Wgpu, vsync: true, ..Default::default() };
    options.wgpu_options.wgpu_setup = egui_wgpu::WgpuSetup::CreateNew(egui_wgpu::WgpuSetupCreateNew {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    });
    #[cfg(all(unix, not(target_os = "macos")))]
    if use_x11 {
        // X11 (XWayland) gives drag & drop of files from the file manager.
        options.event_loop_builder = Some(Box::new(|b| {
            use winit::platform::x11::EventLoopBuilderExtX11;
            b.with_x11();
        }));
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let _ = use_x11;
    // absolute paths, so projects never store paths relative to the launch directory
    let open: Vec<PathBuf> =
        args.iter().filter(|a| !a.starts_with("--")).map(|a| std::fs::canonicalize(a).unwrap_or_else(|_| PathBuf::from(a))).collect();
    eframe::run_native("VeloMov", options, Box::new(move |cc| Ok(Box::new(VeloApp::new(cc, open))))).map_err(|e| anyhow::anyhow!("{}", e))
}

impl VeloApp {
    pub fn new(cc: &eframe::CreationContext, open: Vec<PathBuf>) -> Self {
        let settings = Settings::load();
        let lang = match settings.lang.as_str() {
            "pt" => i18n::Lang::Pt,
            "en" => i18n::Lang::En,
            _ => i18n::system_lang(),
        };
        i18n::set_lang(lang);
        theme::set_palette(&settings.theme, settings.accent);
        theme::apply(&cc.egui_ctx);
        if settings.cache_days > 0 {
            // before anything opens: nothing in the cache is in use yet
            let (n, bytes) = util::clean_cache(settings.cache_days);
            if n > 0 {
                crate::vlog!("cache: removed {} files unused for {} days ({:.0} MB)", n, settings.cache_days, bytes as f64 / 1e6);
            }
        }
        egui_extras_install(&cc.egui_ctx);
        crate::engine::raster::warm_up_fonts();
        media::video::set_hwaccel(settings.hw_decode && std::env::var_os("VELOMOV_NO_HW").is_none());
        let rs = cc.wgpu_render_state.clone().expect("VeloMov needs the wgpu renderer");
        let engine = Engine::new(rs.device.clone(), rs.queue.clone());
        let player = Player::new();
        player.set_master(settings.master_volume);
        let (bg_tx, bg_rx) = channel();
        let mut app = VeloApp {
            rs,
            engine,
            frames: media::Frames::new(false),
            audio: AudioLib::new(),
            player,
            thumbs: Thumbs::new(2),
            proxies: media::proxy::ProxyManager::new(),
            proxy_version: 0,
            thumb_tex: HashMap::new(),
            preview: None,
            tile_tex: HashMap::new(),
            tile_anim: None,
            tile_projects: HashMap::new(),
            samples: SampleFrames::new(480, 270),
            screen: Screen::Home,
            project: Project::default(),
            path: None,
            dirty: false,
            last_save: Instant::now(),
            undo: vec![],
            redo: vec![],
            pre_edit: None,
            rev: 1,
            sel: Selection::default(),
            playhead: 0,
            playing: false,
            shuttle: 1.0,
            shuttle_start: None,
            last_render: None,
            render_pending: false,
            plan_rev: 0,
            last_plan_build: Instant::now(),
            left_tab: LeftTab::Media,
            left_cat: HashMap::new(),
            right_tab: RightTab::Video,
            video_sub: 0,
            tl: TimelineState::default(),
            drag: None,
            clipboard: vec![],
            attr_clip: None,
            toasts: vec![],
            export_dialog: None,
            export_job: None,
            proxy_jobs: vec![],
            bg_tx,
            bg_rx,
            pending_probes: 0,
            settings,
            show_shortcuts: false,
            fullscreen: false,
            home: home::HomeState::default(),
            font_list: None,
            junction: None,
            sfx_preview: None,
            rename_project: None,
            preview_axis_time: None,
            last_gc: Instant::now(),
            show_about: false,
            show_settings: false,
            new_dialog: None,
            silence_cache: None,
            cache_size: None,
            captions: Default::default(),
            pick_files_later: false,
            pending_sfx: None,
            batch_next: HashMap::new(),
            script: script::Script::from_env(),
            last_title: String::new(),
            quitting: false,
            brand: widgets::BrandTex::default(),
        };
        app.home.refresh();
        let media: Vec<PathBuf> = open.iter().filter(|p| util::is_media_path(&p.to_string_lossy())).cloned().collect();
        if let Some(proj) = open.iter().find(|p| !util::is_media_path(&p.to_string_lossy())) {
            if let Err(e) = app.open_project(proj) {
                app.toast_err(&format!("{}: {}", tr("Could not open project"), e));
            }
        } else if !media.is_empty() {
            // files given on the command line go to the main track in order
            app.new_project();
            for path in media {
                match media::probe(&path.to_string_lossy()) {
                    Ok(m) => {
                        let (id, mp, kind, dur, aspect) =
                            (m.id, m.path.clone(), m.kind, util::us_to_s(m.duration), m.width as f64 / m.height.max(1) as f64);
                        if m.has_audio {
                            let _ = app.audio.base(&m.path);
                        }
                        app.edit(|p| p.media.push(m));
                        app.request_thumbs(id, &mp, kind, dur, aspect);
                        let place = if kind == MediaKind::Audio { Place::Overlay(0) } else { Place::Main(i64::MAX) };
                        app.place_media(id, place);
                    }
                    Err(e) => app.toast_err(&format!("{} {}: {}", tr("Cannot import"), path.display(), e)),
                }
            }
            app.flush_edit();
            app.request_proxies();
        }
        app
    }

    // ------------------------------------------------------------------ toasts

    pub fn toast(&mut self, msg: &str) {
        self.toasts.push((msg.to_string(), Instant::now(), false));
    }
    pub fn toast_err(&mut self, msg: &str) {
        self.toasts.push((msg.to_string(), Instant::now(), true));
    }

    // ------------------------------------------------------------------ editing + history

    /// Apply a change to the project (coalesced into one undo step per interaction).
    pub fn edit<R>(&mut self, f: impl FnOnce(&mut Project) -> R) -> R {
        if self.pre_edit.is_none() {
            self.pre_edit = Some(self.project.clone());
        }
        let r = f(&mut self.project);
        self.rev += 1;
        self.dirty = true;
        r
    }

    pub fn flush_edit(&mut self) {
        if let Some(pre) = self.pre_edit.take() {
            self.undo.push(pre);
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    fn commit_if_idle(&mut self, ctx: &egui::Context) {
        if self.pre_edit.is_some() {
            let busy = ctx.input(|i| i.pointer.any_down()) || ctx.wants_keyboard_input();
            if !busy {
                self.flush_edit();
            }
        }
    }

    pub fn undo(&mut self) {
        self.flush_edit();
        if let Some(p) = self.undo.pop() {
            let cur = std::mem::replace(&mut self.project, p);
            self.redo.push(cur);
            self.after_history();
            self.toast(tr("Undo"));
        }
    }

    pub fn redo(&mut self) {
        self.flush_edit();
        if let Some(p) = self.redo.pop() {
            let cur = std::mem::replace(&mut self.project, p);
            self.undo.push(cur);
            self.after_history();
            self.toast(tr("Redo"));
        }
    }

    fn after_history(&mut self) {
        self.rev += 1;
        self.dirty = true;
        let p = &self.project;
        self.sel.clips.retain(|id| p.clip(*id).is_some());
        if let Some(t) = self.sel.transition {
            if p.clip(t).map(|c| c.transition.is_none()).unwrap_or(true) {
                self.sel.transition = None;
            }
        }
    }

    pub fn select_only(&mut self, id: u64) {
        self.sel = Selection { clips: vec![id], transition: None };
        self.on_selection_changed();
    }

    pub fn on_selection_changed(&mut self) {
        // pick a sensible inspector tab for the selection
        if let Some(c) = self.sel.one().and_then(|id| self.project.clip(id)) {
            let is_audio_only = c.media_id().and_then(|m| self.project.media(m)).map(|m| m.kind == MediaKind::Audio).unwrap_or(false);
            let tr = self.project.track_of(c.id).map(|t| t.kind);
            if is_audio_only || tr == Some(TrackKind::Audio) {
                if !matches!(self.right_tab, RightTab::Audio | RightTab::Speed) {
                    self.right_tab = RightTab::Audio;
                }
            } else if matches!(c.kind, ClipKind::Effect(_) | ClipKind::Filter(_) | ClipKind::Adjust) {
                self.right_tab = if matches!(c.kind, ClipKind::Adjust) { RightTab::Adjust } else { RightTab::Video };
            } else if self.right_tab == RightTab::Audio && !c.has_audio(&self.project) {
                self.right_tab = RightTab::Video;
            }
        }
    }

    // ------------------------------------------------------------------ projects

    pub fn new_project(&mut self) {
        self.close_project();
        let mut p = Project::new_named(&default_project_name());
        p.fps = 30.0;
        let dir = util::projects_dir().join(&p.id);
        self.path = Some(dir.join("project.vcut"));
        self.project = p;
        self.screen = Screen::Editor;
        self.rev += 1;
        self.dirty = true;
        self.tl = TimelineState::default();
        self.left_tab = LeftTab::Media;
    }

    /// Ask for name, size, frame rate… before a new project starts.
    pub fn open_new_project(&mut self, then_import: bool) {
        self.new_dialog = Some(new_project::NewProject::new(default_project_name(), then_import));
    }

    /// Start a project with the choices of the "New project" window.
    pub fn create_project(&mut self, d: new_project::NewProject) {
        self.new_project();
        let name = d.name.trim();
        let name = if name.is_empty() { d.default_name.clone() } else { name.to_string() };
        let p = &mut self.project;
        p.name = name;
        p.fps = d.fps;
        p.bg_color = d.bg;
        if d.ratio == Ratio::Custom {
            p.set_custom_size(d.custom.0, d.custom.1);
        } else {
            p.set_short_side(d.short);
            p.apply_ratio(d.ratio);
        }
        self.save_project(true);
        if d.then_import {
            self.pick_files(FilePurpose::Import);
        }
    }

    /// Reversed copies live in the cache: mark them as used, or make them again when cleaned.
    pub fn refresh_reversed(&mut self) {
        let mut remake = vec![];
        for m in self.project.media.iter_mut() {
            if let Some(r) = m.reversed.clone() {
                let rp = std::path::Path::new(&r);
                if rp.exists() {
                    util::touch(rp);
                } else {
                    m.reversed = None;
                    remake.push((m.id, m.path.clone(), util::us_to_s(m.duration)));
                }
            }
        }
        for (id, path, dur) in remake {
            let used = self.project.tracks.iter().flat_map(|t| t.clips.iter()).any(|c| c.reverse && c.media_id() == Some(id));
            if used && std::path::Path::new(&path).exists() {
                self.proxy_jobs.push(media::proxy::spawn_reverse(id, &path, dur));
            }
        }
    }

    /// Silent stretches of a clip with the current settings (timeline ranges), `None` while its
    /// audio is still being read. Cached until the clip or the settings change.
    pub fn silences_of(&mut self, id: u64) -> Option<Vec<(Us, Us)>> {
        let c = self.project.clip(id)?.clone();
        let m = c.media_id().and_then(|m| self.project.media(m))?.clone();
        if !m.has_audio {
            return Some(vec![]);
        }
        let s = &self.settings;
        let key = (id, self.rev, s.silence_db.to_bits() as u64, s.silence_min.to_bits(), s.silence_pad.to_bits());
        if let Some((k, v)) = &self.silence_cache {
            if *k == key {
                return Some(v.clone());
            }
        }
        let pcm = self.audio.base(&m.path)?;
        let v = media::audio::silences(&pcm, &c, m.duration, s.silence_db, util::s_to_us(s.silence_min), util::s_to_us(s.silence_pad));
        self.silence_cache = Some((key, v.clone()));
        Some(v)
    }

    /// Cut the silent parts out of a clip (the rest of its track moves up).
    pub fn remove_silences(&mut self, id: u64) {
        match self.silences_of(id) {
            None => self.toast(tr("Still reading the audio, try again in a moment")),
            Some(v) if v.is_empty() => self.toast(tr("No silences found with these settings")),
            Some(v) => {
                let n = v.len();
                let removed = self.edit(|p| p.remove_ranges(id, &v));
                self.flush_edit();
                self.sel = Selection::default();
                self.toast(&format!("{} {} • {:.1} s {}", n, tr("silences removed"), util::us_to_s(removed), tr("shorter")));
            }
        }
    }

    /// Built-in sound effects live in the cache: move projects to the current recipe
    /// (made again if the cache was cleaned).
    pub fn refresh_builtin_sfx(&mut self) {
        for m in self.project.media.iter_mut().filter(|m| m.builtin) {
            let Some(p) = media::sfx::current_path(&m.path) else { continue };
            let ps = p.to_string_lossy().to_string();
            if ps != m.path {
                if let Ok(probed) = media::probe(&ps) {
                    m.duration = probed.duration;
                }
                m.path = ps;
            }
        }
    }

    /// "Clear cache now": everything that is made again on demand.
    pub fn clear_cache(&mut self) {
        let dir = util::cache_dir();
        for sub in ["audio", "reverse", "proxies", "thumbs"] {
            let _ = std::fs::remove_dir_all(dir.join(sub));
        }
        self.proxies.forget_finished();
        self.frames.proxies.clear();
        self.frames.video.stop_all();
        if self.screen == Screen::Editor {
            self.refresh_reversed();
            self.request_proxies();
        }
        self.cache_size = Some(util::dir_size(&dir));
        self.toast(tr("Cache cleared"));
    }

    pub fn open_project(&mut self, path: &std::path::Path) -> anyhow::Result<()> {
        let p = Project::load(path)?;
        self.close_project();
        self.project = p;
        self.path = Some(path.to_path_buf());
        self.screen = Screen::Editor;
        self.rev += 1;
        self.dirty = false;
        self.tl = TimelineState::default();
        let missing: Vec<String> = self
            .project
            .media
            .iter()
            .filter(|m| !m.builtin && !m.path.starts_with("builtin:") && !std::path::Path::new(&m.path).exists())
            .map(|m| m.name.clone())
            .collect();
        if !missing.is_empty() {
            self.toast_err(&format!("{}: {}", tr("Missing media"), missing.join(", ")));
        }
        self.refresh_reversed();
        self.refresh_builtin_sfx();
        let ids: Vec<(u64, String, MediaKind, f64, f64)> = self
            .project
            .media
            .iter()
            .map(|m| (m.id, m.path.clone(), m.kind, util::us_to_s(m.duration), m.width as f64 / m.height.max(1) as f64))
            .collect();
        for (id, path, kind, dur, aspect) in ids {
            self.request_thumbs(id, &path, kind, dur, aspect);
        }
        self.request_proxies();
        Ok(())
    }

    pub fn close_project(&mut self) {
        if self.screen == Screen::Editor {
            self.flush_edit();
            self.save_project_ex(true, true);
        }
        self.player.pause();
        self.playing = false;
        self.frames.video.stop_all();
        self.undo.clear();
        self.redo.clear();
        self.pre_edit = None;
        self.sel = Selection::default();
        self.playhead = 0;
        self.export_dialog = None;
        self.engine.clear_cache();
    }

    pub fn save_project(&mut self, quiet: bool) {
        self.save_project_ex(quiet, !quiet);
    }

    /// Save the project JSON; `cover` also refreshes the home-screen cover from the preview.
    pub fn save_project_ex(&mut self, quiet: bool, cover: bool) {
        let path = match &self.path {
            Some(p) => p.clone(),
            None => {
                let p = util::projects_dir().join(&self.project.id).join("project.vcut");
                self.path = Some(p.clone());
                p
            }
        };
        match self.project.save(&path) {
            Ok(()) => {
                self.dirty = false;
                self.last_save = Instant::now();
                if cover {
                    self.write_cover(&path, self.quitting);
                }
                if !quiet {
                    self.toast(tr("Project saved"));
                }
            }
            Err(e) => self.toast_err(&format!("{}: {}", tr("Save failed"), e)),
        }
    }

    /// Cover = the current preview frame, scaled down and written in the background
    /// (synchronously when the app is quitting).
    fn write_cover(&mut self, path: &std::path::Path, sync: bool) {
        let dur = self.project.duration();
        if dur == 0 {
            return;
        }
        let Some(d) = self.preview.as_ref() else { return };
        let (mut w, mut h) = (d.w, d.h);
        let mut data = self.engine.gpu.read_rgba(&d.tex);
        if is_dark(&data) {
            // e.g. the playhead sits on a fade from black: take a frame a little into the video
            let t = (dur / 10).clamp(SEC / 2, 3 * SEC).min(dur - 1);
            let (cw, ch) = (480, ((480 * h) / w.max(1)).max(2) & !1);
            let mut frames = media::Frames::new(true);
            data = self.engine.render_rgba(&self.project, t, &mut frames, cw, ch);
            frames.video.stop_all();
            (w, h) = (cw, ch);
        }
        let out = path.parent().map(|p| p.join("cover.png"));
        let job = move || {
            let Some(out) = out else { return };
            let Some(img) = image::RgbaImage::from_raw(w, h, data) else { return };
            let k = (320.0 / w.max(1) as f32).min(180.0 / h.max(1) as f32).min(1.0);
            let (tw, th) = (((w as f32 * k) as u32).max(2), ((h as f32 * k) as u32).max(2));
            let small = image::imageops::resize(&img, tw, th, image::imageops::FilterType::Triangle);
            let _ = small.save(out);
        };
        if sync {
            job();
        } else {
            std::thread::spawn(job);
        }
    }

    pub fn back_home(&mut self) {
        self.close_project();
        self.screen = Screen::Home;
        self.home.refresh();
    }

    // ------------------------------------------------------------------ media import

    pub fn pick_files(&mut self, purpose: FilePurpose) {
        let tx = self.bg_tx.clone();
        let start = util::videos_dir();
        std::thread::spawn(move || {
            let dlg = rfd::FileDialog::new().set_directory(&start);
            let paths: Vec<PathBuf> = match purpose {
                FilePurpose::Import => {
                    let mut exts: Vec<&str> = vec![];
                    exts.extend_from_slice(util::VIDEO_EXTS);
                    exts.extend_from_slice(util::IMAGE_EXTS);
                    exts.extend_from_slice(util::AUDIO_EXTS);
                    dlg.set_title("Import media")
                        .add_filter("Media", &exts)
                        .add_filter("All files", &["*"])
                        .pick_files()
                        .unwrap_or_default()
                }
                FilePurpose::Relink(_) => dlg.set_title("Locate media").pick_file().into_iter().collect(),
                FilePurpose::OpenProject => dlg
                    .set_title("Open project")
                    .set_directory(util::projects_dir())
                    .add_filter("VeloMov project", &["vcut", "json"])
                    .pick_file()
                    .into_iter()
                    .collect(),
                FilePurpose::ExportDir => dlg.set_title("Export to").pick_folder().into_iter().collect(),
            };
            let _ = tx.send(BgMsg::Files { purpose, paths });
        });
    }

    /// Probe files in the background and add them to the project (and optionally the timeline).
    /// Import files into the project (probed in the background, in order). With `place`
    /// they are also put on the timeline one after another.
    pub fn import_paths(&mut self, paths: Vec<PathBuf>, place: Option<DropAt>) {
        let mut todo = vec![];
        for p in paths {
            let p = std::fs::canonicalize(&p).unwrap_or(p);
            let s = p.to_string_lossy().to_string();
            if p.is_dir() {
                continue;
            }
            if s.ends_with(".vcut") {
                if let Err(e) = self.open_project(&p) {
                    self.toast_err(&e.to_string());
                }
                return;
            }
            todo.push(s);
        }
        if todo.is_empty() {
            return;
        }
        self.pending_probes += todo.len();
        let known: HashMap<String, MediaAsset> = self.project.media.iter().map(|m| (m.path.clone(), m.clone())).collect();
        let tx = self.bg_tx.clone();
        std::thread::spawn(move || {
            for s in todo {
                // already in the project: reuse the asset (same id) so it is only placed again
                let r = match known.get(&s) {
                    Some(m) => Ok(m.clone()),
                    None => media::probe(&s),
                };
                let msg = match r {
                    Ok(m) => BgMsg::Probed { media: m, place },
                    Err(e) => BgMsg::ProbeFailed(s, e.to_string()),
                };
                if tx.send(msg).is_err() {
                    return;
                }
            }
        });
    }

    /// Put a probed media on the timeline for an import batch (keeps the files in order).
    fn place_in_batch(&mut self, media: u64, at: DropAt) {
        let Some(kind) = self.project.media(media).map(|m| m.kind) else { return };
        let audio = kind == MediaKind::Audio;
        let key = (at.batch, audio);
        let t = *self.batch_next.entry(key).or_insert(if audio { at.audio_t } else { at.t });
        let track_kind = at.track.and_then(|i| self.project.tracks.get(i)).map(|tr| (tr.kind, tr.main));
        let place = match (kind, track_kind) {
            (MediaKind::Audio, Some((TrackKind::Audio, _))) => Place::Track(at.track.unwrap(), t),
            (MediaKind::Audio, _) => Place::Overlay(t),
            (_, Some((TrackKind::Video, false))) => Place::Track(at.track.unwrap(), t),
            (_, Some((_, false))) => Place::Overlay(t),
            _ => Place::Main(t),
        };
        if let Some(id) = self.place_media(media, place) {
            if let Some(c) = self.project.clip(id) {
                self.batch_next.insert(key, c.end());
            }
        }
        // a project started from these files: show all of it
        if at.t == 0 && at.audio_t == 0 {
            self.tl.fit_request = true;
        }
    }

    /// Proxies are only made (and used) at medium/low preview quality.
    fn wants_proxies(&self) -> bool {
        self.settings.auto_proxy && self.settings.preview_quality >= 1
    }

    /// Proxies for every heavy media of the project (when automatic proxies are on).
    pub fn request_proxies(&mut self) {
        if !self.wants_proxies() {
            return;
        }
        for m in &self.project.media {
            self.proxies.request(m);
        }
    }

    /// Progress of the proxies being made for this project (`None` when idle).
    pub fn proxy_progress(&self) -> Option<f32> {
        use media::proxy::ProxyState;
        let mut n = 0;
        let mut sum = 0.0;
        for m in &self.project.media {
            match self.proxies.state(&m.path) {
                Some(ProxyState::Queued) => n += 1,
                Some(ProxyState::Running(f)) => {
                    n += 1;
                    sum += f;
                }
                _ => {}
            }
        }
        (n > 0).then(|| sum / n as f32)
    }

    pub fn set_preview_quality(&mut self, q: u8) {
        self.settings.preview_quality = q.min(2);
        self.settings.save();
        self.request_proxies();
        self.render_pending = true;
    }

    /// Hand the preview-quality choices to the decoders and the compositor.
    fn sync_preview_quality(&mut self, ctx: &egui::Context) {
        let q = self.settings.preview_quality.min(2);
        self.engine.preview_quality = q;
        if self.engine.playing != self.playing {
            // the frame shown after pausing is redrawn with full-quality blur
            self.engine.playing = self.playing;
            self.render_pending = true;
        }
        self.frames.use_proxies = q >= 1;
        self.frames.video.fast = q >= 2;
        self.frames.video.playing = self.playing;
        self.proxies.set_paused(self.playing);
        let v = self.proxies.version();
        if v != self.proxy_version {
            self.proxy_version = v;
            self.frames.proxies = self.proxies.ready();
            self.render_pending = true;
        }
        if self.proxies.busy() {
            ctx.request_repaint_after(Duration::from_millis(400));
        }
    }

    pub fn request_thumbs(&mut self, id: u64, path: &str, kind: MediaKind, dur_s: f64, aspect: f64) {
        match kind {
            MediaKind::Video => {
                let n = Thumbs::count_for(dur_s);
                // library cover first
                self.thumbs.request(id, u32::MAX, path, kind, (dur_s * 0.1).min(1.0), aspect);
                for i in 0..n {
                    self.thumbs.request(id, i, path, kind, Thumbs::time_of(i, n, dur_s), aspect);
                }
            }
            MediaKind::Image => {
                self.thumbs.request(id, u32::MAX, path, kind, 0.0, aspect);
            }
            MediaKind::Audio => {
                let _ = self.audio.base(path);
            }
        }
    }

    pub fn place_media(&mut self, media: u64, place: Place) -> Option<u64> {
        let dur = util::s_to_us(self.settings.image_dur);
        let id = self.edit(|p| p.insert_media(media, place, dur));
        if let Some(id) = id {
            self.select_only(id);
        }
        id
    }

    pub fn add_builtin_sfx(&mut self, id: &'static str) -> Option<u64> {
        let path = media::sfx::path_for(id)?;
        let ps = path.to_string_lossy();
        if let Some(m) = self.project.media.iter().find(|m| m.builtin && m.path == ps) {
            return Some(m.id);
        }
        let mut m = media::probe(&path.to_string_lossy()).ok()?;
        m.builtin = true;
        m.name = crate::model::catalog::SFX.iter().find(|s| s.id == id).map(|s| tr(s.name).to_string()).unwrap_or_default();
        let mid = m.id;
        let _ = self.audio.base(&m.path);
        self.edit(|p| p.media.push(m));
        Some(mid)
    }

    fn poll_background(&mut self, ctx: &egui::Context) {
        while let Ok(msg) = self.bg_rx.try_recv() {
            match msg {
                BgMsg::Probed { media, place } => {
                    self.pending_probes = self.pending_probes.saturating_sub(1);
                    // the same file twice in one import: keep a single media entry
                    let media = match self.project.media.iter().find(|m| m.path == media.path && !m.builtin) {
                        Some(known) => known.clone(),
                        None => media,
                    };
                    let (id, path, kind, dur, aspect) = (
                        media.id,
                        media.path.clone(),
                        media.kind,
                        util::us_to_s(media.duration),
                        media.width as f64 / media.height.max(1) as f64,
                    );
                    if media.has_audio {
                        let _ = self.audio.base(&media.path);
                    }
                    if self.project.media(id).is_none() {
                        if self.wants_proxies() {
                            self.proxies.request(&media);
                        }
                        self.edit(|p| p.media.push(media));
                        self.request_thumbs(id, &path, kind, dur, aspect);
                    }
                    if let Some(at) = place {
                        self.place_in_batch(id, at);
                    }
                    self.flush_edit();
                }
                BgMsg::ProbeFailed(path, err) => {
                    self.pending_probes = self.pending_probes.saturating_sub(1);
                    self.toast_err(&format!("{} {}: {}", tr("Cannot import"), util::file_name(&path), err));
                }
                BgMsg::Files { purpose, paths } => {
                    if paths.is_empty() {
                        continue;
                    }
                    match purpose {
                        FilePurpose::Import => {
                            if self.screen == Screen::Home {
                                self.new_project();
                            }
                            // a fresh project gets the files on the timeline right away
                            let empty = self.project.tracks.iter().all(|t| t.clips.is_empty());
                            let at = empty.then(|| DropAt { batch: util::new_id(), t: 0, audio_t: 0, track: None });
                            self.import_paths(paths, at);
                        }
                        FilePurpose::OpenProject => {
                            if let Err(e) = self.open_project(&paths[0]) {
                                self.toast_err(&format!("{}: {}", tr("Could not open project"), e));
                            }
                        }
                        FilePurpose::ExportDir => {
                            if let Some(d) = self.export_dialog.as_mut() {
                                d.dir = paths[0].clone();
                            }
                        }
                        FilePurpose::Relink(mid) => {
                            let np = paths[0].to_string_lossy().to_string();
                            match media::probe(&np) {
                                Ok(m) => {
                                    self.edit(|p| {
                                        if let Some(old) = p.media_mut(mid) {
                                            old.path = m.path.clone();
                                            old.name = m.name.clone();
                                            old.reversed = None;
                                        }
                                    });
                                    self.thumbs.forget_media(mid);
                                    self.frames.images.forget(&np);
                                    self.frames.video.stop_all();
                                    self.request_thumbs(
                                        mid,
                                        &np,
                                        m.kind,
                                        util::us_to_s(m.duration),
                                        m.width as f64 / m.height.max(1) as f64,
                                    );
                                }
                                Err(e) => self.toast_err(&e.to_string()),
                            }
                        }
                    }
                }
            }
            ctx.request_repaint();
        }
        // thumbnails
        for d in self.thumbs.poll() {
            if let Some(img) = d.img {
                // also a last-resort placeholder for the preview (never a black frame while seeking)
                let mut data = Vec::with_capacity(img.rgba.len());
                for px in img.rgba.chunks_exact(4) {
                    data.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
                let f = Arc::new(crate::engine::Frame { fmt: crate::engine::PixFmt::Rgba, w: img.w, h: img.h, data, uid: util::new_id() });
                let list = self.frames.video.thumbs.entry(d.path.clone()).or_default();
                list.push((d.time, f));
                list.sort_by(|a, b| a.0.total_cmp(&b.0));
                let ci = egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &img.rgba);
                let tex = ctx.load_texture(format!("thumb-{}-{}", d.media, d.idx), ci, egui::TextureOptions::LINEAR);
                self.thumb_tex.insert((d.media, d.idx), tex);
            }
            ctx.request_repaint();
        }
        // reverse proxies
        let mut done = vec![];
        for (i, j) in self.proxy_jobs.iter().enumerate() {
            if j.done.load(std::sync::atomic::Ordering::Relaxed) {
                done.push(i);
            }
        }
        for i in done.into_iter().rev() {
            let j = self.proxy_jobs.remove(i);
            let res = j.result.lock().unwrap().clone();
            match res {
                Some(Ok(path)) => {
                    let mid = j.media;
                    self.edit(|p| {
                        if let Some(m) = p.media_mut(mid) {
                            m.reversed = Some(path);
                        }
                    });
                    self.flush_edit();
                    self.toast(tr("Reverse done"));
                }
                Some(Err(e)) => self.toast_err(&format!("{}: {}", tr("Reverse failed"), e)),
                None => {}
            }
        }
        if !self.proxy_jobs.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(200));
        }
        // files dropped onto the window. The drop position is not known (X11 drag and drop
        // reports none), so: an empty timeline gets everything in order from the start;
        // otherwise videos/photos go after the last clip and music starts at the playhead.
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            if self.screen == Screen::Home {
                if dropped.len() == 1 && dropped[0].extension().map(|e| e == "vcut").unwrap_or(false) {
                    let p = dropped[0].clone();
                    if let Err(e) = self.open_project(&p) {
                        self.toast_err(&e.to_string());
                    }
                    return;
                }
                self.new_project();
            }
            let empty = self.project.tracks.iter().all(|t| t.clips.is_empty());
            if empty {
                let at = DropAt { batch: util::new_id(), t: 0, audio_t: 0, track: None };
                self.import_paths(dropped, Some(at));
            } else {
                let at = DropAt { batch: util::new_id(), t: DropAt::APPEND, audio_t: self.playhead, track: None };
                self.import_paths(dropped, Some(at));
                self.toast(tr("Added to the timeline (Ctrl+Z to undo)"));
            }
        }
    }

    // ------------------------------------------------------------------ playback

    pub fn rebuild_plan(&mut self) {
        let (plan, _complete) = media::audio::build_plan(&self.project, &self.audio);
        self.player.set_plan(plan);
        self.plan_rev = self.rev;
        self.last_plan_build = Instant::now();
    }

    pub fn play(&mut self) {
        let dur = self.project.duration();
        if dur <= 0 {
            return;
        }
        if self.playhead >= dur - self.project.frame_dur() {
            self.playhead = 0;
        }
        self.flush_edit();
        self.rebuild_plan();
        self.sfx_preview = None;
        self.shuttle = 1.0;
        self.shuttle_start = None;
        self.player.play(self.playhead, dur);
        self.playing = true;
    }

    pub fn pause(&mut self) {
        self.player.pause();
        self.playing = false;
        self.shuttle = 1.0;
        self.shuttle_start = None;
        self.playhead = util::snap_frame(self.playhead, self.project.fps);
    }

    pub fn toggle_play(&mut self) {
        if self.playing {
            self.pause();
        } else {
            self.play();
        }
    }

    /// J/K/L shuttle: speed in {-4,-2,-1,1,2,4}.
    pub fn shuttle(&mut self, dir: i32) {
        if dir == 0 {
            self.pause();
            return;
        }
        let cur = if self.playing { self.shuttle } else { 0.0 };
        let next = if cur == 0.0 || cur.signum() as i32 != dir { dir as f64 } else { (cur * 2.0).clamp(-8.0, 8.0) };
        if next == 1.0 {
            self.play();
            return;
        }
        self.player.pause();
        self.playing = true;
        self.shuttle = next;
        self.shuttle_start = Some((Instant::now(), self.playhead));
        self.toast(&format!("{:+.0}x", next));
    }

    fn tick_playback(&mut self, ctx: &egui::Context) {
        if !self.playing {
            return;
        }
        let dur = self.project.duration();
        if let Some((start, from)) = self.shuttle_start {
            let t = from + (start.elapsed().as_micros() as f64 * self.shuttle) as Us;
            self.playhead = t.clamp(0, dur);
            if t <= 0 || t >= dur {
                self.pause();
            }
        } else {
            self.playhead = self.player.time().min(dur);
            if !self.player.playing() {
                self.playing = false;
                self.playhead = dur;
            }
            // keep audio in sync with edits made while playing
            if self.plan_rev != self.rev || (self.last_plan_build.elapsed() > Duration::from_millis(500) && self.audio.busy()) {
                self.rebuild_plan();
            }
        }
        // auto-scroll the timeline with the playhead
        timeline::follow_playhead(self);
        ctx.request_repaint();
    }

    pub fn seek(&mut self, t: Us) {
        let t = t.clamp(0, self.project.duration().max(0));
        if self.playing && self.shuttle_start.is_none() {
            self.playhead = t;
            self.player.play(t, self.project.duration());
        } else {
            self.playhead = t;
            if let Some((_, from)) = self.shuttle_start.as_mut() {
                *from = t;
                self.shuttle_start = Some((Instant::now(), t));
            }
        }
    }

    /// Preview a sound effect (plays only that sound).
    pub fn preview_sfx(&mut self, id: &'static str) {
        if self.playing {
            self.pause();
        }
        let Some(path) = media::sfx::path_for(id) else { return };
        self.pending_sfx = Some((path.to_string_lossy().to_string(), Instant::now()));
        self.poll_sfx();
    }

    /// Start a requested sound-effect preview as soon as its PCM is decoded (never blocks the UI).
    fn poll_sfx(&mut self) {
        let Some((p, since)) = self.pending_sfx.clone() else { return };
        match self.audio.base(&p) {
            Some(pcm) => {
                self.pending_sfx = None;
                let len = pcm.frames as i64;
                let plan = media::audio::MixPlan {
                    clips: vec![media::audio::MixClip {
                        pcm,
                        start: 0,
                        len,
                        src0: 0.0,
                        step: 1.0,
                        gain: 1.0,
                        fade_in: 0,
                        fade_out: 0,
                        vol_keys: vec![],
                        curve: None,
                    }],
                };
                self.player.set_plan(plan);
                self.player.play(0, (len as f64 / 48.0 * 1000.0) as Us);
                self.sfx_preview = Some(Instant::now());
                self.plan_rev = 0;
            }
            None => {
                if since.elapsed() > Duration::from_secs(5) {
                    self.pending_sfx = None;
                }
            }
        }
    }

    // ------------------------------------------------------------------ preview rendering

    pub fn ensure_display(rs: &egui_wgpu::RenderState, engine: &Engine, slot: &mut Option<Display>, w: u32, h: u32) {
        if let Some(d) = slot.as_ref() {
            if d.w == w && d.h == h {
                return;
            }
        }
        let tex = engine.gpu.display_tex(w, h);
        let srgb = tex
            .texture
            .create_view(&wgpu::TextureViewDescriptor { format: Some(wgpu::TextureFormat::Rgba8UnormSrgb), ..Default::default() });
        match slot {
            Some(d) => {
                rs.renderer.write().update_egui_texture_from_wgpu_texture(&rs.device, &srgb, wgpu::FilterMode::Linear, d.id);
                d.tex = tex;
                d.w = w;
                d.h = h;
            }
            None => {
                let id = rs.renderer.write().register_native_texture(&rs.device, &srgb, wgpu::FilterMode::Linear);
                *slot = Some(Display { tex, id, w, h });
            }
        }
    }

    /// Render the program preview at the current playhead into a texture of w x h.
    pub fn render_preview(&mut self, w: u32, h: u32) -> Option<egui::TextureId> {
        let (w, h) = (w.max(16), h.max(16));
        let mut slot = self.preview.take();
        Self::ensure_display(&self.rs, &self.engine, &mut slot, w, h);
        self.preview = slot;
        let t = self.preview_axis_time.unwrap_or(self.playhead);
        let key = (self.rev, t, w, h);
        if self.last_render != Some(key) || self.render_pending || self.playing {
            let d = self.preview.as_ref().unwrap();
            let view = d.tex.view.clone();
            let target = Target { view: &view, w, h };
            let info = self.engine.render(&self.project, t, &mut self.frames, &target, false);
            self.render_pending = info.pending || self.frames.images.has_pending();
            if self.playing && info.pending {
                STAT_MISS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if self.playing && self.shuttle_start.is_none() {
                self.engine.prefetch(&self.project, t, SEC * 3 / 2, &mut self.frames, w, h);
            }
            self.last_render = Some(key);
        }
        self.preview.as_ref().map(|d| d.id)
    }

    /// Static preview image for a library item (rendered once, cached as an egui texture).
    pub fn tile_texture(&mut self, ctx: &egui::Context, key: &str, budget: &mut u32) -> Option<egui::TextureId> {
        if let Some(t) = self.tile_tex.get(key) {
            return Some(t.id());
        }
        if *budget == 0 {
            ctx.request_repaint();
            return None;
        }
        *budget -= 1;
        let (proj, t, _) = self.tile_project(key)?;
        let (w, h) = (192u32, 108u32);
        let data = self.engine.render_rgba(&proj, t, &mut self.samples, w, h);
        let ci = egui::ColorImage::from_rgba_premultiplied([w as usize, h as usize], &data);
        let tex = ctx.load_texture(format!("tile-{}", key), ci, egui::TextureOptions::LINEAR);
        let id = tex.id();
        self.tile_tex.insert(key.to_string(), tex);
        Some(id)
    }

    /// Synthetic project for a library tile key ("tr:<id>", "fx:<id>", "flt:<id>", "an:<id>", "txt:<id>", "st:<id>").
    pub fn tile_project(&mut self, key: &str) -> Option<(Project, Us, Us)> {
        if let Some(p) = self.tile_projects.get(key) {
            return Some(p.clone());
        }
        let (kind, id) = key.split_once(':')?;
        let r = match kind {
            "tr" => {
                let (p, s, e) = crate::preview::transition_project(id);
                (p, s + (e - s) * 35 / 100, e)
            }
            "fx" => (crate::preview::effect_project(id), SEC, 3 * SEC),
            "flt" => (crate::preview::filter_project(id), SEC / 2, SEC),
            "an" => {
                let def = crate::model::catalog::animation(id)?;
                let (p, total) = crate::preview::anim_project(id, def.kind);
                let t = match def.kind {
                    crate::model::catalog::AnimKind::In => total / 4,
                    crate::model::catalog::AnimKind::Out => total * 7 / 10,
                    crate::model::catalog::AnimKind::Combo => total / 5,
                };
                (p, t, total)
            }
            "txt" => {
                let pr = crate::model::catalog::TEXT_PRESETS.iter().find(|p| p.id == id)?;
                (crate::preview::text_project(pr), SEC / 2, SEC)
            }
            "st" => (crate::preview::sticker_project(id), SEC / 2, SEC),
            _ => return None,
        };
        self.tile_projects.insert(key.to_string(), r.clone());
        Some(r)
    }

    /// Animated preview of a hovered tile.
    pub fn tile_anim_texture(&mut self, key: &str) -> Option<egui::TextureId> {
        let (proj, _, end) = self.tile_project(key)?;
        let restart = self.tile_anim.as_ref().map(|(k, _, _)| k != key).unwrap_or(true);
        if restart {
            let mut slot = self.tile_anim.take().map(|(_, d, _)| d);
            Self::ensure_display(&self.rs, &self.engine, &mut slot, 192, 108);
            self.tile_anim = Some((key.to_string(), slot.unwrap(), Instant::now()));
        }
        let (_, d, start) = self.tile_anim.as_ref().unwrap();
        let (start_t, loop_len) = if key.starts_with("tr:") {
            let s = proj.transition_regions().first().map(|r| r.1).unwrap_or(0);
            let lead = SEC * 3 / 10;
            ((s - lead).max(0), (end - s) + 2 * lead)
        } else {
            (0, end.max(SEC))
        };
        let el = start.elapsed().as_micros() as Us;
        let t = start_t + el % loop_len.max(1);
        let view = d.tex.view.clone();
        let id = d.id;
        let target = Target { view: &view, w: 192, h: 108 };
        self.engine.render(&proj, t, &mut self.samples, &target, false);
        Some(id)
    }

    // ------------------------------------------------------------------ actions used by shortcuts / toolbar / menus

    /// Clips targeted by an action: the selection, or the clips under the playhead.
    pub fn targets(&self, main_only_fallback: bool) -> Vec<u64> {
        if !self.sel.clips.is_empty() {
            return self.sel.clips.clone();
        }
        let t = self.playhead;
        let mi = self.project.main_track_index();
        self.project
            .tracks
            .iter()
            .enumerate()
            .filter(|(i, tr)| !tr.locked && (!main_only_fallback || *i == mi))
            .flat_map(|(_, tr)| tr.clips.iter())
            .filter(|c| c.contains(t))
            .map(|c| c.id)
            .collect()
    }

    pub fn split(&mut self) {
        let t = self.playhead;
        let ids = self.targets(true);
        let mut created = vec![];
        self.edit(|p| {
            for id in ids {
                if let Some(r) = p.split_clip(id, t) {
                    created.push(r);
                }
            }
        });
        if created.is_empty() {
            self.toast(tr("Move the playhead over a clip to split"));
        }
    }

    pub fn split_all(&mut self) {
        let t = self.playhead;
        self.edit(|p| {
            p.split_at(t, None);
        });
    }

    pub fn delete_left(&mut self) {
        let t = self.playhead;
        let ids = self.targets(true);
        let main = self.project.tracks[self.project.main_track_index()].clone();
        let mut new_head = None;
        self.edit(|p| {
            for id in ids {
                let on_main = main.clips.iter().any(|c| c.id == id);
                let start = p.clip(id).map(|c| c.start);
                if p.trim_left_at(id, t) && on_main && p.magnet {
                    new_head = start;
                }
            }
        });
        if let Some(h) = new_head {
            self.playhead = h;
        }
    }

    pub fn delete_right(&mut self) {
        let t = self.playhead;
        let ids = self.targets(true);
        self.edit(|p| {
            for id in ids {
                p.trim_right_at(id, t);
            }
        });
    }

    pub fn delete_selected(&mut self) {
        if let Some(tid) = self.sel.transition {
            self.edit(|p| p.set_transition(tid, None));
            self.sel.transition = None;
            return;
        }
        let ids = self.sel.clips.clone();
        if ids.is_empty() {
            return;
        }
        let mut all = vec![];
        for id in &ids {
            all.extend(self.project.group_members(*id));
        }
        self.edit(|p| p.delete_clips(&all));
        self.sel = Selection::default();
    }

    /// Delete and close the gap (also on secondary tracks).
    pub fn ripple_delete(&mut self) {
        let ids = self.sel.clips.clone();
        if ids.is_empty() {
            return;
        }
        self.edit(|p| {
            for id in &ids {
                if let Some((ti, ci)) = p.find_clip(*id) {
                    let c = p.tracks[ti].clips[ci].clone();
                    let main = p.tracks[ti].main;
                    p.tracks[ti].clips.remove(ci);
                    if !main {
                        for o in p.tracks[ti].clips.iter_mut() {
                            if o.start >= c.end() {
                                o.start -= c.duration;
                            }
                        }
                    }
                }
            }
            p.normalize();
        });
        self.sel = Selection::default();
    }

    pub fn duplicate(&mut self) {
        let ids = self.sel.clips.clone();
        let mut new = vec![];
        self.edit(|p| {
            for id in ids {
                if let Some(n) = p.duplicate(id) {
                    new.push(n);
                }
            }
        });
        if !new.is_empty() {
            self.sel = Selection { clips: new, transition: None };
        }
    }

    pub fn copy(&mut self) {
        self.clipboard.clear();
        for id in self.sel.clips.clone() {
            if let Some((ti, ci)) = self.project.find_clip(id) {
                let t = &self.project.tracks[ti];
                self.clipboard.push((t.kind, t.main, t.clips[ci].clone()));
            }
        }
        if !self.clipboard.is_empty() {
            self.toast(tr("Copied"));
        }
    }

    pub fn cut(&mut self) {
        self.copy();
        self.delete_selected();
    }

    pub fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let t = self.playhead;
        let min_start = self.clipboard.iter().map(|c| c.2.start).min().unwrap_or(0);
        let items = self.clipboard.clone();
        let mut new = vec![];
        self.edit(|p| {
            for (kind, main, c) in items {
                let mut c = c;
                c.id = util::new_id();
                c.link = 0;
                c.group = 0;
                c.transition = None;
                let at = t + (c.start - min_start);
                if main && kind == TrackKind::Video {
                    p.insert_into_main(c.clone(), at);
                    new.push(c.id);
                } else {
                    c.start = at;
                    let id = p.add_clip(c, at);
                    new.push(id);
                }
            }
            p.normalize();
        });
        self.sel = Selection { clips: new, transition: None };
    }

    pub fn select_all(&mut self) {
        self.sel.clips = self.project.tracks.iter().filter(|t| !t.locked).flat_map(|t| t.clips.iter().map(|c| c.id)).collect();
        self.sel.transition = None;
    }

    pub fn freeze(&mut self) {
        let Some(id) = self.targets(true).first().copied() else { return };
        let t = self.playhead;
        let r = self.edit(|p| p.freeze_frame(id, t));
        match r {
            Some(f) => self.select_only(f),
            None => self.toast(tr("Freeze works on video clips under the playhead")),
        }
    }

    /// Reverse every selected video clip.
    pub fn reverse(&mut self) {
        let videos: Vec<u64> = self
            .sel
            .clips
            .iter()
            .copied()
            .filter(|id| {
                self.project
                    .clip(*id)
                    .and_then(|c| c.media_id())
                    .and_then(|m| self.project.media(m))
                    .map(|m| m.kind == MediaKind::Video)
                    .unwrap_or(false)
            })
            .collect();
        if videos.is_empty() {
            self.toast(tr(if self.sel.clips.is_empty() { "Select a video clip first" } else { "Reverse works on video clips" }));
            return;
        }
        for id in videos {
            self.reverse_one(id);
        }
    }

    fn reverse_one(&mut self, id: u64) {
        let Some(c) = self.project.clip(id).cloned() else { return };
        let Some(m) = c.media_id().and_then(|m| self.project.media(m)).cloned() else { return };
        self.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                c.reverse = !c.reverse;
            }
        });
        if !c.reverse && m.reversed.is_none() && !self.proxy_jobs.iter().any(|j| j.media == m.id) {
            self.proxy_jobs.push(media::proxy::spawn_reverse(m.id, &m.path, util::us_to_s(m.duration)));
            self.toast(tr("Reversing..."));
        }
    }

    pub fn mirror(&mut self) {
        let ids = self.sel.clips.clone();
        self.edit(|p| {
            for id in ids {
                if let Some(c) = p.clip_mut(id) {
                    c.tf.flip_h = !c.tf.flip_h;
                }
            }
        });
    }

    pub fn rotate90(&mut self) {
        let ids = self.sel.clips.clone();
        let t = self.playhead;
        self.edit(|p| {
            for id in ids {
                if let Some(c) = p.clip_mut(id) {
                    let r = c.prop(crate::model::keyframe::Prop::Rotation, t);
                    let nr = ((r / 90.0).round() * 90.0 + 90.0) % 360.0;
                    c.set_prop_at(crate::model::keyframe::Prop::Rotation, t, nr);
                }
            }
        });
    }

    pub fn toggle_enabled(&mut self) {
        let ids = self.sel.clips.clone();
        self.edit(|p| {
            for id in ids {
                if let Some(c) = p.clip_mut(id) {
                    c.disabled = !c.disabled;
                }
            }
        });
    }

    pub fn toggle_mute_clip(&mut self) {
        let ids = self.sel.clips.clone();
        self.edit(|p| {
            for id in ids {
                if let Some(c) = p.clip_mut(id) {
                    c.volume = if c.volume > 0.0 { 0.0 } else { 1.0 };
                }
            }
        });
    }

    pub fn nudge_volume(&mut self, db: f64) {
        let ids = self.sel.clips.clone();
        self.edit(|p| {
            for id in ids {
                if let Some(c) = p.clip_mut(id) {
                    let cur = 20.0 * c.volume.max(1e-4).log10();
                    c.volume = 10f64.powf(((cur + db).clamp(-60.0, 20.0)) / 20.0);
                }
            }
        });
    }

    /// Alt+K: toggle a keyframe for the transform properties at the playhead.
    /// Alt+K on every selected clip under the playhead.
    pub fn toggle_keyframe(&mut self) {
        let t = self.playhead;
        let ids: Vec<u64> =
            self.sel.clips.iter().copied().filter(|id| self.project.clip(*id).map(|c| c.contains(t)).unwrap_or(false)).collect();
        if ids.is_empty() {
            if !self.sel.clips.is_empty() {
                self.toast(tr("Move the playhead over the selected clip"));
            }
            return;
        }
        for id in ids {
            self.toggle_keyframe_one(id);
        }
    }

    fn toggle_keyframe_one(&mut self, id: u64) {
        let t = self.playhead;
        use crate::model::keyframe::Prop;
        let Some(c) = self.project.clip(id) else { return };
        let local = t - c.start;
        let props: Vec<Prop> = if matches!(c.kind, ClipKind::Effect(_) | ClipKind::Filter(_)) {
            vec![Prop::EffectAmount]
        } else if matches!(c.kind, ClipKind::Adjust) {
            Prop::ADJUST.to_vec()
        } else {
            vec![Prop::PosX, Prop::PosY, Prop::Scale, Prop::Rotation, Prop::Opacity]
        };
        let on = c.kf.any_at(&props, local);
        self.edit(|p| {
            if let Some(c) = p.clip_mut(id) {
                if on {
                    c.kf.remove_time(local);
                } else {
                    for pr in props {
                        let v = c.prop(pr, t);
                        c.kf.set(pr, local, v);
                    }
                }
            }
        });
        self.toast(if on { tr("Keyframe removed") } else { tr("Keyframe added") });
    }

    pub fn jump_keyframe(&mut self, forward: bool) {
        let Some(c) = self.sel.one().and_then(|id| self.project.clip(id)) else { return };
        let times: Vec<Us> = c.kf.times().into_iter().map(|k| c.start + k).collect();
        let t = self.playhead;
        let target = if forward { times.into_iter().find(|&k| k > t + 1000) } else { times.into_iter().rev().find(|&k| k < t - 1000) };
        if let Some(k) = target {
            self.seek(k);
        }
    }

    pub fn add_text(&mut self, preset: &'static str, at: Option<Us>) {
        let pr = crate::model::catalog::TEXT_PRESETS.iter().find(|p| p.id == preset).unwrap_or(&crate::model::catalog::TEXT_PRESETS[0]);
        let td = crate::preview::text_from_preset(pr, tr("Default text"));
        let t = at.unwrap_or(self.playhead);
        let c = Clip::new(ClipKind::Text(td), t, 3 * SEC);
        let id = self.edit(|p| p.add_clip(c, t));
        self.select_only(id);
        self.right_tab = RightTab::Video;
    }

    pub fn add_sticker(&mut self, sid: &'static str, at: Option<Us>) {
        let col = crate::model::catalog::sticker(sid).map(|s| s.default_color).unwrap_or([255, 255, 255, 255]);
        let t = at.unwrap_or(self.playhead);
        let mut c = Clip::new(ClipKind::Sticker(StickerData { id: sid.into(), color: col }), t, 3 * SEC);
        c.tf.scale = 1.0;
        let id = self.edit(|p| p.add_clip(c, t));
        self.select_only(id);
    }

    /// Selected clips that show a picture (video, photo, text, sticker), in timeline order.
    fn selected_visuals(&self) -> Vec<u64> {
        let mut v: Vec<(Us, u64)> = self
            .sel
            .clips
            .iter()
            .filter_map(|id| self.project.clip(*id))
            .filter(|c| {
                let audio_only = c.media_id().and_then(|m| self.project.media(m)).map(|m| m.kind == MediaKind::Audio).unwrap_or(false);
                matches!(c.kind, ClipKind::Media { .. } | ClipKind::Text(_) | ClipKind::Sticker(_)) && !audio_only
            })
            .map(|c| (c.start, c.id))
            .collect();
        v.sort();
        v.into_iter().map(|x| x.1).collect()
    }

    pub fn add_effect(&mut self, eid: &'static str, at: Option<Us>) {
        let def = crate::model::catalog::effect(eid);
        let mut params = [0.5; 4];
        if let Some(d) = def {
            for (i, pd) in d.params.iter().enumerate().take(4) {
                params[i] = pd.default;
            }
        }
        // clicked in the library with clips selected: one effect over each of them
        let targets = if at.is_none() { self.selected_visuals() } else { vec![] };
        if !targets.is_empty() {
            let spans: Vec<(Us, Us)> = targets.iter().filter_map(|id| self.project.clip(*id)).map(|c| (c.start, c.duration)).collect();
            let ids = self.edit(|p| {
                spans
                    .iter()
                    .map(|(s, d)| p.add_clip(Clip::new(ClipKind::Effect(EffectUse { id: eid.into(), params }), *s, *d), *s))
                    .collect::<Vec<_>>()
            });
            self.sel = Selection { clips: ids, transition: None };
            if spans.len() > 1 {
                self.toast(&format!("{} {}", spans.len(), tr("clips got the effect")));
            }
            return;
        }
        let t = at.unwrap_or(self.playhead);
        let c = Clip::new(ClipKind::Effect(EffectUse { id: eid.into(), params }), t, 3 * SEC);
        let id = self.edit(|p| p.add_clip(c, t));
        self.select_only(id);
    }

    pub fn add_filter(&mut self, fid: &'static str, at: Option<Us>) {
        // With visual clips selected the filter goes onto each of them.
        if at.is_none() {
            let ids: Vec<u64> = self
                .selected_visuals()
                .into_iter()
                .filter(|id| self.project.clip(*id).map(|c| matches!(c.kind, ClipKind::Media { .. })).unwrap_or(false))
                .collect();
            if !ids.is_empty() {
                self.edit(|p| {
                    for id in &ids {
                        if let Some(c) = p.clip_mut(*id) {
                            c.filter = Some(FilterUse { id: fid.into(), intensity: 1.0 });
                        }
                    }
                });
                self.toast(if ids.len() == 1 {
                    tr("Filter applied to the selected clip")
                } else {
                    tr("Filter applied to the selected clips")
                });
                return;
            }
        }
        let t = at.unwrap_or(self.playhead);
        let c = Clip::new(ClipKind::Filter(FilterUse { id: fid.into(), intensity: 1.0 }), t, 3 * SEC);
        let id = self.edit(|p| p.add_clip(c, t));
        self.select_only(id);
    }

    pub fn add_adjust_layer(&mut self, preset: usize, at: Option<Us>) {
        let values = library::ADJUST_PRESETS[preset.min(library::ADJUST_PRESETS.len() - 1)].1;
        // clicked with clips selected: the look goes into each clip's own Adjust settings
        if at.is_none() && preset > 0 {
            let ids: Vec<u64> = self
                .selected_visuals()
                .into_iter()
                .filter(|id| self.project.clip(*id).map(|c| matches!(c.kind, ClipKind::Media { .. })).unwrap_or(false))
                .collect();
            if !ids.is_empty() {
                self.edit(|p| {
                    for id in &ids {
                        if let Some(c) = p.clip_mut(*id) {
                            c.adjust = values;
                        }
                    }
                });
                self.right_tab = RightTab::Adjust;
                self.toast(tr("Adjustment applied to the selected clips"));
                return;
            }
        }
        let t = at.unwrap_or(self.playhead);
        let mut c = Clip::new(ClipKind::Adjust, t, 3 * SEC);
        c.adjust = library::ADJUST_PRESETS[preset.min(library::ADJUST_PRESETS.len() - 1)].1;
        let id = self.edit(|p| p.add_clip(c, t));
        self.select_only(id);
        self.right_tab = RightTab::Adjust;
    }

    /// Apply a transition to the selected junction, or after the selected clip.
    pub fn apply_transition(&mut self, tid: &'static str, target: Option<u64>) {
        let def = crate::model::catalog::transition(tid);
        let tr_use = TransitionUse { id: tid.into(), dur: def.map(|d| d.dur).unwrap_or(SEC / 2), blur: def.map(|d| d.blur).unwrap_or(0.5) };
        let mi = self.project.main_track_index();
        // several main-track clips selected: a transition after each of them
        if target.is_none() && self.junction.is_none() && self.sel.clips.len() > 1 {
            let clips = &self.project.tracks[mi].clips;
            let owners: Vec<u64> = clips
                .iter()
                .enumerate()
                .filter(|(i, c)| self.sel.clips.contains(&c.id) && i + 1 < clips.len())
                .map(|(_, c)| c.id)
                .collect();
            if !owners.is_empty() {
                let tu = tr_use.clone();
                self.edit(|p| {
                    for o in &owners {
                        p.set_transition(*o, Some(tu.clone()));
                    }
                });
                self.toast(&format!("{} {}", owners.len(), tr("transitions added")));
                return;
            }
        }
        let main = &self.project.tracks[mi];
        let owner = target
            .or(self.junction)
            .or(self.sel.transition)
            .or_else(|| {
                let id = self.sel.one()?;
                let idx = main.clips.iter().position(|c| c.id == id)?;
                if idx + 1 < main.clips.len() {
                    Some(id)
                } else if idx > 0 {
                    Some(main.clips[idx - 1].id)
                } else {
                    None
                }
            })
            .or_else(|| {
                // nearest cut to the playhead
                let t = self.playhead;
                main.clips.windows(2).min_by_key(|w| (w[1].start - t).abs()).map(|w| w[0].id)
            });
        let Some(owner) = owner else {
            self.toast(tr("Add at least two clips to the main track to use transitions"));
            return;
        };
        if !main.clips.iter().position(|c| c.id == owner).map(|i| i + 1 < main.clips.len()).unwrap_or(false) {
            self.toast(tr("Transitions go between two clips on the main track"));
            return;
        }
        self.edit(|p| p.set_transition(owner, Some(tr_use)));
        self.sel = Selection { clips: vec![], transition: Some(owner) };
        self.junction = Some(owner);
    }

    pub fn set_ratio(&mut self, r: Ratio) {
        self.edit(|p| p.apply_ratio(r));
        self.engine.clear_cache();
    }

    pub fn open_export(&mut self) {
        if self.project.duration() == 0 {
            self.toast(tr("Add media to the timeline before exporting"));
            return;
        }
        self.flush_edit();
        self.pause();
        self.export_dialog = Some(export_ui::ExportDialog::new(self));
    }

    pub fn set_lang(&mut self, code: &str) {
        self.settings.lang = code.to_string();
        let l = match code {
            "pt" => i18n::Lang::Pt,
            "en" => i18n::Lang::En,
            _ => i18n::system_lang(),
        };
        i18n::set_lang(l);
        self.settings.save();
        self.tile_tex.clear();
    }
}

fn egui_extras_install(_ctx: &egui::Context) {}

static STAT_MISS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// VELOMOV_STATS=1: print UI frame statistics every two seconds.
/// Mostly black picture (RGBA), judged from a sparse sample of pixels.
fn is_dark(rgba: &[u8]) -> bool {
    let px = rgba.chunks_exact(4).step_by(97);
    let (mut n, mut sum) = (0u64, 0u64);
    for p in px {
        n += 1;
        sum += p[0].max(p[1]).max(p[2]) as u64;
    }
    n == 0 || sum / n < 14
}

fn stats_tick(dt: Duration, decoders: usize, playing: bool, playhead: Us) {
    use std::sync::Mutex;
    static S: Mutex<Option<(Instant, u32, f64, f64)>> = Mutex::new(None);
    let mut g = S.lock().unwrap();
    let e = g.get_or_insert((Instant::now(), 0, 0.0, 0.0));
    e.1 += 1;
    let ms = dt.as_secs_f64() * 1000.0;
    e.2 += ms;
    e.3 = e.3.max(ms);
    if e.0.elapsed() >= Duration::from_secs(2) {
        let secs = e.0.elapsed().as_secs_f64();
        eprintln!(
            "[stats] {:.1} fps  ui {:.2} ms avg / {:.2} ms max  decoders {}  playing {}  late frames {}  t={:.2}s",
            e.1 as f64 / secs,
            e.2 / e.1.max(1) as f64,
            e.3,
            decoders,
            playing,
            STAT_MISS.swap(0, std::sync::atomic::Ordering::Relaxed),
            playhead as f64 / 1e6
        );
        *e = (Instant::now(), 0, 0.0, 0.0);
    }
}

/// "New Project", "New Project (2)", … (first name not used by another project).
pub fn default_project_name() -> String {
    let base = tr("New Project").to_string();
    let existing = home::list_projects();
    let mut name = base.clone();
    let mut n = 2;
    while existing.iter().any(|p| p.name == name) {
        name = format!("{} ({})", base, n);
        n += 1;
    }
    name
}

/// Developer automation through environment variables (used for headless UI tests):
/// VELOMOV_SHOT=out.png VELOMOV_SHOT_FRAME=N  -> screenshot after N frames, then quit
/// VELOMOV_SEEK=secs, VELOMOV_SELECT=index (clip index on the main track), VELOMOV_TAB=media|audio|...
/// VELOMOV_RTAB=video|audio|speed|animation|adjust, VELOMOV_SUB=n, VELOMOV_PLAY=1, VELOMOV_SHORTCUTS=1, VELOMOV_EXPORT=1, VELOMOV_SETTINGS=1
fn debug_automation(app: &mut VeloApp, ctx: &egui::Context) {
    static FRAME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = FRAME.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let env = |k: &str| std::env::var(k).ok();
    if n == 3 {
        if let Some(t) = env("VELOMOV_SEEK").and_then(|v| v.parse::<f64>().ok()) {
            app.seek(util::s_to_us(t));
        }
        if let Some(i) = env("VELOMOV_SELECT").and_then(|v| v.parse::<usize>().ok()) {
            let mi = app.project.main_track_index();
            if let Some(c) = app.project.tracks[mi].clips.get(i) {
                let id = c.id;
                app.select_only(id);
            }
        }
        if let Some(i) = env("VELOMOV_SELECT_TRACK").and_then(|v| v.parse::<usize>().ok()) {
            if let Some(c) = app.project.tracks.get(i).and_then(|t| t.clips.first()) {
                let id = c.id;
                app.select_only(id);
            }
        }
        if env("VELOMOV_SELECT_TRANSITION").is_some() {
            let mi = app.project.main_track_index();
            if let Some(c) = app.project.tracks[mi].clips.iter().find(|c| c.transition.is_some()) {
                app.sel = Selection { clips: vec![], transition: Some(c.id) };
            }
        }
        if let Some(t) = env("VELOMOV_TAB") {
            for lt in LeftTab::ALL {
                if format!("{:?}", lt).to_lowercase() == t.to_lowercase() {
                    app.left_tab = lt;
                }
            }
        }
        if let Some(c) = env("VELOMOV_CAT").and_then(|v| v.parse::<usize>().ok()) {
            app.left_cat.insert(app.left_tab, c);
        }
        if let Some(t) = env("VELOMOV_RTAB") {
            app.right_tab = match t.as_str() {
                "audio" => RightTab::Audio,
                "speed" => RightTab::Speed,
                "animation" => RightTab::Animation,
                "adjust" => RightTab::Adjust,
                _ => RightTab::Video,
            };
        }
        if let Some(s) = env("VELOMOV_SUB").and_then(|v| v.parse::<usize>().ok()) {
            app.video_sub = s;
        }
        if env("VELOMOV_SHORTCUTS").is_some() {
            app.show_shortcuts = true;
        }
        if env("VELOMOV_EXPORT").is_some() {
            app.open_export();
        }
        if env("VELOMOV_SETTINGS").is_some() {
            app.open_settings();
        }
        if env("VELOMOV_PLAY").is_some() {
            app.play();
        }
    }
    let shot = ctx.input(|i| {
        i.raw.events.iter().find_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None })
    });
    let script_shot = app.script.as_ref().and_then(|s| s.shot.clone());
    if let Some(img) = &shot {
        let target = script_shot.clone().or_else(|| env("VELOMOV_SHOT"));
        if let Some(path) = target {
            let [w, h] = img.size;
            let mut buf = Vec::with_capacity(w * h * 4);
            for p in &img.pixels {
                buf.extend_from_slice(&p.to_array());
            }
            let _ = image::save_buffer(&path, &buf, w as u32, h as u32, image::ExtendedColorType::Rgba8);
            eprintln!("[velomov] screenshot saved to {}", path);
        }
        if script_shot.is_some() {
            if let Some(s) = app.script.as_mut() {
                s.shot = None;
            }
        } else if env("VELOMOV_SHOT").is_some() && env("VELOMOV_KEEP").is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    if let Some(path) = env("VELOMOV_SHOT") {
        let _ = path;
        let at: u64 = env("VELOMOV_SHOT_FRAME").and_then(|v| v.parse().ok()).unwrap_or(40);
        if n == at {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint();
    }
}

impl eframe::App for VeloApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(mut s) = self.script.take() {
            s.step(self, ctx, raw_input);
            self.script = Some(s);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        theme::BG().to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        debug_automation(self, ctx);
        self.poll_background(ctx);
        match self.screen {
            Screen::Home => home::ui(self, ctx),
            Screen::Editor => self.ui_editor(ctx),
        }
        new_project::window(self, ctx);
        settings_ui::window(self, ctx);
        self.draw_toasts(ctx);
        // files dragged over the window from a file manager
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let screen = ctx.screen_rect();
            egui::Area::new(egui::Id::new("drop-overlay")).fixed_pos(screen.min).order(egui::Order::Foreground).interactable(false).show(
                ctx,
                |ui| {
                    let p = ui.painter();
                    p.rect_filled(screen, widgets::cr(0), Color32::from_black_alpha(150));
                    p.rect_stroke(
                        screen.shrink(18.0),
                        widgets::cr(14),
                        egui::Stroke::new(2.0_f32, theme::ACCENT()),
                        egui::StrokeKind::Inside,
                    );
                    p.text(
                        screen.center() - egui::vec2(0.0, 18.0),
                        egui::Align2::CENTER_CENTER,
                        egui_phosphor::regular::DOWNLOAD_SIMPLE,
                        theme::prop(40.0),
                        theme::ACCENT(),
                    );
                    p.text(
                        screen.center() + egui::vec2(0.0, 26.0),
                        egui::Align2::CENTER_CENTER,
                        tr("Drop to import"),
                        theme::bold(18.0),
                        Color32::WHITE,
                    );
                },
            );
        }
        // window close: on_exit saves the project (with its cover)
        if ctx.input(|i| i.viewport().close_requested()) {
            self.settings.save();
        }
    }

    fn on_exit(&mut self) {
        self.flush_edit();
        if self.screen == Screen::Editor {
            self.quitting = true;
            self.save_project_ex(true, true);
        }
        self.settings.save();
        self.frames.video.stop_all();
    }
}

impl VeloApp {
    fn ui_editor(&mut self, ctx: &egui::Context) {
        let frame_start = Instant::now();
        shortcuts::handle(self, ctx);
        self.tick_playback(ctx);
        self.poll_captions(ctx);
        self.sync_preview_quality(ctx);
        if let Some(t) = self.sfx_preview {
            if t.elapsed() > Duration::from_secs(3) {
                self.sfx_preview = None;
            }
        }
        if self.pending_sfx.is_some() {
            self.poll_sfx();
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        if self.fullscreen {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(Color32::BLACK)).show(ctx, |ui| {
                player::preview_area(self, ui, true);
            });
        } else {
            // panels are rounded cards floating on the dark background
            let g = theme::GAP;
            let card = |fill: Color32, m: egui::Margin| egui::Frame::new().fill(fill).corner_radius(theme::CARD_R).outer_margin(m);
            egui::TopBottomPanel::top("topbar")
                .exact_height(46.0)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(theme::BG()).inner_margin(egui::Margin::symmetric(12, 0)))
                .show(ctx, |ui| {
                    export_ui::topbar(self, ui);
                });
            let total_h = ctx.screen_rect().height();
            egui::TopBottomPanel::bottom("timeline")
                .resizable(true)
                .show_separator_line(false)
                .default_height((total_h * 0.42).clamp(220.0, 520.0))
                .min_height(170.0)
                .max_height(total_h * 0.7)
                .frame(card(theme::TIMELINE_BG(), egui::Margin { left: 2 * g, right: 2 * g, top: g, bottom: 2 * g }))
                .show(ctx, |ui| {
                    timeline::ui(self, ui);
                });
            let w = ctx.screen_rect().width();
            egui::SidePanel::left("library")
                .resizable(true)
                .show_separator_line(false)
                .default_width((w * 0.285).clamp(340.0, 540.0))
                .min_width(300.0)
                .max_width((w * 0.45).max(340.0))
                .frame(card(theme::PANEL(), egui::Margin { left: 2 * g, right: g, top: 0, bottom: g }))
                .show(ctx, |ui| {
                    library::ui(self, ui);
                });
            egui::SidePanel::right("inspector")
                .resizable(true)
                .show_separator_line(false)
                .default_width((w * 0.24).clamp(300.0, 440.0))
                .min_width(280.0)
                .max_width((w * 0.4).max(300.0))
                .frame(card(theme::PANEL(), egui::Margin { left: g, right: 2 * g, top: 0, bottom: g }))
                .show(ctx, |ui| {
                    inspector::ui(self, ui);
                });
            egui::CentralPanel::default().frame(card(theme::PANEL(), egui::Margin { left: g, right: g, top: 0, bottom: g })).show(
                ctx,
                |ui| {
                    player::ui(self, ui);
                },
            );
        }
        export_ui::dialogs(self, ctx);
        shortcuts::help_window(self, ctx);
        library::drag_ghost(self, ctx);
        self.commit_if_idle(ctx);
        // housekeeping
        if self.last_gc.elapsed() > Duration::from_secs(2) {
            self.frames.video.gc(Duration::from_secs(4));
            self.last_gc = Instant::now();
        }
        if self.dirty
            && self.settings.autosave
            && self.pre_edit.is_none()
            && self.last_save.elapsed() > Duration::from_secs(4)
            && !self.playing
        {
            self.save_project(true);
        }
        if self.render_pending || self.pending_probes > 0 || self.thumbs.busy() || self.audio.busy() {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        if std::env::var_os("VELOMOV_STATS").is_some() {
            stats_tick(frame_start.elapsed(), self.frames.video.active(), self.playing, self.playhead);
        }
        // only send the title when it changes: every viewport command forces another repaint
        let title = format!("VeloMov — {}{}", self.project.name, if self.dirty { " •" } else { "" });
        if self.last_title != title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_title = title;
        }
    }

    fn draw_toasts(&mut self, ctx: &egui::Context) {
        self.toasts.retain(|t| t.1.elapsed() < Duration::from_millis(2600));
        if self.toasts.is_empty() {
            return;
        }
        let screen = ctx.screen_rect();
        let mut y = screen.top() + 58.0;
        let toasts = self.toasts.clone();
        for (i, (msg, _, err)) in toasts.iter().enumerate() {
            egui::Area::new(egui::Id::new(("toast", i)))
                .fixed_pos(egui::pos2(screen.center().x, y))
                .pivot(egui::Align2::CENTER_TOP)
                .order(egui::Order::Tooltip)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(if *err { Color32::from_rgb(0x5a, 0x1d, 0x24) } else { Color32::from_rgba_premultiplied(30, 30, 34, 240) })
                        .corner_radius(widgets::cr(8))
                        .inner_margin(egui::Margin::symmetric(14, 8))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(msg).color(Color32::WHITE));
                        });
                });
            y += 40.0;
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }
}
