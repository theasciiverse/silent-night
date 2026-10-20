//! The forest's sound, made here sample by sample (nothing is recorded):
//! wind in the spruces that rises and falls with the gusts on screen, a
//! whistle in the strongest of them, an owl now and then, snow dropping off
//! branches and roofs, and sleigh bells while the sleigh is in the sky.
//!
//! Linux (and other Unix but macOS) pipes the samples to the first player
//! it finds (pw-cat, paplay, aplay, ffplay), so the binary stays static;
//! macOS and Windows play through cpal.

use std::f32::consts::{PI, TAU};
use std::sync::{Arc, Mutex};

pub const RATE: u32 = 44_100;

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Owl { pan: f32 },
    Thump { pan: f32, size: f32 },
}

/// What the picture tells the sound, updated every frame.
#[derive(Default)]
pub struct Shared {
    /// Wind strength, 0 calm to about 1.6 in the hardest gust.
    pub wind: f32,
    /// The sleigh: pan (0 left, 1 right) and loudness, while it is up.
    pub sleigh: Option<(f32, f32)>,
    pub events: Vec<Event>,
    pub muted: bool,
}

// ---------------------------------------------------------------- building blocks

/// State-variable filter (Simper's trapezoidal form).
#[derive(Default, Clone, Copy)]
struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

impl Svf {
    fn set(&mut self, fc: f32, q: f32, fs: f32) {
        let g = (PI * fc.clamp(10.0, fs * 0.45) / fs).tan();
        let k = 1.0 / q;
        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }
    /// (low, band)
    fn run(&mut self, v0: f32) -> (f32, f32) {
        let v3 = v0 - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1)
    }
}

struct Comb {
    buf: Vec<f32>,
    i: usize,
    store: f32,
}

impl Comb {
    fn run(&mut self, x: f32) -> f32 {
        let y = self.buf[self.i];
        self.store = y * 0.7 + self.store * 0.3;
        self.buf[self.i] = x + self.store * 0.8;
        self.i = (self.i + 1) % self.buf.len();
        y
    }
}

struct Allpass {
    buf: Vec<f32>,
    i: usize,
}

impl Allpass {
    fn run(&mut self, x: f32) -> f32 {
        let b = self.buf[self.i];
        self.buf[self.i] = x + b * 0.5;
        self.i = (self.i + 1) % self.buf.len();
        b - x
    }
}

/// A small Freeverb: the forest's echo, for the owl, the bells and the
/// thumps.
struct Reverb {
    combs: Vec<Comb>,
    passes: Vec<Allpass>,
}

impl Reverb {
    fn new(fs: f32, spread: usize) -> Reverb {
        let n = |d: usize| ((d + spread) as f32 * fs / 44_100.0) as usize;
        Reverb {
            combs: [1116, 1188, 1277, 1356, 1422, 1491]
                .iter()
                .map(|&d| Comb { buf: vec![0.0; n(d)], i: 0, store: 0.0 })
                .collect(),
            passes: [556, 441, 341].iter().map(|&d| Allpass { buf: vec![0.0; n(d)], i: 0 }).collect(),
        }
    }
    fn run(&mut self, x: f32) -> f32 {
        let mut y: f32 = self.combs.iter_mut().map(|c| c.run(x)).sum::<f32>() * 0.15;
        for p in &mut self.passes {
            y = p.run(y);
        }
        y
    }
}

fn pan2(pan: f32) -> (f32, f32) {
    let a = pan.clamp(0.0, 1.0) * PI * 0.5;
    (a.cos(), a.sin())
}

// ---------------------------------------------------------------- voices

struct Note {
    start: f32,
    dur: f32,
    f0: f32,
    f1: f32,
    vibrato: f32,
    amp: f32,
}

/// A tawny owl far off in the trees: "hoo ... hu, hu-hoooo".
struct Owl {
    t: f32,
    notes: Vec<Note>,
    pan: f32,
    phase: f32,
    breath: Svf,
}

