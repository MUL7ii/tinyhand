use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering::Relaxed};

use serde_json::{Value, json};

use crate::prefs;

pub const SIZE_RANGE: (f32, f32) = (0.08, 0.20);
const DEFAULT_SIZE: f32 = 0.13;
const DEFAULT_FADE: f32 = 3.0;

pub const INKS: [&str; 9] = ["match", "red", "orange", "yellow", "green", "blue", "purple", "white", "black"];
const INK_RGB: [u32; 9] = [0, 0xFF291F, 0xFF8A00, 0xFFCC00, 0x30C25A, 0x0A84FF, 0x8754F2, 0xFFFFFF, 0x1D1D21];

pub const INTERACT_KEYS: [&str; 4] = ["alt", "ctrl", "shift", "off"];

static SIZE: AtomicU32 = AtomicU32::new(DEFAULT_SIZE.to_bits());
static SHADOW: AtomicBool = AtomicBool::new(true);
static FADE: AtomicU32 = AtomicU32::new(DEFAULT_FADE.to_bits());
static INK: AtomicU8 = AtomicU8::new(0);
static INK_BUTTON: AtomicBool = AtomicBool::new(true);
static INTERACT: AtomicU8 = AtomicU8::new(0);

pub fn pointer_size() -> f32 {
    f32::from_bits(SIZE.load(Relaxed))
}

pub fn shadow() -> bool {
    SHADOW.load(Relaxed)
}

/// Seconds before a stroke starts fading, or None to keep drawings until cleared.
pub fn fade_delay() -> Option<f32> {
    let seconds = f32::from_bits(FADE.load(Relaxed));
    (seconds > 0.0).then_some(seconds.max(0.5))
}

pub fn show_ink_button() -> bool {
    INK_BUTTON.load(Relaxed)
}

/// A fixed ink color the user picked, or None to match the pointer.
pub fn ink_override() -> Option<u32> {
    match INK.load(Relaxed) as usize {
        0 => None,
        i => INK_RGB.get(i).copied(),
    }
}

/// Virtual keys that count as the interact key, and the one to hand back to apps.
pub fn interact_keys() -> (&'static [u32], u32) {
    match INTERACT.load(Relaxed) {
        0 => (&[0xA4, 0xA5, 0x12], 0x12),
        1 => (&[0xA2, 0xA3, 0x11], 0x11),
        2 => (&[0xA0, 0xA1, 0x10], 0x10),
        _ => (&[], 0),
    }
}

pub const MAX_PINNED: usize = 8;
const DEFAULT_PINNED: [&str; 6] = ["lavender", "glove", "coral", "plush", "robot", "laser"];

/// The pointers shown on the toolbar, in order. Never empty, never unknown, never repeated.
pub fn pinned() -> Vec<String> {
    let saved = prefs::get_str("pinned").unwrap_or_default();
    let mut list: Vec<String> = Vec::new();
    for id in saved.split(',').map(str::trim) {
        if crate::styles::is_known(id) && !list.iter().any(|x| x == id) && list.len() < MAX_PINNED {
            list.push(id.to_string());
        }
    }
    if list.is_empty() {
        list = DEFAULT_PINNED.iter().map(|s| s.to_string()).collect();
    }
    list
}

pub fn set_pinned(list: &[String]) {
    prefs::set_str("pinned", &list.join(","));
}

pub fn load() {
    let size = prefs::get_f32("pointer_size").unwrap_or(DEFAULT_SIZE).clamp(SIZE_RANGE.0, SIZE_RANGE.1);
    SIZE.store(size.to_bits(), Relaxed);
    SHADOW.store(prefs::flag("shadow", true), Relaxed);
    FADE.store(prefs::get_f32("fade_seconds").unwrap_or(DEFAULT_FADE).max(0.0).to_bits(), Relaxed);
    INK.store(index_of(&INKS, prefs::get_str("ink_choice").as_deref()), Relaxed);
    INK_BUTTON.store(prefs::flag("show_ink_button", true), Relaxed);
    INTERACT.store(index_of(&INTERACT_KEYS, prefs::get_str("interact_key").as_deref()), Relaxed);
}

fn index_of(list: &[&str], name: Option<&str>) -> u8 {
    name.and_then(|n| list.iter().position(|x| *x == n)).unwrap_or(0) as u8
}

/// Applies one change from the settings window and saves it. Returns false for unknown keys.
pub fn set(key: &str, value: &Value) -> bool {
    match key {
        "pointer_size" => {
            let v = value.as_f64().unwrap_or(DEFAULT_SIZE as f64) as f32;
            let v = v.clamp(SIZE_RANGE.0, SIZE_RANGE.1);
            SIZE.store(v.to_bits(), Relaxed);
            prefs::set_f32("pointer_size", v);
        }
        "shadow" => {
            let on = value.as_bool().unwrap_or(true);
            SHADOW.store(on, Relaxed);
            prefs::set_flag("shadow", on);
        }
        "fade_seconds" => {
            let v = (value.as_f64().unwrap_or(DEFAULT_FADE as f64) as f32).max(0.0);
            FADE.store(v.to_bits(), Relaxed);
            prefs::set_f32("fade_seconds", v);
        }
        "ink_choice" => {
            let name = value.as_str().unwrap_or("match");
            INK.store(index_of(&INKS, Some(name)), Relaxed);
            prefs::set_str("ink_choice", INKS[INK.load(Relaxed) as usize]);
        }
        "show_ink_button" => {
            let on = value.as_bool().unwrap_or(true);
            INK_BUTTON.store(on, Relaxed);
            prefs::set_flag("show_ink_button", on);
        }
        "interact_key" => {
            let name = value.as_str().unwrap_or("alt");
            INTERACT.store(index_of(&INTERACT_KEYS, Some(name)), Relaxed);
            prefs::set_str("interact_key", INTERACT_KEYS[INTERACT.load(Relaxed) as usize]);
        }
        _ => return false,
    }
    true
}

pub fn reset() {
    for key in ["pointer_size", "shadow", "fade_seconds", "ink_choice", "show_ink_button", "interact_key", "tap_sound", "sound_hands", "sound_laser", "pinned"] {
        prefs::remove(key);
    }
    load();
}

/// Everything the settings page shows.
pub fn snapshot() -> Value {
    json!({
        "pointer_size": pointer_size(),
        "size_min": SIZE_RANGE.0,
        "size_max": SIZE_RANGE.1,
        "shadow": shadow(),
        "fade_seconds": f32::from_bits(FADE.load(Relaxed)),
        "ink_choice": INKS[INK.load(Relaxed) as usize],
        "show_ink_button": show_ink_button(),
        "interact_key": INTERACT_KEYS[INTERACT.load(Relaxed) as usize],
        "tap_sound": prefs::tap_sound(),
        "sound_hands": prefs::flag("sound_hands", true),
        "sound_laser": prefs::flag("sound_laser", false),
        "click_through": prefs::flag("click_through", false),
        "version": env!("CARGO_PKG_VERSION"),
    })
}
