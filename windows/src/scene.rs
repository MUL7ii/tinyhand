use std::time::{Duration, Instant};

use tiny_skia::*;

const FADE_FOR: f32 = 0.6;

/// Every buffer here is BGRA, so colours swap red and blue.
pub fn px(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(b, g, r, a)
}

#[derive(Clone, Copy)]
pub struct Rc {
    pub l: i32,
    pub t: i32,
    pub r: i32,
    pub b: i32,
}

impl Rc {
    pub fn around(x: f32, y: f32, rad: f32) -> Self {
        Self { l: (x - rad).floor() as i32, t: (y - rad).floor() as i32, r: (x + rad).ceil() as i32, b: (y + rad).ceil() as i32 }
    }

    pub fn union(self, o: Rc) -> Rc {
        Rc { l: self.l.min(o.l), t: self.t.min(o.t), r: self.r.max(o.r), b: self.b.max(o.b) }
    }

    pub fn clip(self, w: i32, h: i32) -> Option<Rc> {
        let c = Rc { l: self.l.max(0), t: self.t.max(0), r: self.r.min(w), b: self.b.min(h) };
        (c.l < c.r && c.t < c.b).then_some(c)
    }
}

pub fn union_opt(a: Option<Rc>, b: Rc) -> Rc {
    a.map_or(b, |a| a.union(b))
}

struct Stroke {
    pts: Vec<(f32, f32)>,
    ended: Option<Instant>,
}

impl Stroke {
    fn age(&self) -> Option<f32> {
        self.ended.map(|t| t.elapsed().as_secs_f32())
    }

    fn alpha(&self) -> f32 {
        match (self.age(), crate::config::fade_delay()) {
            (Some(a), Some(delay)) => 1.0 - ((a - delay) / FADE_FOR).clamp(0.0, 1.0),
            _ => 1.0,
        }
    }

    fn fading(&self) -> bool {
        matches!((self.age(), crate::config::fade_delay()), (Some(a), Some(delay)) if a > delay)
    }

    fn path(&self) -> Option<Path> {
        let mut pb = PathBuilder::new();
        let first = self.pts.first()?;
        pb.move_to(first.0, first.1);
        if self.pts.len() == 1 {
            pb.line_to(first.0, first.1);
        }
        for w in self.pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            pb.quad_to(a.0, a.1, (a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        }
        if let (true, Some(l)) = (self.pts.len() > 1, self.pts.last()) {
            pb.line_to(l.0, l.1);
        }
        pb.finish()
    }
}

pub enum Pending {
    Fading,
    After(Duration),
    Nothing,
}

/// Strokes are painted once into a persistent layer. Only fading rebuilds it.
pub struct Ink {
    strokes: Vec<Stroke>,
    pub layer: Pixmap,
    drawing: bool,
    color: Color,
    width: f32,
    dirty: Option<Rc>,
}

impl Ink {
    pub fn new(w: i32, h: i32) -> Self {
        Self {
            strokes: vec![],
            layer: Pixmap::new(w as u32, h as u32).expect("ink layer"),
            drawing: false,
            color: px(255, 41, 31, 255),
            width: (h as f32 * 0.0055).max(3.0),
            dirty: None,
        }
    }

    /// Starts over at a new screen size. Strokes do not carry between screens.
    pub fn resize(&mut self, w: i32, h: i32) {
        self.layer = Pixmap::new(w as u32, h as u32).expect("ink layer");
        self.strokes.clear();
        self.drawing = false;
        self.width = (h as f32 * 0.0055).max(3.0);
        self.dirty = None;
    }