impl Owl {
    fn new(pan: f32, rng: &mut fastrand::Rng, fs: f32) -> Owl {
        let p = 0.92 + rng.f32() * 0.16;
        let n = |start: f32, dur: f32, f0: f32, f1: f32, vibrato: f32, amp: f32| Note {
            start,
            dur,
            f0: f0 * p,
            f1: f1 * p,
            vibrato,
            amp,
        };
        let gap = 1.6 + rng.f32() * 1.2;
        let mut breath = Svf::default();
        breath.set(420.0 * p, 4.0, fs);
        Owl {
            t: 0.0,
            notes: vec![
                n(0.0, 0.55, 400.0, 372.0, 0.0, 1.0),
                n(0.55 + gap, 0.10, 418.0, 405.0, 0.0, 0.55),
                n(0.55 + gap + 0.24, 0.09, 410.0, 398.0, 0.0, 0.55),
                n(0.55 + gap + 0.46, 0.95, 414.0, 360.0, 7.0, 0.9),
            ],
            pan,
            phase: 0.0,
            breath,
        }
    }
    fn end(&self) -> f32 {
        let n = self.notes.last().unwrap();
        n.start + n.dur + 0.3
    }
    fn sample(&mut self, dt: f32, noise: f32) -> f32 {
        self.t += dt;
        let Some(n) = self.notes.iter().find(|n| self.t >= n.start && self.t < n.start + n.dur + 0.15) else {
            return 0.0;
        };
        let u = self.t - n.start;
        let attack = (u / 0.05).min(1.0);
        let release = if u > n.dur { 1.0 - (u - n.dur) / 0.15 } else { 1.0 };
        let env = (0.5 - 0.5 * (PI * attack).cos()) * release.max(0.0).powf(1.5);
        let k = (u / n.dur).min(1.0);
        let f = n.f0 + (n.f1 - n.f0) * k + n.vibrato * (TAU * 6.5 * u).sin() * k;
        self.phase = (self.phase + TAU * f * dt) % TAU;
        let tone = self.phase.sin() + 0.12 * (2.0 * self.phase).sin() + 0.04 * (3.0 * self.phase).sin();
        let breath = self.breath.run(noise).1 * 0.6;
        (tone + breath) * env * n.amp
    }
}

/// Snow dropping off a branch or sliding off the roof: a soft low thud
/// and the hiss of powder after it.
struct Thump {
    t: f32,
    pan: f32,
    size: f32,
    lp1: f32,
    lp2: f32,
    hiss: Svf,
}

impl Thump {
    fn sample(&mut self, dt: f32, noise: f32, fs: f32) -> f32 {
        self.t += dt;
        let a = 1.0 - (-TAU * (140.0 + 120.0 / self.size) / fs).exp();
        self.lp1 += (noise - self.lp1) * a;
        self.lp2 += (self.lp1 - self.lp2) * a;
        let thud = self.lp2 * 9.0 * (-self.t / (0.09 + 0.08 * self.size)).exp() * (1.0 - (-self.t / 0.006).exp());
        let hiss = self.hiss.run(noise).1 * 0.25 * (-self.t / (0.25 + 0.3 * self.size)).exp() * (1.0 - (-self.t / 0.03).exp());
        (thud + hiss) * self.size
    }
}

/// One shake of the sleigh's bells.
struct Bell {
    t: f32,
    pan: f32,
    amp: f32,
    partials: [(f32, f32, f32, f32); 5], // freq, amp, decay, phase
}

impl Bell {
    fn new(pan: f32, amp: f32, rng: &mut fastrand::Rng) -> Bell {
        let f0 = 2300.0 + rng.f32() * 900.0;
        let ratios = [1.0, 1.47, 2.09, 2.56, 3.39];
        let amps = [1.0, 0.6, 0.45, 0.3, 0.2];
        let mut partials = [(0.0, 0.0, 0.0, 0.0); 5];
        for i in 0..5 {
            partials[i] = (f0 * ratios[i] * (0.99 + rng.f32() * 0.02), amps[i], 0.05 + rng.f32() * 0.12 / (1.0 + i as f32 * 0.4), rng.f32() * TAU);
        }
        Bell { t: 0.0, pan, amp, partials }
    }
    fn sample(&mut self, dt: f32) -> f32 {
        self.t += dt;
        let mut s = 0.0;
        for p in &mut self.partials {
            p.3 = (p.3 + TAU * p.0 * dt) % TAU;
            s += p.3.sin() * p.1 * (-self.t / p.2).exp();
        }
        s * self.amp * (1.0 - (-self.t / 0.0015).exp())
    }
}

enum Voice {
    Owl(Owl),
    Thump(Thump),
    Bell(Bell),
}

// ---------------------------------------------------------------- the mix

