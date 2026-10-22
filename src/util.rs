//! Small shared helpers: time units, ids, paths, logging.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Timeline time in microseconds.
pub type Us = i64;
pub const SEC: Us = 1_000_000;

#[inline]
pub fn us_to_s(us: Us) -> f64 {
    us as f64 / SEC as f64
}
#[inline]
pub fn s_to_us(s: f64) -> Us {
    (s * SEC as f64).round() as Us
}

/// Duration of one frame at `fps`.
#[inline]
pub fn frame_us(fps: f64) -> Us {
    ((SEC as f64) / fps.max(1.0)).round() as Us
}

/// Snap a time to the frame grid.
pub fn snap_frame(t: Us, fps: f64) -> Us {
    let f = SEC as f64 / fps.max(1.0);
    ((t as f64 / f).round() * f).round() as Us
}

/// HH:MM:SS:FF timecode for the player.
pub fn timecode(t: Us, fps: f64) -> String {
    let t = t.max(0);
    let total_frames = (us_to_s(t) * fps + 1e-6).floor() as i64;
    let fpsi = fps.round().max(1.0) as i64;
    let ff = total_frames % fpsi;
    let secs = total_frames / fpsi;
    format!("{:02}:{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60, ff)
}

/// Compact duration for clip labels: "5.0s", "1:05".
pub fn short_dur(t: Us) -> String {
    let s = us_to_s(t.max(0));
    if s < 60.0 {
        format!("{:.1}s", s)
    } else {
        let m = (s / 60.0).floor() as i64;
        format!("{}:{:02}", m, (s - m as f64 * 60.0).floor() as i64)
    }
}

/// Ruler label: "00:05", "01:30", or "12f" for sub-second ticks.
pub fn ruler_label(t: Us) -> String {
    let s = us_to_s(t).round() as i64;
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{:02}:{:02}", s / 60, s % 60)
    }
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Process-unique, mostly globally unique id (time-seeded so ids from saved projects don't collide).
pub fn new_id() -> u64 {
    let c = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    if c == 0 {
        let seed = now_ms() as u64 * 1000 + (std::process::id() as u64 % 1000);
        ID_COUNTER.store(seed + 2, Ordering::Relaxed);
        return seed + 1;
    }
    c
}

/// Make sure future ids are larger than the ones already used in a loaded project.
pub fn bump_ids(min: u64) {
    let _ = new_id();
    let mut cur = ID_COUNTER.load(Ordering::Relaxed);
    while cur <= min {
        match ID_COUNTER.compare_exchange(cur, min + 1, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(v) => cur = v,
        }
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

pub fn home_dir() -> PathBuf {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// Folder of the running program (bundled tools live next to it on Windows).
pub fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|p| p.canonicalize().ok().or(Some(p))).and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// Portable mode: a `portable.txt` next to the program keeps projects, settings and
/// caches in a `data` folder beside it (for a USB stick or an unzipped folder).
pub fn portable_root() -> Option<PathBuf> {
    static ROOT: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    ROOT.get_or_init(|| {
        let d = exe_dir()?;
        d.join("portable.txt").is_file().then(|| d.join("data"))
    })
    .clone()
}

#[cfg(not(windows))]
fn xdg(var: &str, fallback: &str) -> PathBuf {
    match std::env::var_os(var) {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => home_dir().join(fallback),
    }
}

/// Per-user base folder for one kind of file ("data", "cache" or "config").
fn base_dir(kind: &str) -> PathBuf {
    if let Some(root) = portable_root() {
        return root.join(kind);
    }
    #[cfg(windows)]
    {
        // %APPDATA%\VeloMov for projects and settings, %LOCALAPPDATA%\VeloMov\cache for caches
        let env = |v: &str| std::env::var_os(v).filter(|p| !p.is_empty()).map(PathBuf::from);
        let roaming = env("APPDATA").unwrap_or_else(|| home_dir().join("AppData").join("Roaming"));
        let local = env("LOCALAPPDATA").unwrap_or_else(|| home_dir().join("AppData").join("Local"));
        match kind {
            "cache" => local.join("VeloMov").join("cache"),
            "config" => roaming.join("VeloMov").join("config"),
            _ => roaming.join("VeloMov"),
        }
    }
    #[cfg(not(windows))]
    {
        match kind {
            "cache" => xdg("XDG_CACHE_HOME", ".cache").join("velomov"),
            "config" => xdg("XDG_CONFIG_HOME", ".config").join("velomov"),
            _ => xdg("XDG_DATA_HOME", ".local/share").join("velomov"),
        }
    }
}

pub fn data_dir() -> PathBuf {
    let p = base_dir("data");
    let _ = std::fs::create_dir_all(&p);
    p
}
pub fn cache_dir() -> PathBuf {
    let p = base_dir("cache");
    let _ = std::fs::create_dir_all(&p);
    p
}
pub fn config_dir() -> PathBuf {
    let p = base_dir("config");
    let _ = std::fs::create_dir_all(&p);
    p
}
pub fn projects_dir() -> PathBuf {
    let p = data_dir().join("projects");
    let _ = std::fs::create_dir_all(&p);
    p
}
pub fn videos_dir() -> PathBuf {
    #[cfg(not(windows))]
    {
        let out = std::process::Command::new("xdg-user-dir").arg("VIDEOS").output();
        if let Ok(o) = out {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !s.is_empty() && std::path::Path::new(&s).is_dir() {
                return PathBuf::from(s);
            }
        }
    }
    let v = home_dir().join("Videos");
    if v.is_dir() {
        v
    } else {
        home_dir()
    }
}

/// FNV-1a 64 bit; used for cache file names.
pub fn hash_str(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

pub fn hash_bytes(h0: u64, bytes: &[u8]) -> u64 {
    let mut h = h0 ^ 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// File-identity key: path + size + mtime, so caches invalidate when a file changes.
pub fn file_key(path: &str) -> u64 {
    let meta = std::fs::metadata(path);
    let (size, mtime) = match meta {
        Ok(m) => (m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)),
        Err(_) => (0, 0),
    };
    hash_str(&format!("{}|{}|{}", path, size, mtime))
}

/// Deterministic per-integer hash -> [0,1)
pub fn hash01(n: u64) -> f64 {
    let mut x = n.wrapping_mul(0x9E3779B97F4A7C15);
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58476D1CE4E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D049BB133111EB);
    x ^= x >> 31;
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// Smooth 1D value noise in [-1,1] (used for shakes / wobbles).
pub fn noise1(t: f64, seed: u64) -> f64 {
    let i = t.floor();
    let f = t - i;
    let a = hash01((i as i64 as u64).wrapping_add(seed.wrapping_mul(7919))) * 2.0 - 1.0;
    let b = hash01(((i as i64 + 1) as u64).wrapping_add(seed.wrapping_mul(7919))) * 2.0 - 1.0;
    let u = f * f * (3.0 - 2.0 * f);
    a + (b - a) * u
}

#[macro_export]
macro_rules! vlog {
    ($($arg:tt)*) => {{
        if std::env::var_os("VELOMOV_QUIET").is_none() {
            eprintln!("[velomov] {}", format!($($arg)*));
        }
    }};
}

/// Extension of a path in lowercase.
pub fn ext_lower(path: &str) -> String {
    std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

pub fn file_stem_of(path: &str) -> String {
    std::path::Path::new(path).file_stem().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

pub fn file_name(path: &str) -> String {
    std::path::Path::new(path).file_name().map(|e| e.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

pub const VIDEO_EXTS: &[&str] =
    &["mp4", "mov", "mkv", "webm", "avi", "m4v", "mts", "m2ts", "ts", "flv", "wmv", "3gp", "mpg", "mpeg", "ogv", "gif"];
pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"];
pub const AUDIO_EXTS: &[&str] = &["mp3", "wav", "flac", "ogg", "oga", "opus", "m4a", "aac", "wma", "aiff", "aif"];

pub fn is_media_path(path: &str) -> bool {
    let e = ext_lower(path);
    VIDEO_EXTS.contains(&e.as_str()) || IMAGE_EXTS.contains(&e.as_str()) || AUDIO_EXTS.contains(&e.as_str())
}

/// A command for an external tool (ffmpeg, ffprobe, curl…). A copy bundled next to VeloMov
/// wins over PATH, and on Windows no console window pops up for it.
pub fn command(program: &str) -> std::process::Command {
    let mut prog = PathBuf::from(program);
    if prog.components().count() == 1 {
        if let Some(dir) = exe_dir() {
            let exe = if cfg!(windows) { format!("{}.exe", program) } else { program.to_string() };
            for cand in [dir.join(&exe), dir.join("ffmpeg").join(&exe)] {
                if cand.is_file() {
                    prog = cand;
                    break;
                }
            }
        }
    }
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(prog);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// An `ffmpeg` command that is killed if VeloMov goes away (Linux parent-death signal),
/// for jobs that write files in the background (proxies, reversed clips).
pub fn ffmpeg_job() -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = command("ffmpeg");
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: prctl is async-signal-safe and touches no memory of the parent.
        unsafe {
            cmd.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
    }
    cmd
}

/// Open a file or folder with the system's default application.
pub fn open_path(p: &std::path::Path) {
    #[cfg(windows)]
    let r = command("explorer").arg(p).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(p).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(p).spawn();
    if let Err(e) = r {
        crate::vlog!("could not open {}: {}", p.display(), e);
    }
}

/// Mark a cache file as just used (automatic cache cleaning goes by last use).
pub fn touch(path: &std::path::Path) {
    if let Ok(f) = std::fs::File::options().write(true).open(path) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
}

/// Total size of the files under `dir`.
pub fn dir_size(dir: &std::path::Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            _ => e.metadata().map(|m| m.len()).unwrap_or(0),
        })
        .sum()
}

/// Delete cache files (extracted audio, proxies, reversed clips, thumbnails, sound effects)
/// nobody used for `days` days, plus leftovers of interrupted jobs. Everything there is
/// recreated on demand. Returns (files, bytes) removed.
pub fn clean_cache(days: u32) -> (usize, u64) {
    let root = cache_dir();
    let now = std::time::SystemTime::now();
    let age = |m: &std::fs::Metadata| {
        let used = [m.modified().ok(), m.accessed().ok()].into_iter().flatten().max();
        used.and_then(|t| now.duration_since(t).ok()).map(|d| d.as_secs()).unwrap_or(0)
    };
    let limit = days as u64 * 86_400;
    let (mut files, mut bytes) = (0usize, 0u64);
    let mut remove = |p: &std::path::Path, m: &std::fs::Metadata| {
        if std::fs::remove_file(p).is_ok() {
            files += 1;
            bytes += m.len();
        }
    };
    for sub in ["audio", "proxies", "reverse", "thumbs", "sfx"] {
        let Ok(rd) = std::fs::read_dir(root.join(sub)) else { continue };
        for e in rd.flatten() {
            let Ok(m) = e.metadata() else { continue };
            let leftover = e.file_name().to_string_lossy().contains(".tmp") && age(&m) > 86_400;
            if m.is_file() && (age(&m) > limit || leftover) {
                remove(&e.path(), &m);
            }
        }
    }
    // temporary export audio and half-made reversed clips left by a crash
    let mut tmp_bytes = 0;
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Ok(m) = e.metadata() else { continue };
            if age(&m) <= 86_400 {
                continue;
            }
            if m.is_file() && name.starts_with("export-") {
                remove(&e.path(), &m);
            } else if m.is_dir() && name.starts_with("reverse-tmp-") {
                let size = dir_size(&e.path());
                if std::fs::remove_dir_all(e.path()).is_ok() {
                    tmp_bytes += size;
                }
            }
        }
    }
    (files, bytes + tmp_bytes)
}
