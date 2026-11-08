use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

use crate::hand::find_asset;

pub const CHOICES: [&str; 6] = ["thock", "click", "pop", "wood", "boop", "squeak"];

const RATE: u32 = 44100;
/// Audio queued ahead of the speakers. Small, so a tap is heard within a few milliseconds.
const QUEUE_FRAMES: u32 = 280;
const TICK: Duration = Duration::from_millis(3);
const STOP_AFTER_IDLE: Duration = Duration::from_secs(30);

enum Cmd {
    Open,
    Close,
    Play,
    Preview(usize),
}

const PREVIEW_LINGER: Duration = Duration::from_millis(1500);

static TX: OnceLock<Sender<Cmd>> = OnceLock::new();
static CHOICE: AtomicUsize = AtomicUsize::new(0);
static ENABLED: AtomicBool = AtomicBool::new(true);

/// 44.1 kHz, 16-bit, mono. Returns the samples or None for anything else.
fn pcm(bytes: &[u8]) -> Option<Vec<i16>> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let (mut at, mut ok) = (12, false);
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        let body = bytes.get(at + 8..(at + 8 + size).min(bytes.len()))?;
        match &bytes[at..at + 4] {
            b"fmt " if body.len() >= 16 => {
                let tag = u16::from_le_bytes([body[0], body[1]]);
                let channels = u16::from_le_bytes([body[2], body[3]]);
                let rate = u32::from_le_bytes(body[4..8].try_into().ok()?);
                let bits = u16::from_le_bytes([body[14], body[15]]);
                ok = tag == 1 && channels == 1 && rate == RATE && bits == 16;
            }
            b"data" => return ok.then(|| body.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()),
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    None
}

/// An open shared-mode stream. WASAPI converts our 44.1 kHz mono to the device format.
struct Stream {
    client: IAudioClient,
    render: IAudioRenderClient,
    frames: u32,
    running: bool,
}

impl Stream {
    fn open() -> windows::core::Result<Self> {
        unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
            let format = WAVEFORMATEX { wFormatTag: WAVE_FORMAT_PCM as u16, nChannels: 1, nSamplesPerSec: RATE, nAvgBytesPerSec: RATE * 2, nBlockAlign: 2, wBitsPerSample: 16, cbSize: 0 };
            let flags = AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
            client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, 200_000, 0, &format, None)?;
            let frames = client.GetBufferSize()?;
            let render: IAudioRenderClient = client.GetService()?;
            Ok(Self { client, render, frames, running: false })
        }
    }

    fn start(&mut self) {
        if !self.running && unsafe { self.client.Start() }.is_ok() {
            self.running = true;
        }
    }

    fn stop(&mut self) {
        if self.running {
            unsafe {
                let _ = self.client.Stop();
                let _ = self.client.Reset();
            }
            self.running = false;
        }
    }

    /// Tops the queue up to a few milliseconds, with the current tap or silence.
    fn pump(&mut self, voice: &mut Option<(usize, usize)>, clips: &[Vec<i16>]) {
        let Ok(padding) = (unsafe { self.client.GetCurrentPadding() }) else { return };
        let room = self.frames.saturating_sub(padding);
        let n = QUEUE_FRAMES.saturating_sub(padding).min(room);
        if n == 0 {
            return;
        }
        let Ok(buffer) = (unsafe { self.render.GetBuffer(n) }) else { return };
        let out = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<i16>(), n as usize) };
        out.fill(0);
        if let Some((clip, pos)) = voice {
            let data = &clips[*clip];
            let take = (data.len() - *pos).min(out.len());
            out[..take].copy_from_slice(&data[*pos..*pos + take]);
            *pos += take;
            if *pos >= data.len() {
                *voice = None;
            }
        }
        unsafe {
            let _ = self.render.ReleaseBuffer(n, 0);
        }
    }
}

/// Everything that touches the audio device lives on this one thread. The mouse hook
/// only sends a message, because a slow or blocked hook stalls the whole system.
fn run(rx: Receiver<Cmd>) {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let clips: Vec<Vec<i16>> = CHOICES
        .iter()
        .map(|name| find_asset(&format!("sounds/tap_{name}.wav")).and_then(|p| std::fs::read(p).ok()).and_then(|b| pcm(&b)).unwrap_or_default())
        .collect();

    let mut stream: Option<Stream> = None;
    let mut voice: Option<(usize, usize)> = None;
    let mut wanted = false;
    let mut last_tap = Instant::now();

    loop {
        let cmd = match stream.as_ref().is_some_and(|s| s.running) {
            true => match rx.recv_timeout(TICK) {
                Ok(cmd) => Some(cmd),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            false => match rx.recv() {
                Ok(cmd) => Some(cmd),
                Err(_) => break,
            },
        };
        match cmd {
            Some(Cmd::Open) => {
                wanted = true;
                if stream.is_none() {
                    match Stream::open() {
                        Ok(s) => stream = Some(s),
                        Err(e) => crate::log::write(&format!("audio open failed: {e}")),
                    }
                }
                last_tap = Instant::now();
                if let Some(s) = &mut stream {
                    s.start();
                }
            }
            Some(Cmd::Close) => {
                wanted = false;
                voice = None;
                if let Some(mut s) = stream.take() {
                    s.stop();
                }
            }
            Some(Cmd::Play) if wanted => {
                let i = CHOICE.load(Relaxed);
                if clips.get(i).is_some_and(|c| !c.is_empty()) {
                    voice = Some((i, 0));
                }
                last_tap = Instant::now();
                if let Some(s) = &mut stream {
                    s.start();
                }
            }
            Some(Cmd::Preview(i)) => {
                // Settings can preview a sound while the pointer is off.
                if stream.is_none() {
                    match Stream::open() {
                        Ok(s) => stream = Some(s),
                        Err(e) => crate::log::write(&format!("audio open failed: {e}")),
                    }
                }
                if clips.get(i).is_some_and(|c| !c.is_empty()) {
                    voice = Some((i, 0));
                }
                last_tap = Instant::now();
                if let Some(s) = &mut stream {
                    s.start();
                }
            }
            _ => {}
        }
        if let Some(s) = &mut stream {
            if s.running {
                s.pump(&mut voice, &clips);
                if voice.is_none() && last_tap.elapsed() > STOP_AFTER_IDLE {
                    s.stop();
                }
            }
        }
        if !wanted && voice.is_none() && last_tap.elapsed() > PREVIEW_LINGER {
            if let Some(mut s) = stream.take() {
                s.stop();
            }
        }
    }
}

pub fn load() {
    TX.get_or_init(|| {
        let (tx, rx) = channel();
        let _ = std::thread::Builder::new().name("audio".into()).spawn(move || run(rx));
        tx
    });
}

fn send(cmd: Cmd) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(cmd);
    }
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Relaxed);
}

pub fn set_choice(name: &str) {
    if let Some(i) = CHOICES.iter().position(|c| *c == name) {
        CHOICE.store(i, Relaxed);
    }
}

/// Plays one sound once, for the settings page.
pub fn preview(name: &str) {
    if let Some(i) = CHOICES.iter().position(|c| *c == name) {
        send(Cmd::Preview(i));
    }
}

/// Opened while the pointer is on, like the Mac engine that stops when idle.
pub fn open() {
    send(Cmd::Open);
}

pub fn close() {
    send(Cmd::Close);
}

/// Safe to call from the mouse hook: a channel send, no device access.
pub fn play() {
    if ENABLED.load(Relaxed) {
        send(Cmd::Play);
    }
}
