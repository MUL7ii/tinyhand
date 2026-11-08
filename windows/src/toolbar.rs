use std::time::Instant;

use tiny_skia::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::hand::HandAsset;
use crate::hook;
use crate::scene::{draw_laser, px};
use crate::surface::Surface;

pub const WM_TB_WANT: u32 = WM_APP + 22;
const TIMER_ANIM: usize = 2;
const WM_MOUSELEAVE: u32 = 0x02A3;

const BUTTON: f32 = 40.0;
const GAP: f32 = 6.0;
const PAD_X: f32 = 10.0;
const PAD_Y: f32 = 8.0;
const RADIUS: f32 = 16.0;
const FADE_IN: f32 = 0.15;
const FADE_OUT: f32 = 0.35;
const MARGIN_BOTTOM: f32 = 14.0;

pub enum Action {
    Pick(String),
    ToggleClickThrough,
    Exit,
}

enum Kind {
    Style(String),
    ClickThrough,
    Exit,
}

struct Item {
    kind: Kind,
    icon: Option<Pixmap>,
    x: f32,
}

pub struct Toolbar {
    pub hwnd: HWND,
    items: Vec<Item>,
    divider_x: f32,
    surface: Surface,
    scale: f32,
    selected: String,
    through: bool,
    hover: Option<usize>,
    alpha: f32,
    target: f32,
    last: Instant,
    tracking: bool,
    frame: RECT,
}

