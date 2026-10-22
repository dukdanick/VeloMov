//! Speech to text for auto captions with whisper.cpp (`whisper-cli`, run as a
//! subprocess like FFmpeg): mixes the timeline audio, downloads the chosen model
//! on first use and returns timed words, which `group_words` turns into captions.

use crate::model::{catalog, Clip, ClipKind, Project, TextData, TrackKind, Word};
use crate::util::{self, Us};
use anyhow::{anyhow, bail, Context, Result};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct ModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub file: &'static str,
    /// Approximate download size in bytes (the server's size is used when known).
    pub bytes: u64,
    pub desc: &'static str,
}

#[rustfmt::skip]
pub static MODELS: &[ModelInfo] = &[
    ModelInfo { id: "tiny", name: "Tiny", file: "ggml-tiny.bin", bytes: 77_691_713,
        desc: "Fastest and lightest. Fine for clear speech, but makes more mistakes, especially in Portuguese and with background noise." },
    ModelInfo { id: "base", name: "Base", file: "ggml-base.bin", bytes: 147_951_465,
        desc: "Recommended. A good balance: quick even on modest computers and accurate enough for most videos." },
    ModelInfo { id: "small", name: "Small", file: "ggml-small.bin", bytes: 487_601_967,
        desc: "More accurate, above all for Portuguese, accents and noisy audio. About 3× slower than Base." },
    ModelInfo { id: "medium", name: "Medium", file: "ggml-medium.bin", bytes: 1_533_763_059,
        desc: "Very accurate. Slow on the processor (about 8–10× slower than Base) and needs about 2 GB of memory." },
    ModelInfo { id: "large-v3-turbo", name: "Large Turbo", file: "ggml-large-v3-turbo-q5_0.bin", bytes: 574_041_195,
        desc: "Almost as accurate as Large and much faster. A great choice for strong computers (compressed q5 version)." },
    ModelInfo { id: "large-v3", name: "Large", file: "ggml-large-v3.bin", bytes: 3_095_033_483,
        desc: "The most accurate. Very slow without a strong processor; needs about 4 GB of memory." },
];

pub const DEFAULT_MODEL: &str = "base";

pub fn model(id: &str) -> &'static ModelInfo {
    MODELS.iter().find(|m| m.id == id).unwrap_or(&MODELS[1])
}

pub fn models_dir() -> PathBuf {
    let p = util::data_dir().join("whisper-models");
    let _ = std::fs::create_dir_all(&p);
    p
}

pub fn model_path(id: &str) -> PathBuf {
    models_dir().join(model(id).file)
}

pub fn model_ready(id: &str) -> bool {
    std::fs::metadata(model_path(id)).map(|m| m.len() > 1_000_000).unwrap_or(false)
}

