use std::num::NonZeroIsize;

use raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;
use wry::dpi::{PhysicalPosition, PhysicalSize};
use wry::{Rect, WebView, WebViewBuilder};

pub const CLASS: windows::core::PCWSTR = w!("TinyHandSettings");

const PAGE: &str = include_str!("settings.html");

struct Parent(HWND);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = NonZeroIsize::new(self.0.0 as isize).ok_or(HandleError::Unavailable)?;
        // SAFETY: the handle stays valid while Parent is borrowed, and the window outlives the webview.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(raw))) })
    }
}

/// The settings window. Dropping it tears down WebView2 completely, so a closed
/// settings window costs no memory.
pub struct SettingsWindow {
    pub hwnd: HWND,
    webview: Option<WebView>,
}

impl SettingsWindow {
    pub fn open(on_message: impl Fn(String) + 'static) -> Option<Self> {
        let (client_w, client_h) = (780, 560);
        unsafe {
            let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
            let mut frame = RECT { left: 0, top: 0, right: client_w, bottom: client_h };
            let _ = AdjustWindowRect(&mut frame, style, false);
            let (w, h) = (frame.right - frame.left, frame.bottom - frame.top);
            let x = (GetSystemMetrics(SM_CXSCREEN) - w) / 2;
            let y = (GetSystemMetrics(SM_CYSCREEN) - h) / 2;
            let hinst = HINSTANCE(GetModuleHandleW(None).ok()?.0);
            let hwnd = match CreateWindowExW(WINDOW_EX_STYLE(0), CLASS, w!("TinyHand Settings"), style, x, y, w, h, None, None, Some(hinst), None) {
                Ok(h) => h,
                Err(e) => {
                    crate::log::write(&format!("settings: window failed: {e}"));
                    return None;
                }
            };
            for (kind, metric) in [(ICON_SMALL, SM_CXSMICON), (ICON_BIG, SM_CXICON)] {
                let _ = SendMessageW(hwnd, WM_SETICON, Some(WPARAM(kind as usize)), Some(LPARAM(crate::icon::logo_for(metric).0 as isize)));
            }
            let webview = WebViewBuilder::new()
                .with_html(PAGE.replace("__LOGO__", &crate::icon::data_url()))
                .with_ipc_handler(move |request| on_message(request.into_body()))
                .with_bounds(Rect { position: PhysicalPosition::new(0, 0).into(), size: PhysicalSize::new(client_w as u32, client_h as u32).into() })
                .build_as_child(&Parent(hwnd));
            match webview {
                Ok(webview) => {
                    let _ = ShowWindow(hwnd, SW_SHOW);
                    let _ = SetForegroundWindow(hwnd);
                    Some(Self { hwnd, webview: Some(webview) })
                }
                Err(e) => {
                    crate::log::write(&format!("settings: WebView2 failed: {e}"));
                    let _ = DestroyWindow(hwnd);
                    None
                }
            }
        }
    }

    pub fn resize(&self) {
        let mut r = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut r);
        }
        if let Some(webview) = &self.webview {
            let _ = webview.set_bounds(Rect {
                position: PhysicalPosition::new(0, 0).into(),
                size: PhysicalSize::new((r.right - r.left).max(1) as u32, (r.bottom - r.top).max(1) as u32).into(),
            });
        }
    }

    pub fn send(&self, script: &str) {
        if let Some(webview) = &self.webview {
            let _ = webview.evaluate_script(script);
        }
    }

    pub fn focus(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(self.hwnd);
        }
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        // The webview must go before its parent window.
        self.webview.take();
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
