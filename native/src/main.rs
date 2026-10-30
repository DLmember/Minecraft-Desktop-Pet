#![windows_subsystem = "windows"]
mod autostart;
mod config;
mod hyp_chat;
mod keyboard;
mod online;
mod render;
mod ui;
mod updates;
mod music;
mod theme;
mod window_order;
mod menu_clock;
use config::{Position, Settings, Store};
use glam::Vec2;
use render::{Cape, Frame, Pose, Skin};
use std::{
    path::PathBuf,
    ptr::{null, null_mut},
    time::Instant,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{LibraryLoader::GetModuleHandleW, Threading::*},
    UI::{Controls::*, HiDpi::*, Input::KeyboardAndMouse::*, Shell::*, WindowsAndMessaging::*},
};
const TRAY_MSG: u32 = WM_APP + 1;
const KEY_MSG: u32 = WM_APP + 2;
const SHOW_MSG: u32 = WM_APP + 10;
const SNAPSHOT_MSG: u32 = WM_APP + 11;
static mut APP: *mut App = null_mut();
static mut PET_HWND: HWND = null_mut();
// Diagnostic runs can isolate their injected test events from the user's typing.
static mut TEST_INPUT_ONLY: bool = false;
const VERSION: &str = env!("CARGO_PKG_VERSION");
pub fn product_title() -> String {
    format!("Minecraft桌宠 · v{}", VERSION.strip_suffix(".0").unwrap_or(VERSION))
}
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn report_error(s: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(s).as_ptr(),
            wide(&product_title()).as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    w: i32,
    h: i32,
    source_width: usize,
    source_columns: Vec<usize>,
}
impl Surface {
    unsafe fn new(w: i32, h: i32) -> Self {
        let dc = CreateCompatibleDC(null_mut());
        let mut info: BITMAPINFO = std::mem::zeroed();
        info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = w;
        info.bmiHeader.biHeight = -h;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB;
        let mut bits = null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        let old = SelectObject(dc, bitmap);
        Self {
            dc,
            bitmap,
            old,
            bits: bits as *mut u8,
            w,
            h,
            source_width: 0,
            source_columns: Vec::new(),
        }
    }
    unsafe fn update(&mut self, hwnd: HWND, frame: &Frame, pos: POINT, opacity: u8) {
        // Match the previous renderer's 1.25 DPR cap and nearest pixel sampling.
        if self.w as usize == frame.width && self.h as usize == frame.height {
            // Same size as the DIB: one block copy instead of ~150k per-pixel moves.
            std::ptr::copy_nonoverlapping(
                frame.pixels.as_ptr(),
                self.bits,
                frame.pixels.len(),
            );
            self.present(hwnd, pos, opacity);
            return;
        }
        if self.source_width != frame.width {
            self.source_columns = (0..self.w as usize)
                .map(|x| x * frame.width / self.w as usize)
                .collect();
            self.source_width = frame.width;
        }
        let mut last_sy = usize::MAX;
        for y in 0..self.h {
            let sy = y as usize * frame.height / self.h as usize;
            let dest = self.bits.add(y as usize * self.w as usize * 4);
            if sy == last_sy {
                std::ptr::copy_nonoverlapping(
                    dest.sub(self.w as usize * 4),
                    dest,
                    self.w as usize * 4,
                );
                continue;
            }
            last_sy = sy;
            for (x, &sx) in self.source_columns.iter().enumerate() {
                std::ptr::copy_nonoverlapping(
                    frame.pixels.as_ptr().add((sy * frame.width + sx) * 4),
                    dest.add(x * 4),
                    4,
                );
            }
        }
        self.present(hwnd, pos, opacity);
    }
    unsafe fn present(&self, hwnd: HWND, pos: POINT, opacity: u8) {
        let size = SIZE {
            cx: self.w,
            cy: self.h,
        };
        let origin = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: opacity,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        UpdateLayeredWindow(
            hwnd,
            null_mut(),
            &pos,
            &size,
            self.dc,
            &origin,
            0,
            &blend,
            ULW_ALPHA,
        );
    }
}
impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}
pub struct App {
    settings: Settings,
    store: Store,
    skin: Skin,
    cape: Option<Cape>,
    pose: Pose,
    key_recovery: keyboard::KeyRecovery,
    frame: Frame,
    surface: Option<Surface>,
    opacity: u8,
    opacity_from: f32,
    opacity_target: u8,
    opacity_started: f64,
    hwnd: HWND,
    panel: HWND,
    hook: HHOOK,
    download: Option<std::sync::mpsc::Receiver<Result<online::Download, String>>>,
    online_id: String,
    online_notice: String,
    icon: HICON,
    font: HFONT,
    controls: Vec<(i32, HWND)>,
    status: HWND,
    pos: POINT,
    dpi: f32,
    width: i32,
    height: i32,
    drag: Option<POINT>,
    hidden: bool,
    start: Instant,
    last: f32,
    awake: f32,
    cursor: POINT,
    shift: [bool; 2],
    notice: String,
    menu_open: bool,
    taskbar_msg: u32,
    panel_scale: f32,
    layout: Vec<(HWND, [i32; 4])>,
    page_controls: Vec<(HWND, u8)>,
    panel_page: u8,
    building_page: u8,
    slider_pending: bool,
    resizing: bool,
    render_count: u64,
    raster_us: u64,
    present_us: u64,
    tick_last: f64,
    tick_gap_max_us: u64,
    tick_gap_sum_us: u64,
    tick_gap_count: u64,
    click_count: u64,
    mouse_buttons: [bool; 5],
    tray_ok: bool,
    tray_error: u32,
    tray_retry: u8,
    tray_retry_at: f64,
    last_draw: f64,
    next_draw: f64,
    next_topmost_check: f64,
    topmost_foreground: HWND,
    music: music::Monitor,
    panel_dark: bool,
    theme_override: Option<bool>,
    github_images: HIMAGELIST,
    update_check: Option<std::sync::mpsc::Receiver<Result<String,String>>>,
    diagnostic: bool,
    input_trace: Vec<serde_json::Value>,
}
impl App {
    fn new(dir: PathBuf, diagnostic: bool) -> Self {
        let mut store = Store::new(dir);
        let settings = store.load();
        let bytes =
            std::fs::read(&settings.skin_path).unwrap_or_else(|_| render::DEFAULT_SKIN.to_vec());
        let skin = Skin::load(&bytes).unwrap_or_else(|_| Skin::load(render::DEFAULT_SKIN).unwrap());
        let cape = load_cape(&settings);
        let pose = Pose::new(&settings);
        let notice = store.notice.clone();
        Self {
            settings,
            store,
            skin,
            cape,
            pose,
            key_recovery: keyboard::KeyRecovery::default(),
            frame: Frame::new(450, 525),
            surface: None,
            opacity: 255,
            opacity_from: 255.,
            opacity_target: 255,
            opacity_started: 0.,
            hwnd: null_mut(),
            panel: null_mut(),
            hook: null_mut(),
            download: None,
            online_id: String::new(),
            online_notice: String::new(),
            icon: null_mut(),
            font: null_mut(),
            controls: Vec::new(),
            status: null_mut(),
            pos: POINT { x: 0, y: 0 },
            dpi: 1.,
            width: 360,
            height: 420,
            drag: None,
            hidden: false,
            start: Instant::now(),
            last: 0.,
            awake: 1.,
            cursor: POINT { x: i32::MIN, y: 0 },
            shift: [false; 2],
            notice,
            menu_open: false,
            taskbar_msg: 0,
            panel_scale: 1.,
            layout: Vec::new(),
            page_controls: Vec::new(),
            panel_page: 0,
            building_page: 3,
            slider_pending: false,
            resizing: false,
            render_count: 0,
            raster_us: 0,
            present_us: 0,
            tick_last: 0.,
            tick_gap_max_us: 0,
            tick_gap_sum_us: 0,
            tick_gap_count: 0,
            click_count: 0,
            mouse_buttons: [false; 5],
            tray_ok: false,
            tray_error: 0,
            tray_retry: 0,
            tray_retry_at: 0.,
            last_draw: 0.,
            next_draw: 0.,
            next_topmost_check: 0.,
            topmost_foreground: null_mut(),
            music: music::Monitor::new(),
            panel_dark: false,
            theme_override: None,
            github_images: 0,
            update_check: None,
            diagnostic,
            input_trace: Vec::new(),
        }
    }
    fn save(&mut self) {
        self.slider_pending = false;
        match self.store.save(&self.settings) {
            Ok(()) => self.notice = "已保存".into(),
            Err(e) => self.notice = format!("保存失败：{e}"),
        };
        unsafe {
            if !self.status.is_null() {
                SetWindowTextW(self.status, wide(&self.notice).as_ptr());
            }
        }
    }
    fn reset_keys(&mut self) {
        self.pose.reset_keys();
        self.key_recovery.clear();
    }
    fn wake(&mut self) {
        self.awake = self.start.elapsed().as_secs_f32() + 0.85;
    }
    unsafe fn geometry(&mut self) {
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        let monitor = if self.hwnd.is_null() {
            MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST)
        } else {
            MonitorFromWindow(self.hwnd, MONITOR_DEFAULTTONEAREST)
        };
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        let mut dx = 96;
        let mut dy = 96;
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        self.dpi = dx as f32 / 96.;
        let fit = self
            .settings
            .scale
            .min((info.rcWork.right - info.rcWork.left) as f32 / (360. * self.dpi))
            .min(((info.rcWork.bottom - info.rcWork.top - 8) as f32) / (420. * self.dpi));
        self.width = (360. * fit * self.dpi).round() as i32;
        self.height = (420. * fit * self.dpi).round() as i32;
        self.surface = Some(Surface::new(self.width, self.height));
        self.frame = Frame::new(
            (360. * fit * self.dpi.min(1.25)).round() as usize,
            (420. * fit * self.dpi.min(1.25)).round() as usize,
        );
        if self.hwnd.is_null() {
            self.pos = if let Some(p) = &self.settings.position {
                POINT {
                    x: (p.x * self.dpi).round() as i32,
                    y: (p.y * self.dpi).round() as i32,
                }
            } else {
                POINT {
                    x: info.rcWork.right - self.width - 48,
                    y: info.rcWork.bottom - self.height - 32,
                }
            };
        }
    }
    unsafe fn constrain(&mut self) {
        let center = POINT {
            x: self.pos.x + self.width / 2,
            y: self.pos.y + self.height / 2,
        };
        let monitor = MonitorFromPoint(center, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        let b = self.frame.bounds;
        let sx = self.width as f32 / self.frame.width as f32;
        let sy = self.height as f32 / self.frame.height as f32;
        let left = (b[0] as f32 * sx) as i32 - 3;
        let top = (b[1] as f32 * sy) as i32 - 3;
        let right = (b[2] as f32 * sx) as i32 + 3;
        let bottom = (b[3] as f32 * sy) as i32 + 3;
        self.pos.x = self.pos.x.clamp(
            info.rcWork.left - left,
            (info.rcWork.right - right).max(info.rcWork.left - left),
        );
        self.pos.y = self.pos.y.clamp(
            info.rcWork.top - top,
            (info.rcWork.bottom - bottom).max(info.rcWork.top - top),
        );
    }
    unsafe fn move_to(&mut self) {
        SetWindowPos(
            self.hwnd,
            null_mut(),
            self.pos.x,
            self.pos.y,
            self.width,
            self.height,
            SWP_NOACTIVATE | SWP_NOZORDER,
        );
    }
    fn save_position(&mut self) {
        self.settings.position = Some(Position {
            x: self.pos.x as f32 / self.dpi,
            y: self.pos.y as f32 / self.dpi,
        });
        self.save();
    }
    unsafe fn draw(&mut self, now: f32, dt: f32) {
        let logical = Vec2::new(
            (self.cursor.x - self.pos.x) as f32 / self.dpi,
            (self.cursor.y - self.pos.y) as f32 / self.dpi,
        );
        self.pose.update(
            &self.settings,
            &self.skin,
            logical,
            self.width as f32 / self.dpi,
            self.height as f32 / self.dpi,
            self.shift.iter().any(|x| *x),
            now,
            dt,
        );
        let raster_start = Instant::now();
        self.frame
            .draw_ext(&self.settings, &self.skin, self.cape.as_ref(), &self.pose);
        self.advance_opacity();
        self.raster_us += raster_start.elapsed().as_micros() as u64;
        let present_start = Instant::now();
        if let Some(surface) = &mut self.surface {
            surface.update(self.hwnd, &self.frame, self.pos, self.opacity);
        }
        self.present_us += present_start.elapsed().as_micros() as u64;
        self.render_count += 1;
    }
    unsafe fn mouse_press(&mut self,index:usize,cursor:POINT,now:f32) {
        self.click_count+=1;
        if self.settings.tease_enabled && self.pet_hit(cursor) {self.pose.tease(now);}
        else {self.pose.click(now,self.settings.swing_left(index));}
        self.wake();
    }
    unsafe fn native_mouse_press(&mut self,index:usize) {
        if TEST_INPUT_ONLY || self.mouse_buttons[index] {return;}
        let mut cursor=POINT{x:0,y:0};GetCursorPos(&mut cursor);
        self.mouse_press(index,cursor,self.start.elapsed().as_secs_f32());
        self.mouse_buttons[index]=true;
    }
    unsafe fn pet_hit(&self,cursor:POINT)->bool {
        if self.hidden || self.menu_open || self.width<=0 || self.height<=0 {return false;}
        if !self.panel.is_null() && GetAncestor(WindowFromPoint(cursor),GA_ROOT)==self.panel {return false;}
        let x=cursor.x-self.pos.x;let y=cursor.y-self.pos.y;
        if x<0 || y<0 || x>=self.width || y>=self.height {return false;}
        let sx=x as usize*self.frame.width/self.width as usize;
        let sy=y as usize*self.frame.height/self.height as usize;
        self.frame.pixels[(sy*self.frame.width+sx)*4+3]>32
    }
    fn hover_opacity(&self) -> u8 {
        if !self.settings.click_through || self.width <= 0 || self.height <= 0 {
            return 255;
        }
        let x = self.cursor.x as i64 - self.pos.x as i64;
        let y = self.cursor.y as i64 - self.pos.y as i64;
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return 255;
        }
        // Match Surface's nearest-pixel sampling, using the original alpha.
        // Transparent margins and holes do not count as hovering the pet.
        let sx = x as usize * self.frame.width / self.width as usize;
        let sy = y as usize * self.frame.height / self.height as usize;
        if self.frame.pixels[(sy * self.frame.width + sx) * 4 + 3] != 0 {
            64 // 25% rounded to the 8-bit alpha supported by Windows.
        } else {
            255
        }
    }
    unsafe fn update_hover_opacity(&mut self) {
        let previous = self.opacity;
        self.advance_opacity();
        if previous != self.opacity {
            if let Some(surface) = &self.surface {
                // Reuse the existing bitmap; hovering does not rerasterize the
                // character or wake its animation when head tracking is off.
                surface.present(self.hwnd, self.pos, self.opacity);
            }
        }
    }
    fn advance_opacity(&mut self) {
        let now = self.start.elapsed().as_secs_f64();
        let t = ((now - self.opacity_started) / 0.15).clamp(0., 1.) as f32;
        let eased = t * t * (3. - 2. * t);
        let value = self.opacity_from + (self.opacity_target as f32 - self.opacity_from) * eased;
        let target = self.hover_opacity();
        if target != self.opacity_target {
            self.opacity_from = value;
            self.opacity_target = target;
            self.opacity_started = now;
        }
        self.opacity = value.round() as u8;
    }
    unsafe fn tick(&mut self) {
        let clock = self.start.elapsed().as_secs_f64();
        let now = clock as f32;
        if self.tick_last > 0. {
            let gap = ((clock - self.tick_last) * 1e6).max(0.) as u64;
            self.tick_gap_sum_us += gap;
            self.tick_gap_count += 1;
            self.tick_gap_max_us = self.tick_gap_max_us.max(gap);
        }
        self.tick_last = clock;
        self.poll_download();
        self.poll_update();
        self.music.set_enabled(self.settings.music_notes);
        self.pose.music_playing = self.music.playing();
        self.maintain_topmost(clock);
        // Explorer may still be starting when the pet launches; retry a few
        // times instead of leaving the user without a tray entry.
        if !self.tray_ok && self.tray_retry < 5 && clock >= self.tray_retry_at {
            self.tray_retry += 1;
            self.tray_retry_at = clock + 1.;
            self.tray_ok = self.tray(NIM_ADD);
            if self.tray_ok {
                self.tray_error = 0;
            } else {
                self.tray_error = GetLastError();
                if self.tray_retry == 5 {
                    let _ = std::fs::write(
                        self.store.dir.join("tray-error.txt"),
                        format!("error={} icon={}\n", self.tray_error, self.icon as isize),
                    );
                }
            }
        }
        let mut cursor = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor);
        // Poll the buttons instead of installing WH_MOUSE_LL. A low-level mouse
        // hook runs synchronously on its installing thread, so every cursor
        // sample would wait for the current frame and the whole desktop would
        // feel sticky. The frame timer already ticks at 120 Hz and is enough to
        // catch any human click as a fresh press.
        let buttons = [
            GetAsyncKeyState(VK_LBUTTON as i32) < 0,
            GetAsyncKeyState(VK_RBUTTON as i32) < 0,
            GetAsyncKeyState(VK_MBUTTON as i32) < 0,
            GetAsyncKeyState(VK_XBUTTON1 as i32) < 0,
            GetAsyncKeyState(VK_XBUTTON2 as i32) < 0,
        ];
        for (index, down) in buttons.iter().enumerate() {
            if *down && !self.mouse_buttons[index] && !TEST_INPUT_ONLY {
                self.mouse_press(index,cursor,now);
            }
        }
        self.mouse_buttons = buttons;
        let moved = cursor.x != self.cursor.x || cursor.y != self.cursor.y;
        self.cursor = cursor;
        if moved && self.settings.track_mouse {

            self.awake = self.awake.max(now + 0.05);
        } // Direct head updates need no trailing animation.
        if let Some(offset) = self.drag {

            self.pos = POINT {
                x: cursor.x - offset.x,
                y: cursor.y - offset.y,
            };
            self.move_to();
            self.awake = now + 0.85;
        }
        for vk in self
            .key_recovery
            .releases(&self.pose.held_codes(), clock, |physical| {
                GetAsyncKeyState(physical as i32) < 0
            })
        {
            self.trace_key(vk, false, "confirmed-async-release");
            self.pose.key(vk, false, now, &self.settings);
            self.wake();
        }
        let shift = [
            GetAsyncKeyState(VK_LSHIFT as i32) < 0,
            GetAsyncKeyState(VK_RSHIFT as i32) < 0,
        ];
        if shift != self.shift {
            self.shift = shift;

            self.wake();
        }
        let dt = (clock - self.last_draw).min(0.05) as f32;
        self.last = now;
        // 静止时项目会停止重绘以省 CPU；眨眼是时间轴动画，必须在这时也允许出帧，
        // 否则桌宠不动就永远不会眨眼。潜行扭动的计时同样靠每帧推进，按住 Shift
        // 期间也要保持出帧，眨眼和扭动之外仍然不绘制，空闲开销不变。
        let blinking = self.settings.da_blink && self.skin.eyes.is_some()
            && (self.pose.blink_at(now) > 0.0001 || self.pose.eyelid < 0.9999);
        let dancing = (self.settings.da_dance && self.settings.posture == "standing") || self.pose.music_playing || self.pose.needs_frames() || self.settings.breathing_animation;
        if !self.hidden && (now <= self.awake || blinking || dancing || self.pose.gesture_due(&self.settings,now)) && clock >= self.next_draw {
            self.draw(now, dt);
            self.last_draw = clock;
            let period = 1. / self.settings.fps as f64;
            self.next_draw += period;
            if self.next_draw <= clock {
                self.next_draw = clock + period;
            }
        }
        if !self.hidden {
            self.update_hover_opacity();
        }
    }
    unsafe fn trace_key(&mut self, vk: u32, down: bool, source: &str) {
        if !self.diagnostic {
            return;
        }
        if self.input_trace.len() >= 512 {
            self.input_trace.remove(0);
        }
        let physical = keyboard::physical_key(vk);
        self.input_trace.push(serde_json::json!({
            "time": self.start.elapsed().as_secs_f64(),
            "vk": vk, "down": down, "source": source,
            "asyncDown": GetAsyncKeyState(physical as i32) < 0,
            "heldBefore": self.pose.held_codes(),
        }));
    }
    unsafe fn flags(&mut self) {
        self.next_topmost_check = 0.;
        let mut flags = GetWindowLongPtrW(self.hwnd, GWL_EXSTYLE) as u32;
        if self.settings.click_through {
            flags |= WS_EX_TRANSPARENT;
        } else {
            flags &= !WS_EX_TRANSPARENT;
        }
        // 32 位 Windows 上 SetWindowLongPtrW 即 SetWindowLongW，参数为 i32
        SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, flags as _);
        GetCursorPos(&mut self.cursor);
        self.update_hover_opacity();
        SetWindowPos(
            self.hwnd,
            if self.settings.always_on_top {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            },
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
    unsafe fn maintain_topmost(&mut self, clock: f64) {
        if !self.settings.always_on_top || self.hidden || self.menu_open {
            return;
        }
        let foreground = GetForegroundWindow();
        if clock < self.next_topmost_check && foreground == self.topmost_foreground {
            return;
        }
        self.next_topmost_check = clock + 0.25;
        self.topmost_foreground = foreground;
        // Check independently of animation. Transparent margins do not count.
        let b = self.frame.bounds;
        let sx = self.width as f32 / self.frame.width as f32;
        let sy = self.height as f32 / self.frame.height as f32;
        window_order::maintain(
            self.hwnd,
            RECT {
                left: self.pos.x + (b[0] as f32 * sx).floor() as i32,
                top: self.pos.y + (b[1] as f32 * sy).floor() as i32,
                right: self.pos.x + (b[2] as f32 * sx).ceil() as i32,
                bottom: self.pos.y + (b[3] as f32 * sy).ceil() as i32,
            },
        );
    }
    unsafe fn hook(&mut self) {
        hyp_chat::enable(self.settings.hyp_quick_chat);
        let needed = self.settings.keyboard_enabled
            || self.settings.hyp_quick_chat
            || hyp_chat::has_pending_keyup();
        if !needed && !self.hook.is_null() {
            UnhookWindowsHookEx(self.hook);
            self.hook = null_mut();
        }
        if needed && self.hook.is_null() {
            self.hook = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(keyboard_hook),
                GetModuleHandleW(null()),
                0,
            );
            if self.hook.is_null() {
                self.notice = "全局键盘监听未启动，请关闭后重新开启。".into();
            }
        }
        self.reset_keys();
    }
    unsafe fn changed(&mut self) {
        self.settings.clean();
        self.save();
        self.flags();
        self.wake();
        self.refresh_panel();
    }
    unsafe fn import(&mut self, path: PathBuf) {
        self.download = None; // A later local choice wins over an older network request.
        self.online_notice.clear();
        let result = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| Skin::load(&bytes).map(|skin| (bytes, skin)));
        match result {
            Ok((bytes, skin)) => {
                let dest = self.store.dir.join("skin.png");
                if let Err(e) = config::atomic(&dest, &bytes) {
                    report_error(&e);
                    return;
                }
                self.skin = skin;
                self.settings.skin_path = dest.to_string_lossy().into_owned();
                self.settings.skin_name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                if self.skin.legacy {
                    self.settings.arm_model = "classic".into();
                }
                self.changed();
            }
            Err(e) => report_error(&e),
        }
    }
    unsafe fn center(&mut self) {
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST), &mut info);
        self.pos = POINT {
            x: (info.rcWork.left + info.rcWork.right - self.width) / 2,
            y: (info.rcWork.top + info.rcWork.bottom - self.height) / 2,
        };
        self.hidden = false;
        ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        self.next_topmost_check = 0.;
        self.move_to();
        self.save_position();
        self.wake();
    }
    unsafe fn command(&mut self, id: u32) {
        match id {
            100 => self.show_panel(),
            101 => self.choose_skin(),
            104 => self.fetch_skin(),
            102 => {
                self.download = None;
                self.online_notice.clear();
                self.skin = Skin::load(render::DEFAULT_SKIN).unwrap();
                self.settings.skin_path.clear();
                self.settings.skin_name = "内置 · 林间旅人".into();
                self.changed();
            }
            110 => {
                self.reset_keys();
                self.settings.posture = "standing".into();
                self.changed();
            }
            111 => {
                self.reset_keys();
                self.settings.posture = "sitting".into();
                self.changed();
            }
            116 | 117 => {
                self.reset_keys();
                self.pose.reset_swing();
                self.settings.posture = "custom".into();
                self.pose.sit = if self.settings.is_sitting() { 1. } else { 0. };
                self.changed();
                if id == 116 {
                    self.show_page(2);
                }
            }
            118 | 119 => {
                self.settings.main_hand = if id == 118 { "right" } else { "left" }.into();
                self.pose.reset_swing();
                self.changed();
            }
            112 => {
                self.reset_keys();
                self.settings.typing_mode = "precise".into();
                self.changed();
            }
            113 => {
                self.reset_keys();
                self.settings.typing_mode = "alternating".into();
                self.changed();
            }
            114 => {
                self.settings.arm_model = "classic".into();
                self.changed();
            }
            115 => {
                if self.skin.legacy {
                    report_error("旧版 64×32 皮肤仅支持经典手臂。");
                } else {
                    self.settings.arm_model = "slim".into();
                    self.changed();
                }
            }
            120 => {
                self.settings.always_on_top = !self.settings.always_on_top;
                self.changed();
            }
            121 => {
                self.settings.click_through = !self.settings.click_through;
                self.changed();
            }
            122 => {
                self.settings.keyboard_enabled = !self.settings.keyboard_enabled;
                self.hook();
                self.changed();
            }
            123 => {
                self.settings.track_mouse = !self.settings.track_mouse;
                self.changed();
            }
            137 => {
                self.settings.hyp_quick_chat = !self.settings.hyp_quick_chat;
                self.hook();
                self.changed();
            }
            124 => {
                self.settings.outer_layer = !self.settings.outer_layer;
                self.changed();
            }
            138 => {
                self.settings.show_cape = !self.settings.show_cape;
                self.changed();
            }
            139 => {
                self.settings.chibi = !self.settings.chibi;

                self.changed();
            }
            163 => {
                self.settings.da_blink = !self.settings.da_blink;
                self.changed();
            }
            164 => {
                self.settings.da_dance = !self.settings.da_dance;
                self.changed();
            }
            125 => {
                self.settings.fps = 30;
                self.changed();
            }
            126 => {
                self.settings.fps = 60;
                self.changed();
            }
            160 => {
                let enabled = !self.settings.autostart;
                match autostart::set_enabled(enabled, &self.store.dir) {
                    Ok(()) => {
                        self.settings.autostart = enabled;
                        self.changed();
                    }
                    Err(e) => {
                        self.notice = format!("开机自启设置失败：{e}");
                        self.refresh_panel();
                    }
                }
            }
            174 => {
                let mut text = [0u16; 32];
                let len = GetWindowTextW(self.ctrl(334), text.as_mut_ptr(), text.len() as i32);
                if let Some(color) = config::normalize_keyboard_color(&String::from_utf16_lossy(&text[..len as usize])) {
                    self.settings.keyboard_color = color;
                    SetWindowTextW(self.ctrl(334), wide(&self.settings.keyboard_color).as_ptr());
                    self.changed();
                } else { report_error("请输入六位十六进制色号，例如 #83878A。"); }
            }
            130 => self.center(),
            131 => {
                self.hidden = !self.hidden;
                self.next_topmost_check = 0.;
                ShowWindow(
                    self.hwnd,
                    if self.hidden {
                        SW_HIDE
                    } else {
                        SW_SHOWNOACTIVATE
                    },
                );
                self.wake();
            }
            132 => {
                self.settings.set_head(&Settings::default().head());
                self.changed();
            }
            133 => {
                self.settings.model_rotation = [0.; 3];
                self.changed();
            }
            134 => {
                *self.settings.active_custom_pose_mut() = config::CustomPose::default();
                self.pose.reset_swing();
                self.changed();
            }
            135 | 136 => {
                self.reset_keys();
                self.pose.reset_swing();
                self.settings.custom_base = if id == 136 { "sitting" } else { "standing" }.into();
                self.settings.posture = "custom".into();
                self.pose.sit = if self.settings.is_sitting() { 1. } else { 0. };
                self.changed();
            }
            165 | 166 => {
                self.settings.main_hand = if id == 165 { "mapped" } else { "reverse" }.into();
                self.pose.reset_swing(); self.changed();
            }
            167 => { self.settings.typing_letters = !self.settings.typing_letters; self.changed(); }
            168 => { self.pose.feed(self.start.elapsed().as_secs_f32()); self.wake(); }
            169 => { self.open_github(false); }
            170 => { self.check_update(); }
            171 => { self.settings.music_notes = !self.settings.music_notes; self.changed(); }
            172 => { self.settings.tease_enabled = !self.settings.tease_enabled; self.changed(); }
            173 => { self.settings.breathing_animation = !self.settings.breathing_animation;  self.changed(); }
            175 => { self.settings.occasional_stretch = !self.settings.occasional_stretch; self.changed(); }
            176 => { self.settings.occasional_tilt = !self.settings.occasional_tilt; self.changed(); }
            177 => { self.settings.occasional_sleepy = !self.settings.occasional_sleepy; self.changed(); }
            178..=181 => {
                let kind=[render::Gesture::Wave,render::Gesture::Bounce,render::Gesture::Shy,render::Gesture::Clap][(id-178) as usize];
                self.pose.trigger_gesture(kind,self.start.elapsed().as_secs_f32()); self.wake();
            }
            150..=152 => self.show_page((id - 150) as u8),
            140..=143 => {
                self.settings.scale = [0.75, 1., 1.25, 1.5][(id - 140) as usize];
                self.resize_pet();
                self.changed();
            }
            199 => {
                self.save();
                DestroyWindow(self.hwnd);
            }
            _ => {}
        }
    }
    unsafe fn resize_pet(&mut self) {
        if self.resizing {
            return;
        }
        self.resizing = true;
        let oldw = self.width;
        let oldh = self.height;
        self.geometry();
        self.pos.x += (oldw - self.width) / 2;
        self.pos.y += oldh - self.height;
        self.draw(self.start.elapsed().as_secs_f32(), 0.05);
        self.constrain();
        self.move_to();
        self.save_position();
        self.resizing = false;
    }
    unsafe fn popup(&mut self) {
        if self.menu_open {
            return;
        }
        self.menu_open = true;
        self.drag = None;
        let menu = CreatePopupMenu();
        // DestroyMenu(menu) also releases all attached submenus.
        let append = |target: HMENU, id: u32, label: &str, checked: bool| {
            AppendMenuW(target, if id == 0 { MF_SEPARATOR } else {
                MF_STRING | if checked { MF_CHECKED } else { 0 }
            }, id as usize, wide(label).as_ptr());
        };
        let submenu = |label: &str, items: &[(u32, &str, bool)]| {
            let child = CreatePopupMenu();
            for &(id, text, checked) in items { append(child, id, text, checked); }
            AppendMenuW(menu, MF_POPUP | MF_STRING, child as usize, wide(label).as_ptr());
        };
        append(menu, 100, "打开控制台", false);
        submenu("触发动作", &[
            (178,"打招呼",false),(179,"开心蹦蹦",false),
            (180,"害羞",false),(181,"鼓掌",false),(168,"喂食苹果",false),
        ]);
        append(menu, 0, "", false);
        let posture = match self.settings.posture.as_str() {
            "sitting" => "坐下", "custom" => "自定义姿态", _ => "站立",
        };
        submenu(&format!("姿态：{posture}"), &[
            (110, "站立", self.settings.posture == "standing"),
            (111, "坐下", self.settings.posture == "sitting"),
            (117, "自定义姿态", self.settings.posture == "custom"),
        ]);
        submenu("外观", &[
            (101, "导入皮肤…", false),
            (0, "", false),
            (114, "经典手臂", self.settings.arm_model == "classic"),
            (115, "纤细手臂", self.settings.arm_model == "slim"),
            (0, "", false),
            (138, "显示正版披风", self.settings.show_cape),
            (139, "Q版小人", self.settings.chibi),
        ]);
        submenu(&format!("大小：{:.0}%", self.settings.scale * 100.), &[
            (140, "75%", (self.settings.scale - 0.75).abs() < 0.001),
            (141, "100%", (self.settings.scale - 1.).abs() < 0.001),
            (142, "125%", (self.settings.scale - 1.25).abs() < 0.001),
            (143, "150%", (self.settings.scale - 1.5).abs() < 0.001),
        ]);
        let hand = match self.settings.main_hand.as_str() {
            "left" => "左手", "mapped" => "实际映射", "reverse" => "反向映射", _ => "右手",
        };
        submenu(&format!("鼠标挥动：{hand}"), &[
            (118, "右手", self.settings.main_hand == "right"),
            (119, "左手", self.settings.main_hand == "left"),
            (165, "实际映射", self.settings.main_hand == "mapped"),
            (166, "反向映射", self.settings.main_hand == "reverse"),
        ]);
        let typing = if !self.settings.keyboard_enabled { "已关闭" }
            else if self.settings.typing_mode == "alternating" { "双手交替" } else { "精准按键" };
        submenu(&format!("键盘互动：{typing}"), &[
            (122, "全局键盘互动", self.settings.keyboard_enabled),
            (0, "", false),
            (112, "精准按键", self.settings.typing_mode == "precise"),
            (113, "双手交替", self.settings.typing_mode == "alternating"),
        ]);
        append(menu, 0, "", false);
        append(menu, 120, "保持置顶", self.settings.always_on_top);
        append(menu, 121, "完全鼠标穿透", self.settings.click_through);
        append(menu, 123, "视线跟随鼠标", self.settings.track_mouse);
        submenu("桌面与启动", &[
            (130, "移到屏幕中央", false),
            (131, if self.hidden { "显示桌宠" } else { "暂时隐藏桌宠" }, false),
            (0, "", false),
            (160, "随 Windows 登录启动", self.settings.autostart),
        ]);
        append(menu, 0, "", false);
        append(menu, 199, "退出 Minecraft桌宠", false);
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        SetForegroundWindow(self.hwnd);
        let menu_clock = menu_clock::MenuClock::start(self.hwnd,self.settings.fps);
        let id = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            p.x,
            p.y,
            0,
            self.hwnd,
            null(),
        );
        drop(menu_clock);
        DestroyMenu(menu);
        PostMessageW(self.hwnd, WM_NULL, 0, 0);
        self.menu_open = false;
        if id != 0 {
            self.command(id as u32);
        }
    }
    unsafe fn tray(&self, action: u32) -> bool {
        let mut data: NOTIFYICONDATAW = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = 1;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = TRAY_MSG;
        data.hIcon = self.icon;
        let tip = wide(&format!("Minecraft桌宠 v{VERSION} · 右键设置 / 双击控制台"));
        // x86 上 NOTIFYICONDATAW 是 packed 结构，不能对字段取引用：先填局部数组再整体赋值
        let mut sz_tip = data.szTip;
        let len = tip.len().min(sz_tip.len());
        sz_tip[..len].copy_from_slice(&tip[..len]);
        data.szTip = sz_tip;
        Shell_NotifyIconW(action, &data) != 0
    }
}
unsafe extern "system" fn keyboard_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 && !PET_HWND.is_null() {
        let k = &*(l as *const KBDLLHOOKSTRUCT);
        if hyp_chat::own_input(k) {
            return CallNextHookEx(null_mut(), code, w, l);
        }
        if TEST_INPUT_ONLY && k.flags & LLKHF_INJECTED == 0 {
            return CallNextHookEx(null_mut(), code, w, l);
        }
        if matches!(
            w as u32,
            WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP
        ) {
            let vk = if k.vkCode == 13 && (k.flags & LLKHF_EXTENDED) != 0 {
                0x1000
            } else {
                k.vkCode
            };
            PostMessageW(
                PET_HWND,
                KEY_MSG,
                vk as usize,
                if matches!(w as u32, WM_KEYDOWN | WM_SYSKEYDOWN) {
                    1
                } else {
                    0
                },
            );
            if hyp_chat::intercept(k, matches!(w as u32, WM_KEYDOWN | WM_SYSKEYDOWN), PET_HWND) {
                return 1;
            }
        }
    }
    CallNextHookEx(null_mut(), code, w, l)
}
unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if APP.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let a = &mut *APP;
    if hwnd == a.panel {
        return ui::panel_proc(a, hwnd, msg, w, l);
    }
    if a.taskbar_msg != 0 && msg == a.taskbar_msg {
        a.tray_ok = a.tray(NIM_ADD);
        a.tray_error = if a.tray_ok { 0 } else { GetLastError() };
        a.next_topmost_check = 0.;
        return 0;
    }
    match msg {
        hyp_chat::MESSAGE => {
            if let Err(message) = hyp_chat::insert(w as HWND) {
                a.notice = message.into();
                if !a.status.is_null() {
                    SetWindowTextW(a.status, wide(message).as_ptr());
                }
            }
            0
        }
        menu_clock::MESSAGE => {
            menu_clock::acknowledge();
            if a.menu_open {a.tick();}
            0
        }
        WM_TIMER => {
            a.tick();
            0
        }
        msg if msg==WM_APP+12 && a.diagnostic => {
            if a.settings.tease_enabled {a.pose.tease(a.start.elapsed().as_secs_f32());a.wake();} 0
        }
        msg if msg==WM_APP+13 && a.diagnostic => {
            let point=POINT{x:(l as u16 as i16) as i32,y:((l>>16) as u16 as i16) as i32};
            if w<5 {a.mouse_press(w,point,a.start.elapsed().as_secs_f32());} 0
        }
        KEY_MSG => {
            if a.settings.keyboard_enabled {
                a.trace_key(w as u32, l != 0, "hook-message");
                a.key_recovery.event(w as u32, l != 0);
                a.pose.key(
                    w as u32,
                    l != 0,
                    a.start.elapsed().as_secs_f32(),
                    &a.settings,
                );
                if a.settings.is_sitting() {
                    a.wake();
                }
            }
            0
        }
        WM_LBUTTONDOWN => {
            a.native_mouse_press(0);
            let mut p = POINT { x: 0, y: 0 };
            GetCursorPos(&mut p);
            a.drag = Some(POINT {
                x: p.x - a.pos.x,
                y: p.y - a.pos.y,
            });
            SetCapture(hwnd);
            0
        }
        WM_LBUTTONUP => {
            if a.drag.take().is_some() {
                ReleaseCapture();
                a.constrain();
                a.move_to();
                a.save_position();
                a.wake();
            }
            0
        }
        WM_CAPTURECHANGED => {
            if a.drag.take().is_some() {
                a.constrain();
                a.move_to();
                a.save_position();
            }
            0
        }
        WM_LBUTTONDBLCLK => {
            a.drag = None;
            ReleaseCapture();
            a.show_panel();
            0
        }
        WM_RBUTTONDOWN => {a.native_mouse_press(1);0}
        WM_MBUTTONDOWN => {a.native_mouse_press(2);0}
        WM_XBUTTONDOWN => {a.native_mouse_press(if (w>>16)&0xffff==1 {3}else{4});1}
        WM_RBUTTONUP | WM_CONTEXTMENU => {
            a.popup();
            0
        }
        WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
        WM_DROPFILES => {
            ui::drop_skin(a, w);
            0
        }
        WM_DISPLAYCHANGE => {
            a.next_topmost_check = 0.;
            a.constrain();
            a.move_to();
            a.save_position();
            a.wake();
            0
        }
        WM_DPICHANGED => {
            let dpi = (w & 0xffff) as f32 / 96.;
            if (dpi - a.dpi).abs() > 0.01 {
                a.resize_pet();
                a.wake();
            }
            0
        }
        WM_POWERBROADCAST => {
            a.next_topmost_check = 0.;
            a.reset_keys();
            a.shift = [false; 2];
            a.wake();
            1
        }
        WM_QUERYENDSESSION => {
            a.save();
            1
        }
        WM_ENDSESSION => {
            if w != 0 {
                a.save();
            }
            0
        }
        TRAY_MSG => {
            match l as u32 {
                WM_RBUTTONUP | WM_CONTEXTMENU => a.popup(),
                // 单击即可打开控制台；双击也走同一条路径（重复调用只是置前）。
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => a.show_panel(),
                _ => {}
            }
            0
        }
        WM_HOTKEY => {
            a.show_panel();
            0
        }
        WM_COMMAND => {
            a.command((w & 0xffff) as u32);
            0
        }
        WM_CLOSE => {
            a.save();
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            a.tray(NIM_DELETE);
            UnregisterHotKey(hwnd, 1);
            if !a.hook.is_null() {
                UnhookWindowsHookEx(a.hook);
                a.hook = null_mut();
            }
            if !a.panel.is_null() {
                DestroyWindow(a.panel);
            }
            PostQuitMessage(0);
            0
        }
        SHOW_MSG => {
            a.show_panel();
            0
        }
        SNAPSHOT_MSG => {
            if a.diagnostic {
                ui::diagnostic_snapshot(a);
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}
/// 读取上次保存的披风文件；没有或损坏时返回 None。
fn load_cape(settings: &Settings) -> Option<Cape> {
    if settings.cape_path.is_empty() {
        return None;
    }
    std::fs::read(&settings.cape_path)
        .ok()
        .and_then(|bytes| Cape::load(&bytes).ok())
}

/// Whether this program may write into `dir` at all. Some machines run the
/// pet under a security policy that denies %APPDATA% writes (os error 5).
fn writable(dir: &std::path::Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write-probe");
    let ok = std::fs::write(&probe, b"1").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}
/// Move an existing APPDATA profile next to the executable when the original
/// location is not writable, rewriting the saved skin/cape paths.
fn migrate(old: &std::path::Path, new: &std::path::Path) {
    if new.join("settings.json").exists() || !old.join("settings.json").is_file() {
        return;
    }
    for name in [
        "settings.json",
        "settings.json.bak",
        "skin.png",
        "cape.png",
        "head-follow.json",
        "head-follow.json.bak",
    ] {
        let src = old.join(name);
        if src.is_file() {
            let _ = std::fs::copy(&src, new.join(name));
        }
    }
    let from = old.to_string_lossy().to_string();
    let to = new.to_string_lossy().to_string();
    // JSON stores Windows paths with doubled separators.
    let from_json = from.replace('\\', "\\\\");
    let to_json = to.replace('\\', "\\\\");
    for name in ["settings.json", "settings.json.bak"] {
        let path = new.join(name);
        if let Ok(text) = std::fs::read_to_string(&path) {
            let fixed = text
                .replace(&from_json, &to_json)
                .replace(&from, &to);
            let _ = std::fs::write(&path, fixed);
        }
    }
}
fn default_data_dir() -> PathBuf {
    let appdata = PathBuf::from(std::env::var_os("APPDATA").unwrap_or_default())
        .join("minecraft-desktop-pet");
    if writable(&appdata) {
        return appdata;
    }
    // Portable fallback: the executable's own folder is usually still writable
    // when security software blocks the user profile.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let portable = parent.join("pet-data");
            if writable(&portable) {
                migrate(&appdata, &portable);
                return portable;
            }
        }
    }
    appdata
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--render-test") {
        render_test(&args);
        return;
    }
    let dir = args
        .iter()
        .find_map(|a| a.strip_prefix("--user-data-dir=").map(PathBuf::from))
        .unwrap_or_else(default_data_dir);
    let diag = args.iter().any(|a| a == "--diagnostic");
    let first = !dir.join("settings.json").exists();
    let mut app = Box::new(App::new(dir.clone(), diag));
    if let Some(e) = &app.store.read_error {
        report_error(e);
        return;
    }
    // 程序被移动或改名后，自启项会在下次启动时自动改写成当前路径。
    autostart::sync(app.settings.autostart, &dir);
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        TEST_INPUT_ONLY = diag && args.iter().any(|a| a == "--test-input-only");
        let mutex_name = format!(
            "Local\\MinecraftDesktopPet-{}",
            dir.to_string_lossy().replace(['\\', ':', '/'], "_")
        );
        let mutex = CreateMutexW(null(), 0, wide(&mutex_name).as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let old = FindWindowW(wide("MinecraftDesktopPetNative").as_ptr(), null());
            if !old.is_null() {
                PostMessageW(old, WM_APP + 10, 0, 0);
            }
            CloseHandle(mutex);
            return;
        }
        let module = GetModuleHandleW(null());
        let cc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES | ICC_STANDARD_CLASSES,
        };
        InitCommonControlsEx(&cc);
        let class = wide("MinecraftDesktopPetNative");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            hInstance: module,
            hIcon: LoadIconW(module, 1usize as *const u16),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);
        app.taskbar_msg = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
        app.icon = wc.hIcon;
        APP = &mut *app;
        app.geometry();
        let hwnd = CreateWindowExW(
            // OBS excludes WS_EX_TOOLWINDOW from its window-capture list.
            // NOACTIVATE still keeps this passive popup off the taskbar without
            // hiding it from capture enumeration. Reuse the existing DWM bitmap.
            WS_EX_LAYERED | WS_EX_NOACTIVATE,
            class.as_ptr(),
            wide(&product_title()).as_ptr(),
            WS_POPUP,
            app.pos.x,
            app.pos.y,
            app.width,
            app.height,
            null_mut(),
            null_mut(),
            module,
            null(),
        );
        if hwnd.is_null() {
            report_error(&format!(
                "窗口创建失败：{}",
                std::io::Error::last_os_error()
            ));
            return;
        }
        app.hwnd = hwnd;
        PET_HWND = hwnd;
        DragAcceptFiles(hwnd, 1);
        app.flags();
        app.hook();
        app.tray_ok = app.tray(NIM_ADD);
        app.tray_error = if app.tray_ok { 0 } else { GetLastError() };
        if !app.tray_ok {
            let _ = std::fs::write(
                app.store.dir.join("tray-error.txt"),
                format!("error={} icon={}\n", app.tray_error, app.icon as isize),
            );
        }
        // 托盘不可用时的备用入口：Ctrl+Alt+Shift+M 打开控制台。
        RegisterHotKey(hwnd, 1, MOD_CONTROL | MOD_ALT | MOD_SHIFT, 'M' as u32);
        GetCursorPos(&mut app.cursor);
        app.draw(0., 0.05);
        app.constrain();
        app.move_to();
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        // Low-priority fallback for Windows-owned modal loops (menus/file picker).
        SetTimer(hwnd, 1, 16, None);
        app.save();
        if !app.store.notice.is_empty() {
            app.notice = app.store.notice.clone();
        }
        if !app.tray_ok {
            app.notice = format!(
                "托盘图标添加失败（错误 {}）；可再次双击程序或从控制台打开设置。",
                app.tray_error
            );
        }
        if first || args.iter().any(|a| a == "--settings") || !app.tray_ok || !app.store.notice.is_empty() {
            app.show_panel();
        }
        if diag {
            app.theme_override=if args.iter().any(|s|s=="--test-theme=dark") {Some(true)} else if args.iter().any(|s|s=="--test-theme=light") {Some(false)} else {None};
            app.apply_theme();
            ui::diagnostic_init(&mut app);
        }
        let mut timer = CreateWaitableTimerExW(
            null(),
            null(),
            CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
            TIMER_ALL_ACCESS,
        );
        if timer.is_null() {
            timer = CreateWaitableTimerExW(null(), null(), 0, TIMER_ALL_ACCESS);
        }
        let mut msg: MSG = std::mem::zeroed();
        'messages: loop {
            // Bound a message batch so input floods cannot starve animation frames.
            for _ in 0..64 {
                if PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) == 0 {
                    break;
                }
                if msg.message == WM_QUIT {
                    break 'messages;
                }
                if app.panel.is_null() || IsDialogMessageW(app.panel, &msg) == 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            app.tick();
            let now = app.start.elapsed().as_secs_f64();
            let poll = if app.hidden {
                0.05
            } else {
                (1. / app.settings.fps as f64).min(0.008)
            };
            // 低级鼠标钩子（WH_MOUSE_LL）挂在消息循环所在线程上，系统要等它
            // 返回才能把鼠标事件派发给其他程序。这里的等待时间必须保持原来的
            // 帧间隔上限，任何更长的睡眠都会让全系统的鼠标变卡。
            let wait = if !app.hidden && now <= app.awake as f64 {
                (app.next_draw - now).min(poll).max(0.0001)
            } else {
                poll
            };
            if !timer.is_null() {
                let due = -((wait * 10_000_000.).ceil() as i64).max(1);
                if SetWaitableTimer(timer, &due, 0, None, null(), 0) != 0 {
                    MsgWaitForMultipleObjectsEx(
                        1,
                        &timer,
                        INFINITE,
                        QS_ALLINPUT,
                        MWMO_INPUTAVAILABLE,
                    );
                } else {
                    MsgWaitForMultipleObjectsEx(
                        0,
                        null(),
                        (wait * 1000.).ceil() as u32,
                        QS_ALLINPUT,
                        MWMO_INPUTAVAILABLE,
                    );
                }
            } else {
                MsgWaitForMultipleObjectsEx(
                    0,
                    null(),
                    (wait * 1000.).ceil() as u32,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
            }
        }
        if !timer.is_null() {
            CancelWaitableTimer(timer);
            CloseHandle(timer);
        }
        app.surface = None;
        if app.github_images != 0 { ImageList_Destroy(app.github_images); }
        if !app.font.is_null() {
            DeleteObject(app.font);
        }
        CloseHandle(mutex);
        APP = null_mut();
        PET_HWND = null_mut();
    }
}
fn render_test(args: &[String]) {
    let dir = args
        .iter()
        .find_map(|a| a.strip_prefix("--output=").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("../.tmp/native-render"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut s = Settings::default();
    let bytes = args
        .iter()
        .find_map(|a| a.strip_prefix("--skin="))
        .map(|p| std::fs::read(p).unwrap())
        .unwrap_or_else(|| render::DEFAULT_SKIN.to_vec());
    let skin = Skin::load(&bytes).unwrap();
    if args.iter().any(|a| a == "--demo-eyes-skin") {
        // 生成一张带眼睛/眉毛素材的测试皮肤，方便验证眨眼、瞳孔和眉毛动画。
        // 眼睛画在识别格 (4,6)-(5,7)：上一行眼皮色、下一行眼白+瞳孔；
        // 眉毛画在材质包 brows 的独立区 x=60..64（右眉 60..62、左眉 62..64）。
        let mut atlas = skin.image.clone();
        let mut set = |x: u32, y: u32, c: [u8; 3]| {
            atlas.put_pixel(x, y, image::Rgba([c[0], c[1], c[2], 255]));
        };
        let lid = [0xd7u8, 0xa3, 0x79];
        let white = [0xf2u8, 0xe4, 0xc9];
        let pupil = [0x3bu8, 0x61, 0x4c];
        let brow = [0x4au8, 0x33, 0x26];
        set(4, 6, lid);
        set(5, 6, lid);
        set(4, 7, white);
        set(5, 7, pupil);
        for x in 60..64 {
            set(x, 7, brow);
        }
        atlas.save(dir.join("demo-eyes-skin.png")).unwrap();
        println!("written {}", dir.join("demo-eyes-skin.png").display());
        return;
    }
    if args.iter().any(|a| a == "--typing-return-test") {
        s.track_mouse = false;
        s.typing_squash = 0.; // Inspect arms without the intentionally restarting body pulse.
        for custom in [false, true] {
            s.posture = if custom { "custom" } else { "sitting" }.into();
            s.custom_base = "sitting".into();
            s.custom_sitting_pose.left_arm = [-30., -20., 25.];
            s.custom_sitting_pose.right_arm = [-40., 30., -35.];
            let mut p = Pose::new(&s);
            let mut samples = Vec::new();
            for i in 0..180 {
                let now = i as f32 / 60.;
                for (frame, vk, down) in [
                    (30, 81, true),
                    (45, 84, true),
                    (54, 84, false),
                    (63, 81, false),
                    (72, 80, true),
                    (81, 80, false),
                    (90, 81, true),
                    (93, 81, false),
                ] {
                    if i == frame {
                        p.key(vk, down, now, &s);
                    }
                }
                p.update(&s, &skin, Vec2::ZERO, 360., 420., false, now, 1. / 60.);
                samples.push(serde_json::json!({"frame":i,"arms":p.arms.map(|q|q.to_array())}));
                if i >= 27 && i <= 123 && i % 3 == 0 {
                    let mut f = Frame::new(360, 420);
                    f.draw(&s, &skin, &p);
                    image::save_buffer(
                        dir.join(format!(
                            "{}-{i:03}.png",
                            if custom { "custom" } else { "sitting" }
                        )),
                        &f.rgba(),
                        360,
                        420,
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
            }
            std::fs::write(
                dir.join(format!(
                    "{}-samples.json",
                    if custom { "custom" } else { "sitting" }
                )),
                serde_json::to_vec_pretty(&samples).unwrap(),
            )
            .unwrap();
        }
        return;
    }
    if args.iter().any(|a| a == "--motion-test") {
        s.track_mouse = false;
        for seated in [false, true] {
            s.posture = if seated { "sitting" } else { "standing" }.into();
            let mut p = Pose::new(&s);
            for i in 0..120 {
                p.update(
                    &s,
                    &skin,
                    Vec2::ZERO,
                    360.,
                    420.,
                    false,
                    i as f32 / 120.,
                    1. / 120.,
                );
            }
            if seated {
                p.key(81, true, 1., &s);
            }
            for i in 120..240 {
                p.update(
                    &s,
                    &skin,
                    Vec2::ZERO,
                    360.,
                    420.,
                    false,
                    i as f32 / 120.,
                    1. / 120.,
                );
            }
            p.click(2., false);
            for i in 0..49 {
                p.update(
                    &s,
                    &skin,
                    Vec2::ZERO,
                    360.,
                    420.,
                    false,
                    2. + i as f32 / 120.,
                    1. / 120.,
                );
                let mut f = Frame::new(450, 525);
                f.draw(&s, &skin, &p);
                image::save_buffer(
                    dir.join(format!(
                        "{}-{i:02}.png",
                        if seated { "sitting" } else { "standing" }
                    )),
                    &f.rgba(),
                    450,
                    525,
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
        }
        return;
    }
    if args.iter().any(|a| a == "--sway-test") {
        // 潜行扭动：按住 Shift，3 秒后开始扭；6 秒后每 0.3 秒截一帧（一个来回 1.8 秒）。
        s.posture = "standing".into();
        let mut p = Pose::new(&s);
        let mut shots = 0;
        for frame in 0..60 * 10 {
            let now = frame as f32 / 60.;
            p.update(&s, &skin, Vec2::new(225., 262.5), 450., 525., true, now, 1. / 60.);
            if frame >= 60 * 6 && (frame - 60 * 6) % 18 == 0 {
                let mut f = Frame::new(450, 525);
                f.draw(&s, &skin, &p);
                image::save_buffer(
                    dir.join(format!(
                        "dance-{shots}-{:.01}deg.png",
                        p.body_roll.to_degrees()
                    )),
                    &f.rgba(),
                    450,
                    525,
                    image::ColorType::Rgba8,
                )
                .unwrap();
                shots += 1;
            }
        }
        return;
    }
    if args.iter().any(|a| a == "--blink-test") {
        // 输出一次完整眨眼的连续帧，用于人工核对眼皮位置与过渡。
        s.track_mouse = false;
        let mut p = Pose::new(&s);
        let start = (0..2400)
            .map(|i| i as f32 / 240.)
            .find(|t| p.blink_at(*t) > 0.999)
            .unwrap_or(0.);
        for i in 0..48 {
            let now = start - 0.10 + i as f32 / 240.;
            p.update(&s, &skin, Vec2::ZERO, 450., 525., false, now, 1. / 240.);
            let mut f = Frame::new(450, 525);
            f.draw(&s, &skin, &p);
            image::save_buffer(
                dir.join(format!("blink-{i:02}-{:.3}.png", now)),
                &f.rgba(),
                450,
                525,
                image::ColorType::Rgba8,
            )
            .unwrap();
        }
        return;
    }
    s.outer_layer = !args.iter().any(|a| a == "--no-outer");
    for (name, sit, slim, crouch) in [
        ("standing", false, false, false),
        ("sitting", true, false, false),
        ("slim", false, true, false),
        ("crouch", false, false, true),
    ] {
        s.posture = if sit { "sitting" } else { "standing" }.into();
        s.arm_model = if slim { "slim" } else { "classic" }.into();
        s.track_mouse = false;
        let mut p = Pose::new(&s);
        for i in 0..120 {
            p.update(
                &s,
                &skin,
                Vec2::new(180., 160.),
                360.,
                420.,
                crouch,
                i as f32 / 60.,
                1. / 60.,
            );
        }
        let mut f = Frame::new(450, 525);
        f.draw(&s, &skin, &p);
        image::save_buffer(
            dir.join(format!("{name}.png")),
            &f.rgba(),
            450,
            525,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}
