#![windows_subsystem = "windows"]

mod cursor;
mod hook;
mod scene;
mod surface;

use std::cell::RefCell;
use std::sync::atomic::Ordering::Relaxed;
use std::time::Instant;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::UpdateWindow;
use windows::Win32::Media::timeBeginPeriod;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use scene::{Ink, draw_laser};
use surface::Surface;

const TIMER: usize = 1;
const FRAME_MS: u32 = 8;
const HK_TOGGLE: i32 = 1;
const HK_CLEAR: i32 = 2;
const HK_ESC: i32 = 3;
const WM_TRAY: u32 = WM_APP + 10;
const CMD_TOGGLE: usize = 1;
const CMD_QUIT: usize = 2;
const DRAG_THRESHOLD: f32 = 4.0;

struct App {
    hwnd: HWND,
    surface: Surface,
    ink: Ink,
    active: bool,
    held: bool,
    down_at: (f32, f32),
    cursor: (f32, f32),
    press: f32,
    dirty: bool,
    last_frame: Instant,
    last_taskbar: Instant,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

impl App {
    fn new(hwnd: HWND, w: i32, h: i32) -> Self {
        let mut ink = Ink::new();
        ink.width = (h as f32 * 0.0055).max(3.0);
        Self {
            hwnd,
            surface: Surface::new(w, h),
            ink,
            active: false,
            held: false,
            down_at: (0.0, 0.0),
            cursor: (0.0, 0.0),
            press: 0.0,
            dirty: true,
            last_frame: Instant::now(),
            last_taskbar: Instant::now() - std::time::Duration::from_secs(10),
        }
    }

    fn toggle(&mut self) {
        if self.active {
            self.deactivate();
        } else {
            self.activate();
        }
    }

    fn activate(&mut self) {
        self.active = true;
        self.refresh_taskbar();
        hook::ACTIVE.store(true, Relaxed);
        cursor::hide();
        unsafe {
            let _ = RegisterHotKey(Some(self.hwnd), HK_ESC, MOD_NOREPEAT, VK_ESCAPE.0 as u32);
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            SetTimer(Some(self.hwnd), TIMER, FRAME_MS, None);
        }
        self.last_frame = Instant::now();
        self.dirty = true;
        self.tick();
    }

    fn deactivate(&mut self) {
        self.active = false;
        self.held = false;
        hook::ACTIVE.store(false, Relaxed);
        self.ink.clear();
        cursor::restore();
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER);
            let _ = UnregisterHotKey(Some(self.hwnd), HK_ESC);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    fn refresh_taskbar(&mut self) {
        self.last_taskbar = Instant::now();
        let mut r = RECT::default();
        unsafe {
            if let Ok(bar) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
                let _ = GetWindowRect(bar, &mut r);
            }
        }
        hook::set_passthrough(r);
    }

    fn tick(&mut self) {
        if !self.active {
            return;
        }
        if self.last_taskbar.elapsed().as_millis() > 500 {
            self.refresh_taskbar();
        }
        let dt = self.last_frame.elapsed().as_secs_f32();
        self.last_frame = Instant::now();

        let mut p = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut p);
        }
        let at = (p.x as f32, p.y as f32);
        if at != self.cursor {
            self.cursor = at;
            self.dirty = true;
        }

        if self.held {
            if self.ink.is_drawing() {
                self.ink.extend(at);
            } else if (at.0 - self.down_at.0).hypot(at.1 - self.down_at.1) >= DRAG_THRESHOLD {
                self.ink.begin(self.down_at);
                self.ink.extend(at);
            }
        } else if self.press > 0.002 {
            self.press *= 0.82f32.powf(dt / 0.016);
            self.dirty = true;
        }

        if self.dirty || self.ink.alive() {
            self.render();
            self.dirty = false;
        }
    }

    fn render(&mut self) {
        let pix = &mut self.surface.pix;
        pix.fill(tiny_skia::Color::TRANSPARENT);
        self.ink.draw(pix);
        let h = self.surface.size().1 as f32;
        let r = (h * 0.0038).max(4.0) * (1.0 + 0.2 * self.press);
        draw_laser(&mut self.surface.pix, self.cursor, r);
        self.surface.present(self.hwnd);
    }

    fn on_message(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_TIMER => self.tick(),
            hook::WM_TH_DOWN => {
                self.held = true;
                self.press = 1.0;
                self.down_at = self.cursor;
                self.dirty = true;
            }
            hook::WM_TH_UP => {
                self.held = false;
                self.ink.end();
                self.dirty = true;
            }
            WM_HOTKEY => match wp.0 as i32 {
                HK_TOGGLE => self.toggle(),
                HK_CLEAR => {
                    self.ink.clear();
                    self.dirty = true;
                }
                HK_ESC => self.deactivate(),
                _ => {}
            },
            WM_TRAY => match (lp.0 as u32) & 0xFFFF {
                WM_LBUTTONUP => self.toggle(),
                WM_RBUTTONUP => self.tray_menu(hwnd),
                _ => {}
            },
            WM_COMMAND => match wp.0 & 0xFFFF {
                CMD_TOGGLE => self.toggle(),
                CMD_QUIT => unsafe { PostQuitMessage(0) },
                _ => {}
            },
            _ => return None,
        }
        Some(LRESULT(0))
    }

    fn tray_menu(&self, hwnd: HWND) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else { return };
            let label = if self.active { w!("Turn pointer off") } else { w!("Turn pointer on") };
            let _ = AppendMenuW(menu, MF_STRING, CMD_TOGGLE, label);
            let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, w!("Quit TinyHand"));
            let mut p = POINT::default();
            let _ = GetCursorPos(&mut p);
            let _ = SetForegroundWindow(hwnd);
            let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, p.x, p.y, Some(0), hwnd, None);
            let _ = DestroyMenu(menu);
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // try_borrow_mut: ShowWindow and friends re-enter this proc.
    let handled = APP.with(|cell| {
        let mut slot = cell.try_borrow_mut().ok()?;
        slot.as_mut()?.on_message(hwnd, msg, wp, lp)
    });
    handled.unwrap_or_else(|| unsafe { DefWindowProcW(hwnd, msg, wp, lp) })
}

fn add_tray(hwnd: HWND) {
    let mut nid = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() },
        ..Default::default()
    };
    for (d, s) in nid.szTip.iter_mut().zip("TinyHand".encode_utf16()) {
        *d = s;
    }
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

fn remove_tray(hwnd: HWND) {
    let nid = NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, ..Default::default() };
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
}

fn main() {
    cursor::restore();
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        cursor::restore();
        default_panic(info);
    }));

    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        timeBeginPeriod(1);

        let hinst = HINSTANCE(GetModuleHandleW(None).expect("module").0);
        let class = w!("TinyHandOverlay");
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: class,
            ..Default::default()
        });

        let (w, h) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            class,
            w!("TinyHand"),
            WS_POPUP,
            0,
            0,
            w,
            h,
            None,
            None,
            Some(hinst),
            None,
        )
        .expect("overlay window");
        let _ = UpdateWindow(hwnd);

        APP.with(|a| *a.borrow_mut() = Some(App::new(hwnd, w, h)));
        hook::start(hwnd);
        add_tray(hwnd);

        let mods = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
        let _ = RegisterHotKey(Some(hwnd), HK_TOGGLE, mods, 'H' as u32);
        let _ = RegisterHotKey(Some(hwnd), HK_CLEAR, mods, 'X' as u32);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        hook::ACTIVE.store(false, Relaxed);
        cursor::restore();
        remove_tray(hwnd);
    }
}
