use std::path::PathBuf;

fn file() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("TinyHand").join("settings.txt"))
}

fn read() -> Vec<(String, String)> {
    let text = file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().into(), v.trim().into())).collect()
}

fn get(key: &str) -> Option<String> {
    read().into_iter().find(|(k, _)| k == key).map(|(_, v)| v).filter(|v| !v.is_empty())
}

fn set(key: &str, value: &str) {
    let Some(path) = file() else { return };
    let mut all = read();
    all.retain(|(k, _)| k != key);
    all.push((key.into(), value.into()));
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let text: String = all.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
    let _ = std::fs::write(path, text);
}

pub fn load_style() -> Option<String> {
    get("style")
}

pub fn save_style(id: &str) {
    set("style", id);
}

pub fn load_click_through() -> bool {
    get("click_through").is_some_and(|v| v == "1")
}

pub fn save_click_through(on: bool) {
    set("click_through", if on { "1" } else { "0" });
}