pub struct Synth {
    fs: f32,
    dt: f32,
    rng: fastrand::Rng,
    shared: Arc<Mutex<Shared>>,
    target_wind: f32,
    wind: f32,
    gain: f32,
    muted: bool,
    sleigh: Option<(f32, f32)>,
    next_jingle: f32,
    brown: [f32; 2],
    rumble: [Svf; 2],
    rustle: [Svf; 2],
    whistle: [Svf; 2],
    lfo: f32,
    counter: u32,
    voices: Vec<(Voice, f32)>, // the voice and its end time (owl) or 0
    reverb: [Reverb; 2],
    /// High-pass state: nothing below hearing goes to the speakers.
    hp: [(f32, f32); 2],
}

impl Synth {
    pub fn new(fs: f32, shared: Arc<Mutex<Shared>>, seed: u64) -> Synth {
        Synth {
            fs,
            dt: 1.0 / fs,
            rng: fastrand::Rng::with_seed(seed ^ 0x5157),
            shared,
            target_wind: 0.0,
            wind: 0.0,
            gain: 0.0,
            muted: false,
            sleigh: None,
            next_jingle: 0.0,
            brown: [0.0; 2],
            rumble: [Svf::default(); 2],
            rustle: [Svf::default(); 2],
            whistle: [Svf::default(); 2],
            lfo: 0.0,
            counter: 0,
            voices: Vec::new(),
            reverb: [Reverb::new(fs, 0), Reverb::new(fs, 23)],
            hp: [(0.0, 0.0); 2],
        }
    }

    fn noise(&mut self) -> f32 {
        self.rng.f32() * 2.0 - 1.0
    }

    /// Fills `out` with stereo samples, -1 to 1.
    pub fn render(&mut self, out: &mut [[f32; 2]]) {
        let events = {
            let mut s = self.shared.lock().unwrap();
            self.target_wind = s.wind;
            self.sleigh = s.sleigh;
            self.muted = s.muted;
            std::mem::take(&mut s.events)
        };
        for e in events {
            let v = match e {
                Event::Owl { pan } => {
                    let o = Owl::new(pan, &mut self.rng, self.fs);
                    let end = o.end();
                    (Voice::Owl(o), end)
                }
                Event::Thump { pan, size } => {
                    let mut hiss = Svf::default();
                    hiss.set(2400.0, 0.7, self.fs);
                    (Voice::Thump(Thump { t: 0.0, pan, size: size.clamp(0.2, 2.0), lp1: 0.0, lp2: 0.0, hiss }), 1.6)
                }
            };
            self.voices.push(v);
        }

        let wind_k = 1.0 - (-self.dt / 0.25).exp();
        let gain_k = 1.0 - (-self.dt / 0.08).exp();
        for frame in out.iter_mut() {
            self.wind += (self.target_wind - self.wind) * wind_k;
            let target_gain = if self.muted { 0.0 } else { 1.0 };
            self.gain += (target_gain - self.gain) * gain_k;
            let v = self.wind.max(0.0);

            if self.counter % 32 == 0 {
                self.lfo = (self.lfo + TAU * 0.13 * 32.0 * self.dt) % TAU;
                for ch in 0..2 {
                    let wobble = (self.lfo + ch as f32 * 1.3).sin();
                    self.rumble[ch].set(110.0 + 220.0 * v, 0.8, self.fs);
                    self.rustle[ch].set(500.0 + 1500.0 * v + 150.0 * wobble, 0.9, self.fs);
                    self.whistle[ch].set(620.0 + 520.0 * v + 110.0 * wobble, 11.0, self.fs);
                }
            }
            self.counter = self.counter.wrapping_add(1);

            // wind in the trees
            let a_rumble = 1.6 * v.powf(1.5);
            let a_rustle = 0.006 + 0.10 * v * v;
            let a_whistle = 0.5 * (v - 0.75).max(0.0).powi(2);
            let mut dry = [0.0f32; 2];
            for ch in 0..2 {
                let n = self.noise();
                self.brown[ch] = (self.brown[ch] + 0.03 * n) * 0.997;
                let rumble = self.rumble[ch].run(self.brown[ch]).1;
                let rustle = self.rustle[ch].run(n).1;
                let whistle = self.whistle[ch].run(n).1;
                dry[ch] = rumble * a_rumble + rustle * a_rustle + whistle * a_whistle;
            }

            // sleigh bells, shaken at a trot
            if let Some((pan, vol)) = self.sleigh {
                self.next_jingle -= self.dt;
                if self.next_jingle <= 0.0 && self.voices.len() < 40 {
                    let amp = vol * (0.025 + self.rng.f32() * 0.03);
                    let b = Bell::new(pan, amp, &mut self.rng);
                    self.voices.push((Voice::Bell(b), 0.0));
                    self.next_jingle = if self.rng.f32() < 0.25 { 0.02 + self.rng.f32() * 0.03 } else { 0.06 + self.rng.f32() * 0.12 };
                }
            }

            let mut send = [0.0f32; 2];
            let noise = self.noise();
            let fs = self.fs;
            let dt = self.dt;
            for (voice, _) in &mut self.voices {
                let (s, pan, dry_k, wet_k) = match voice {
                    Voice::Owl(o) => (o.sample(dt, noise) * 0.07, o.pan, 0.35, 1.0),
                    Voice::Thump(t) => (t.sample(dt, noise, fs) * 0.22, t.pan, 1.0, 0.5),
                    Voice::Bell(b) => (b.sample(dt), b.pan, 0.8, 0.8),
                };
                let (l, r) = pan2(pan);
                dry[0] += s * l * dry_k;
                dry[1] += s * r * dry_k;
                send[0] += s * l * wet_k;
                send[1] += s * r * wet_k;
            }
            self.voices.retain(|(v, end)| match v {
                Voice::Owl(o) => o.t < *end,
                Voice::Thump(t) => t.t < *end,
                Voice::Bell(b) => b.t < 1.0,
            });

            let hp_k = (-TAU * 35.0 * self.dt).exp();
            for ch in 0..2 {
                let wet = self.reverb[ch].run(send[ch]);
                let x = dry[ch] + wet * 0.9;
                let (px, py) = self.hp[ch];
                let y = hp_k * (py + x - px);
                self.hp[ch] = (x, y);
                frame[ch] = (y * self.gain).tanh();
            }
        }
    }
}

