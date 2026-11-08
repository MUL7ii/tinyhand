use std::ffi::c_void;
use std::ptr::null_mut;

use tiny_skia::Pixmap;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

/// Software canvas blitted to a layered window with per-pixel alpha.
pub struct Surface {
    pub pix: Pixmap,
    w: i32,
    h: i32,
    dc: HDC,
    bits: *mut u32,
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
            Self { pix: Pixmap::new(w as u32, h as u32).expect("pixmap"), w, h, dc, bits: bits.cast() }
        }
    }

    pub fn size(&self) -> (i32, i32) {
        (self.w, self.h)
    }

    pub fn present(&mut self, hwnd: HWND) {
        let src = self.pix.data();
        let dst = unsafe { std::slice::from_raw_parts_mut(self.bits.cast::<u8>(), src.len()) };
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2];
            d[1] = s[1];
            d[2] = s[0];
            d[3] = s[3];
        }
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        let origin = POINT::default();
        let size = SIZE { cx: self.w, cy: self.h };
        unsafe {
            let _ = UpdateLayeredWindow(hwnd, None, Some(&origin), Some(&size), Some(self.dc), Some(&origin), COLORREF(0), Some(&blend), ULW_ALPHA);
        }
    }
}
