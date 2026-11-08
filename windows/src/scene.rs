use std::time::Instant;

use tiny_skia::*;

const FADE_DELAY: f32 = 3.0;
const FADE_FOR: f32 = 0.6;

struct Stroke {
    pts: Vec<(f32, f32)>,
    ended: Option<Instant>,
}

impl Stroke {
    fn alpha(&self) -> f32 {
        match self.ended {
            None => 1.0,
            Some(t) => 1.0 - ((t.elapsed().as_secs_f32() - FADE_DELAY) / FADE_FOR).clamp(0.0, 1.0),
        }
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
        if self.pts.len() > 1 {
            let l = self.pts[self.pts.len() - 1];
            pb.line_to(l.0, l.1);
        }
        pb.finish()
    }
}

pub struct Ink {
    strokes: Vec<Stroke>,
    drawing: bool,
    pub color: Color,
    pub width: f32,
}

impl Ink {
    pub fn new() -> Self {
        Self { strokes: vec![], drawing: false, color: Color::from_rgba8(255, 41, 31, 255), width: 4.0 }
    }

    pub fn is_drawing(&self) -> bool {
        self.drawing
    }

    pub fn alive(&self) -> bool {
        !self.strokes.is_empty()
    }

    pub fn begin(&mut self, p: (f32, f32)) {
        self.strokes.push(Stroke { pts: vec![p], ended: None });
        self.drawing = true;
    }

    pub fn extend(&mut self, p: (f32, f32)) {
        let Some(s) = self.strokes.last_mut().filter(|_| self.drawing) else { return };
        let l = s.pts[s.pts.len() - 1];
        if (p.0 - l.0).hypot(p.1 - l.1) >= 1.5 {
            s.pts.push(p);
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
    }

    pub fn draw(&mut self, pix: &mut Pixmap) {
        self.strokes.retain(|s| s.alpha() > 0.0);
        let stroke = tiny_skia::Stroke { width: self.width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Default::default() };
        for s in &self.strokes {
            let Some(path) = s.path() else { continue };
            let mut paint = Paint::default();
            let mut c = self.color;
            c.set_alpha(s.alpha());
            paint.set_color(c);
            paint.anti_alias = true;
            pix.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }
}

pub fn draw_laser(pix: &mut Pixmap, at: (f32, f32), r: f32) {
    let red = Color::from_rgba8(255, 41, 31, 255);
    let glow = RadialGradient::new(
        Point::from_xy(at.0, at.1),
        0.0,
        Point::from_xy(at.0, at.1),
        r * 3.2,
        vec![
            GradientStop::new(0.0, Color::from_rgba8(255, 41, 31, 150)),
            GradientStop::new(1.0, Color::from_rgba8(255, 41, 31, 0)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    );
    let mut paint = Paint::default();
    paint.anti_alias = true;
    if let Some(shader) = glow {
        paint.shader = shader;
        if let Some(rect) = Rect::from_xywh(at.0 - r * 3.2, at.1 - r * 3.2, r * 6.4, r * 6.4) {
            pix.fill_rect(rect, &paint, Transform::identity(), None);
        }
    }
    let mut dot = |radius: f32, color: Color| {
        let mut p = Paint::default();
        p.set_color(color);
        p.anti_alias = true;
        if let Some(path) = PathBuilder::from_circle(at.0, at.1, radius) {
            pix.fill_path(&path, &p, FillRule::Winding, Transform::identity(), None);
        }
    };
    dot(r, red);
    dot(r * 0.45, Color::from_rgba8(255, 220, 209, 242));
}
