use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use tiny_skia::PixmapMut;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use crate::hook;
use crate::scene::{Ink, Pending, Rc, draw_laser, union_opt};
use crate::surface::Surface;

const DRAG_THRESHOLD: f32 = 4.0;

pub enum Next {
    Frame,
    Sleep(Option<u32>),
}

pub struct Renderer {
    hwnd: HWND,
    surface: Surface,
    ink: Ink,
    cursor: (f32, f32),
    dot: Option<(f32, f32, f32)>,
    down_at: (f32, f32),
    press: f32,
    held: bool,
    down_seq: u32,
    up_seq: u32,
    last: Instant,
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

impl Renderer {
    pub fn new(hwnd: HWND, w: i32, h: i32) -> Self {
        Self {
            hwnd,
            surface: Surface::new(w, h),
            ink: Ink::new(w, h),
            cursor: (0.0, 0.0),
            dot: None,
            down_at: (0.0, 0.0),
            press: 0.0,
            held: false,
            down_seq: 0,
            up_seq: 0,
            last: Instant::now(),
        }
    }

    /// Blank slate and an empty frame. Window thread only.
    pub fn reset(&mut self) {
        self.surface.clear();
        self.ink.clear();
        self.ink.take_dirty();
        self.dot = None;
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
        let at = cursor_pos();
        let mut busy = at != self.cursor;
        self.cursor = at;

        let down = hook::DOWN_SEQ.load(Relaxed);
        let up = hook::UP_SEQ.load(Relaxed);
        if up != self.up_seq {
            self.up_seq = up;
            self.ink.end();
        }
        if down != self.down_seq {
            self.down_seq = down;
            self.press = 1.0;
            self.down_at = (hook::DOWN_X.load(Relaxed) as f32, hook::DOWN_Y.load(Relaxed) as f32);
        }
        self.held = hook::HELD.load(Relaxed);

        if self.held {
            busy = true;
            if self.ink.is_drawing() {
                self.ink.extend(at);
            } else if (at.0 - self.down_at.0).hypot(at.1 - self.down_at.1) >= DRAG_THRESHOLD {
                self.ink.begin(self.down_at);
                self.ink.extend(at);
            }
        } else if self.press > 0.002 {
            self.press *= 0.82f32.powf(dt / 0.016);
            busy = true;
        } else {
            self.press = 0.0;
        }
        busy |= self.ink.tick();

        self.draw(at);

        if busy {
            return Next::Frame;
        }
        match self.ink.pending() {
            Pending::Fading => Next::Frame,
            Pending::After(d) => Next::Sleep(Some(d.as_millis() as u32 + 1)),
            Pending::Nothing => Next::Sleep(None),
        }
    }

    fn draw(&mut self, at: (f32, f32)) {
        let (w, h) = (self.surface.w, self.surface.h);
        let r = (h as f32 * 0.0038).max(4.0) * (1.0 + 0.2 * self.press);
        let dot = (at.0, at.1, r);
        let dot_moved = self.dot != Some(dot);
        let ink_dirty = self.ink.take_dirty();
        if !dot_moved && ink_dirty.is_none() {
            return;
        }

        let reach = r * 3.2 + 3.0;
        let now = Rc::around(at.0, at.1, reach);
        let mut dirty = union_opt(ink_dirty, now);
        if let Some(p) = self.dot {
            dirty = dirty.union(Rc::around(p.0, p.1, p.2 * 3.2 + 3.0));
        }
        self.dot = Some(dot);
        let Some(d) = dirty.clip(w, h) else { return };

        let stride = w as usize * 4;
        let src = self.ink.layer.data();
        let dst = self.surface.bytes_mut();
        for y in d.t as usize..d.b as usize {
            let range = y * stride + d.l as usize * 4..y * stride + d.r as usize * 4;
            dst[range.clone()].copy_from_slice(&src[range]);
        }
        if let Some(mut pix) = PixmapMut::from_bytes(self.surface.bytes_mut(), w as u32, h as u32) {
            draw_laser(&mut pix, at, r);
        }
        self.surface.present(self.hwnd);
    }
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
        let next = lock(&shared).frame();
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
