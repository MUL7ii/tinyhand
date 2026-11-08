use tiny_skia::*;

use crate::hand::HandAsset;
use crate::scene::Rc;

const MIN_ROD: f32 = 0.05;
const GRIP_LEAN: f32 = 0.30;
const GRIP_DEPTH: f32 = 0.12;
const GRIP_FOLLOW: f32 = 0.08;
const LIFT: f32 = 0.06;
/// The shadow is a soft blur, so it is drawn and blurred at a quarter of the size.
const SHADOW_SCALE: f32 = 0.25;
/// The follow and press rates were tuned at 120 ticks per second.
const TICK_HZ: f32 = 120.0;

/// Geometry is in y-up screen space, like the Mac layers. Draw flips it.
#[derive(Clone, Copy)]
pub struct Pose {
    rotation: f32,
    rod_len: f32,
    hand_pts: f32,
    width: f32,
    grip: (f32, f32),
    lift: f32,
    blur: f32,
    opacity: f32,
}

impl Pose {
    pub fn key(&self) -> Vec<i32> {
        [self.rotation, self.rod_len, self.hand_pts, self.width, self.grip.0, self.lift, self.blur]
            .iter()
            .map(|v| (v * 100.0).round() as i32)
            .collect()
    }
}

pub struct Arm {
    pub asset: HandAsset,
    grip_x: Option<f32>,
    grip_target: f32,
}

impl Arm {
    pub fn new(asset: HandAsset) -> Self {
        Self { asset, grip_x: None, grip_target: 0.0 }
    }

    pub fn settling(&self) -> bool {
        self.grip_x.is_some_and(|g| (self.grip_target - g).abs() > 0.5)
    }

    /// cursor is in screen pixels, y down.
    pub fn aim(&mut self, cursor: (f32, f32), screen: (f32, f32), press: f32, size: f32, dt: f32) -> Pose {
        let (sw, sh) = screen;
        let a = &mut self.asset;
        let (cx, cy) = (cursor.0, sh - cursor.1);

        let target = (cx + GRIP_LEAN * sw).clamp(sw * 0.15, sw * 1.05);
        let follow = 1.0 - (1.0 - GRIP_FOLLOW).powf(dt * TICK_HZ);
        let gx = self.grip_x.map_or(target, |g| g + (target - g) * follow);
        self.grip_x = Some(gx);
        self.grip_target = target;
        let grip = (gx, -GRIP_DEPTH * sh);

        a.fit(size * sh / (a.wrist - a.hand_top));

        let (dx, dy) = (cx - grip.0, cy - grip.1);
        let reach = dx.hypot(dy);
        let aim_angle = dy.atan2(dx);

        let hand_pts = size * sh * (1.0 - 0.06 * press);
        let s = hand_pts / (a.wrist - a.hand_top);
        let width = s * a.aspect;
        let tip_pts = (a.wrist - a.tip) * s;
        let tip_off_x = (a.tip_x - a.rod_x) * width;

        let along = (reach * reach - tip_off_x * tip_off_x).max(0.0).sqrt();
        let rod_len = (along - tip_pts).max(MIN_ROD * sh);
        let tip_angle = (rod_len + tip_pts).atan2(tip_off_x);

        let lift = LIFT * sh * (1.0 - 0.8 * press);
        Pose {
            rotation: aim_angle - tip_angle,
            rod_len,
            hand_pts,
            width,
            grip,
            lift,
            blur: (lift * 0.35).max(1.0) + 5.0 * press,
            opacity: 0.26 + 0.02 * press,
        }
    }

    fn corners(&self, p: &Pose, sh: f32) -> [(f32, f32); 4] {
        let anchor = self.asset.rod_x * p.width;
        let (sin, cos) = p.rotation.sin_cos();
        let top = p.rod_len + p.hand_pts;
        [(0.0, 0.0), (p.width, 0.0), (p.width, top), (0.0, top)].map(|(x, y)| {
            let (lx, ly) = (x - anchor, y);
            (p.grip.0 + cos * lx - sin * ly, sh - (p.grip.1 + sin * lx + cos * ly))
        })
    }

    pub fn bbox(&self, p: &Pose, sh: f32) -> Rc {
        let c = self.corners(p, sh);
        let pad = 6.0 + p.lift * 1.1 + p.blur * 2.0;
        let min = |f: fn(&(f32, f32)) -> f32| c.iter().map(f).fold(f32::MAX, f32::min);
        let max = |f: fn(&(f32, f32)) -> f32| c.iter().map(f).fold(f32::MIN, f32::max);
        Rc { l: (min(|c| c.0) - pad).floor() as i32, t: (min(|c| c.1) - pad).floor() as i32, r: (max(|c| c.0) + pad).ceil() as i32, b: (max(|c| c.1) + pad).ceil() as i32 }
    }

    pub fn draw(&self, pix: &mut PixmapMut, p: &Pose, sh: f32, shadow: bool) {
        let (sin, cos) = p.rotation.sin_cos();
        let base = Transform::from_row(1.0, 0.0, 0.0, -1.0, 0.0, sh)
            .pre_concat(Transform::from_translate(p.grip.0, p.grip.1))
            .pre_concat(Transform::from_row(cos, sin, -sin, cos, 0.0, 0.0))
            .pre_concat(Transform::from_translate(-self.asset.rod_x * p.width, 0.0));
        if shadow {
            self.draw_shadow(pix, p, base);
        }
        self.parts(pix, base, p);
    }

