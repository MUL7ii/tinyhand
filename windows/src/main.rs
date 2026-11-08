#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod arm;
mod config;
mod cursor;
mod hand;
mod hook;
mod icon;
mod log;
mod monitor;
mod prefs;
mod renderer;
mod scene;
mod settings;
mod sound;
mod styles;
mod surface;
mod thumbs;
mod toolbar;

use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::*;
use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, GetSysColorBrush};
use windows::Win32::Media::timeBeginPeriod;
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, SetConsoleCtrlHandler};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, PCWSTR, PWSTR, w};

use renderer::{Renderer, lock};
use toolbar::{Action, Toolbar};

const TIMER_TASKBAR: usize = 1;
const HK_TOGGLE: i32 = 1;
const HK_CLEAR: i32 = 2;
const HK_ESC: i32 = 3;
const HK_THROUGH: i32 = 4;
const WM_TRAY: u32 = WM_APP + 10;
const CMD_TOGGLE: usize = 1;
const CMD_QUIT: usize = 2;
const CMD_THROUGH: usize = 3;
const CMD_SETTINGS: usize = 4;

static HWND_RAW: AtomicIsize = AtomicIsize::new(0);

const TIMER_INITIAL: usize = 3;
const TIMER_RAISE: usize = 4;
const TIMER_POPUPS: usize = 5;

struct App {
    hwnd: HWND,
    renderer: Arc<Mutex<Renderer>>,
    toolbar: Toolbar,
    active: bool,
    /// A capture tool has focus. TinyHand steps aside until it leaves.
    suspended: bool,
    /// Cursor is in the toolbar's reveal zone.
    near: bool,
    /// The toolbar shows for a few seconds after turning on.
    initial: bool,
    style_id: String,
    settings: Option<settings::SettingsWindow>,
}

const YIELD_TO: [&str; 3] = ["screenclippinghost.exe", "snippingtool.exe", "screensketch.exe"];

/// Lower-cased exe name of the process that owns a window.
fn process_name(hwnd: HWND) -> Option<String> {
    if hwnd.0.is_null() {
        return None;
    }
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
        ok.then(|| path.rsplit('\\').next().unwrap_or("").to_string())
    }
}

fn should_yield(hwnd: HWND) -> bool {
    process_name(hwnd).is_some_and(|n| YIELD_TO.contains(&n.as_str()))
}

const SHELL_HOSTS: [&str; 3] = ["startmenuexperiencehost.exe", "searchhost.exe", "shellexperiencehost.exe"];

/// Windows' own popups. Clicks on them must reach Windows, or menus and flyouts cannot be used.
fn is_system_popup(hwnd: HWND) -> bool {
    let mut class = [0u16; 64];
    let n = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
    match String::from_utf16_lossy(&class[..n]).as_str() {
        "#32768" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd" | "TopLevelWindowForOverflowXamlIsland" | "NotifyIconOverflowWindow" | "XamlExplorerHostIslandWindow" => true,
        // Apps host their UI in these too, so only the shell's own count.
        "Windows.UI.Core.CoreWindow" => process_name(hwnd).is_some_and(|n| SHELL_HOSTS.contains(&n.as_str())),
        _ => false,
    }
}

unsafe extern "system" fn collect_popup(hwnd: HWND, lp: LPARAM) -> BOOL {
    let found = unsafe { &mut *(lp.0 as *mut Vec<RECT>) };
    if found.len() < hook::PASS_SLOTS - 2 && unsafe { IsWindowVisible(hwnd) }.as_bool() && is_system_popup(hwnd) {
        let mut r = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut r) }.is_ok() && r.right > r.left && r.bottom > r.top {
            found.push(r);
        }
    }
    BOOL(1)
}

