//! Builds the app icon from the shared logo, so the exe carries the official mark.

use std::path::PathBuf;

use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

const LOGO: &str = "../.github/logo.png";
const SIZES: [u32; 7] = [16, 24, 32, 48, 64, 128, 256];

fn scaled(src: &Pixmap, size: u32) -> Pixmap {
    // Halve until close, then one bilinear step, so small sizes stay crisp.
    let mut cur = src.clone();
    while cur.width() / 2 >= size {
        let (w, h) = (cur.width() / 2, cur.height() / 2);
        let mut next = Pixmap::new(w, h).unwrap();
        next.draw_pixmap(0, 0, cur.as_ref(), &PixmapPaint { quality: FilterQuality::Bilinear, ..Default::default() }, Transform::from_scale(0.5, 0.5), None);
        cur = next;
    }
    let mut out = Pixmap::new(size, size).unwrap();
    let k = size as f32 / cur.width() as f32;
    out.draw_pixmap(0, 0, cur.as_ref(), &PixmapPaint { quality: FilterQuality::Bilinear, ..Default::default() }, Transform::from_scale(k, k), None);
    out
}

fn main() {
    println!("cargo:rerun-if-changed={LOGO}");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let logo = Pixmap::load_png(LOGO).expect("logo.png");
    let images: Vec<(u32, Vec<u8>)> = SIZES.iter().map(|&s| (s, scaled(&logo, s).encode_png().expect("png"))).collect();

    // An .ico is a small directory followed by the images. PNG payloads are allowed.
    let mut ico = vec![0, 0, 1, 0];
    ico.extend((images.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * images.len() as u32;
    for (size, png) in &images {
        let dim = if *size >= 256 { 0 } else { *size as u8 };
        ico.extend([dim, dim, 0, 0]);
        ico.extend(1u16.to_le_bytes());
        ico.extend(32u16.to_le_bytes());
        ico.extend((png.len() as u32).to_le_bytes());
        ico.extend(offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &images {
        ico.extend(png);
    }
    let path = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("tinyhand.ico");
    std::fs::write(&path, ico).unwrap();

    let mut res = winresource::WindowsResource::new();
    res.set_icon(path.to_str().unwrap());
    res.set("ProductName", "TinyHand for Windows");
    res.set("FileDescription", "TinyHand, a tiny hand pointer");
    res.set("LegalCopyright", "MIT License. Based on TinyHand by Kamran Ahmed.");
    if let Err(e) = res.compile() {
        println!("cargo:warning=could not embed the app icon: {e}");
    }
}