    /// base maps group-local space (origin at the rod's bottom-left, y up) to the target.
    fn parts(&self, pix: &mut PixmapMut, base: Transform, p: &Pose) {
        self.part(pix, base, p, Part::Rod);
        self.part(pix, base, p, Part::Hand);
    }

    fn part(&self, pix: &mut PixmapMut, base: Transform, p: &Pose, part: Part) {
        let a = &self.asset;
        let img = &a.display;
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        let mut paint = Paint::default();
        paint.shader = Pattern::new(img.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, 1.0, Transform::identity());
        paint.anti_alias = true;
        let sx = p.width / iw;
        let (v_top, v_wrist, v_end) = (a.hand_top * ih, a.wrist * ih, a.rod_end * ih);
        let columns = |(lo, hi): (f32, f32)| ((lo * iw - 2.0).max(0.0), (hi * iw + 2.0).min(iw));

        match part {
            Part::Rod => {
                let k = p.rod_len / (v_end - v_wrist).max(1.0);
                let ts = base.pre_concat(Transform::from_row(sx, 0.0, 0.0, -k, 0.0, p.rod_len + k * v_wrist));
                let (l, r) = columns((a.rod_min_x, a.rod_max_x));
                if let Some(rect) = Rect::from_ltrb(l, v_wrist - 1.0, r, v_end.max(v_wrist + 1.0)) {
                    pix.fill_rect(rect, &paint, ts, None);
                }
            }
            Part::Hand => {
                let k = p.hand_pts / (v_wrist - v_top).max(1.0);
                let ts = base.pre_concat(Transform::from_row(sx, 0.0, 0.0, -k, 0.0, p.rod_len + p.hand_pts + k * v_top));
                let (l, r) = columns(a.hand_columns());
                if let Some(rect) = Rect::from_ltrb(l, v_top, r, v_wrist + 1.0) {
                    pix.fill_rect(rect, &paint, ts, None);
                }
            }
        }
    }

    /// One shadow per piece, like the Mac layers, each in a buffer cropped to just that piece.
    fn draw_shadow(&self, pix: &mut PixmapMut, p: &Pose, base: Transform) {
        let a = &self.asset;
        let (hand_l, hand_r) = a.hand_columns();
        for (part, cols, rows) in [
            (Part::Rod, (a.rod_min_x, a.rod_max_x), (0.0, p.rod_len)),
            (Part::Hand, (hand_l, hand_r), (p.rod_len, p.rod_len + p.hand_pts)),
        ] {
            self.shadow_piece(pix, p, base, part, (cols.0 * p.width, cols.1 * p.width), rows);
        }
    }

    /// Drawn and blurred at reduced size in unrotated local space, then rotated into place.
    fn shadow_piece(&self, pix: &mut PixmapMut, p: &Pose, base: Transform, part: Part, x: (f32, f32), y: (f32, f32)) {
        let pad = (p.blur * 2.0).ceil() + 2.0;
        let (x0, x1, y0, y1) = (x.0 - pad, x.1 + pad, y.0 - pad, y.1 + pad);
        let k = SHADOW_SCALE;
        let (w, h) = ((((x1 - x0) * k).ceil() as u32).max(1), (((y1 - y0) * k).ceil() as u32).max(1));
        let Some(mut sil) = Pixmap::new(w, h) else { return };
        let to_buf = Transform::from_row(k, 0.0, 0.0, -k, -x0 * k, y1 * k);
        self.part(&mut sil.as_mut(), to_buf, p, part);

        let mut alpha: Vec<u8> = sil.data().chunks_exact(4).map(|px| px[3]).collect();
        let radius = (p.blur / 2.0 * k).round().max(1.0) as usize;
        box_blur(&mut alpha, w as usize, h as usize, radius);
        for (px, a) in sil.data_mut().chunks_exact_mut(4).zip(&alpha) {
            px.copy_from_slice(&[0, 0, 0, (*a as f32 * p.opacity) as u8]);
        }

        let offset = Transform::from_translate(p.lift * 0.6, p.lift);
        let from_buf = Transform::from_row(1.0 / k, 0.0, 0.0, -1.0 / k, x0, y1);
        let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..Default::default() };
        pix.draw_pixmap(0, 0, sil.as_ref(), &paint, offset.pre_concat(base).pre_concat(from_buf), None);
    }
}

#[derive(Clone, Copy)]
enum Part {
    Rod,
    Hand,
}

fn pass(src: &[u8], dst: &mut [u8], lines: usize, len: usize, line_step: usize, step: usize, r: usize) {
    let win = (2 * r + 1) as u32;
    for l in 0..lines {
        let b = l * line_step;
        let mut sum: u32 = (0..=r.min(len - 1)).map(|i| src[b + i * step] as u32).sum();
        for i in 0..len {
            dst[b + i * step] = (sum / win) as u8;
            if i + r + 1 < len {
                sum += src[b + (i + r + 1) * step] as u32;
            }
            if i >= r {
                sum -= src[b + (i - r) * step] as u32;
            }
        }
    }
}

/// Three box passes approximate a Gaussian.
fn box_blur(a: &mut [u8], w: usize, h: usize, r: usize) {
    let mut tmp = vec![0u8; a.len()];
    for _ in 0..3 {
        pass(a, &mut tmp, h, w, w, 1, r);
        pass(&tmp, a, w, h, 1, w, r);
    }
}
