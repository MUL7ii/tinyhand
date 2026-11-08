use std::ffi::c_void;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use tiny_skia::PixmapMut;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, PostMessageW};

use crate::hook;
use crate::scene::{Ink, Pending, Rc, draw_laser};
use crate::styles::Style;
use crate::toolbar::{WM_SCREEN_CHANGED, WM_TB_WANT};
use crate::surface::Surface;

const DRAG_THRESHOLD: f32 = 4.0;
const TICK_HZ: f32 = 120.0;

pub enum Next {
    Frame,
    Sleep(Option<u32>),
}

pub struct Renderer {
    hwnd: HWND,
    surface: Surface,
    ink: Ink,
    style: Style,
    style_ink: u32,
    cursor: (f32, f32),
    key: Vec<i32>,
    shape: Option<Rc>,
    down_at: (f32, f32),
    press: f32,
    held: bool,
    stale_press: bool,
    down_seq: u32,
    up_seq: u32,
    last: Instant,
    drew: bool,
    near_toolbar: bool,
}

// The raw HWND and DIB pointer are only used behind the mutex.
unsafe impl Send for Renderer {}

fn cursor_pos() -> (f32, f32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x as f32, p.y as f32)
}

fn monitor_rect(at: (f32, f32)) -> RECT {
    crate::monitor::at(at).full
}

impl Renderer {
    pub fn new(hwnd: HWND, w: i32, h: i32, style: Style, ink_rgb: u32) -> Self {
        let mut ink = Ink::new(w, h);
        ink.set_color(crate::config::ink_override().unwrap_or(ink_rgb));
        Self {
            hwnd,
            surface: Surface::new(w, h),
            ink,
            style,
            style_ink: ink_rgb,
            cursor: (0.0, 0.0),
            key: vec![],
            shape: None,
            down_at: (0.0, 0.0),
            press: 0.0,
            held: false,
            stale_press: false,
            down_seq: 0,
            up_seq: 0,
            last: Instant::now(),
            drew: false,
            near_toolbar: false,
        }
    }

    /// Follows the cursor onto another monitor. Returns true when the screen changed.
    fn sync_screen(&mut self, at: (f32, f32)) -> bool {
        let r = monitor_rect(at);
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        if w <= 0 || h <= 0 || ((r.left, r.top) == self.surface.origin && (w, h) == (self.surface.w, self.surface.h)) {
            return false;
        }
        self.surface = Surface::new(w, h);
        self.surface.origin = (r.left, r.top);
        self.ink.resize(w, h);
        self.key.clear();
        self.shape = None;
        self.drew = true;
        true
    }

    pub fn set_style(&mut self, style: Style, ink_rgb: u32) {
        self.style = style;
        self.style_ink = ink_rgb;
        self.refresh();
    }

    /// Picks up changed settings: ink color, pointer size, shadow.
    pub fn refresh(&mut self) {
        self.ink.set_color(crate::config::ink_override().unwrap_or(self.style_ink));
        self.key.clear();
    }

    /// Blank slate and an empty frame. Window thread only.
    pub fn reset(&mut self) {
        self.near_toolbar = false;
        self.sync_screen(cursor_pos());
        self.stale_press = false;
        self.surface.clear();
        self.ink.clear();
        self.ink.take_dirty();
        self.key.clear();
        self.shape = None;
        self.press = 0.0;
        self.held = false;
        self.cursor = cursor_pos();
        self.down_seq = hook::DOWN_SEQ.load(Relaxed);
        self.up_seq = hook::UP_SEQ.load(Relaxed);
        self.last = Instant::now();
        self.surface.present_full(self.hwnd);
    }

    pub fn clear_ink(&mut self) {
        self.ink.clear();
    }

    pub fn cursor_moved(&self) -> bool {
        cursor_pos() != self.cursor
    }

    pub fn frame(&mut self) -> Next {
        let dt = self.last.elapsed().as_secs_f32();
        self.last = Instant::now();
        let global = cursor_pos();
        let mut busy = global != self.cursor;
        self.cursor = global;
        if self.sync_screen(global) {
            // A press that began on the old screen cannot carry a stroke onto this one.
            self.stale_press = hook::HELD.load(Relaxed);
            unsafe {
                let _ = PostMessageW(Some(self.hwnd), WM_SCREEN_CHANGED, WPARAM(0), LPARAM(0));
            }
        }
        let (ox, oy) = (self.surface.origin.0 as f32, self.surface.origin.1 as f32);
        let at = (global.0 - ox, global.1 - oy);

        let down = hook::DOWN_SEQ.load(Relaxed);
        let up = hook::UP_SEQ.load(Relaxed);
        if up != self.up_seq {
            self.up_seq = up;
            self.ink.end();
        }
        if down != self.down_seq {
            self.down_seq = down;
            self.press = 1.0;
            self.stale_press = false;
            self.down_at = (hook::DOWN_X.load(Relaxed) as f32 - ox, hook::DOWN_Y.load(Relaxed) as f32 - oy);
        }
        self.held = hook::HELD.load(Relaxed);
        if !self.held {
            self.stale_press = false;
        }

        if self.held {
            busy = true;
            if self.stale_press {
            } else if self.ink.is_drawing() {
                self.ink.extend(at);
            } else if (at.0 - self.down_at.0).hypot(at.1 - self.down_at.1) >= DRAG_THRESHOLD {
                self.ink.begin(self.down_at);
                self.ink.extend(at);
            }
        } else if self.press > 0.002 {
            self.press *= 0.82f32.powf(dt * TICK_HZ);
            busy = true;
        } else {
            self.press = 0.0;
        }
        busy |= self.ink.tick();

        let near = !self.ink.is_drawing() && hook::in_zone(global.0 as i32, global.1 as i32);
        if near != self.near_toolbar {
            self.near_toolbar = near;
            unsafe {
                let _ = PostMessageW(Some(self.hwnd), WM_TB_WANT, WPARAM(near as usize), LPARAM(0));
            }
        }

        busy |= self.draw(at, dt);

        if busy {
            return Next::Frame;
        }
        match self.ink.pending() {
            Pending::Fading => Next::Frame,
            Pending::After(d) => Next::Sleep(Some(d.as_millis() as u32 + 1)),
            Pending::Nothing => Next::Sleep(None),
        }
    }

