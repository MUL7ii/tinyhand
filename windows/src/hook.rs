use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, Ordering::Relaxed};

use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

pub const WM_TH_DOWN: u32 = WM_APP + 1;
pub const WM_TH_UP: u32 = WM_APP + 2;

pub static ACTIVE: AtomicBool = AtomicBool::new(false);
static TARGET: AtomicIsize = AtomicIsize::new(0);
static SWALLOWED: AtomicBool = AtomicBool::new(false);
static PASS: [AtomicI32; 4] = [const { AtomicI32::new(0) }; 4];

pub fn set_passthrough(r: RECT) {
    for (slot, v) in PASS.iter().zip([r.left, r.top, r.right, r.bottom]) {
        slot.store(v, Relaxed);
    }
}

fn in_passthrough(p: POINT) -> bool {
    p.x >= PASS[0].load(Relaxed) && p.y >= PASS[1].load(Relaxed) && p.x < PASS[2].load(Relaxed) && p.y < PASS[3].load(Relaxed)
}

fn post(msg: u32) {
    let hwnd = HWND(TARGET.load(Relaxed) as *mut c_void);
    unsafe {
        let _ = PostMessageW(Some(hwnd), msg, WPARAM(0), LPARAM(0));
    }
}

unsafe extern "system" fn proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && ACTIVE.load(Relaxed) {
        let info = unsafe { &*(lp.0 as *const MSLLHOOKSTRUCT) };
        if info.flags & LLMHF_INJECTED == 0 {
            match wp.0 as u32 {
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
                    if !in_passthrough(info.pt) {
                        SWALLOWED.store(true, Relaxed);
                        post(WM_TH_DOWN);
                        return LRESULT(1);
                    }
                    SWALLOWED.store(false, Relaxed);
                }
                WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
                    if SWALLOWED.swap(false, Relaxed) {
                        post(WM_TH_UP);
                        return LRESULT(1);
                    }
                }
                _ => {}
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

/// Runs the hook on its own thread so the callback never waits on rendering.
pub fn start(target: HWND) {
    TARGET.store(target.0 as isize, Relaxed);
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
