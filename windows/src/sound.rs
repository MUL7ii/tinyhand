use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicUsize, Ordering::Relaxed};

use windows::Win32::Media::Audio::*;
use windows::Win32::Media::MMSYSERR_NOERROR;
use windows::core::PSTR;

use crate::hand::find_asset;

pub const CHOICES: [&str; 6] = ["thock", "click", "pop", "wood", "boop", "squeak"];

struct Clip {
    data: Vec<u8>,
    hdr: UnsafeCell<WAVEHDR>,
}

struct Bank(Vec<Clip>);

// Headers are only touched by whichever thread is playing, one call at a time.
unsafe impl Sync for Bank {}
unsafe impl Send for Bank {}

static BANK: OnceLock<Bank> = OnceLock::new();
static DEVICE: AtomicIsize = AtomicIsize::new(0);
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

pub fn load() {
    BANK.get_or_init(|| {
        let clips = CHOICES
            .iter()
            .map(|name| {
                let data = find_asset(&format!("sounds/tap_{name}.wav")).and_then(|p| std::fs::read(p).ok()).and_then(|b| pcm(&b)).unwrap_or_default();
                Clip { data, hdr: UnsafeCell::new(WAVEHDR::default()) }
            })
            .collect();
        Bank(clips)
    });
}

pub fn set_enabled(on: bool) {
    ENABLED.store(on, Relaxed);
}

pub fn set_choice(name: &str) {
    if let Some(i) = CHOICES.iter().position(|c| *c == name) {
        CHOICE.store(i, Relaxed);
    }
}

fn handle() -> HWAVEOUT {
    HWAVEOUT(DEVICE.load(Relaxed) as *mut c_void)
}

/// Opened while the pointer is on, like the Mac engine that stops when idle.
pub fn open() {
    let Some(bank) = BANK.get() else { return };
    if DEVICE.load(Relaxed) != 0 {
        return;
    }
    let format = WAVEFORMATEX { wFormatTag: WAVE_FORMAT_PCM as u16, nChannels: 1, nSamplesPerSec: 44100, nAvgBytesPerSec: 88200, nBlockAlign: 2, wBitsPerSample: 16, cbSize: 0 };
    let mut device = HWAVEOUT::default();
    unsafe {
        if waveOutOpen(Some(&mut device), WAVE_MAPPER, &format, None, None, CALLBACK_NULL) != MMSYSERR_NOERROR {
            return;
        }
        for clip in &bank.0 {
            let hdr = &mut *clip.hdr.get();
            *hdr = WAVEHDR { lpData: PSTR(clip.data.as_ptr() as *mut u8), dwBufferLength: clip.data.len() as u32, ..Default::default() };
            waveOutPrepareHeader(device, hdr, size_of::<WAVEHDR>() as u32);
        }
    }
    DEVICE.store(device.0 as isize, Relaxed);
}

pub fn close() {
    let device = handle();
    if DEVICE.swap(0, Relaxed) == 0 {
        return;
    }
    let Some(bank) = BANK.get() else { return };
    unsafe {
        waveOutReset(device);
        for clip in &bank.0 {
            waveOutUnprepareHeader(device, clip.hdr.get(), size_of::<WAVEHDR>() as u32);
        }
        waveOutClose(device);
    }
}

/// Called from the mouse hook. Cuts off the previous tap, like Mac's .interrupts.
pub fn play() {
    if !ENABLED.load(Relaxed) {
        return;
    }
    let (device, Some(bank)) = (handle(), BANK.get()) else { return };
    if device.0.is_null() {
        return;
    }
    let Some(clip) = bank.0.get(CHOICE.load(Relaxed)).filter(|c| !c.data.is_empty()) else { return };
    unsafe {
        waveOutReset(device);
        waveOutWrite(device, clip.hdr.get(), size_of::<WAVEHDR>() as u32);
    }
}
