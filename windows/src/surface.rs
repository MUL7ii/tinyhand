use std::ffi::c_void;
use std::ptr::null_mut;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Layered-window backing store. Pixels are premultiplied BGRA.
pub struct Surface {
    pub w: i32,
    pub h: i32,
    dc: HDC,
    bits: *mut u8,
}

impl Surface {
    pub fn new(w: i32, h: i32) -> Self {
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = null_mut();
            let bmp = CreateDIBSection(Some(screen), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).expect("DIB section");
            SelectObject(dc, bmp.into());
            ReleaseDC(None, screen);
            Self { w, h, dc, bits: bits.cast() }
        }
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, self.w as usize * self.h as usize * 4) }
    }

    pub fn clear(&mut self) {
        self.bytes_mut().fill(0);
    }

    fn blend() -> BLENDFUNCTION {
        BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 }
    }

    /// Sets position and size as well. Window thread only.
    pub fn present_full(&self, hwnd: HWND) {
        let origin = POINT::default();
        let size = SIZE { cx: self.w, cy: self.h };
        unsafe {
            let _ = UpdateLayeredWindow(hwnd, None, Some(&origin), Some(&size), Some(self.dc), Some(&origin), COLORREF(0), Some(&Self::blend()), ULW_ALPHA);
        }
    }

    /// Content only, so it is safe from any thread.
    pub fn present_rect(&self, hwnd: HWND, dirty: RECT) {
        let origin = POINT::default();
        let blend = Self::blend();
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcDst: HDC::default(),
            pptDst: std::ptr::null(),
            psize: std::ptr::null(),
            hdcSrc: self.dc,
            pptSrc: &origin,
            crKey: COLORREF(0),
            pblend: &blend,
            dwFlags: ULW_ALPHA,
            prcDirty: &dirty,
        };
        unsafe {
            let _ = UpdateLayeredWindowIndirect(hwnd, &info);
        }
    }
}