// ---------------------------------------------------------------- output

pub struct Sound {
    shared: Arc<Mutex<Shared>>,
    /// The player or device the sound goes to, if any.
    pub player: Option<String>,
    backend: Backend,
}

#[allow(dead_code)]
enum Backend {
    None,
    #[cfg(all(unix, not(target_os = "macos")))]
    Pipe {
        child: std::process::Child,
        stop: Arc<std::sync::atomic::AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    },
    #[cfg(any(target_os = "macos", windows))]
    Cpal(cpal::Stream),
}

impl Sound {
    /// Starts the sound, or a silent stand-in when nothing can play it.
    pub fn start(muted: bool, seed: u64) -> Sound {
        let shared = Arc::new(Mutex::new(Shared { muted, ..Shared::default() }));
        let (backend, player) = open(&shared, seed);
        log::info!("sound to {}", player.as_deref().unwrap_or("nothing (no player found)"));
        Sound { shared, player, backend }
    }

    /// A silent one, for --no-sound.
    pub fn none() -> Sound {
        Sound { shared: Arc::new(Mutex::new(Shared::default())), player: None, backend: Backend::None }
    }

    pub fn update(&self, wind: f32, sleigh: Option<(f32, f32)>, events: Vec<Event>) {
        let mut s = self.shared.lock().unwrap();
        s.wind = wind;
        s.sleigh = sleigh;
        if matches!(self.backend, Backend::None) {
            return;
        }
        s.events.extend(events);
    }

    /// Mutes or unmutes; returns true when the sound is now on.
    pub fn toggle(&self) -> bool {
        let mut s = self.shared.lock().unwrap();
        s.muted = !s.muted;
        !s.muted
    }

    pub fn muted(&self) -> bool {
        self.shared.lock().unwrap().muted
    }
}

