use std::path::PathBuf;

use tiny_skia::*;

/// Where the art and sounds live. TH_ASSETS wins, then next to the exe, then the repo.
pub fn find_asset(rel: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let mut roots = vec![];
    if let Ok(p) = std::env::var("TH_ASSETS") {
        roots.push(PathBuf::from(p));
    }
    roots.push(dir.join("assets"));
    roots.push(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets")));
    roots.into_iter().map(|r| r.join(rel)).find(|p| p.exists())
}

fn find(name: &str) -> Option<PathBuf> {
    find_asset(&format!("{name}.png"))
}

/// Buffers are BGRA, so swap red and blue once at load.
fn swap_rb(p: &mut Pixmap) {
    for px in p.data_mut().chunks_exact_mut(4) {
        px.swap(0, 2);
    }
}

pub struct HandAsset {
    full: Pixmap,
    pub display: Pixmap,
    pub aspect: f32,
    pub hand_top: f32,
    pub tip: f32,
    pub wrist: f32,
    pub rod_end: f32,
    pub tip_x: f32,
    pub rod_x: f32,
    min_x: f32,
    max_x: f32,
    /// Columns that actually hold the stick, so thin parts are not painted full width.
    pub rod_min_x: f32,
    pub rod_max_x: f32,
}

impl HandAsset {
    pub fn hand_columns(&self) -> (f32, f32) {
        (self.min_x, self.max_x)
    }

    /// Square crop of just the hand, for toolbar buttons.
    pub fn thumbnail(&self, px: u32) -> Pixmap {
        let (w, h) = (self.full.width() as f32, self.full.height() as f32);
        let side = ((self.max_x - self.min_x) * w).max((self.wrist - self.hand_top) * h) * 1.06;
        let (cx, cy) = ((self.min_x + self.max_x) / 2.0 * w, (self.hand_top + self.wrist) / 2.0 * h);
        let scale = px as f32 / side;
        let small = resized(&self.full, ((h * scale).round() as u32).max(1));
        let k = small.height() as f32 / h;
        let mut out = Pixmap::new(px, px).expect("thumbnail");
        let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..Default::default() };
        let ts = Transform::from_translate(px as f32 / 2.0 - cx * k, px as f32 / 2.0 - cy * k);
        out.draw_pixmap(0, 0, small.as_ref(), &paint, ts, None);
        out
    }

    pub fn load(id: &str) -> Option<Self> {
        let mut full = Pixmap::load_png(find(&format!("hand_{id}"))?).ok()?;
        swap_rb(&mut full);
        let m = measure(&full)?;
        Some(Self { display: full.clone(), full, ..m })
    }

    /// Keeps the texture near its on-screen size so minification stays crisp.
    pub fn fit(&mut self, rest_px: f32) {
        let step = 64u32;
        let needed = (rest_px.ceil() as u32).div_ceil(step) * step;
        let full_h = self.full.height();
        let target = if needed * 10 >= full_h * 9 { full_h } else { needed.max(step) };
        if target != self.display.height() {
            self.display = resized(&self.full, target);
        }
    }

    fn blank(full: Pixmap) -> Self {
        Self { display: full.clone(), full, aspect: 0.0, hand_top: 0.0, tip: 0.0, wrist: 0.0, rod_end: 0.0, tip_x: 0.5, rod_x: 0.5, min_x: 0.0, max_x: 1.0, rod_min_x: 0.0, rod_max_x: 1.0 }
    }
}

fn measure(p: &Pixmap) -> Option<HandAsset> {
    let (w, h) = (p.width() as usize, p.height() as usize);
    let data = p.data();
    let solid = |x: usize, y: usize| data[(y * w + x) * 4 + 3] > 128;
    let counts: Vec<usize> = (0..h).map(|y| (0..w).filter(|&x| solid(x, y)).count()).collect();
    let center = |y: usize| {
        let xs: Vec<usize> = (0..w).filter(|&x| solid(x, y)).collect();
        match (xs.first(), xs.last()) {
            (Some(&lo), Some(&hi)) => (lo + hi) as f32 / 2.0 / w as f32,
            _ => 0.5,
        }
    };

    let top = counts.iter().position(|&c| c > 0)?;
    let search_end = h.min(top + (0.45 * h as f64) as usize);
    let max_w = *counts[top..search_end].iter().max()?;
    if max_w == 0 {
        return None;
    }
    let fist = (top..h).find(|&y| counts[y] as f64 > 0.6 * max_w as f64)?;
    let wrist = (fist..h).find(|&y| (counts[y] as f64) < 0.35 * max_w as f64)?;
    let rod_w = counts[(h - 1).min(wrist + h / 50)];
    let last_row = counts.iter().rposition(|&c| c > 0).unwrap_or(h - 1);
    let from = (h - 1).min(wrist + h / 33);
    let handle = (from..h).find(|&y| counts[y] as f64 > 1.8 * rod_w as f64);
    let rod_end = handle.unwrap_or(last_row);

    let fh = h as f32;
    let mut asset = HandAsset::blank(p.clone());
    asset.aspect = w as f32 / fh;
    asset.hand_top = ((top as f32 - 0.004 * fh).max(0.0)) / fh;
    asset.tip = top as f32 / fh;
    asset.wrist = wrist as f32 / fh;
    asset.rod_end = rod_end as f32 / fh;
    asset.tip_x = center((h - 1).min(top + h / 200));
    asset.rod_x = center((wrist + rod_end) / 2);
    let xs = || (top..wrist).flat_map(|y| (0..w).filter(move |&x| solid(x, y)));
    asset.min_x = xs().min().unwrap_or(0) as f32 / w as f32;
    asset.max_x = xs().max().unwrap_or(w - 1) as f32 / w as f32;
    let rod_rows = || (wrist..=rod_end.min(h - 1)).flat_map(|y| (0..w).filter(move |&x| solid(x, y)));
    asset.rod_min_x = rod_rows().min().unwrap_or(0) as f32 / w as f32;
    asset.rod_max_x = (rod_rows().max().unwrap_or(w - 1) + 1) as f32 / w as f32;
    Some(asset)
}

fn halve(src: &Pixmap) -> Pixmap {
    let (w, h) = (src.width() as usize / 2, src.height() as usize / 2);
    let mut out = Pixmap::new(w as u32, h as u32).expect("halved pixmap");
    let sw = src.width() as usize;
    let (s, d) = (src.data(), out.data_mut());
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let i = |dx: usize, dy: usize| s[((2 * y + dy) * sw + 2 * x + dx) * 4 + c] as u32;
                d[(y * w + x) * 4 + c] = ((i(0, 0) + i(1, 0) + i(0, 1) + i(1, 1) + 2) / 4) as u8;
            }
        }
    }
    out
}

pub fn resized(full: &Pixmap, height: u32) -> Pixmap {
    let mut cur = full.clone();
    while cur.height() / 2 >= height && cur.width() >= 2 {
        cur = halve(&cur);
    }
    let width = ((cur.width() as f32 * height as f32 / cur.height() as f32).round() as u32).max(1);
    let mut out = Pixmap::new(width, height).expect("resized pixmap");
    let ts = Transform::from_scale(width as f32 / cur.width() as f32, height as f32 / cur.height() as f32);
    let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..Default::default() };
    out.draw_pixmap(0, 0, cur.as_ref(), &paint, ts, None);
    out
}
