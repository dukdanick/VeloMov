//! Real-time audio output (cpal -> ALSA/PipeWire). The audio clock drives the
//! playhead while playing; without a device a wall clock is used instead.

use crate::media::audio::{mix, MixPlan, RATE};
use crate::util::Us;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

pub struct Shared {
    pub plan: Mutex<Arc<MixPlan>>,
    pub playing: AtomicBool,
    /// timeline frames (48 kHz) handed to the device so far
    pub pos: AtomicI64,
    pub end: AtomicI64,
    /// device latency in microseconds
    pub latency_us: AtomicI64,
    pub master: AtomicU32,
    pub muted: AtomicBool,
}

pub struct Player {
    stream: Option<cpal::Stream>,
    pub shared: Arc<Shared>,
    pub device_name: String,
    pub error: Option<String>,
    wall: Option<(Instant, Us)>,
}

fn run<T>(device: &cpal::Device, config: &cpal::StreamConfig, shared: Arc<Shared>) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = config.channels as usize;
    let dev_rate = config.sample_rate.0 as f64;
    let ratio = RATE as f64 / dev_rate;
    let mut frac: f64 = 0.0;
    let mut tmp: Vec<f32> = vec![];
    let err_fn = |e| crate::vlog!("audio stream error: {}", e);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
                let frames = data.len() / channels.max(1);
                let ts = info.timestamp();
                if let Some(d) = ts.playback.duration_since(&ts.callback) {
                    shared.latency_us.store(d.as_micros() as i64, Ordering::Relaxed);
                }
                if !shared.playing.load(Ordering::Relaxed) {
                    for s in data.iter_mut() {
                        *s = T::from_sample(0.0f32);
                    }
                    frac = 0.0;
                    return;
                }
                let pos = shared.pos.load(Ordering::Relaxed);
                // number of 48k frames needed for `frames` device frames
                let need = ((frames as f64) * ratio + frac).floor() as usize + 2;
                tmp.resize(need * 2, 0.0);
                let plan = shared.plan.lock().map(|p| p.clone()).ok();
                match plan {
                    Some(p) => mix(&p, pos, &mut tmp),
                    None => tmp.iter_mut().for_each(|v| *v = 0.0),
                }
                let gain = if shared.muted.load(Ordering::Relaxed) { 0.0 } else { f32::from_bits(shared.master.load(Ordering::Relaxed)) };
                let mut src = frac;
                for f in 0..frames {
                    let i = src.floor() as usize;
                    let t = (src - i as f64) as f32;
                    let l = tmp[i * 2] + (tmp[(i + 1) * 2] - tmp[i * 2]) * t;
                    let r = tmp[i * 2 + 1] + (tmp[(i + 1) * 2 + 1] - tmp[i * 2 + 1]) * t;
                    for c in 0..channels {
                        let v = if channels == 1 {
                            (l + r) * 0.5
                        } else if c % 2 == 0 {
                            l
                        } else {
                            r
                        };
                        data[f * channels + c] = T::from_sample(v * gain);
                    }
                    src += ratio;
                }
                let advanced = src.floor() as i64;
                frac = src - advanced as f64;
                let np = pos + advanced;
                shared.pos.store(np, Ordering::Relaxed);
                if np >= shared.end.load(Ordering::Relaxed) {
                    shared.playing.store(false, Ordering::Relaxed);
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| e.to_string())
}

impl Player {
    pub fn new() -> Self {
        let shared = Arc::new(Shared {
            plan: Mutex::new(Arc::new(MixPlan::default())),
            playing: AtomicBool::new(false),
            pos: AtomicI64::new(0),
            end: AtomicI64::new(i64::MAX),
            latency_us: AtomicI64::new(0),
            master: AtomicU32::new(1f32.to_bits()),
            muted: AtomicBool::new(false),
        });
        let mut p = Player { stream: None, shared, device_name: String::new(), error: None, wall: None };
        if std::env::var_os("VELOMOV_NO_AUDIO").is_some() {
            p.error = Some("audio disabled".into());
            return p;
        }
        match p.open() {
            Ok(()) => {}
            Err(e) => {
                crate::vlog!("audio output unavailable: {}", e);
                p.error = Some(e);
            }
        }
        p
    }

    fn open(&mut self) -> Result<(), String> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("no audio output device")?;
        self.device_name = device.name().unwrap_or_default();
        let def = device.default_output_config().map_err(|e| e.to_string())?;
        // prefer 48 kHz if the device supports it
        let mut cfg: cpal::StreamConfig = def.config();
        if let Ok(mut ranges) = device.supported_output_configs() {
            if ranges.any(|r| {
                r.min_sample_rate().0 <= RATE
                    && r.max_sample_rate().0 >= RATE
                    && r.sample_format() == def.sample_format()
                    && r.channels() == def.channels()
            }) {
                cfg.sample_rate = cpal::SampleRate(RATE);
            }
        }
        let sh = self.shared.clone();
        let stream = match def.sample_format() {
            cpal::SampleFormat::F32 => run::<f32>(&device, &cfg, sh),
            cpal::SampleFormat::I16 => run::<i16>(&device, &cfg, sh),
            cpal::SampleFormat::U16 => run::<u16>(&device, &cfg, sh),
            cpal::SampleFormat::I32 => run::<i32>(&device, &cfg, sh),
            other => Err(format!("unsupported sample format {:?}", other)),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        crate::vlog!("audio output: {} ({} Hz, {} ch, {:?})", self.device_name, cfg.sample_rate.0, cfg.channels, def.sample_format());
        self.stream = Some(stream);
        Ok(())
    }

    pub fn has_device(&self) -> bool {
        self.stream.is_some()
    }

    pub fn set_plan(&self, plan: MixPlan) {
        if let Ok(mut g) = self.shared.plan.lock() {
            *g = Arc::new(plan);
        }
    }

    pub fn play(&mut self, from: Us, end: Us) {
        let f = (from as f64 * RATE as f64 / 1e6) as i64;
        self.shared.pos.store(f, Ordering::Relaxed);
        self.shared.end.store((end as f64 * RATE as f64 / 1e6) as i64, Ordering::Relaxed);
        self.shared.playing.store(true, Ordering::Relaxed);
        self.wall = Some((Instant::now(), from));
    }

    pub fn pause(&mut self) {
        self.shared.playing.store(false, Ordering::Relaxed);
        self.wall = None;
    }

    pub fn playing(&self) -> bool {
        if self.has_device() {
            self.shared.playing.load(Ordering::Relaxed)
        } else {
            self.wall.is_some()
        }
    }

    /// Current playback time (what the listener hears).
    pub fn time(&self) -> Us {
        if self.has_device() {
            let pos = self.shared.pos.load(Ordering::Relaxed);
            let lat = self.shared.latency_us.load(Ordering::Relaxed).clamp(0, 200_000);
            ((pos as f64 / RATE as f64 * 1e6) as Us - lat).max(0)
        } else if let Some((start, from)) = self.wall {
            from + start.elapsed().as_micros() as Us
        } else {
            0
        }
    }

    pub fn set_master(&self, v: f32) {
        self.shared.master.store(v.to_bits(), Ordering::Relaxed);
    }

    pub fn set_muted(&self, m: bool) {
        self.shared.muted.store(m, Ordering::Relaxed);
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}
