use std::{
    cell::RefCell,
    sync::atomic::{AtomicBool, Ordering},
};
use windows_sys::Win32::{
    Foundation::*,
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

pub const MESSAGE: u32 = WM_APP + 12;
const TAG: usize = 0x4D435048;
pub const TEXT: &str = "/p chat ";
static ENABLED: AtomicBool = AtomicBool::new(false);
thread_local! {
    static SWALLOWED: RefCell<[bool; 256]> = const { RefCell::new([false; 256]) };
}
pub fn enable(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}
pub fn has_pending_keyup() -> bool {
    SWALLOWED.with(|s| s.borrow().iter().any(|x| *x))
}
pub fn own_input(k: &KBDLLHOOKSTRUCT) -> bool {
    k.dwExtraInfo == TAG
}

unsafe fn is_hash(k: &KBDLLHOOKSTRUCT, foreground: HWND) -> bool {
    if k.vkCode >= 256 || foreground.is_null() {
        return false;
    }
    // Shortcut combinations must retain their original meaning.
    for vk in [VK_CONTROL, VK_MENU, VK_LWIN, VK_RWIN] {
        if GetAsyncKeyState(vk as i32) < 0 {
            return false;
        }
    }
    let mut state = [0u8; 256];
    state[VK_SHIFT as usize] =
        if GetAsyncKeyState(VK_LSHIFT as i32) < 0 || GetAsyncKeyState(VK_RSHIFT as i32) < 0 {
            0x80
        } else {
            0
        };
    state[VK_CAPITAL as usize] = (GetKeyState(VK_CAPITAL as i32) & 1) as u8;
    state[k.vkCode as usize] |= 0x80;
    let layout = GetKeyboardLayout(GetWindowThreadProcessId(foreground, std::ptr::null_mut()));
    translates_hash(k.vkCode, k.scanCode, &state, layout)
}

unsafe fn translates_hash(vk: u32, scan: u32, state: &[u8; 256], layout: HKL) -> bool {
    let mut chars = [0u16; 8];
    // Flag 4 does not modify dead-key/keyboard translation state (Windows 10+).
    ToUnicodeEx(vk, scan, state.as_ptr(), chars.as_mut_ptr(), 8, 4, layout) == 1
        && chars[0] == b'#' as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_character_in_foreground_layout_without_consuming_dead_keys() {
        unsafe {
            let layout = LoadKeyboardLayoutW(crate::wide("00000409").as_ptr(), 0);
            assert!(!layout.is_null());
            let mut state = [0u8; 256];
            let scan = MapVirtualKeyExW(0x33, 0, layout);
            assert!(!translates_hash(0x33, scan, &state, layout));
            state[VK_SHIFT as usize] = 0x80;
            assert!(translates_hash(0x33, scan, &state, layout));
            state[VK_CAPITAL as usize] = 1;
            assert!(translates_hash(0x33, scan, &state, layout));
            assert!(!translates_hash(
                0x32,
                MapVirtualKeyExW(0x32, 0, layout),
                &state,
                layout
            ));
            // UK # is its own key, not Shift+3. Match text rather than a US shortcut.
            let uk = LoadKeyboardLayoutW(crate::wide("00000809").as_ptr(), 0);
            assert!(!uk.is_null());
            let mapped = VkKeyScanExW(b'#' as u16, uk);
            assert!(mapped >= 0);
            let vk = mapped as u32 & 255;
            let mut state = [0u8; 256];
            if mapped & 0x100 != 0 {
                state[VK_SHIFT as usize] = 0x80;
            }
            assert!(translates_hash(vk, MapVirtualKeyExW(vk, 0, uk), &state, uk));
        }
    }

    #[test]
    fn queued_replacement_respects_disabled_setting_and_focus() {
        unsafe {
            enable(false);
            assert!(insert(std::ptr::null_mut()).is_ok());
            enable(true);
            assert!(insert(std::ptr::null_mut()).is_err());
            enable(false);
        }
        assert_eq!(TEXT, "/p chat ");
        assert!(!TEXT.contains(['\r', '\n']));
    }
}

pub unsafe fn intercept(k: &KBDLLHOOKSTRUCT, down: bool, hwnd: HWND) -> bool {
    if own_input(k) || k.vkCode >= 256 {
        return false;
    }
    let index = k.vkCode as usize;
    if SWALLOWED.with(|s| {
        let mut keys = s.borrow_mut();
        let swallowed = keys[index];
        if !down {
            keys[index] = false;
        }
        swallowed
    }) {
        // Holding # produces one replacement, with its matching key-up consumed.
        return true;
    }
    if !down || !ENABLED.load(Ordering::Relaxed) {
        return false;
    }
    let foreground = GetForegroundWindow();
    if !is_hash(k, foreground) {
        return false;
    }
    if PostMessageW(hwnd, MESSAGE, foreground as usize, 0) == 0 {
        return false;
    }
    SWALLOWED.with(|s| s.borrow_mut()[index] = true);
    true
}

pub unsafe fn insert(foreground: HWND) -> Result<(), &'static str> {
    if !ENABLED.load(Ordering::Relaxed) {
        return Ok(());
    }
    if foreground.is_null() || GetForegroundWindow() != foreground {
        return Err("聊天前缀未输入：输入窗口已切换，请重新输入 #。");
    }
    let mut inputs = Vec::with_capacity(TEXT.len() * 2);
    for ch in TEXT.encode_utf16() {
        for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
            let mut input: INPUT = std::mem::zeroed();
            input.r#type = INPUT_KEYBOARD;
            input.Anonymous.ki = KEYBDINPUT {
                wVk: 0,
                wScan: ch,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: TAG,
            };
            inputs.push(input);
        }
    }
    // Unicode input ignores a physically held Shift/Caps Lock. No clipboard,
    // synthetic modifier release, or Enter press is needed.
    if SendInput(
        inputs.len() as u32,
        inputs.as_ptr(),
        std::mem::size_of::<INPUT>() as i32,
    ) != inputs.len() as u32
    {
        return Err("聊天前缀输入失败：请检查目标窗口是否以管理员权限运行。");
    }
    Ok(())
}
