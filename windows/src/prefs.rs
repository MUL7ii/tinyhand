use std::path::PathBuf;

fn file() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("TinyHand").join("style.txt"))
}

pub fn load_style() -> Option<String> {
    let text = std::fs::read_to_string(file()?).ok()?;
    Some(text.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn save_style(id: &str) {
    let Some(path) = file() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, id);
}
