//! Optional ordinary GDI window. No layered styles, GPU context, or second renderer.
use super::*;
pub struct Capture {
    pub hwnd: HWND,
    pixels: Vec<u8>,
    width: i32,
    height: i32,
    buffer: Option<Surface>,
    parked: bool,
}
impl Capture {
    pub unsafe fn open() -> Result<Box<Self>, String> {
        let name = wide("MinecraftPetBroadcastCapture");
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of_val(&wc) as u32;
        wc.lpfnWndProc = Some(proc);
        wc.hInstance = GetModuleHandleW(null());
        wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        wc.lpszClassName = name.as_ptr();
        RegisterClassExW(&wc);
        let mut result = Box::new(Self { hwnd: null_mut(), pixels: Vec::new(), width: 0, height: 0, buffer: None, parked: false });
        result.hwnd = CreateWindowExW(0, name.as_ptr(), wide("Minecraft桌宠 · 直播采集（绿色背景）").as_ptr(),
            WS_POPUP, 64, 64, 450, 560,
            null_mut(), null_mut(), wc.hInstance, &mut *result as *mut _ as *const _);
        if result.hwnd.is_null() { return Err("无法创建直播采集窗口。".into()); }
        // Stable, entirely client-owned capture bounds in both placement modes.
        // Unsupported attributes are harmless on older Windows versions.
        let corner: u32 = 1; // DWMWCP_DONOTROUND
        let no_border: u32 = 0xfffffffe; // DWMWA_COLOR_NONE
        let no_nc: u32 = 1; // DWMNCRP_DISABLED
        for (attribute,value) in [(33,corner),(34,no_border),(2,no_nc)] {
            windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(result.hwnd,attribute,&value as *const _ as *const _,4);
        }
        result.place();
        ShowWindow(result.hwnd, SW_SHOWNORMAL);
        Ok(result)
    }
    pub unsafe fn update(&mut self, frame: &Frame) {
        if IsWindowVisible(self.hwnd) == 0 { return; }
        self.width = frame.width as i32; self.height = frame.height as i32;
        self.pixels.resize(frame.pixels.len(), 0);
        composite(&frame.pixels, &mut self.pixels);
        // WM_PAINT may have an empty clip when occluded. Submit the completed
        // client image explicitly as well; do not use PrintWindow to drive it.
        let dc=GetDC(self.hwnd);
        if !dc.is_null() { self.paint(dc); ReleaseDC(self.hwnd,dc); GdiFlush(); ValidateRect(self.hwnd,null()); }
    }
    pub unsafe fn toggle_placement(&mut self) {
        self.parked = !self.parked;
        self.place();
    }
    unsafe fn place(&self) {
        let mut info:MONITORINFO=std::mem::zeroed();info.cbSize=std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(MonitorFromWindow(self.hwnd,MONITOR_DEFAULTTONEAREST),&mut info);
        let mut current:RECT=std::mem::zeroed();GetWindowRect(self.hwnd,&mut current);
        let width=(current.right-current.left).min(info.rcWork.right-info.rcWork.left);
        let height=(current.bottom-current.top).min(info.rcWork.bottom-info.rcWork.top);
        let px=info.rcWork.left+(info.rcWork.right-info.rcWork.left-width)/2;
        let py=info.rcWork.top+(info.rcWork.bottom-info.rcWork.top-height)/2;
        SetWindowPos(self.hwnd,HWND_NOTOPMOST,px,py,width,height,SWP_NOACTIVATE);
        SetWindowPos(self.hwnd,if self.parked {HWND_BOTTOM}else{HWND_TOP},0,0,0,0,
            SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE);
    }
    pub unsafe fn bring_forward(&mut self) { self.parked=false;self.place(); }
    unsafe fn paint(&mut self, target: HDC) {
        let mut rc: RECT = std::mem::zeroed(); GetClientRect(self.hwnd, &mut rc);
        if rc.right <= 0 || rc.bottom <= 0 { return; }
        if self.buffer.as_ref().map(|s| (s.w,s.h)) != Some((rc.right,rc.bottom)) {
            self.buffer = Some(Surface::new(rc.right,rc.bottom));
        }
        let dc = self.buffer.as_ref().unwrap().dc;
        let brush = CreateSolidBrush(0x0000ff00); FillRect(dc, &rc, brush); DeleteObject(brush);
        if !self.pixels.is_empty() {
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = self.width; info.bmiHeader.biHeight = -self.height;
        info.bmiHeader.biPlanes = 1; info.bmiHeader.biBitCount = 32;
        let scale = (rc.right as f64 / self.width as f64).min(rc.bottom as f64 / self.height as f64);
        let w = (self.width as f64 * scale) as i32; let h = (self.height as f64 * scale) as i32;
        SetStretchBltMode(dc, COLORONCOLOR);
        StretchDIBits(dc, (rc.right-w)/2, (rc.bottom-h)/2, w, h, 0, 0, self.width, self.height,
            self.pixels.as_ptr() as *const _, &info, DIB_RGB_COLORS, SRCCOPY);
        }
        // Only the completed frame reaches the window/capture DC.
        BitBlt(target,0,0,rc.right,rc.bottom,dc,0,0,SRCCOPY);
    }
}
fn composite(source: &[u8], dest: &mut [u8]) {
    // The renderer supplies premultiplied BGRA: source-over an opaque green matte.
    for (s,d) in source.chunks_exact(4).zip(dest.chunks_exact_mut(4)) {
        d.copy_from_slice(&[s[0], s[1].saturating_add(255-s[3]), s[2], 255]);
    }
}
unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_NCCREATE {
        let create = &*(l as *const CREATESTRUCTW);
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        (*(create.lpCreateParams as *mut Capture)).hwnd = hwnd;
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut Capture;
    if !ptr.is_null() { match msg {
        WM_PAINT => {
            let mut ps = std::mem::zeroed(); let dc = BeginPaint(hwnd, &mut ps);
            (*ptr).paint(dc); EndPaint(hwnd, &ps); return 0;
        }
        WM_PRINT | WM_PRINTCLIENT => { (*ptr).paint(w as HDC); return 0; }
        WM_ERASEBKGND => return 1,
        WM_NCHITTEST => {
            let mut rc:RECT=std::mem::zeroed();GetWindowRect(hwnd,&mut rc);
            let x=(l as u32 & 0xffff) as i16 as i32;
            let y=((l as u32 >> 16)&0xffff) as i16 as i32;
            let edge=(8*GetDpiForWindow(hwnd)/96).max(6) as i32;
            let left=x<rc.left+edge;let right=x>=rc.right-edge;
            let top=y<rc.top+edge;let bottom=y>=rc.bottom-edge;
            return match (left,right,top,bottom) {
                (true,_,true,_)=>HTTOPLEFT,(_,true,true,_)=>HTTOPRIGHT,
                (true,_,_,true)=>HTBOTTOMLEFT,(_,true,_,true)=>HTBOTTOMRIGHT,
                (true,_,_,_)=>HTLEFT,(_,true,_,_)=>HTRIGHT,
                (_,_,true,_)=>HTTOP,(_,_,_,true)=>HTBOTTOM,
                _=>HTCAPTION,
            } as isize;
        }
        WM_GETMINMAXINFO => {
            let info=&mut *(l as *mut MINMAXINFO);
            info.ptMinTrackSize=POINT{x:160,y:180};return 0;
        }
        WM_SIZE => { InvalidateRect(hwnd,null(),0);return 0; }
        WM_CLOSE => { ShowWindow(hwnd, SW_HIDE); (*ptr).pixels = Vec::new(); (*ptr).buffer = None; return 0; }
        WM_DISPLAYCHANGE => { if (*ptr).parked { (*ptr).place(); } }
        WM_NCDESTROY => { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0); }
        _ => {}
    }}
    DefWindowProcW(hwnd,msg,w,l)
}
impl Drop for Capture { fn drop(&mut self) { unsafe { DestroyWindow(self.hwnd); } } }
#[cfg(test)] mod tests {
    #[test] fn green_background_preserves_premultiplied_edges() {
        let mut out=[0;12]; super::composite(&[0,0,0,0, 10,20,30,255, 40,50,60,128],&mut out);
        assert_eq!(out,[0,255,0,255,10,20,30,255,40,177,60,255]);
    }
}