/// Keeps the hook's pass-through list up to date with whatever system popups are open.
fn scan_popups() {
    let mut found: Vec<RECT> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect_popup), LPARAM((&raw mut found) as isize));
    }
    for slot in 0..hook::PASS_SLOTS - 2 {
        hook::set_passthrough(2 + slot, found.get(slot).copied().unwrap_or_default());
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
            app.raise();
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
        scan_popups();
        hook::reset_keys();
        hook::HELD.store(false, Relaxed);
        hook::ACTIVE.store(true, Relaxed);
        cursor::hide();
        unsafe {
            let _ = RegisterHotKey(Some(self.hwnd), HK_ESC, MOD_NOREPEAT, VK_ESCAPE.0 as u32);
            SetTimer(Some(self.hwnd), TIMER_TASKBAR, 500, None);
            SetTimer(Some(self.hwnd), TIMER_INITIAL, 3000, None);
            SetTimer(Some(self.hwnd), TIMER_RAISE, 250, None);
            SetTimer(Some(self.hwnd), TIMER_POPUPS, 100, None);
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
        sound::open();
        self.near = false;
        self.initial = true;
        self.toolbar.show();
        self.toolbar.set_wanted(true);
        hook::wake();
    }

    fn sound_key(&self) -> &'static str {
        if self.style_id == "laser" { "sound_laser" } else { "sound_hands" }
    }

    /// Hands tap by default and the laser stays quiet, each with its own switch.
    fn refresh_sound(&mut self) {
        let on = prefs::flag(self.sound_key(), self.style_id != "laser");
        sound::set_enabled(on);
        self.toolbar.set_sound(on);
    }

    fn toggle_sound(&mut self) {
        let key = self.sound_key();
        prefs::set_flag(key, !prefs::flag(key, self.style_id != "laser"));
        self.refresh_sound();
    }

    fn toggle_click_through(&mut self) {
        let on = !hook::CLICK_THROUGH.load(Relaxed);
        hook::CLICK_THROUGH.store(on, Relaxed);
        hook::cancel_pending();
        if hook::HELD.swap(false, Relaxed) {
            hook::UP_SEQ.fetch_add(1, Relaxed);
        }
        self.toolbar.set_click_through(on);
        prefs::set_flag("click_through", on);
        hook::wake();
    }

    fn pick(&mut self, id: &str) {
        let (style, ink) = styles::load(id);
        lock(&self.renderer).set_style(style, ink);
        prefs::save_style(id);
        let pinned = config::pinned();
        let rebuild = !pinned.iter().any(|p| *p == self.style_id) || !pinned.iter().any(|p| p == id);
        self.style_id = id.to_string();
        if rebuild {
            let list = self.toolbar_styles();
            self.toolbar.set_styles(&list);
        }
        self.toolbar.select(id);
        self.refresh_sound();
        hook::wake();
    }

    fn stop(&mut self) {
        hook::ACTIVE.store(false, Relaxed);
        hook::HELD.store(false, Relaxed);
        hook::reset_keys();
        cursor::restore();
        lock(&self.renderer).reset();
        sound::close();
        for slot in 2..hook::PASS_SLOTS {
            hook::set_passthrough(slot, RECT::default());
        }
        self.toolbar.hide();
        unsafe {
            let _ = KillTimer(Some(self.hwnd), TIMER_TASKBAR);
            let _ = KillTimer(Some(self.hwnd), TIMER_INITIAL);
            let _ = KillTimer(Some(self.hwnd), TIMER_RAISE);
            let _ = KillTimer(Some(self.hwnd), TIMER_POPUPS);
            let _ = UnregisterHotKey(Some(self.hwnd), HK_ESC);
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    /// Some windows (Paint) open above even topmost windows, which hides the hand
    /// while the real cursor is already blanked. Put both back on top, toolbar last.
    fn raise(&self) {
        if !self.active || self.suspended {
            return;
        }
        let place = |hwnd: HWND| unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        };
        place(self.hwnd);
        place(self.toolbar.hwnd);
    }

    fn refresh_taskbar(&self) {
        let mut r = RECT::default();
        unsafe {
            if let Ok(bar) = FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
                let _ = GetWindowRect(bar, &mut r);
            }
        }
        hook::set_passthrough(0, r);
    }

    fn on_message(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if msg == WM_ACTIVATE && (hwnd == self.hwnd || hwnd == self.toolbar.hwnd) && wp.0 & 0xFFFF != 0 {
            self.give_back_focus(HWND(lp.0 as *mut c_void));
            return Some(LRESULT(0));
        }
        if self.settings.as_ref().is_some_and(|s| s.hwnd == hwnd) {
            match msg {
                WM_SIZE => self.settings.as_ref()?.resize(),
                WM_CLOSE => self.close_settings(),
                _ => return None,
            }
            return Some(LRESULT(0));
        }
        if hwnd == self.toolbar.hwnd {
            let (result, action) = self.toolbar.handle(msg, wp, lp)?;
            match action {
                Some(Action::Pick(id)) => self.pick(&id),
                Some(Action::ToggleClickThrough) => self.toggle_click_through(),
                Some(Action::ToggleSound) => self.toggle_sound(),
                Some(Action::Exit) => self.deactivate(),
                None => {}
            }
            return Some(result);
        }
        match msg {
            WM_TIMER if wp.0 == TIMER_INITIAL => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), TIMER_INITIAL);
                }
                self.initial = false;
                self.toolbar.set_wanted(self.near);
            }
            WM_TIMER if wp.0 == TIMER_RAISE => self.raise(),
            WM_TIMER if wp.0 == TIMER_POPUPS => scan_popups(),
            WM_TIMER => self.refresh_taskbar(),
            hook::WM_REPLAY_CLICK => hook::send_click(lp, wp.0),
            hook::WM_REPLAY_KEY => hook::send_alt_and_key(wp.0, lp),
            hook::WM_INTERACT => {
                if self.active && !self.suspended {
                    if wp.0 != 0 {
                        cursor::restore();
                    } else {
                        cursor::hide();
                    }
                }
            }
            toolbar::WM_SCREEN_CHANGED => {
                if self.active && !self.suspended {
                    self.toolbar.follow_cursor();
                }
            }
            toolbar::WM_TB_WANT => {
                self.near = wp.0 != 0;
                if self.active && !self.suspended {
                    self.toolbar.set_wanted(self.near || self.initial);
                }
            }
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
                HK_THROUGH => self.toggle_click_through(),
                _ => {}
            },
            WM_TRAY => {
                if (lp.0 as u32) & 0xFFFF == WM_LBUTTONUP {
                    self.toggle();
                }
            }
            WM_COMMAND => self.command(wp.0 & 0xFFFF),
            _ => return None,
        }
        Some(LRESULT(0))
    }

    fn command(&mut self, cmd: usize) {
        match cmd {
            CMD_TOGGLE => self.toggle(),
            CMD_THROUGH => self.toggle_click_through(),
            CMD_SETTINGS => self.open_settings(),
            CMD_QUIT => unsafe { PostQuitMessage(0) },
            _ => {}
        }
    }

    fn open_settings(&mut self) {
        if let Some(s) = &self.settings {
            s.focus();
            return;
        }
        // Settings is an ordinary window, so the pointer turns off first, as on the Mac.
        self.deactivate();
        self.settings = settings::SettingsWindow::open(|message| {
            APP.with(|c| {
                if let Some(app) = c.try_borrow_mut().ok().as_mut().and_then(|s| s.as_mut()) {
                    app.on_settings_message(&message);
                }
            });
        });
    }

    fn close_settings(&mut self) {
        self.settings = None;
    }

    /// The pinned pointers, plus the current one if it is not pinned, as on the Mac.
    fn toolbar_styles(&self) -> Vec<String> {
        let mut list = config::pinned();
        if !list.contains(&self.style_id) {
            list.push(self.style_id.clone());
        }
        list
    }

    fn push_settings(&self) {
        let Some(s) = &self.settings else { return };
        let mut state = config::snapshot();
        state["style"] = serde_json::json!(self.style_id);
        state["pinned"] = serde_json::json!(config::pinned());
        state["max_pinned"] = serde_json::json!(config::MAX_PINNED);
        state["hands"] = serde_json::Value::Array(styles::all().map(|(id, title)| serde_json::json!({ "id": id, "title": title })).collect());
        s.send(&format!("window.__state({state})"));
        s.send(&format!("window.__thumbs({})", serde_json::json!(thumbs::all())));
    }

    fn notice(&self, text: &str) {
        if let Some(s) = &self.settings {
            s.send(&format!("window.__notice({})", serde_json::json!(text)));
        }
    }

    fn apply_pinned(&mut self) {
        let list = self.toolbar_styles();
        self.toolbar.set_styles(&list);
        self.toolbar.select(&self.style_id.clone());
        self.push_settings();
    }

    fn pin(&mut self, id: &str) {
        let mut list = config::pinned();
        if let Some(at) = list.iter().position(|x| x == id) {
            if list.len() == 1 {
                return self.notice("Keep at least one pointer in the toolbar.");
            }
            list.remove(at);
        } else if !styles::is_known(id) {
            return;
        } else if list.len() >= config::MAX_PINNED {
            return self.notice("The toolbar is full. Remove one first.");
        } else {
            list.push(id.to_string());
        }
        config::set_pinned(&list);
        self.apply_pinned();
    }

    fn reorder(&mut self, ids: &[String]) {
        let mut list: Vec<String> = Vec::new();
        for id in ids {
            if styles::is_known(id) && !list.contains(id) && list.len() < config::MAX_PINNED {
                list.push(id.clone());
            }
        }
        if !list.is_empty() {
            config::set_pinned(&list);
            self.apply_pinned();
        }
    }

    fn on_settings_message(&mut self, message: &str) {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(message) else { return };
        match msg["type"].as_str() {
            Some("get") | Some("thumbs") => self.push_settings(),
            Some("pin") => self.pin(msg["id"].as_str().unwrap_or("")),
            Some("reorder") => {
                let ids: Vec<String> = msg["ids"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default();
                self.reorder(&ids);
            }
            Some("pick") => {
                let id = msg["id"].as_str().unwrap_or("").to_string();
                if styles::is_known(&id) {
                    self.pick(&id);
                    self.push_settings();
                }
            }
            Some("set") => self.set_setting(msg["key"].as_str().unwrap_or(""), &msg["value"]),
            Some("preview_sound") => sound::preview(msg["name"].as_str().unwrap_or("")),
            Some("open_url") => open_url(msg["url"].as_str().unwrap_or("")),
            Some("reset") => {
                config::reset();
                sound::set_choice(&prefs::tap_sound());
                self.refresh_sound();
                lock(&self.renderer).refresh();
                self.push_settings();
            }
            _ => {}
        }
    }

    fn set_setting(&mut self, key: &str, value: &serde_json::Value) {
        match key {
            "tap_sound" => {
                let name = value.as_str().unwrap_or("thock");
                if sound::CHOICES.contains(&name) {
                    prefs::set_str("tap_sound", name);
                    sound::set_choice(name);
                    sound::preview(name);
                }
            }
            "sound_hands" | "sound_laser" => {
                prefs::set_flag(key, value.as_bool().unwrap_or(false));
                self.refresh_sound();
            }
            "click_through" => {
                if value.as_bool().unwrap_or(false) != hook::CLICK_THROUGH.load(Relaxed) {
                    self.toggle_click_through();
                }
            }
            _ => {
                if config::set(key, value) {
                    lock(&self.renderer).refresh();
                    hook::wake();
                }
            }
        }
    }

    /// The full-screen overlay must never hold keyboard focus. When Windows hands it over
    /// (for example after a flyout closes), pass it back to the window that had it.
    fn give_back_focus(&self, previous: HWND) {
        let ours = |h: HWND| h == self.hwnd || h == self.toolbar.hwnd;
        unsafe {
            if !previous.0.is_null() && !ours(previous) && IsWindowVisible(previous).as_bool() {
                let _ = SetForegroundWindow(previous);
                return;
            }
            let mut next = GetWindow(self.hwnd, GW_HWNDNEXT).ok();
            while let Some(w) = next {
                let tool = GetWindowLongPtrW(w, GWL_EXSTYLE) & WS_EX_TOOLWINDOW.0 as isize != 0;
                if !ours(w) && IsWindowVisible(w).as_bool() && !tool && GetWindowTextLengthW(w) > 0 {
                    let _ = SetForegroundWindow(w);
                    return;
                }
                next = GetWindow(w, GW_HWNDNEXT).ok();
            }
        }
    }
}