/// Spoken-language choices: (whisper code, name).
pub static LANGUAGES: &[(&str, &str)] = &[
    ("auto", "Detect automatically"),
    ("pt", "Português"),
    ("en", "English"),
    ("es", "Español"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("it", "Italiano"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("zh", "中文"),
    ("ru", "Русский"),
    ("ar", "العربية"),
    ("hi", "हिन्दी"),
    ("tr", "Türkçe"),
    ("nl", "Nederlands"),
    ("pl", "Polski"),
];

/// The whisper.cpp command-line program, if installed: next to VeloMov (Windows and
/// portable builds), in VeloMov's own folders (Linux install), or in PATH.
pub fn find_whisper() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("VELOMOV_WHISPER").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(p);
    }
    let exe = if cfg!(windows) { "whisper-cli.exe" } else { "whisper-cli" };
    let mut dirs = vec![];
    if let Some(d) = util::exe_dir() {
        dirs.push(d.join("whisper"));
        dirs.push(d.clone());
        dirs.push(d.join("../lib/velomov/whisper"));
    }
    dirs.push(util::data_dir().join("whisper"));
    for d in dirs {
        let p = d.join(exe);
        if p.is_file() {
            return Some(p);
        }
    }
    let path = std::env::var_os("PATH")?;
    for d in std::env::split_paths(&path) {
        for name in [exe, if cfg!(windows) { "whisper-cpp.exe" } else { "whisper-cpp" }] {
            let p = d.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

// ------------------------------------------------------------------ model download

#[derive(Default)]
pub struct Download {
    pub model: String,
    frac: AtomicU32,
    pub done: AtomicBool,
    pub cancel: AtomicBool,
    pub error: Mutex<Option<String>>,
}

impl Download {
    pub fn frac(&self) -> f32 {
        f32::from_bits(self.frac.load(Ordering::Relaxed))
    }
    fn set(&self, f: f32) {
        self.frac.store(f.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
}

fn model_url(m: &ModelInfo) -> String {
    format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}", m.file)
}

/// Size the server reports for the model (follows redirects), if it answers.
fn remote_size(url: &str) -> Option<u64> {
    let out = util::command("curl").args(["-sIL", "--max-time", "20", url]).output().ok()?;
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case("content-length").then(|| v.trim().parse::<u64>().ok()).flatten()
        })
        .filter(|n| *n > 1_000_000)
        .last()
}

/// Download a model in the background with curl (on Windows 10+ and every Linux).
pub fn download(id: &str) -> Arc<Download> {
    let d = Arc::new(Download { model: id.to_string(), ..Default::default() });
    let d2 = d.clone();
    let m = model(id);
    std::thread::Builder::new()
        .name("velomov-model-dl".into())
        .spawn(move || {
            if let Err(e) = fetch(m, &d2) {
                *d2.error.lock().unwrap() = Some(format!("{:#}", e));
            }
            d2.done.store(true, Ordering::Relaxed);
        })
        .expect("download thread");
    d
}

fn fetch(m: &ModelInfo, d: &Download) -> Result<()> {
    let url = model_url(m);
    let dest = models_dir().join(m.file);
    let part = dest.with_extension("part");
    let total = remote_size(&url).unwrap_or(m.bytes);
    let mut child = util::command("curl")
        .args(["-fL", "--retry", "3", "--connect-timeout", "20", "-s", "-S", "-o"])
        .arg(&part)
        .arg(&url)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not run curl")?;
    loop {
        if d.cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(&part);
            return Ok(());
        }
        if let Some(st) = child.try_wait()? {
            if !st.success() {
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    use std::io::Read;
                    let _ = e.read_to_string(&mut err);
                }
                let _ = std::fs::remove_file(&part);
                bail!("download failed: {}", err.trim());
            }
            break;
        }
        let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        d.set(have as f32 / total.max(1) as f32 * 0.99);
        std::thread::sleep(Duration::from_millis(200));
    }
    let len = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if len < 1_000_000 {
        let _ = std::fs::remove_file(&part);
        bail!("the downloaded file is incomplete");
    }
    std::fs::rename(&part, &dest)?;
    d.set(1.0);
    Ok(())
}

// ------------------------------------------------------------------ transcription

pub struct Request {
    pub project: Project,
    /// Timeline range to transcribe.
    pub range: (Us, Us),
    /// Only hear these clips (None = the whole mix).
    pub only: Option<Vec<u64>>,
    pub lang: String,
    pub model: String,
    pub threads: u32,
    /// Let whisper use the graphics card when it was built with GPU support.
    pub gpu: bool,
}

#[derive(Default)]
pub struct Job {
    stage: Mutex<String>,
    frac: AtomicU32,
    pub done: AtomicBool,
    pub cancel: AtomicBool,
    pub error: Mutex<Option<String>>,
    /// Words in timeline time, and the detected language.
    pub result: Mutex<Option<(Vec<Word>, String)>>,
    pub started: Mutex<Option<Instant>>,
}

