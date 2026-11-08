use std::collections::HashMap;
use std::sync::Mutex;

use crate::hand::HandAsset;
use crate::icon::base64;

const SIZE: u32 = 96;

/// Settings thumbnails as PNG data URLs. Built once in the background after launch,
/// so opening the Pointers page is instant and nothing loads while the pointer is in use.
static CACHE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn png(id: &str) -> Option<String> {
    let mut thumb = HandAsset::load(id)?.thumbnail(SIZE);
    // Buffers are BGRA here, PNG wants RGBA.
    for px in thumb.data_mut().chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    Some(format!("data:image/png;base64,{}", base64(&thumb.encode_png().ok()?)))
}

pub fn build_in_background() {
    let _ = std::thread::Builder::new().name("thumbs".into()).spawn(|| {
        let mut all = HashMap::new();
        for (id, _) in crate::styles::all().filter(|(id, _)| *id != crate::styles::LASER) {
            if let Some(url) = png(id) {
                all.insert(id.to_string(), url);
            }
        }
        *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some(all);
    });
}

/// Every thumbnail built so far, as id to data URL.
pub fn all() -> HashMap<String, String> {
    CACHE.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_default()
}
