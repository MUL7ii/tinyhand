use std::ffi::c_void;
use std::ptr::null_mut;

use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Layered-window backing store. Pixels are premultiplied BGRA.
pub struct Surface {
    pub w: i32,
    pub h: i32,
    /// Where the window sits on the virtual desktop.
    pub origin: (i32, i32),
    dc: HDC,
    bmp: HBITMAP,
    bits: *mut u8,
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.dc);
            let _ = DeleteObject(self.bmp.into());
        }
    }
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
            Self { w, h, origin: (0, 0), dc, bmp, bits: bits.cast() }
        }
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, self.w as usize * self.h as usize * 4) }
    }

    pub fn clear(&mut self) {
        self.bytes_mut().fill(0);
    }

    fn blend_with(alpha: u8) -> BLENDFUNCTION {
        BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: AC_SRC_ALPHA as u8 }
    }

    /// Places the window at (x, y) with the whole surface at the given opacity.
    pub fn present_at(&self, hwnd: HWND, x: i32, y: i32, alpha: u8) {
        let origin = POINT::default();
        let at = POINT { x, y };
        let size = SIZE { cx: self.w, cy: self.h };
        unsafe {
            let _ = UpdateLayeredWindow(hwnd, None, Some(&at), Some(&size), Some(self.dc), Some(&origin), COLORREF(0), Some(&Self::blend_with(alpha)), ULW_ALPHA);
        }
    }

    /// Window thread only.
    pub fn present_full(&self, hwnd: HWND) {
        self.present_at(hwnd, self.origin.0, self.origin.1, 255);
    }

    pub fn present(&self, hwnd: HWND) {
        self.present_full(hwnd);
    }
}
