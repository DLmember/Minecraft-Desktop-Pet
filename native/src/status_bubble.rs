use crate::wide;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    ptr::{null, null_mut},
    time::{Duration, SystemTime},
};
use windows_sys::Win32::{
    Foundation::*, Graphics::Gdi::*, System::LibraryLoader::GetModuleHandleW,
    UI::{HiDpi::GetDpiForWindow, WindowsAndMessaging::*},
};

const MAX_BYTES: u64 = 4096;
const MAX_AGE: Duration = Duration::from_secs(30);

fn read_status(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let modified = file.metadata().ok()?.modified().ok()?;
    if SystemTime::now().duration_since(modified).unwrap_or_default() > MAX_AGE {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    let lines: Vec<String> = text
        .trim_start_matches('\u{feff}')
        .lines()
        .map(|line| line.trim())
        .map(|line| line.chars().filter(|ch| !ch.is_control()).take(80).collect())
        .filter(|line: &String| !line.is_empty())
        .take(8)
        .collect();
    Some(lines.join("\n"))
}

pub struct Bubble {
    path: PathBuf,
    hwnd: HWND,
    next_read: f64,
    text: String,
    alpha: u8,
    layout: Option<[i32; 4]>,
}

impl Bubble {
    pub fn new(path: PathBuf) -> Self {
        Self { path, hwnd: null_mut(), next_read: 0., text: String::new(), alpha: 0, layout: None }
    }

    pub unsafe fn update(&mut self, pet: HWND, now: f64, hidden: bool, alpha: u8) {
        if now >= self.next_read {
            self.next_read = now + 0.5;
            let text = read_status(&self.path).unwrap_or_default();
            if self.text != text {
                self.text = text;
                if !self.hwnd.is_null() {
                    SetWindowTextW(self.hwnd, wide(&self.text).as_ptr());
                    InvalidateRect(self.hwnd, null(), 1);
                }
            }
        }
        if hidden || alpha == 0 || self.text.is_empty() {
            if self.layout.take().is_some() {
                ShowWindow(self.hwnd, SW_HIDE);
            }
            return;
        }
        if self.hwnd.is_null() {
            self.hwnd = create_window(pet, &self.text);
            if self.hwnd.is_null() {
                return;
            }
        }
        if self.alpha != alpha {
            SetLayeredWindowAttributes(self.hwnd, 0, alpha, LWA_ALPHA);
            self.alpha = alpha;
        }
        let scale = GetDpiForWindow(pet).max(96) as f32 / 96.;
        let mut rect: RECT = std::mem::zeroed();
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        if GetWindowRect(pet, &mut rect) == 0
            || GetMonitorInfoW(MonitorFromWindow(pet, MONITOR_DEFAULTTONEAREST), &mut info) == 0
        {
            return;
        }
        let work = info.rcWork;
        let width = ((360. * scale) as i32).min(work.right - work.left);
        let height = ((20. + 19. * self.text.lines().count() as f32) * scale) as i32;
        let x = ((rect.left + rect.right - width) / 2).clamp(work.left, work.right - width);
        let above = rect.top - height - 4;
        let below = rect.bottom + 4;
        let y = if above < work.top && below + height <= work.bottom { below } else { above }
            .clamp(work.top, (work.bottom - height).max(work.top));
        let layout = [x, y, width, height];
        if self.layout != Some(layout) {
            if SetWindowPos(self.hwnd, null_mut(), x, y, width, height,
                SWP_NOACTIVATE | SWP_NOZORDER | SWP_SHOWWINDOW) != 0
            {
                self.layout = Some(layout);
            }
        }
    }
}

unsafe fn create_window(pet: HWND, text: &str) -> HWND {
    let class = wide("MinecraftDesktopPetStatusBubble");
    let instance = GetModuleHandleW(null());
    RegisterClassW(&WNDCLASSW {
        hInstance: instance,
        lpszClassName: class.as_ptr(),
        lpfnWndProc: Some(window_proc),
        ..std::mem::zeroed()
    });
    CreateWindowExW(
        WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_LAYERED,
        class.as_ptr(), wide(text).as_ptr(), WS_POPUP,
        0, 0, 0, 0, pet, null_mut(), instance, null(),
    )
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg != WM_PAINT {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let mut paint: PAINTSTRUCT = std::mem::zeroed();
    let dc = BeginPaint(hwnd, &mut paint);
    let mut rect: RECT = std::mem::zeroed();
    GetClientRect(hwnd, &mut rect);
    let background = GetStockObject(LTGRAY_BRUSH);
    FillRect(dc, &rect, background as HBRUSH);
    FrameRect(dc, &rect, GetStockObject(BLACK_BRUSH) as HBRUSH);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, 0x202020);
    let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.;
    let face = wide("Microsoft YaHei UI");
    let font = CreateFontW(
        -(15. * scale) as i32, 0, 0, 0, FW_NORMAL as i32, 0, 0, 0,
        DEFAULT_CHARSET as u32, 0, 0, 0, 0, face.as_ptr(),
    );
    let previous = SelectObject(dc, font as HGDIOBJ);
    let mut text = [0u16; 1290];
    let count = GetWindowTextW(hwnd, text.as_mut_ptr(), text.len() as i32);
    rect.left += (8. * scale) as i32;
    rect.right -= (8. * scale) as i32;
    rect.top += (10. * scale) as i32;
    let line_height = (19. * scale) as i32;
    for line in text[..count as usize].split(|unit| *unit == b'\n' as u16) {
        rect.bottom = rect.top + line_height;
        DrawTextW(dc, line.as_ptr(), line.len() as i32, &mut rect,
            DT_CENTER | DT_NOPREFIX | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
        rect.top = rect.bottom;
    }
    SelectObject(dc, previous);
    DeleteObject(font as HGDIOBJ);
    EndPaint(hwnd, &paint);
    0
}
