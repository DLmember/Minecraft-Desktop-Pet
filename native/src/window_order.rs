use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
    UI::WindowsAndMessaging::*,
};

unsafe fn visible_on_desktop(hwnd: HWND) -> bool {
    if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 {
        return false;
    }
    // Other virtual desktops can retain WS_VISIBLE while cloaked.
    let mut cloaked = 0u32;
    DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED as u32,
        &mut cloaked as *mut _ as *mut _,
        std::mem::size_of_val(&cloaked) as u32,
    );
    cloaked == 0
}

pub unsafe fn maintain(hwnd: HWND, visible_bounds: RECT) {
    if hwnd.is_null() || !visible_on_desktop(hwnd) {
        return;
    }
    let mut repair = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST == 0;
    if !repair {
        let mut other = GetWindow(hwnd, GW_HWNDPREV);
        // Bound traversal because windows may disappear while walking the list.
        for _ in 0..256 {
            if other.is_null() || other == hwnd {
                break;
            }
            let mut pid = 0;
            GetWindowThreadProcessId(other, &mut pid);
            if pid != std::process::id() && visible_on_desktop(other) {
                let mut rect: RECT = std::mem::zeroed();
                if GetWindowRect(other, &mut rect) != 0
                    && rect.left < visible_bounds.right
                    && rect.right > visible_bounds.left
                    && rect.top < visible_bounds.bottom
                    && rect.bottom > visible_bounds.top
                {
                    repair = true;
                    break;
                }
            }
            other = GetWindow(other, GW_HWNDPREV);
        }
    }
    if repair {
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}
