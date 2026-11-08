use std::ffi::c_void;
use std::ptr::null_mut;

use tiny_skia::Pixmap;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetSystemMetricsForDpi};
use windows::Win32::UI::WindowsAndMessaging::*;

/// The official TinyHand mark, shared with the Mac app and the README.
const LOGO: &[u8] = include_bytes!("../../.github/logo.png");

/// A window icon at the size Windows wants for this display scaling.
pub fn logo_for(metric: SYSTEM_METRICS_INDEX) -> HICON {
    let size = unsafe { GetSystemMetricsForDpi(metric, GetDpiForSystem()) }.max(16);
    logo(size)
}

pub fn logo(size: i32) -> HICON {
    let Ok(src) = Pixmap::decode_png(LOGO) else { return HICON::default() };
    let px = crate::hand::resized(&src, size as u32);
    let (w, h) = (px.width() as i32, px.height() as i32);
    unsafe {
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
        let Ok(color) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else { return HICON::default() };
        // Icons carry plain (not premultiplied) BGRA.
        let out = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (w * h * 4) as usize);
        for (d, s) in out.chunks_exact_mut(4).zip(px.data().chunks_exact(4)) {
            let a = s[3] as u32;
            let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a / 2) / a).min(255) as u8 };
            d.copy_from_slice(&[un(s[2]), un(s[1]), un(s[0]), s[3]]);
        }
        let mask = CreateBitmap(w, h, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon
    }
}

/// The logo as a data URL, for the settings page.
pub fn data_url() -> String {
    format!("data:image/png;base64,{}", base64(LOGO))
}

pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { TABLE[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { TABLE[n as usize & 63] as char } else { '=' });
    }
    out
}