/// Opens a link in the default browser. Only TinyHand's own sites, never arbitrary input.
fn open_url(url: &str) {
    let allowed = ["https://github.com/nilbuild/", "https://github.com/MUL7ii/", "https://tinyhand.app", "https://kamran.fyi"];
    if !allowed.iter().any(|p| url.starts_with(p)) {
        return;
    }
    let wide: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

/// Runs outside the app borrow and its own tiny window. While it is open the overlay keeps
/// redrawing and every click goes through, so the menu can actually be used.
fn show_tray_menu(tray: HWND) {
    let active = APP.with(|c| c.try_borrow().ok().and_then(|s| s.as_ref().map(|a| a.active))).unwrap_or(false);
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let label = if active { w!("Turn pointer off") } else { w!("Turn pointer on") };
        let _ = AppendMenuW(menu, MF_STRING, CMD_TOGGLE, label);
        let checked = if hook::CLICK_THROUGH.load(Relaxed) { MF_CHECKED } else { MF_UNCHECKED };
        let _ = AppendMenuW(menu, MF_STRING | checked, CMD_THROUGH, w!("Click through\tCtrl+Alt+C"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_SETTINGS, w!("Settings…"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, w!("Quit TinyHand"));
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        hook::PAUSE.store(true, Relaxed);
        if active {
            cursor::restore();
        }
        let _ = SetForegroundWindow(tray);
        let cmd = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY, p.x, p.y, Some(0), tray, None);
        let _ = PostMessageW(Some(tray), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        hook::PAUSE.store(false, Relaxed);
        APP.with(|c| {
            if let Some(app) = c.try_borrow_mut().ok().as_mut().and_then(|s| s.as_mut()) {
                if app.active && !app.suspended {
                    cursor::hide();
                }
                app.command(cmd.0 as usize);
            }
        });
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_TRAY && (lp.0 as u32) & 0xFFFF == WM_RBUTTONUP {
        show_tray_menu(hwnd);
        return LRESULT(0);
    }
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
    nid.hIcon = icon::logo_for(SM_CXSMICON);
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
        let thread = std::thread::current();
        log::write(&format!("panic on '{}': {info}\n{}", thread.name().unwrap_or("?"), std::backtrace::Backtrace::force_capture()));
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

        let bar_class = w!("TinyHandToolbar");
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: bar_class,
            ..Default::default()
        });
        let bar = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
            bar_class,
            w!("TinyHand toolbar"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(hinst),
            None,
        )
        .expect("toolbar window");
        let _ = DwmSetWindowAttribute(bar, DWMWA_TRANSITIONS_FORCEDISABLED, (&raw const off).cast(), size_of::<BOOL>() as u32);

        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: settings::CLASS,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            ..Default::default()
        });

        // The tray icon and its menu get their own tiny window, so the overlay never takes focus.
        let tray_class = w!("TinyHandTray");
        RegisterClassExW(&WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: tray_class,
            ..Default::default()
        });
        let tray = CreateWindowExW(WS_EX_TOOLWINDOW, tray_class, w!("TinyHand tray"), WS_POPUP, 0, 0, 0, 0, None, None, Some(hinst), None).expect("tray window");
        HWND_RAW.store(tray.0 as isize, Relaxed);

        config::load();
        let id = std::env::args().nth(1).or_else(prefs::load_style).unwrap_or_else(|| "lavender".into());
        let (style, ink) = styles::load(&id);
        let mut shown = config::pinned();
        if !shown.contains(&id) {
            shown.push(id.clone());
        }
        let shown: Vec<&str> = shown.iter().map(String::as_str).collect();
        let mut toolbar = Toolbar::new(bar, &shown, &id);
        thumbs::build_in_background();
        let through = prefs::flag("click_through", false);
        hook::CLICK_THROUGH.store(through, Relaxed);
        toolbar.set_click_through(through);
        sound::load();
        sound::set_choice(&prefs::tap_sound());
        let renderer = Arc::new(Mutex::new(Renderer::new(hwnd, w, h, style, ink)));
        let mut app = App { hwnd, renderer: renderer.clone(), toolbar, active: false, suspended: false, near: false, initial: false, style_id: id.clone(), settings: None };
        app.refresh_sound();
        APP.with(|a| *a.borrow_mut() = Some(app));
        SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT);
        hook::init();
        hook::start(hwnd);
        let _ = std::thread::Builder::new().name("render".into()).spawn(move || renderer::run(renderer));
        add_tray(tray);
        // Release builds have no console of their own. Borrow the terminal's so Ctrl+C reaches us.
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
        let _ = SetConsoleCtrlHandler(Some(on_ctrl), true);

        let mods = MOD_CONTROL | MOD_ALT | MOD_NOREPEAT;
        let _ = RegisterHotKey(Some(hwnd), HK_TOGGLE, mods, 'H' as u32);
        let _ = RegisterHotKey(Some(hwnd), HK_CLEAR, mods, 'X' as u32);
        let _ = RegisterHotKey(Some(hwnd), HK_THROUGH, mods, 'C' as u32);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        hook::ACTIVE.store(false, Relaxed);
        cursor::restore();
        remove_tray(tray);
    }
}