fn round_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let k = 0.5523 * r;
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.cubic_to(x + w - r + k, y, x + w, y + r - k, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.cubic_to(x + w, y + h - r + k, x + w - r + k, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.cubic_to(x + r - k, y + h, x, y + h - r + k, x, y + h - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    pb.close();
    pb.finish()
}

fn fill(pix: &mut PixmapMut, path: &Path, color: Color) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    pix.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

impl Toolbar {
    pub fn new(hwnd: HWND, styles: &[&str], selected: &str) -> Self {
        let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
        let icon_px = (36.0 * scale).round() as u32;
        let mut items: Vec<Item> = styles
            .iter()
            .map(|id| Item {
                kind: Kind::Style(id.to_string()),
                icon: HandAsset::load(id).map(|a| a.thumbnail(icon_px)),
                x: 0.0,
            })
            .collect();
        let mut x = PAD_X * scale;
        for item in &mut items {
            item.x = x;
            x += (BUTTON + GAP) * scale;
        }
        let divider_x = x;
        x += (1.0 + GAP) * scale;
        items.push(Item { kind: Kind::ClickThrough, icon: None, x });
        x += (BUTTON + GAP) * scale;
        items.push(Item { kind: Kind::Exit, icon: None, x });
        x += (BUTTON + PAD_X) * scale;

        let (w, h) = (x.round() as i32, ((BUTTON + 2.0 * PAD_Y) * scale).round() as i32);
        Self {
            hwnd,
            items,
            divider_x,
            surface: Surface::new(w, h),
            scale,
            selected: selected.to_string(),
            through: false,
            hover: None,
            alpha: 0.0,
            target: 0.0,
            last: Instant::now(),
            tracking: false,
            frame: RECT::default(),
        }
    }

    fn size(&self) -> (i32, i32) {
        (self.surface.w, self.surface.h)
    }

    fn place(&mut self) {
        let mut work = RECT::default();
        unsafe {
            let _ = SystemParametersInfoW(SPI_GETWORKAREA, 0, Some((&raw mut work).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
        }
        let (w, h) = self.size();
        let left = work.left + (work.right - work.left - w) / 2;
        let top = work.bottom - h - (MARGIN_BOTTOM * self.scale) as i32;
        self.frame = RECT { left, top, right: left + w, bottom: top + h };
        let s = self.scale;
        hook::set_zone(RECT {
            left: left - (80.0 * s) as i32,
            top: top - (100.0 * s) as i32,
            right: left + w + (80.0 * s) as i32,
            bottom: top + h + (10.0 * s) as i32,
        });
    }

    pub fn show(&mut self) {
        self.place();
        self.alpha = 0.0;
        self.target = 0.0;
        self.set_interactive(false);
        self.paint();
        unsafe {
            let _ = SetWindowPos(self.hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
    }

    pub fn hide(&mut self) {
        self.alpha = 0.0;
        self.target = 0.0;
        self.hover = None;
        self.set_interactive(false);
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_ANIM);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    pub fn set_click_through(&mut self, on: bool) {
        self.through = on;
        self.paint();
    }

    pub fn select(&mut self, id: &str) {
        self.selected = id.to_string();
        self.paint();
    }

    pub fn set_wanted(&mut self, wanted: bool) {
        let target = if wanted { 1.0 } else { 0.0 };
        if target == self.target {
            return;
        }
        self.target = target;
        self.last = Instant::now();
        self.set_interactive(wanted);
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_ANIM, 8, None);
        }
    }

    /// Hidden means clicks fall through the window and the hook stops letting them pass.
    fn set_interactive(&self, on: bool) {
        unsafe {
            let ex = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE);
            let flag = WS_EX_TRANSPARENT.0 as isize;
            SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, if on { ex & !flag } else { ex | flag });
        }
        hook::set_passthrough(1, if on { self.frame } else { RECT::default() });
    }

    fn animate(&mut self) {
        let dt = self.last.elapsed().as_secs_f32();
        self.last = Instant::now();
        let rate = if self.target > self.alpha { 1.0 / FADE_IN } else { -1.0 / FADE_OUT };
        self.alpha = (self.alpha + rate * dt).clamp(0.0, 1.0);
        if (self.alpha - self.target).abs() < 0.001 || (self.target == 1.0 && self.alpha >= 1.0) {
            self.alpha = self.target;
            unsafe {
                let _ = KillTimer(Some(self.hwnd), TIMER_ANIM);
            }
        }
        self.present();
    }

    fn present(&self) {
        self.surface.present_at(self.hwnd, self.frame.left, self.frame.top, (self.alpha * 255.0).round() as u8);
    }

    fn paint(&mut self) {
        let (w, h) = self.size();
        let s = self.scale;
        self.surface.clear();
        let Some(mut pix) = PixmapMut::from_bytes(self.surface.bytes_mut(), w as u32, h as u32) else { return };

        if let Some(path) = round_rect(0.5, 0.5, w as f32 - 1.0, h as f32 - 1.0, RADIUS * s) {
            fill(&mut pix, &path, px(18, 18, 18, 158));
            let mut paint = Paint::default();
            paint.set_color(px(255, 255, 255, 31));
            paint.anti_alias = true;
            let stroke = Stroke { width: 1.0, ..Default::default() };
            pix.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }

        let (btn, top) = (BUTTON * s, PAD_Y * s);
        for (i, item) in self.items.iter().enumerate() {
            let chosen = match &item.kind {
                Kind::Style(id) => *id == self.selected,
                Kind::ClickThrough => self.through,
                Kind::Exit => false,
            };
            let tint = if chosen { 46 } else if self.hover == Some(i) { 24 } else { 0 };
            if tint > 0 {
                if let Some(path) = round_rect(item.x, top, btn, btn, 10.0 * s) {
                    fill(&mut pix, &path, px(255, 255, 255, tint));
                }
            }
            let (cx, cy) = (item.x + btn / 2.0, top + btn / 2.0);
            match (&item.kind, &item.icon) {
                (_, Some(icon)) => {
                    let (ix, iy) = (cx - icon.width() as f32 / 2.0, cy - icon.height() as f32 / 2.0);
                    pix.draw_pixmap(ix.round() as i32, iy.round() as i32, icon.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
                }
                (Kind::Style(_), None) => draw_laser(&mut pix, (cx, cy), 5.0 * s),
                (Kind::ClickThrough, None) => {
                    let arrow = [(0.0, 0.0), (0.0, 16.0), (4.2, 12.2), (7.0, 18.5), (9.6, 17.4), (6.9, 11.3), (12.4, 11.0)];
                    let mut pb = PathBuilder::new();
                    for (i, (ax, ay)) in arrow.iter().enumerate() {
                        let (px_, py_) = (cx + (ax - 6.0) * s, cy + (ay - 9.0) * s);
                        if i == 0 {
                            pb.move_to(px_, py_);
                        } else {
                            pb.line_to(px_, py_);
                        }
                    }
                    pb.close();
                    if let Some(path) = pb.finish() {
                        fill(&mut pix, &path, px(255, 255, 255, 230));
                    }
                }
                (Kind::Exit, None) => {
                    let d = 6.0 * s;
                    let mut pb = PathBuilder::new();
                    pb.move_to(cx - d, cy - d);
                    pb.line_to(cx + d, cy + d);
                    pb.move_to(cx + d, cy - d);
                    pb.line_to(cx - d, cy + d);
                    if let Some(path) = pb.finish() {
                        let mut paint = Paint::default();
                        paint.set_color(px(255, 255, 255, 217));
                        paint.anti_alias = true;
                        let stroke = Stroke { width: 1.8 * s, line_cap: LineCap::Round, ..Default::default() };
                        pix.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                    }
                }
            }
        }

        if let Some(rect) = Rect::from_xywh(self.divider_x, (h as f32 - 24.0 * s) / 2.0, s.max(1.0), 24.0 * s) {
            let mut paint = Paint::default();
            paint.set_color(px(255, 255, 255, 46));
            pix.fill_rect(rect, &paint, Transform::identity(), None);
        }
        self.present();
    }

    fn hit(&self, x: i32, y: i32) -> Option<usize> {
        let s = self.scale;
        let (btn, top) = (BUTTON * s, PAD_Y * s);
        let (x, y) = (x as f32, y as f32);
        self.items.iter().position(|i| x >= i.x && x < i.x + btn && y >= top && y < top + btn)
    }

    pub fn handle(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<(LRESULT, Option<Action>)> {
        let point = || ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32);
        match msg {
            WM_MOUSEACTIVATE => Some((LRESULT(MA_NOACTIVATE as isize), None)),
            WM_TIMER if wp.0 == TIMER_ANIM => {
                self.animate();
                Some((LRESULT(0), None))
            }
            WM_MOUSEMOVE => {
                if !self.tracking {
                    let mut t = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: self.hwnd, dwHoverTime: 0 };
                    unsafe {
                        let _ = TrackMouseEvent(&mut t);
                    }
                    self.tracking = true;
                }
                let (x, y) = point();
                let hover = self.hit(x, y);
                if hover != self.hover {
                    self.hover = hover;
                    self.paint();
                }
                Some((LRESULT(0), None))
            }
            WM_MOUSELEAVE => {
                self.tracking = false;
                if self.hover.take().is_some() {
                    self.paint();
                }
                Some((LRESULT(0), None))
            }
            WM_LBUTTONUP => {
                let (x, y) = point();
                let action = self.hit(x, y).map(|i| match &self.items[i].kind {
                    Kind::Style(id) => Action::Pick(id.clone()),
                    Kind::ClickThrough => Action::ToggleClickThrough,
                    Kind::Exit => Action::Exit,
                });
                Some((LRESULT(0), action))
            }
            _ => None,
        }
    }
}
