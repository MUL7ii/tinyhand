use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicU32, Ordering::Relaxed};

use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PCWSTR;

/// The window thread does the actual present.
pub const WM_PRESENT: u32 = WM_APP + 3;
pub static PRESENT_PENDING: AtomicBool = AtomicBool::new(false);
pub static ACTIVE: AtomicBool = AtomicBool::new(false);
pub static HELD: AtomicBool = AtomicBool::new(false);
/// Plain clicks reach the apps. Drags draw and stay out of the apps.
pub static CLICK_THROUGH: AtomicBool = AtomicBool::new(false);
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

/// Past this distance a press becomes a stroke. Matches the renderer.
pub const DRAG_PIXELS: f32 = 4.0;
/// A held press that may still turn out to be a plain click.
static PENDING: AtomicBool = AtomicBool::new(false);
static BUTTON: AtomicI32 = AtomicI32::new(0);

pub fn cancel_pending() {
    PENDING.store(false, Relaxed);
}

fn mouse_input(flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

fn replay_click(x: i32, y: i32, button: i32) {
    let (down, up) = match button {
        0 => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        1 => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        _ => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
    };
    unsafe {
        let _ = SetCursorPos(x, y);
        SendInput(&[mouse_input(down), mouse_input(up)], size_of::<INPUT>() as i32);
    }
}

/// Alt is held to click for real. The key is hidden from apps while it is.
pub const WM_INTERACT: u32 = WM_APP + 4;
pub static INTERACT: AtomicBool = AtomicBool::new(false);
/// Alt joined another key (Alt+Tab), so it was handed back to the app.
static PASSED: AtomicBool = AtomicBool::new(false);
static TARGET: AtomicIsize = AtomicIsize::new(0);

pub fn reset_keys() {
    INTERACT.store(false, Relaxed);
    PASSED.store(false, Relaxed);
    cancel_pending();
}

fn post_interact(on: bool) {
    let hwnd = HWND(TARGET.load(Relaxed) as *mut c_void);
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_INTERACT, WPARAM(on as usize), LPARAM(0));
    }
    wake();
}

fn key_input(vk: u32, scan: u32, extended: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk as u16),
                wScan: scan as u16,
                dwFlags: if extended { KEYEVENTF_EXTENDEDKEY } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

unsafe extern "system" fn key_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && ACTIVE.load(Relaxed) {
        let k = unsafe { &*(lp.0 as *const KBDLLHOOKSTRUCT) };
        if k.flags.0 & LLKHF_INJECTED.0 == 0 {
            let msg = wp.0 as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let is_alt = [VK_LMENU, VK_RMENU, VK_MENU].iter().any(|v| v.0 as u32 == k.vkCode);
            if is_alt && down && !PASSED.load(Relaxed) {
                if !INTERACT.swap(true, Relaxed) {
                    post_interact(true);
                }
                return LRESULT(1);
            }
            if is_alt && !down && !PASSED.swap(false, Relaxed) && INTERACT.swap(false, Relaxed) {
                post_interact(false);
                return LRESULT(1);
            }
            if !is_alt && down && INTERACT.load(Relaxed) && !PASSED.load(Relaxed) {
                // Give the app its Alt back, then the key, in order.
                PASSED.store(true, Relaxed);
                INTERACT.store(false, Relaxed);
                post_interact(false);
                let extended = k.flags.0 & LLKHF_EXTENDED.0 != 0;
                let inputs = [key_input(VK_MENU.0 as u32, 0, false), key_input(k.vkCode, k.scanCode, extended)];
                unsafe {
                    SendInput(&inputs, size_of::<INPUT>() as i32);
                }
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

unsafe extern "system" fn proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && ACTIVE.load(Relaxed) {
        let info = unsafe { &*(lp.0 as *const MSLLHOOKSTRUCT) };
        let msg = wp.0 as u32;
        if msg == WM_MOUSEMOVE {
            if PENDING.load(Relaxed) {
                let (dx, dy) = (info.pt.x - DOWN_X.load(Relaxed), info.pt.y - DOWN_Y.load(Relaxed));
                if ((dx * dx + dy * dy) as f32).sqrt() >= DRAG_PIXELS {
                    PENDING.store(false, Relaxed);
                }
            }
            if IDLE.swap(false, Relaxed) {
                wake();
            }
        } else if info.flags & LLMHF_INJECTED == 0 {
            match msg {
                WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN => {
                    SWALLOWED.store(false, Relaxed);
                    if !INTERACT.load(Relaxed) && !in_passthrough(info.pt) {
                        DOWN_X.store(info.pt.x, Relaxed);
                        DOWN_Y.store(info.pt.y, Relaxed);
                        HELD.store(true, Relaxed);
                        DOWN_SEQ.fetch_add(1, Relaxed);
                        SWALLOWED.store(true, Relaxed);
                        crate::sound::play();
                        // Click-through holds the press until it is clear whether it is a click or a stroke.
                        PENDING.store(CLICK_THROUGH.load(Relaxed), Relaxed);
                        BUTTON.store(if msg == WM_LBUTTONDOWN { 0 } else if msg == WM_RBUTTONDOWN { 1 } else { 2 }, Relaxed);
                        wake();
                        return LRESULT(1);
                    }
                }
                WM_LBUTTONUP | WM_RBUTTONUP | WM_MBUTTONUP => {
                    if SWALLOWED.swap(false, Relaxed) {
                        HELD.store(false, Relaxed);
                        UP_SEQ.fetch_add(1, Relaxed);
                        if PENDING.swap(false, Relaxed) {
                            replay_click(DOWN_X.load(Relaxed), DOWN_Y.load(Relaxed), BUTTON.load(Relaxed));
                        }
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
pub fn start(target: HWND) {
    TARGET.store(target.0 as isize, Relaxed);
    std::thread::spawn(|| unsafe {
        let hinst = GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).ok();
        if SetWindowsHookExW(WH_MOUSE_LL, Some(proc), hinst, 0).is_err() {
            return;
        }
        let _ = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_proc), hinst, 0);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}
