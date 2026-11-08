#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cursor;
mod hook;
mod renderer;
mod scene;
mod surface;

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Media::timeBeginPeriod;
use windows::Win32::System::Console::SetConsoleCtrlHandler;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, PCWSTR, PWSTR, w};

use renderer::{Renderer, lock};

const TIMER_TASKBAR: usize = 1;
const HK_TOGGLE: i32 = 1;
const HK_CLEAR: i32 = 2;
const HK_ESC: i32 = 3;
const WM_TRAY: u32 = WM_APP + 10;
const CMD_TOGGLE: usize = 1;
const CMD_QUIT: usize = 2;

static HWND_RAW: AtomicIsize = AtomicIsize::new(0);

struct App {
    hwnd: HWND,
    renderer: Arc<Mutex<Renderer>>,
    active: bool,
    /// A capture tool has focus. TinyHand steps aside until it leaves.
    suspended: bool,
}

const YIELD_TO: [&str; 3] = ["screenclippinghost.exe", "snippingtool.exe", "screensketch.exe"];

fn should_yield(hwnd: HWND) -> bool {
    if hwnd.0.is_null() {
        return false;
    }
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        if !ok {
            return false;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
        let name = path.rsplit('\\').next().unwrap_or("");
        YIELD_TO.contains(&name)
    }
}

unsafe extern "system" fn on_foreground(_: HWINEVENTHOOK, _: u32, hwnd: HWND, _: i32, _: i32, _: u32, _: u32) {
    if hwnd.0.is_null() {
        return;
    }
    let yielding = should_yield(hwnd);
    APP.with(|cell| {
        if let Some(app) = cell.try_borrow_mut().ok().as_mut().and_then(|s| s.as_mut()) {
            app.set_suspended(yielding);
        }
    });
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

impl App {
    fn toggle(&mut self) {
        if self.active {
            self.deactivate();
        } else {
            self.activate();
        }
    }

    fn activate(&mut self) {
        self.active = true;
        self.suspended = should_yield(unsafe { GetForegroundWindow() });
        if !self.suspended {
            self.start();
        }
    }

    fn deactivate(&mut self) {
        let running = self.active && !self.suspended;
        self.active = false;
        if running {
            self.stop();
        }
    }

    fn set_suspended(&mut self, suspended: bool) {
        if suspended == self.suspended {
            return;
        }
        self.suspended = suspended;
        if self.active {
            if suspended {
                self.stop();
            } else {
                self.start();
            }
        }
    }

    fn start(&mut self) {
        lock(&self.renderer).reset();
        self.refresh_taskbar();
        hook::HELD.store(false, Relaxed);
        hook::ACTIVE.store(true, Relaxed);
        cursor::hide();
        unsafe {
            let _ = RegisterHotKey(Some(self.hwnd), HK_ESC, MOD_NOREPEAT, VK_ESCAPE.0 as u32);
            SetTimer(Some(self.hwnd), TIMER_TASKBAR, 500, None);
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        hook::wake();
    }

    fn stop(&mut self) {
        hook::ACTIVE.store(false, Relaxed);
        hook::HELD.store(false, Relaxed);
        cursor::restore();
        lock(&self.renderer).reset();
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_TASKBAR);
            let _ = UnregisterHotKey(Some(self.hwnd), HK_ESC);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    fn refresh_taskbar(&self) {
        let mut r = RECT::default();
        unsafe {
            if let Ok(bar) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
                let _ = GetWindowRect(bar, &mut r);
            }
        }
        hook::set_passthrough(r);
    }

    fn on_message(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_TIMER => self.refresh_taskbar(),
            hook::WM_PRESENT => {
                hook::PRESENT_PENDING.store(false, Relaxed);
                if self.active {
                    lock(&self.renderer).present();
                }
            }
            WM_HOTKEY => match wp.0 as i32 {
                HK_TOGGLE => self.toggle(),
                HK_CLEAR => {
                    lock(&self.renderer).clear_ink();
                    hook::wake();
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

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, ..Default::default() }
}

fn add_tray(hwnd: HWND) {
    let mut nid = tray_data(hwnd);
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() };
    for (d, s) in nid.szTip.iter_mut().zip("TinyHand".encode_utf16()) {
        *d = s;
    }
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

fn remove_tray(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd));
    }
}

/// Ctrl+C in a dev console: put the cursors back before dying.
unsafe extern "system" fn on_ctrl(_: u32) -> BOOL {
    cursor::restore();
    remove_tray(HWND(HWND_RAW.load(Relaxed) as *mut c_void));
    std::process::exit(0)
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
        HWND_RAW.store(hwnd.0 as isize, Relaxed);

        // Without this the shell fades the window out on hide, leaving old ink on screen.
        let off = BOOL(1);
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED, (&raw const off).cast(), size_of::<BOOL>() as u32);

        let renderer = Arc::new(Mutex::new(Renderer::new(hwnd, w, h)));
        APP.with(|a| *a.borrow_mut() = Some(App { hwnd, renderer: renderer.clone(), active: false, suspended: false }));
        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT);
        hook::init();
        hook::start();
        std::thread::spawn(move || renderer::run(renderer));
        add_tray(hwnd);
        let _ = SetConsoleCtrlHandler(Some(on_ctrl), true);

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
