use tiny_skia::*;

use crate::hand::HandAsset;
use crate::scene::Rc;

const MIN_ROD: f32 = 0.05;
const GRIP_LEAN: f32 = 0.30;
const GRIP_DEPTH: f32 = 0.12;
const GRIP_FOLLOW: f32 = 0.08;
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
}

impl Pose {
    pub fn key(&self) -> Vec<i32> {
        [self.rotation, self.rod_len, self.hand_pts, self.width, self.grip.0].iter().map(|v| (v * 100.0).round() as i32).collect()
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
        Pose { rotation: aim_angle - tip_angle, rod_len, hand_pts, width, grip }
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
        let pad = 6.0;
        let min = |f: fn(&(f32, f32)) -> f32| c.iter().map(f).fold(f32::MAX, f32::min);
        let max = |f: fn(&(f32, f32)) -> f32| c.iter().map(f).fold(f32::MIN, f32::max);
        Rc { l: (min(|c| c.0) - pad).floor() as i32, t: (min(|c| c.1) - pad).floor() as i32, r: (max(|c| c.0) + pad).ceil() as i32, b: (max(|c| c.1) + pad).ceil() as i32 }
    }

    pub fn draw(&self, pix: &mut PixmapMut, p: &Pose, sh: f32) {
        let a = &self.asset;
        let img = &a.display;
        let (iw, ih) = (img.width() as f32, img.height() as f32);
        let flip = Transform::from_row(1.0, 0.0, 0.0, -1.0, 0.0, sh);
        let (sin, cos) = p.rotation.sin_cos();
        let base = flip
            .pre_concat(Transform::from_translate(p.grip.0, p.grip.1))
            .pre_concat(Transform::from_row(cos, sin, -sin, cos, 0.0, 0.0))
            .pre_concat(Transform::from_translate(-a.rod_x * p.width, 0.0));

        let mut paint = Paint::default();
        paint.shader = Pattern::new(img.as_ref(), SpreadMode::Pad, FilterQuality::Bilinear, 1.0, Transform::identity());
        paint.anti_alias = true;
        let sx = p.width / iw;

        let (v_top, v_wrist, v_end) = (a.hand_top * ih, a.wrist * ih, a.rod_end * ih);
        let k_rod = p.rod_len / (v_end - v_wrist).max(1.0);
        let rod = base.pre_concat(Transform::from_row(sx, 0.0, 0.0, -k_rod, 0.0, p.rod_len + k_rod * v_wrist));
        if let Some(r) = Rect::from_ltrb(0.0, v_wrist - 1.0, iw, v_end.max(v_wrist + 1.0)) {
            pix.fill_rect(r, &paint, rod, None);
        }

        let k_hand = p.hand_pts / (v_wrist - v_top).max(1.0);
        let hand = base.pre_concat(Transform::from_row(sx, 0.0, 0.0, -k_hand, 0.0, p.rod_len + p.hand_pts + k_hand * v_top));
        if let Some(r) = Rect::from_ltrb(0.0, v_top, iw, v_wrist + 1.0) {
            pix.fill_rect(r, &paint, hand, None);
        }
    }
}
