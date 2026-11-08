use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::Relaxed};
use std::sync::mpsc::{Receiver, Sender, channel};

use windows::Win32::Media::Audio::*;
use windows::Win32::Media::MMSYSERR_NOERROR;
use windows::core::PSTR;

use crate::hand::find_asset;

pub const CHOICES: [&str; 6] = ["thock", "click", "pop", "wood", "boop", "squeak"];

enum Cmd {
    Open,
    Close,
    Play,
}

static TX: OnceLock<Sender<Cmd>> = OnceLock::new();
static CHOICE: AtomicUsize = AtomicUsize::new(0);
static ENABLED: AtomicBool = AtomicBool::new(true);

/// 44.1 kHz, 16-bit, mono. Returns the raw samples or None for anything else.
fn pcm(bytes: &[u8]) -> Option<Vec<u8>> {
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
                ok = tag == 1 && channels == 1 && rate == 44100 && bits == 16;
            }
            b"data" => return ok.then(|| body.to_vec()),
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    None
}

/// Everything that touches the audio device lives on this one thread. The mouse hook
/// only sends a message, because a slow or blocked hook stalls the whole system.
struct Player {
    clips: Vec<Vec<u8>>,
    headers: Vec<Box<WAVEHDR>>,
    device: Option<HWAVEOUT>,
}

impl Player {
    fn open(&mut self) {
        if self.device.is_some() {
            return;
        }
        let format = WAVEFORMATEX { wFormatTag: WAVE_FORMAT_PCM as u16, nChannels: 1, nSamplesPerSec: 44100, nAvgBytesPerSec: 88200, nBlockAlign: 2, wBitsPerSample: 16, cbSize: 0 };
        let mut device = HWAVEOUT::default();
        unsafe {
            if waveOutOpen(Some(&mut device), WAVE_MAPPER, &format, None, None, CALLBACK_NULL) != MMSYSERR_NOERROR {
                return;
            }
            for (clip, hdr) in self.clips.iter().zip(&mut self.headers) {
                **hdr = WAVEHDR { lpData: PSTR(clip.as_ptr() as *mut u8), dwBufferLength: clip.len() as u32, ..Default::default() };
                waveOutPrepareHeader(device, &mut **hdr, size_of::<WAVEHDR>() as u32);
            }
        }
        self.device = Some(device);
    }

    fn close(&mut self) {
        let Some(device) = self.device.take() else { return };
        unsafe {
            waveOutReset(device);
            for hdr in &mut self.headers {
                waveOutUnprepareHeader(device, &mut **hdr, size_of::<WAVEHDR>() as u32);
            }
            waveOutClose(device);
        }
    }

    /// Cuts off the previous tap, like Mac's .interrupts.
    fn play(&mut self) {
        let Some(device) = self.device else { return };
        let i = CHOICE.load(Relaxed);
        let Some(hdr) = self.headers.get_mut(i).filter(|_| self.clips.get(i).is_some_and(|c| !c.is_empty())) else { return };
        unsafe {
            waveOutReset(device);
            waveOutWrite(device, &mut **hdr, size_of::<WAVEHDR>() as u32);
        }
    }
}

fn run(rx: Receiver<Cmd>) {
    let clips: Vec<Vec<u8>> = CHOICES
        .iter()
        .map(|name| find_asset(&format!("sounds/tap_{name}.wav")).and_then(|p| std::fs::read(p).ok()).and_then(|b| pcm(&b)).unwrap_or_default())
        .collect();
    let headers = clips.iter().map(|_| Box::new(WAVEHDR::default())).collect();
    let mut player = Player { clips, headers, device: None };
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        batch.extend(rx.try_iter());
        // Taps that piled up while the device was busy collapse into one.
        let last = batch.len() - 1;
        for (i, cmd) in batch.iter().enumerate() {
            match cmd {
                Cmd::Open => player.open(),
                Cmd::Close => player.close(),
                Cmd::Play if matches!(batch.get(i + 1), Some(Cmd::Play)) && i != last => {}
                Cmd::Play => player.play(),
            }
        }
    }
}

/// Off until the waveOut backend is replaced: processes with it open got stuck exiting.
const AUDIO_ENABLED: bool = false;

pub fn load() {
    if !AUDIO_ENABLED {
        return;
    }
    TX.get_or_init(|| {
        let (tx, rx) = channel();
        std::thread::spawn(move || run(rx));
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
