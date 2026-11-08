use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

const IDS: [SYSTEM_CURSOR_ID; 13] = [
    OCR_NORMAL,
    OCR_IBEAM,
    OCR_WAIT,
    OCR_CROSS,
    OCR_UP,
    OCR_SIZENWSE,
    OCR_SIZENESW,
    OCR_SIZEWE,
    OCR_SIZENS,
    OCR_SIZEALL,
    OCR_NO,
    OCR_HAND,
    OCR_APPSTARTING,
];

pub fn hide() {
    let and = [0xFFu8; 128];
    let xor = [0u8; 128];
    unsafe {
        let hinst = GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).ok();
        for id in IDS {
            if let Ok(c) = CreateCursor(hinst, 0, 0, 32, 32, and.as_ptr().cast(), xor.as_ptr().cast()) {
                let _ = SetSystemCursor(c, id);
            }
        }
    }
}

/// Reloads the user's cursors. No broadcast: SPIF_SENDCHANGE makes the shell redraw.
pub fn restore() {
    unsafe {
        let _ = SystemParametersInfoW(SPI_SETCURSORS, 0, None, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0));
    }
}