    /// Returns true while the pointer is still easing toward its target.
    fn draw(&mut self, at: (f32, f32), dt: f32) -> bool {
        let (w, h) = (self.surface.w, self.surface.h);
        let screen = (w as f32, h as f32);

        let mut pose = None;
        let (key, shape, settling) = match &mut self.style {
            Style::Hand(arm) => {
                let p = arm.aim(at, screen, self.press, crate::config::pointer_size(), dt);
                pose = Some(p);
                (p.key(), arm.bbox(&p, screen.1), arm.settling())
            }
            Style::Laser => {
                let r = laser_radius(screen.1, self.press);
                (vec![at.0 as i32 * 100, at.1 as i32 * 100, (r * 100.0) as i32], Rc::around(at.0, at.1, r * 3.2 + 3.0), false)
            }
        };

        // Holding Alt shows the real cursor, so the pointer steps out of the way.
        let hidden = hook::INTERACT.load(Relaxed);
        let mut key = key;
        key.push(hidden as i32);
        let shape = (!hidden).then_some(shape);

        let ink_dirty = self.ink.take_dirty();
        if key == self.key && ink_dirty.is_none() {
            return settling;
        }
        let dirty = [ink_dirty, shape, self.shape].into_iter().flatten().reduce(Rc::union);
        self.key = key;
        self.shape = shape;
        let Some(d) = dirty.and_then(|d| d.clip(w, h)) else { return settling };

        let stride = w as usize * 4;
        let src = self.ink.layer.data();
        let dst = self.surface.bytes_mut();
        for y in d.t as usize..d.b as usize {
            let range = y * stride + d.l as usize * 4..y * stride + d.r as usize * 4;
            dst[range.clone()].copy_from_slice(&src[range]);
        }
        if hidden {
            self.drew = true;
            return settling;
        }
        if let Some(mut pix) = PixmapMut::from_bytes(self.surface.bytes_mut(), w as u32, h as u32) {
            match (&self.style, pose) {
                (Style::Hand(arm), Some(p)) => arm.draw(&mut pix, &p, screen.1, crate::config::shadow()),
                _ => draw_laser(&mut pix, at, laser_radius(screen.1, self.press)),
            }
        }
        self.drew = true;
        settling
    }

    pub fn present(&self) {
        self.surface.present(self.hwnd);
    }

    pub fn hwnd_raw(&self) -> isize {
        self.hwnd.0 as isize
    }
}

fn laser_radius(screen_h: f32, press: f32) -> f32 {
    (screen_h * 0.0038).max(4.0) * (1.0 + 0.2 * press)
}

pub fn lock(r: &Mutex<Renderer>) -> MutexGuard<'_, Renderer> {
    r.lock().unwrap_or_else(|e| e.into_inner())
}

/// Frames follow the compositor's vsync. Idle blocks on an event, so CPU is zero.
pub fn run(shared: Arc<Mutex<Renderer>>) {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
    }
    loop {
        if !hook::ACTIVE.load(Relaxed) {
            hook::IDLE.store(true, Relaxed);
            hook::wait(INFINITE);
            continue;
        }
        let frame = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut r = lock(&shared);
            let next = r.frame();
            (next, std::mem::take(&mut r.drew), r.hwnd_raw())
        }));
        // A panic here would freeze the overlay while the hook keeps swallowing clicks. Log it and carry on.
        let Ok((next, drew, hwnd)) = frame else {
            crate::log::write("render frame panicked, retrying");
            std::thread::sleep(std::time::Duration::from_millis(100));
            continue;
        };
        if drew && !hook::PRESENT_PENDING.swap(true, Relaxed) {
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut c_void)), hook::WM_PRESENT, WPARAM(0), LPARAM(0));
            }
        }
        match next {
            Next::Frame => unsafe {
                let _ = DwmFlush();
            },
            Next::Sleep(ms) => {
                hook::IDLE.store(true, Relaxed);
                if lock(&shared).cursor_moved() {
                    hook::IDLE.store(false, Relaxed);
                    continue;
                }
                hook::wait(ms.unwrap_or(INFINITE));
                hook::IDLE.store(false, Relaxed);
            }
        }
    }
}
