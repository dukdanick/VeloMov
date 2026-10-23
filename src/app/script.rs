//! UI test scripts: `VELOMOV_SCRIPT=file.txt velomov ...` replays mouse and
//! keyboard input through egui's raw input, one step per frame, so every
//! editor interaction can be exercised headless (used for development).
//!
//! Commands (one per line, `#` comments):
//!   wait N | sleep MS | move X Y | down | up | click X Y | rclick X Y | dclick X Y
//!   drag X1 Y1 X2 Y2 [STEPS] | key ctrl+shift+b | type TEXT | scroll DX DY [ctrl]
//!   shot FILE.png | dump FILE.json | saveas FILE.vmov | log TEXT | quit
//!   dropfiles PATH... (files dropped from a file manager) | mods shift|ctrl|none (held keys)

use super::VeloApp;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, Vec2};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub struct Script {
    lines: VecDeque<String>,
    frames: VecDeque<Vec<Event>>,
    wait: u32,
    sleep_until: Option<Instant>,
    pos: Pos2,
    pub shot: Option<String>,
    shot_wait: u32,
    drop: Vec<std::path::PathBuf>,
    mods: Modifiers,
}

fn key_of(name: &str) -> Option<Key> {
    let n = name.to_lowercase();
    Some(match n.as_str() {
        "space" => Key::Space,
        "enter" | "return" => Key::Enter,
        "esc" | "escape" => Key::Escape,
        "tab" => Key::Tab,
        "delete" | "del" => Key::Delete,
        "backspace" => Key::Backspace,
        "left" => Key::ArrowLeft,
        "right" => Key::ArrowRight,
        "up" => Key::ArrowUp,
        "down" => Key::ArrowDown,
        "home" => Key::Home,
        "end" => Key::End,
        "[" => Key::OpenBracket,
        "]" => Key::CloseBracket,
        "." => Key::Period,
        "," => Key::Comma,
        "=" => Key::Equals,
        "-" => Key::Minus,
        "f1" => Key::F1,
        _ => return Key::from_name(&name.to_uppercase()).or_else(|| Key::from_name(name)),
    })
}

fn parse_mods_key(spec: &str) -> Option<(Modifiers, Key)> {
    let mut m = Modifiers::NONE;
    let mut key = None;
    for part in spec.split('+') {
        match part.to_lowercase().as_str() {
            "ctrl" => {
                m.ctrl = true;
                m.command = true;
            }
            "shift" => m.shift = true,
            "alt" => m.alt = true,
            other => key = key_of(other),
        }
    }
    key.map(|k| (m, k))
}

impl Script {
    pub fn from_env() -> Option<Script> {
        let path = std::env::var("VELOMOV_SCRIPT").ok()?;
        let text = std::fs::read_to_string(&path).ok()?;
        let lines = text.lines().map(|l| l.split('#').next().unwrap_or("").trim().to_string()).filter(|l| !l.is_empty()).collect();
        Some(Script {
            lines,
            frames: VecDeque::new(),
            wait: 0,
            sleep_until: None,
            pos: Pos2::new(10.0, 10.0),
            shot: None,
            shot_wait: 0,
            mods: Modifiers::NONE,
            drop: vec![],
        })
    }

    fn button(&self, button: PointerButton, pressed: bool) -> Event {
        Event::PointerButton { pos: self.pos, button, pressed, modifiers: self.mods }
    }

