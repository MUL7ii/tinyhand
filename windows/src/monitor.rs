use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

pub struct Info {
    /// The whole monitor, in virtual-desktop pixels.
    pub full: RECT,
    /// The monitor minus the taskbar.
    pub work: RECT,
    /// Display scaling: 1.0 is 100%.
    pub scale: f32,
}

/// The monitor under a point.
pub fn at(p: (f32, f32)) -> Info {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: p.0 as i32, y: p.1 as i32 }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        Info { full: info.rcMonitor, work: info.rcWork, scale: dx.max(96) as f32 / 96.0 }
    }
}
