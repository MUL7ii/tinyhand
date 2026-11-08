use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering::Relaxed};

use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

/// The window thread does the actual present.
pub const WM_PRESENT: u32 = WM_APP + 3;
pub static PRESENT_PENDING: AtomicBool = AtomicBool::new(false);
pub static ACTIVE: AtomicBool = AtomicBool::new(false);
pub static HELD: AtomicBool = AtomicBool::new(false);
/// Set by the render thread while it sleeps. The hook wakes it on mouse moves.
pub static IDLE: AtomicBool = AtomicBool::new(false);
pub static DOWN_SEQ: AtomicU32 = AtomicU32::new(0);
pub static UP_SEQ: AtomicU32 = AtomicU32::new(0);
pub static DOWN_X: AtomicI32 = AtomicI32::new(0);
pub static DOWN_Y: AtomicI32 = AtomicI32::new(0);

static WAKE: AtomicIsize = AtomicIsize::new(0);
static SWALLOWED: AtomicBool = AtomicBool::new(false);
/// Clicks inside these rects reach the windows under the overlay: taskbar, toolbar.
static PASS: [[AtomicI32; 4]; 2] = [const { [const { AtomicI32::new(0) }; 4] }; 2];
/// Cursor zone that reveals the toolbar.
pub static ZONE: [AtomicI32; 4] = [const { AtomicI32::new(0) }; 4];

pub fn set_zone(r: RECT) {
    for (slot, v) in ZONE.iter().zip([r.left, r.top, r.right, r.bottom]) {
        slot.store(v, Relaxed);
    }
}

pub fn in_zone(x: i32, y: i32) -> bool {
    x >= ZONE[0].load(Relaxed) && y >= ZONE[1].load(Relaxed) && x < ZONE[2].load(Relaxed) && y < ZONE[3].load(Relaxed)
}

pub fn init() {
    let event = unsafe { CreateEventW(None, false, false, PCWSTR::null()) }.expect("wake event");
    WAKE.store(event.0 as isize, Relaxed);
}

fn handle() -> HANDLE {
    HANDLE(WAKE.load(Relaxed) as *mut c_void)
}

pub fn wake() {
    unsafe {
        let _ = SetEvent(handle());
    }
}

pub fn wait(ms: u32) {
    unsafe {
        WaitForSingleObject(handle(), ms);
    }
}

pub fn set_passthrough(index: usize, r: RECT) {
    for (slot, v) in PASS[index].iter().zip([r.left, r.top, r.right, r.bottom]) {
        slot.store(v, Relaxed);
    }
}

fn in_passthrough(p: POINT) -> bool {
    PASS.iter().any(|r| p.x >= r[0].load(Relaxed) && p.y >= r[1].load(Relaxed) && p.x < r[2].load(Relaxed) && p.y < r[3].load(Relaxed))
}

unsafe extern "system" fn proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && ACTIVE.load(Relaxed) {
        let info = unsafe { &*(lp.0 as *const MSLLHOOKSTRUCT) };
        let msg = wp.0 as u32;
        if msg == WM_MOUSEMOVE {
            if IDLE.swap(false, Relaxed) {
                wake();
            }
        } else if info.flags & LLMHF_INJECTED == 0 {
            match msg {
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
                    if !in_passthrough(info.pt) {
                        SWALLOWED.store(true, Relaxed);
                        DOWN_X.store(info.pt.x, Relaxed);
                        DOWN_Y.store(info.pt.y, Relaxed);
                        HELD.store(true, Relaxed);
                        DOWN_SEQ.fetch_add(1, Relaxed);
                        wake();
                        return LRESULT(1);
                    }
                    SWALLOWED.store(false, Relaxed);
                }
                WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
                    if SWALLOWED.swap(false, Relaxed) {
                        HELD.store(false, Relaxed);
                        UP_SEQ.fetch_add(1, Relaxed);
                        wake();
                        return LRESULT(1);
                    }
                }
                _ => {}
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

/// Own thread, so the callback never waits on rendering.
pub fn start() {
    std::thread::spawn(|| unsafe {
        let hinst = GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).ok();
        if SetWindowsHookExW(WH_MOUSE_LL, Some(proc), hinst, 0).is_err() {
            return;
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}