impl Drop for Sound {
    fn drop(&mut self) {
        #[cfg(all(unix, not(target_os = "macos")))]
        if let Backend::Pipe { child, stop, thread } = &mut self.backend {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = child.kill();
            if let Some(t) = thread.take() {
                let _ = t.join();
            }
            let _ = child.wait();
        }
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn find_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(name).is_file()))
        .unwrap_or(false)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn open(shared: &Arc<Mutex<Shared>>, seed: u64) -> (Backend, Option<String>) {
    use std::io::Write;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    let rate = RATE.to_string();
    let players: [(&str, Vec<String>); 4] = [
        ("pw-cat", ["--playback", "--raw", "--format", "s16", "--rate", &rate, "--channels", "2", "--latency", "60ms", "-"].map(String::from).to_vec()),
        ("paplay", ["--raw", "--format=s16le", &format!("--rate={rate}"), "--channels=2", "--latency-msec=80"].map(String::from).to_vec()),
        ("aplay", ["-q", "-t", "raw", "-f", "S16_LE", "-r", &rate, "-c", "2", "-B", "120000"].map(String::from).to_vec()),
        ("ffplay", ["-nodisp", "-loglevel", "quiet", "-f", "s16le", "-ar", &rate, "-ch_layout", "stereo", "-i", "-"].map(String::from).to_vec()),
    ];
    for (name, args) in players {
        if !find_on_path(name) {
            continue;
        }
        let mut child = match Command::new(name).args(&args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() {
            Ok(c) => c,
            Err(e) => {
                log::warn!("{name}: {e}");
                continue;
            }
        };
        // a player that cannot play this (an old pw-cat, no sound server)
        // exits at once; then the next one is tried
        std::thread::sleep(Duration::from_millis(150));
        if let Ok(Some(status)) = child.try_wait() {
            log::warn!("{name} exited at once ({status})");
            continue;
        }
        let mut stdin = child.stdin.take().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let mut synth = Synth::new(RATE as f32, Arc::clone(shared), seed);
        let thread_stop = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            // Kept a little ahead of the clock, so the player's buffer, not
            // the pipe, sets the delay and the sound stays with the picture.
            let start = Instant::now();
            let mut frames: u64 = 0;
            let mut buf = vec![[0.0f32; 2]; 512];
            let mut bytes = Vec::with_capacity(512 * 4);
            while !thread_stop.load(Ordering::Relaxed) {
                let ahead = (start.elapsed().as_secs_f64() + 0.1) * RATE as f64;
                if frames as f64 >= ahead {
                    std::thread::sleep(Duration::from_millis(4));
                    continue;
                }
                synth.render(&mut buf);
                bytes.clear();
                for s in &buf {
                    for c in s {
                        bytes.extend_from_slice(&((c.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
                    }
                }
                if let Err(e) = stdin.write_all(&bytes) {
                    log::warn!("sound player stopped: {e}");
                    break;
                }
                frames += buf.len() as u64;
            }
        });
        return (Backend::Pipe { child, stop, thread: Some(thread) }, Some(name.to_string()));
    }
    (Backend::None, None)
}

#[cfg(any(target_os = "macos", windows))]
fn open(shared: &Arc<Mutex<Shared>>, seed: u64) -> (Backend, Option<String>) {
    match open_cpal(shared, seed) {
        Ok((stream, name)) => (Backend::Cpal(stream), Some(name)),
        Err(e) => {
            log::warn!("audio: {e}");
            (Backend::None, None)
        }
    }
}

#[cfg(any(target_os = "macos", windows))]
fn open_cpal(shared: &Arc<Mutex<Shared>>, seed: u64) -> Result<(cpal::Stream, String), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no output device")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let config = supported.config();
    let channels = (config.channels as usize).max(1);
    let synth = Synth::new(config.sample_rate as f32, Arc::clone(shared), seed);
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => build_stream(&device, config, synth, channels, |v| v),
        cpal::SampleFormat::I16 => build_stream(&device, config, synth, channels, |v| (v * 32767.0) as i16),
        cpal::SampleFormat::U16 => build_stream(&device, config, synth, channels, |v| ((v * 0.5 + 0.5) * 65535.0) as u16),
        f => return Err(format!("unsupported sample format {f:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok((stream, device.to_string()))
}

#[cfg(any(target_os = "macos", windows))]
fn build_stream<T: cpal::SizedSample + Send + 'static>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut synth: Synth,
    channels: usize,
    conv: fn(f32) -> T,
) -> Result<cpal::Stream, cpal::Error> {
    use cpal::traits::DeviceTrait;
    let mut buf: Vec<[f32; 2]> = Vec::new();
    device.build_output_stream(
        config,
        move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
            buf.resize(data.len() / channels, [0.0; 2]);
            synth.render(&mut buf);
            for (frame, s) in data.chunks_mut(channels).zip(&buf) {
                for (c, out) in frame.iter_mut().enumerate() {
                    *out = conv(match (channels, c) {
                        (1, _) => (s[0] + s[1]) * 0.5,
                        (_, 0) => s[0],
                        (_, 1) => s[1],
                        _ => 0.0,
                    });
                }
            }
        },
        |e| log::warn!("audio: {e}"),
        None,
    )
}

#[cfg(not(any(unix, windows)))]
fn open(_: &Arc<Mutex<Shared>>, _: u64) -> (Backend, Option<String>) {
    (Backend::None, None)
}