impl Job {
    pub fn frac(&self) -> f32 {
        f32::from_bits(self.frac.load(Ordering::Relaxed))
    }
    fn set(&self, f: f32) {
        self.frac.store(f.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
    pub fn stage(&self) -> String {
        self.stage.lock().unwrap().clone()
    }
    fn set_stage(&self, s: &str) {
        *self.stage.lock().unwrap() = s.to_string();
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

pub fn spawn(req: Request) -> Arc<Job> {
    let job = Arc::new(Job::default());
    *job.started.lock().unwrap() = Some(Instant::now());
    let j2 = job.clone();
    std::thread::Builder::new()
        .name("velomov-transcribe".into())
        .spawn(move || {
            match run(&req, &j2) {
                Ok(r) => *j2.result.lock().unwrap() = r,
                Err(e) => {
                    crate::vlog!("transcription failed: {:#}", e);
                    *j2.error.lock().unwrap() = Some(format!("{:#}", e));
                }
            }
            j2.done.store(true, Ordering::Relaxed);
        })
        .expect("transcribe thread");
    job
}

fn run(req: &Request, job: &Job) -> Result<Option<(Vec<Word>, String)>> {
    let whisper = find_whisper().ok_or_else(|| anyhow!("whisper-cli was not found (see the install guide)"))?;
    let (from, to) = req.range;
    if to <= from {
        bail!("nothing to transcribe");
    }
    // 1. model (downloaded on first use)
    let need_dl = !model_ready(&req.model);
    if need_dl {
        job.set_stage("Downloading the speech model");
        let m = model(&req.model);
        let d = Download { model: req.model.clone(), ..Default::default() };
        std::thread::scope(|s| -> Result<()> {
            let h = s.spawn(|| fetch(m, &d));
            while !h.is_finished() {
                d.cancel.store(job.cancelled(), Ordering::Relaxed);
                job.set(d.frac() * 0.25);
                std::thread::sleep(Duration::from_millis(100));
            }
            h.join().map_err(|_| anyhow!("download thread panicked"))?
        })?;
        if job.cancelled() {
            return Ok(None);
        }
    }
    // 2. audio: the timeline mix (or the chosen clips) as 16 kHz mono
    job.set_stage("Preparing audio");
    let base = if need_dl { 0.25 } else { 0.0 };
    let mut p = req.project.clone();
    if let Some(only) = &req.only {
        for t in p.tracks.iter_mut() {
            t.clips.retain(|c| only.contains(&c.id));
        }
    }
    let lib = super::audio::AudioLib::new();
    let start = Instant::now();
    let plan = loop {
        let (plan, complete) = super::audio::build_plan(&p, &lib);
        if complete || start.elapsed().as_secs() > 600 {
            break plan;
        }
        if job.cancelled() {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let tag = format!("{:x}", util::new_id());
    let mix = util::cache_dir().join(format!("captions-{}-mix.wav", tag));
    let wav = util::cache_dir().join(format!("captions-{}.wav", tag));
    let out_base = util::cache_dir().join(format!("captions-{}", tag));
    let json = out_base.with_extension("json");
    let cleanup = || {
        for f in [&mix, &wav, &json] {
            let _ = std::fs::remove_file(f);
        }
    };
    super::audio::render_wav(&plan, from, to, &mix)?;
    let st = util::command("ffmpeg")
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&mix)
        .args(["-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
        .arg(&wav)
        .status()
        .context("could not run ffmpeg")?;
    let _ = std::fs::remove_file(&mix);
    if !st.success() {
        cleanup();
        bail!("could not prepare the audio");
    }
    job.set(base + 0.05);
    if job.cancelled() {
        cleanup();
        return Ok(None);
    }
    // 3. whisper
    job.set_stage("Recognising speech");
    let threads = if req.threads > 0 {
        req.threads
    } else {
        std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(4).clamp(1, 8)
    };
    let mut cmd = util::command(whisper.to_str().unwrap_or("whisper-cli"));
    cmd.arg("-m")
        .arg(model_path(&req.model))
        .arg("-f")
        .arg(&wav)
        .args(["-l", &req.lang, "-ml", "1", "-sow", "-oj", "-pp", "-np", "-t", &threads.to_string(), "-of"])
        .arg(&out_base);
    if !req.gpu {
        cmd.arg("-ng");
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if let Some(dir) = whisper.parent() {
        // the DLLs / shared libraries live next to the program
        cmd.current_dir(dir);
    }
    let mut child = cmd.spawn().with_context(|| format!("could not run {}", whisper.display()))?;
    let stderr = child.stderr.take().unwrap();
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let (log2, span) = (log.clone(), 1.0 - base - 0.07);
    let job_frac = Arc::new(AtomicU32::new(0f32.to_bits()));
    let jf = job_frac.clone();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stderr).split(b'\n').map_while(|l| l.ok()) {
            let line = String::from_utf8_lossy(&line).to_string();
            if let Some(pct) = line.split("progress =").nth(1).and_then(|s| s.trim().trim_end_matches('%').trim().parse::<f32>().ok()) {
                jf.store((pct / 100.0).to_bits(), Ordering::Relaxed);
            } else {
                let mut l = log2.lock().unwrap();
                l.push(line);
                if l.len() > 40 {
                    l.remove(0);
                }
            }
        }
    });
    let status = loop {
        if job.cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            cleanup();
            return Ok(None);
        }
        if let Some(st) = child.try_wait()? {
            break st;
        }
        job.set(base + 0.05 + f32::from_bits(job_frac.load(Ordering::Relaxed)) * span);
        std::thread::sleep(Duration::from_millis(100));
    };
    let _ = reader.join();
    if !status.success() {
        cleanup();
        let tail = log.lock().unwrap().iter().rev().take(4).rev().cloned().collect::<Vec<_>>().join("\n");
        bail!("whisper failed ({}): {}", status, tail);
    }
    let bytes = std::fs::read(&json).context("whisper wrote no result")?;
    cleanup();
    let (words, lang) = parse_json(&bytes, from)?;
    job.set(1.0);
    Ok(Some((words, lang)))
}

/// Words from whisper-cli's JSON (`-ml 1 -sow` makes one segment per word). Pieces without a
/// leading space continue the previous word; sound tags like "[Music]" are dropped.
pub fn parse_json(bytes: &[u8], offset: Us) -> Result<(Vec<Word>, String)> {
    // a word cut in the middle of a UTF-8 sequence must not lose the whole result
    let text = String::from_utf8_lossy(bytes);
    let v: serde_json::Value = serde_json::from_str(&text).context("could not read whisper's result")?;
    let lang = v.pointer("/result/language").and_then(|l| l.as_str()).unwrap_or("").to_string();
    let mut words: Vec<Word> = vec![];
    let mut in_tag = false;
    for seg in v.get("transcription").and_then(|t| t.as_array()).into_iter().flatten() {
        let raw = seg.get("text").and_then(|t| t.as_str()).unwrap_or("").replace('\u{FFFD}', "");
        let a = seg.pointer("/offsets/from").and_then(|x| x.as_i64()).unwrap_or(0) * 1000 + offset;
        let b = seg.pointer("/offsets/to").and_then(|x| x.as_i64()).unwrap_or(0) * 1000 + offset;
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with('[') || t.starts_with('(') || t.starts_with('♪') || t.starts_with('*') {
            in_tag = !(t.ends_with(']') || t.ends_with(')') || t.ends_with('♪') || t.ends_with('*'));
            continue;
        }
        if in_tag {
            if t.ends_with(']') || t.ends_with(')') || t.ends_with('♪') || t.ends_with('*') {
                in_tag = false;
            }
            continue;
        }
        let joins = !raw.starts_with(' ') && !words.is_empty();
        match words.last_mut() {
            Some(w) if joins => {
                w.text.push_str(t);
                w.end = w.end.max(b);
            }
            _ => words.push(Word { text: t.to_string(), start: a, end: b.max(a + 1) }),
        }
    }
    // known subtitle-credit hallucinations on silence
    let lower: Vec<String> = words.iter().map(|w| w.text.to_lowercase()).collect();
    if let Some(i) = lower.iter().position(|w| w.contains("amara.org")) {
        let from = lower[..i].iter().rposition(|w| w.ends_with('.') || w.ends_with('!') || w.ends_with('?')).map(|p| p + 1).unwrap_or(0);
        words.drain(from..=i);
    }
    Ok((words, lang))
}

/// Split words into caption lines: at most `max_chars` characters, a new line after a
/// sentence ends, at pauses longer than 0.6 s and after 3.5 s.
pub fn group_words(words: &[Word], max_chars: usize) -> Vec<Vec<Word>> {
    let mut out: Vec<Vec<Word>> = vec![];
    let mut cur: Vec<Word> = vec![];
    let mut chars = 0usize;
    for w in words {
        let n = w.text.chars().count();
        if let (Some(first), Some(last)) = (cur.first(), cur.last()) {
            let ends_sentence = last.text.ends_with(['.', '?', '!', '…', ':', ';']);
            let pause = w.start - last.end > 600_000;
            let long = w.end - first.start > 3_500_000;
            if chars + 1 + n > max_chars.max(1) || ends_sentence || pause || long {
                out.push(std::mem::take(&mut cur));
                chars = 0;
            }
        }
        chars += if cur.is_empty() { n } else { n + 1 };
        cur.push(w.clone());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn is_auto_caption(c: &Clip) -> bool {
    matches!(&c.kind, ClipKind::Text(td) if !td.words.is_empty())
}

/// Put recognised words on a new text track as caption clips in caption style `style`.
/// `max_chars` 0 = by the canvas shape. With `replace`, earlier auto captions in `range`
/// go away. Returns (number of captions, first caption id).
pub fn insert_captions(p: &mut Project, words: &[Word], range: (Us, Us), style: &str, max_chars: usize, replace: bool) -> (usize, Option<u64>) {
    let portrait = p.height > p.width;
    let max_chars = match max_chars {
        0 if portrait => 18,
        0 => 32,
        n => n,
    };
    let groups = group_words(words, max_chars);
    let style = catalog::caption_style(style).unwrap_or(&catalog::CAPTION_STYLES[0]);
    let mut size = if portrait { 16.0 } else { 14.0 };
    if max_chars <= 1 || style.id == "single" {
        size *= 1.4;
    }
    let y = (-(p.height as f64) * if portrait { 0.2 } else { 0.3 }).round();
    if replace {
        for t in p.tracks.iter_mut() {
            t.clips.retain(|c| !(is_auto_caption(c) && c.start < range.1 && c.end() > range.0));
        }
    }
    let ti = p.add_track(TrackKind::Text);
    let mut first = None;
    for (gi, g) in groups.iter().enumerate() {
        let start = g[0].start;
        let last_end = g.last().map(|w| w.end).unwrap_or(start);
        let mut end = last_end + 300_000;
        if let Some(next) = groups.get(gi + 1).map(|n| n[0].start) {
            // short pauses keep the caption up (no flicker between lines)
            end = if next - last_end < 1_000_000 { next } else { end.min(next) };
        }
        let end = end.max(start + crate::model::edit::MIN_CLIP * 6);
        let mut td = TextData {
            text: g.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "),
            size,
            words: g.iter().map(|w| Word { text: w.text.clone(), start: w.start - start, end: w.end - start }).collect(),
            preset: "caption".into(),
            ..Default::default()
        };
        style.apply(&mut td);
        let mut c = Clip::new(ClipKind::Text(td), start, end - start);
        c.tf.y = y;
        first.get_or_insert(c.id);
        p.tracks[ti].clips.push(c);
    }
    p.tracks[ti].sort();
    p.normalize();
    (groups.len(), first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_word_segments() {
        let json = r#"{"result":{"language":"pt"},"transcription":[
            {"offsets":{"from":0,"to":320},"text":""},
            {"offsets":{"from":320,"to":600},"text":" Olá"},
            {"offsets":{"from":600,"to":700},"text":","},
            {"offsets":{"from":700,"to":1200},"text":" mundo"},
            {"offsets":{"from":1200,"to":1500},"text":" [Música]"}]}"#;
        let (w, lang) = parse_json(json.as_bytes(), 1_000_000).unwrap();
        assert_eq!(lang, "pt");
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].text, "Olá,");
        assert_eq!(w[0].start, 1_320_000);
        assert_eq!(w[0].end, 1_700_000);
        assert_eq!(w[1].text, "mundo");
    }

    #[test]
    fn groups_by_length_sentence_and_pause() {
        let mk = |t: &str, s: Us, e: Us| Word { text: t.into(), start: s * 1000, end: e * 1000 };
        let words = [mk("one", 0, 200), mk("two.", 200, 400), mk("three", 400, 600), mk("four", 2000, 2200), mk("five", 2200, 2400)];
        let g = group_words(&words, 30);
        let texts: Vec<usize> = g.iter().map(|g| g.len()).collect();
        assert_eq!(texts, vec![2, 1, 2]);
        let g = group_words(&words[3..], 5);
        assert_eq!(g.len(), 2);
    }
}