    /// Expand the next command into per-frame event batches.
    fn next_command(&mut self, app: &mut VeloApp, ctx: &egui::Context) -> bool {
        let Some(line) = self.lines.pop_front() else { return false };
        let a: Vec<&str> = line.split_whitespace().collect();
        let f = |i: usize| a.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        eprintln!("[script] {}", line);
        match a[0] {
            "wait" => self.wait = f(1) as u32,
            "sleep" => self.sleep_until = Some(Instant::now() + Duration::from_millis(f(1) as u64)),
            "move" => {
                self.pos = Pos2::new(f(1), f(2));
                self.frames.push_back(vec![Event::PointerMoved(self.pos)]);
            }
            "down" => self.frames.push_back(vec![self.button(PointerButton::Primary, true)]),
            "up" => self.frames.push_back(vec![self.button(PointerButton::Primary, false)]),
            "click" | "rclick" | "dclick" => {
                self.pos = Pos2::new(f(1), f(2));
                let b = if a[0] == "rclick" { PointerButton::Secondary } else { PointerButton::Primary };
                self.frames.push_back(vec![Event::PointerMoved(self.pos)]);
                self.frames.push_back(vec![self.button(b, true)]);
                self.frames.push_back(vec![self.button(b, false)]);
                if a[0] == "dclick" {
                    self.frames.push_back(vec![self.button(b, true)]);
                    self.frames.push_back(vec![self.button(b, false)]);
                }
                self.frames.push_back(vec![]);
            }
            "drag" => {
                let (x1, y1, x2, y2) = (f(1), f(2), f(3), f(4));
                let steps = if a.len() > 5 { f(5).max(1.0) as usize } else { 12 };
                self.pos = Pos2::new(x1, y1);
                self.frames.push_back(vec![Event::PointerMoved(self.pos)]);
                self.frames.push_back(vec![self.button(PointerButton::Primary, true)]);
                for i in 1..=steps {
                    let t = i as f32 / steps as f32;
                    let p = Pos2::new(x1 + (x2 - x1) * t, y1 + (y2 - y1) * t);
                    self.frames.push_back(vec![Event::PointerMoved(p)]);
                }
                self.pos = Pos2::new(x2, y2);
                self.frames.push_back(vec![]);
                self.frames.push_back(vec![self.button(PointerButton::Primary, false)]);
                self.frames.push_back(vec![]);
            }
            "key" => {
                if let Some((m, k)) = a.get(1).and_then(|s| parse_mods_key(s)) {
                    self.frames.push_back(vec![
                        Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m },
                        Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m },
                    ]);
                    self.frames.push_back(vec![]);
                } else {
                    eprintln!("[script] unknown key {:?}", a.get(1));
                }
            }
            "type" => {
                let text = line.splitn(2, ' ').nth(1).unwrap_or("").to_string();
                self.frames.push_back(vec![Event::Text(text)]);
            }
            "scroll" => {
                let m =
                    if a.get(3) == Some(&"ctrl") { Modifiers { ctrl: true, command: true, ..Default::default() } } else { Modifiers::NONE };
                self.frames.push_back(vec![Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: Vec2::new(f(1), f(2)),
                    modifiers: m,
                }]);
                self.frames.push_back(vec![]);
            }
            "shot" => {
                self.shot = a.get(1).map(|s| s.to_string());
                self.shot_wait = 3;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            "saveas" => {
                // "Save as…" without the file dialog
                if let Some(path) = a.get(1) {
                    app.finish_save_as(std::path::PathBuf::from(path));
                }
            }
            "dump" => {
                if let Some(path) = a.get(1) {
                    let state = serde_json::json!({
                        "playhead": app.playhead,
                        "playing": app.playing,
                        "selection": app.sel.clips,
                        "transition": app.sel.transition,
                        "left_tab": format!("{:?}", app.left_tab),
                        "right_tab": format!("{:?}", app.right_tab),
                        "undo": app.undo.len(),
                        "redo": app.redo.len(),
                        "export_dialog": app.export_dialog.is_some(),
                        "fullscreen": app.fullscreen,
                        "split_mode": app.tl.split_mode,
                        "pps": app.tl.pps,
                        "scroll_x": app.tl.scroll_x,
                        "scroll_y": app.tl.scroll_y,
                        "project": app.project,
                    });
                    let _ = std::fs::write(path, serde_json::to_string_pretty(&state).unwrap_or_default());
                }
            }
            // ---- timeline-aware helpers (positions from the last drawn frame)
            "clickclip" | "rclickclip" | "dragclip" => {
                // clickclip TRACK CLIP FRAC | dragclip TRACK CLIP FRAC TO_SECS TO_TRACK
                let (ti, ci, fr) = (f(1) as usize, f(2) as usize, f(3));
                let Some(c) = app.project.tracks.get(ti).and_then(|t| t.clips.get(ci)) else {
                    eprintln!("[script] no clip {} {}", ti, ci);
                    return true;
                };
                let row = app.tl.rows.iter().find(|r| r.0 == ti).map(|r| r.1);
                let Some(row) = row else { return true };
                let x = app.tl.x0 + (crate::util::us_to_s(c.start) as f32 + crate::util::us_to_s(c.duration) as f32 * fr) * app.tl.pps;
                // optional vertical position inside the row (0 = top, 1 = bottom)
                let yf = if a[0] == "dragclip" { a.get(6) } else { a.get(4) }.and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.5);
                let y = row.top() + row.height() * yf;
                if a[0] == "dragclip" {
                    let to_t = f(4);
                    let to_row = app.tl.rows.iter().find(|r| r.0 == f(5) as usize).map(|r| r.1.center().y).unwrap_or(y);
                    let tx = app.tl.x0 + to_t * app.tl.pps;
                    self.lines.push_front(format!("drag {} {} {} {} 16", x, y, tx, to_row));
                } else {
                    let cmd = if a[0] == "rclickclip" { "rclick" } else { "click" };
                    self.lines.push_front(format!("{} {} {}", cmd, x, y));
                }
            }
            "clickruler" => {
                let x = app.tl.x0 + f(1) * app.tl.pps;
                self.lines.push_front(format!("click {} {}", x, app.tl.ruler_y));
            }
            "clicktime" => {
                let x = app.tl.x0 + f(1) * app.tl.pps;
                let y = app.tl.rows.iter().find(|r| r.0 == f(2) as usize).map(|r| r.1.center().y).unwrap_or(0.0);
                self.lines.push_front(format!("click {} {}", x, y));
            }
            "clickjunction" => {
                let mi = app.project.main_track_index();
                let ci = f(1) as usize;
                let clips = &app.project.tracks[mi].clips;
                if ci + 1 < clips.len() {
                    let t = (crate::util::us_to_s(clips[ci + 1].start) + crate::util::us_to_s(clips[ci].end())) as f32 / 2.0;
                    let x = app.tl.x0 + t * app.tl.pps;
                    let y = app.tl.rows.iter().find(|r| r.0 == mi).map(|r| r.1.center().y).unwrap_or(0.0);
                    let off = if clips[ci].transition.is_some()
                        && (crate::util::us_to_s(clips[ci].end()) - crate::util::us_to_s(clips[ci + 1].start)) as f32 * app.tl.pps > 46.0
                    {
                        -6.0
                    } else {
                        0.0
                    };
                    self.lines.push_front(format!("click {} {}", x, y + off));
                }
            }
            "dragto" => {
                // dragto X Y TO_SECS TO_TRACK : drag something from the library onto the timeline
                let tx = app.tl.x0 + f(3) * app.tl.pps;
                let ty = app.tl.rows.iter().find(|r| r.0 == f(4) as usize).map(|r| r.1.center().y).unwrap_or(0.0);
                self.lines.push_front(format!("drag {} {} {} {} 20", f(1), f(2), tx, ty));
            }
            "seek" => app.seek(crate::util::s_to_us(f(1) as f64)),
            "rtab" => {
                app.right_tab = match a.get(1).copied().unwrap_or("") {
                    "audio" => super::RightTab::Audio,
                    "speed" => super::RightTab::Speed,
                    "animation" => super::RightTab::Animation,
                    "adjust" => super::RightTab::Adjust,
                    _ => super::RightTab::Video,
                }
            }
            "ltab" => {
                let want = a.get(1).copied().unwrap_or("").to_lowercase();
                if let Some(t) = super::LeftTab::ALL.iter().find(|t| format!("{:?}", t).to_lowercase() == want) {
                    app.left_tab = *t;
                }
                if let Some(c) = a.get(2).and_then(|v| v.parse::<usize>().ok()) {
                    app.left_cat.insert(app.left_tab, c);
                }
            }
            "mods" => {
                self.mods = parse_mods_key(&format!("{}+a", a.get(1).copied().unwrap_or("none"))).map(|m| m.0).unwrap_or(Modifiers::NONE);
                self.frames.push_back(vec![]);
            }
            "dropfiles" => {
                self.drop = a[1..].iter().map(std::path::PathBuf::from).collect();
                self.frames.push_back(vec![]);
            }
            "log" => eprintln!("[script] {}", line),
            "quit" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            other => eprintln!("[script] unknown command {}", other),
        }
        true
    }

    /// Called from `raw_input_hook` before every frame.
    pub fn step(&mut self, app: &mut VeloApp, ctx: &egui::Context, raw: &mut egui::RawInput) {
        ctx.request_repaint();
        raw.focused = true;
        raw.modifiers = self.mods;
        // keep egui's hover position stable between commands
        if self.frames.is_empty() {
            raw.events.push(Event::PointerMoved(self.pos));
        }
        if self.shot.is_some() {
            if self.shot_wait > 0 {
                self.shot_wait -= 1;
            }
            return; // wait for the screenshot to come back before continuing
        }
        if let Some(t) = self.sleep_until {
            if Instant::now() < t {
                return;
            }
            self.sleep_until = None;
        }
        if self.wait > 0 {
            self.wait -= 1;
            return;
        }
        if self.frames.is_empty() {
            // run instantaneous commands until one produces input or waits
            while self.frames.is_empty() && self.wait == 0 && self.sleep_until.is_none() && self.shot.is_none() {
                if !self.next_command(app, ctx) {
                    break;
                }
            }
        }
        if let Some(batch) = self.frames.pop_front() {
            for e in &batch {
                if let Event::Key { modifiers, .. } = e {
                    raw.modifiers = *modifiers;
                }
                if let Event::PointerMoved(p) = e {
                    self.pos = *p;
                }
            }
            raw.events.extend(batch);
            if !self.drop.is_empty() {
                raw.dropped_files = self.drop.drain(..).map(|p| egui::DroppedFile { path: Some(p), ..Default::default() }).collect();
            }
        }
    }
}
