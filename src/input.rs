// Counts clicks, key presses, scrolls and mouse movement through Raw Input.
// Unlike a keyboard hook, Raw Input can't slow down or change anyone's input: Windows just
// hands this window a copy of each event. Nothing about which key was pressed is kept.

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::UI::Input::{
    GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK,
    RID_INPUT, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE,
};

use crate::state::Counters;

// RAWMOUSE.usButtonFlags
const BUTTON_DOWNS: u16 = 0x0001 | 0x0004 | 0x0010 | 0x0040 | 0x0100; // left, right, middle, 4, 5
const WHEEL: u16 = 0x0400;
const HWHEEL: u16 = 0x0800;
// RAWMOUSE.usFlags: absolute positions come from tablets and remote desktop, not a moving mouse
const MOUSE_MOVE_ABSOLUTE: u16 = 0x01;
// RAWKEYBOARD.Flags
const KEY_BREAK: u16 = 0x01;

/// Asks Windows for mouse and keyboard input even while other apps have focus.
pub fn register(hwnd: HWND) -> bool {
    let devices = [
        RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x02, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd }, // mouse
        RAWINPUTDEVICE { usUsagePage: 0x01, usUsage: 0x06, dwFlags: RIDEV_INPUTSINK, hwndTarget: hwnd }, // keyboard
    ];
    unsafe {
        RegisterRawInputDevices(devices.as_ptr(), devices.len() as u32, std::mem::size_of::<RAWINPUTDEVICE>() as u32) != 0
    }
}

/// Handles one WM_INPUT. `keys_down` only tells a fresh press from a held key repeating;
/// it's indexed by virtual-key code and never stored or sent anywhere.
pub fn handle(lparam: LPARAM, counters: &mut Counters, keys_down: &mut [bool; 256]) {
    let mut raw: RAWINPUT = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<RAWINPUT>() as u32;
    let read = unsafe {
        GetRawInputData(
            lparam as HRAWINPUT,
            RID_INPUT,
            &mut raw as *mut RAWINPUT as *mut _,
            &mut size,
            std::mem::size_of::<RAWINPUTHEADER>() as u32,
        )
    };
    if read == u32::MAX || read == 0 {
        return;
    }

    match raw.header.dwType {
        RIM_TYPEMOUSE => {
            let mouse = unsafe { raw.data.mouse };
            let flags = unsafe { mouse.Anonymous.Anonymous.usButtonFlags };
            counters.clicks += (flags & BUTTON_DOWNS).count_ones() as u64;
            if flags & (WHEEL | HWHEEL) != 0 {
                counters.scrolls += 1;
            }
            if mouse.usFlags & MOUSE_MOVE_ABSOLUTE == 0 {
                let (dx, dy) = (mouse.lLastX as f64, mouse.lLastY as f64);
                counters.mickeys += (dx * dx + dy * dy).sqrt();
            }
        }
        RIM_TYPEKEYBOARD => {
            let keyboard = unsafe { raw.data.keyboard };
            let key = (keyboard.VKey as usize).min(255);
            if key == 255 {
                return; // 0xFF marks fake keys some drivers send alongside real ones
            }
            if keyboard.Flags & KEY_BREAK != 0 {
                keys_down[key] = false;
            } else if !keys_down[key] {
                keys_down[key] = true;
                counters.keys += 1;
            }
        }
        _ => {}
    }
}