    pub fn set_color(&mut self, rgb: u32) {
        self.color = px((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255);
    }

    pub fn is_drawing(&self) -> bool {
        self.drawing
    }

    pub fn take_dirty(&mut self) -> Option<Rc> {
        self.dirty.take()
    }

    fn paint(&mut self, path: &Path, alpha: f32) {
        let mut c = self.color;
        c.set_alpha(alpha);
        let mut paint = Paint::default();
        paint.set_color(c);
        paint.anti_alias = true;
        let stroke = tiny_skia::Stroke { width: self.width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
        self.layer.stroke_path(path, &paint, &stroke, Transform::identity(), None);
        let b = path.bounds();
        let pad = self.width / 2.0 + 2.0;
        let r = Rc { l: (b.left() - pad).floor() as i32, t: (b.top() - pad).floor() as i32, r: (b.right() + pad).ceil() as i32, b: (b.bottom() + pad).ceil() as i32 };
        self.dirty = Some(union_opt(self.dirty, r));
    }

    pub fn begin(&mut self, p: (f32, f32)) {
        self.strokes.push(Stroke { pts: vec![p], ended: None });
        self.drawing = true;
        let mut pb = PathBuilder::new();
        pb.move_to(p.0, p.1);
        pb.line_to(p.0, p.1);
        if let Some(path) = pb.finish() {
            self.paint(&path, 1.0);
        }
    }

    pub fn extend(&mut self, p: (f32, f32)) {
        let Some(s) = self.strokes.last_mut().filter(|_| self.drawing) else { return };
        let l = s.pts[s.pts.len() - 1];
        if (p.0 - l.0).hypot(p.1 - l.1) < 1.5 {
            return;
        }
        s.pts.push(p);
        let n = s.pts.len();
        let mid = |a: (f32, f32), b: (f32, f32)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        let mut pb = PathBuilder::new();
        if n == 2 {
            pb.move_to(s.pts[0].0, s.pts[0].1);
        } else {
            let m = mid(s.pts[n - 3], s.pts[n - 2]);
            let e = mid(s.pts[n - 2], s.pts[n - 1]);
            pb.move_to(m.0, m.1);
            pb.quad_to(s.pts[n - 2].0, s.pts[n - 2].1, e.0, e.1);
        }
        pb.line_to(p.0, p.1);
        if let Some(path) = pb.finish() {
            self.paint(&path, 1.0);
        }
    }

    pub fn end(&mut self) {
        if let Some(s) = self.strokes.last_mut().filter(|_| self.drawing) {
            s.ended = Some(Instant::now());
        }
        self.drawing = false;
    }

    pub fn clear(&mut self) {
        self.strokes.clear();
        self.drawing = false;
        self.layer.fill(Color::TRANSPARENT);
        self.dirty = Some(self.full());
    }

    fn full(&self) -> Rc {
        Rc { l: 0, t: 0, r: self.layer.width() as i32, b: self.layer.height() as i32 }
    }

    /// Returns true while strokes are fading.
    pub fn tick(&mut self) -> bool {
        let before = self.strokes.len();
        self.strokes.retain(|s| s.alpha() > 0.0);
        let fading = self.strokes.iter().any(Stroke::fading);
        if fading || self.strokes.len() != before {
            self.layer.fill(Color::TRANSPARENT);
            let paths: Vec<_> = self.strokes.iter().filter_map(|s| s.path().map(|p| (p, s.alpha()))).collect();
            for (path, alpha) in paths {
                self.paint(&path, alpha);
            }
            self.dirty = Some(self.full());
        }
        fading
    }

    pub fn pending(&self) -> Pending {
        let Some(delay) = crate::config::fade_delay() else { return Pending::Nothing };
        if self.strokes.iter().any(Stroke::fading) {
            return Pending::Fading;
        }
        self.strokes
            .iter()
            .filter_map(Stroke::age)
            .map(|a| Duration::from_secs_f32((delay - a).max(0.0)))
            .min()
            .map_or(Pending::Nothing, Pending::After)
    }
}

pub fn draw_laser(pix: &mut PixmapMut, at: (f32, f32), r: f32) {
    let glow = RadialGradient::new(
        Point::from_xy(at.0, at.1),
        0.0,
        Point::from_xy(at.0, at.1),
        r * 3.2,
        vec![GradientStop::new(0.0, px(255, 41, 31, 150)), GradientStop::new(1.0, px(255, 41, 31, 0))],
        SpreadMode::Pad,
        Transform::identity(),
    );
    if let (Some(shader), Some(rect)) = (glow, Rect::from_xywh(at.0 - r * 3.2, at.1 - r * 3.2, r * 6.4, r * 6.4)) {
        let mut paint = Paint::default();
        paint.shader = shader;
        pix.fill_rect(rect, &paint, Transform::identity(), None);
    }
    for (radius, color) in [(r, px(255, 41, 31, 255)), (r * 0.45, px(255, 220, 209, 242))] {
        let mut p = Paint::default();
        p.set_color(color);
        p.anti_alias = true;
        if let Some(path) = PathBuilder::from_circle(at.0, at.1, radius) {
            pix.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
        }
    }
}
