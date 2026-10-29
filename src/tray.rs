use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos, GetSystemMetrics,
    PostMessageW, SetForegroundWindow, TrackPopupMenu, HICON, ICONINFO, MF_CHECKED, MF_GRAYED, MF_SEPARATOR,
    MF_STRING, SM_CXSMICON, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_NULL,
};

use crate::util::wide;

pub const WM_TRAY: u32 = WM_APP + 1;

pub struct Tray {
    data: NOTIFYICONDATAW,
    sharing: HICON,
    paused: HICON,
}

impl Tray {
    pub fn add(hwnd: HWND) -> Tray {
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = 1;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = WM_TRAY;
        let tray = Tray { data, sharing: dot((0x85, 0xee, 0x14)), paused: dot((0x8f, 0x8b, 0x84)) };
        tray.show();
        tray
    }

    /// Adds the icon again, e.g. after Explorer restarts and forgets it.
    pub fn show(&self) {
        unsafe { Shell_NotifyIconW(NIM_ADD, &self.data) };
    }

    pub fn update(&mut self, sharing: bool, tip: &str) {
        self.data.hIcon = if sharing { self.sharing } else { self.paused };
        let tip: Vec<u16> = tip.encode_utf16().take(self.data.szTip.len() - 1).collect();
        self.data.szTip = [0; 128];
        self.data.szTip[..tip.len()].copy_from_slice(&tip);
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &self.data) };
    }

    pub fn remove(&self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &self.data);
            DestroyIcon(self.sharing);
            DestroyIcon(self.paused);
        }
    }
}

pub enum Item<'a> {
    Text(&'a str),
    Action { id: u32, label: &'a str, checked: bool },
    Separator,
}

/// Shows the tray menu at the cursor and returns the chosen id, or 0.
/// Runs a modal loop, so callers mustn't be holding on to app state.
pub fn menu(hwnd: HWND, items: &[Item]) -> u32 {
    unsafe {
        let menu = CreatePopupMenu();
        for item in items {
            match item {
                Item::Text(text) => AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, wide(text).as_ptr()),
                Item::Action { id, label, checked } => AppendMenuW(
                    menu,
                    MF_STRING | if *checked { MF_CHECKED } else { 0 },
                    *id as usize,
                    wide(label).as_ptr(),
                ),
                Item::Separator => AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null()),
            };
        }
        let mut cursor = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor);
        // Without this the menu doesn't close when you click elsewhere
        SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            cursor.x,
            cursor.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        chosen as u32
    }
}

/// A small anti-aliased dot in the given colour, sized for the tray.
fn dot((r, g, b): (u8, u8, u8)) -> HICON {
    unsafe {
        let size = GetSystemMetrics(SM_CXSMICON).max(16);
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size,
            biHeight: -size, // top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(std::ptr::null_mut(), &info, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
        let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, (size * size) as usize);

        let (center, radius) = (size as f32 / 2.0, size as f32 * 0.34);
        for y in 0..size {
            for x in 0..size {
                let distance = ((x as f32 + 0.5 - center).powi(2) + (y as f32 + 0.5 - center).powi(2)).sqrt();
                let alpha = (radius - distance + 0.5).clamp(0.0, 1.0);
                // Premultiplied BGRA
                let premultiply = |c: u8| (c as f32 * alpha) as u32;
                pixels[(y * size + x) as usize] =
                    ((alpha * 255.0) as u32) << 24 | premultiply(r) << 16 | premultiply(g) << 8 | premultiply(b);
            }
        }

        let mask_bits = vec![0u8; ((size + 15) / 16 * 2 * size) as usize];
        let mask = CreateBitmap(size, size, 1, 1, mask_bits.as_ptr() as *const _);
        let icon_info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&icon_info);
        DeleteObject(color);
        DeleteObject(mask);
        icon
    }
}
