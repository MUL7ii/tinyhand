use std::path::PathBuf;
use std::sync::Mutex;

fn file() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("TinyHand").join("settings.txt"))
}

/// Settings live in memory and are written back whole on every change, so hot paths never touch disk.
static CACHE: Mutex<Option<Vec<(String, String)>>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut Vec<(String, String)>) -> T) -> T {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let all = guard.get_or_insert_with(|| {
        let text = file().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
        text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect()
    });
    f(all)
}

fn save(all: &[(String, String)]) {
    let Some(path) = file() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text: String = all.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    let _ = std::fs::write(path, text);
}

pub fn get_str(key: &str) -> Option<String> {
    with(|all| all.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())).filter(|v| !v.is_empty())
}

pub fn set_str(key: &str, value: &str) {
    with(|all| {
        all.retain(|(k, _)| k != key);
        all.push((key.into(), value.into()));
        save(all);
    });
}

pub fn remove(key: &str) {
    with(|all| {
        all.retain(|(k, _)| k != key);
        save(all);
    });
}

pub fn get_f32(key: &str) -> Option<f32> {
    get_str(key)?.parse().ok()
}

pub fn set_f32(key: &str, value: f32) {
    set_str(key, &value.to_string());
}

pub fn load_style() -> Option<String> {
    get_str("style")
}

pub fn save_style(id: &str) {
    set_str("style", id);
}

pub fn tap_sound() -> String {
    get_str("tap_sound").unwrap_or_else(|| "thock".into())
}

pub fn flag(key: &str, default: bool) -> bool {
    get_str(key).map_or(default, |v| v == "1")
}

pub fn set_flag(key: &str, on: bool) {
    set_str(key, if on { "1" } else { "0" });
}
