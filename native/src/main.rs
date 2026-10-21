#![windows_subsystem = "windows"]
mod autostart;
mod blockbench;
mod config;
mod hyp_chat;
mod keyboard;
mod online;
mod render;
mod ui;
mod window_order;
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
const CLICK_MSG: u32 = WM_APP + 3;
const SHOW_MSG: u32 = WM_APP + 10;
const SNAPSHOT_MSG: u32 = WM_APP + 11;
const AUTOSTART_MSG: u32 = WM_APP + 13;
static mut APP: *mut App = null_mut();
static mut PET_HWND: HWND = null_mut();
// Diagnostic runs can isolate their injected test events from the user's typing.
static mut TEST_INPUT_ONLY: bool = false;
const VERSION: &str = env!("CARGO_PKG_VERSION");
pub fn product_title() -> String {
    format!("Minecraft桌宠 v{VERSION}")
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
    bbmodel: Option<blockbench::BbModel>,
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
    mouse_hook: HHOOK,
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
    last_draw: f64,
    next_draw: f64,
    next_topmost_check: f64,
    topmost_foreground: HWND,
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
        let bbmodel = if settings.model_mode == "blockbench" && !settings.blockbench_path.is_empty() {
            blockbench::BbModel::load_file(std::path::Path::new(&settings.blockbench_path)).ok()
        } else {
            None
        };
        let pose = Pose::new(&settings);
        let notice = store.notice.clone();
        Self {
            settings,
            store,
            skin,
            cape,
            bbmodel,
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
            mouse_hook: null_mut(),
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
            last_draw: 0.,
            next_draw: 0.,
            next_topmost_check: 0.,
            topmost_foreground: null_mut(),
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
        if self.settings.model_mode == "blockbench" && self.bbmodel.is_some() {
            self.frame.clear();
            let head_rot = if self.settings.blockbench_head_track {
                Some(self.pose.head_rotation)
            } else {
                None
            };
            self.bbmodel.as_ref().unwrap().draw(
                &mut self.frame,
                &self.settings,
                if self.settings.blockbench_animation.is_empty() {
                    None
                } else {
                    Some(&self.settings.blockbench_animation)
                },
                now * self.settings.blockbench_speed,
                head_rot,
                Some(&self.pose),
            );
        } else {
            self.frame
                .draw_ext(&self.settings, &self.skin, self.cape.as_ref(), &self.pose);
        }
        self.advance_opacity();
        self.raster_us += raster_start.elapsed().as_micros() as u64;
        let present_start = Instant::now();
        if let Some(surface) = &mut self.surface {
            surface.update(self.hwnd, &self.frame, self.pos, self.opacity);
        }
        self.present_us += present_start.elapsed().as_micros() as u64;
        self.render_count += 1;
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
        self.poll_download();
        self.maintain_topmost(clock);
        let mut cursor = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor);
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
        if !self.hidden && now <= self.awake && clock >= self.next_draw {
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
        SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, flags as isize);
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
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        if ext == "bbmodel" || ext == "json" {
            self.import_blockbench(path);
            return;
        }
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
                self.settings.model_mode = "player".into();
                if self.skin.legacy {
                    self.settings.arm_model = "classic".into();
                }
                self.changed();
            }
            Err(e) => report_error(&e),
        }
    }
    unsafe fn import_blockbench(&mut self, path: PathBuf) {
        match blockbench::BbModel::load_file(&path) {
            Ok(model) => {
                let dest = self.store.dir.join("custom_model.bbmodel");
                if let Ok(bytes) = std::fs::read(&path) {
                    let _ = config::atomic(&dest, &bytes);
                }
                let model_name = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                self.settings.model_mode = "blockbench".into();
                self.settings.blockbench_path = path.to_string_lossy().into_owned();
                self.settings.blockbench_name = model_name;
                let anim_names = model.animation_names();
                if let Some(idle) = anim_names.iter().find(|n| {
                    let l = n.to_lowercase();
                    l == "idle" || l == "stand" || l == "default" || l == "loop"
                }) {
                    self.settings.blockbench_animation = idle.clone();
                } else {
                    self.settings.blockbench_animation.clear();
                }
                self.bbmodel = Some(model);
                self.notice = format!("已加载 Blockbench 模型: {}", self.settings.blockbench_name);
                self.changed();
            }
            Err(e) => report_error(&format!("无法导入 Blockbench 模型：{e}")),
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
            150..=153 => self.show_page((id - 150) as u8),
            500 => {
                if self.settings.model_mode == "blockbench" {
                    self.settings.model_mode = "player".into();
                    self.changed();
                } else if self.bbmodel.is_some() {
                    self.settings.model_mode = "blockbench".into();
                    self.changed();
                } else {
                    self.choose_model();
                }
            }
            501 => self.choose_model(),
            502 => {
                self.settings.model_mode = "player".into();
                self.changed();
            }
            505 => {
                self.settings.blockbench_loop = !self.settings.blockbench_loop;
                self.changed();
            }
            506 => {
                self.settings.blockbench_head_track = !self.settings.blockbench_head_track;
                self.changed();
            }
            513 => {
                self.settings.keyboard_bounce = !self.settings.keyboard_bounce;
                self.changed();
            }
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
        let items = [
            (100, "打开控制台", false),
            (101, "导入皮肤…", false),
            (501, "导入 Blockbench 模型…", false),
            (513, "按键身体回弹", self.settings.keyboard_bounce),
            (0, "", false),
            (110, "站立", self.settings.posture == "standing"),
            (111, "坐下", self.settings.posture == "sitting"),
            (117, "自定义姿态", self.settings.posture == "custom"),
            (118, "右手挥动", self.settings.main_hand == "right"),
            (119, "左手挥动", self.settings.main_hand == "left"),
            (0, "", false),
            (112, "精准按键", self.settings.typing_mode == "precise"),
            (113, "双手交替", self.settings.typing_mode == "alternating"),
            (114, "经典手臂", self.settings.arm_model == "classic"),
            (115, "纤细手臂", self.settings.arm_model == "slim"),
            (0, "", false),
            (120, "保持置顶", self.settings.always_on_top),
            (121, "完全鼠标穿透", self.settings.click_through),
            (122, "全局键盘互动", self.settings.keyboard_enabled),
            (123, "视线跟随鼠标", self.settings.track_mouse),
            (138, "显示正版披风", self.settings.show_cape),
            (0, "", false),
            (140, "大小 75%", (self.settings.scale - 0.75).abs() < 0.01),
            (141, "大小 100%", (self.settings.scale - 1.).abs() < 0.01),
            (142, "大小 125%", (self.settings.scale - 1.25).abs() < 0.01),
            (143, "大小 150%", (self.settings.scale - 1.5).abs() < 0.01),
            (130, "把桌宠移到屏幕中央", false),
            (
                131,
                if self.hidden {
                    "显示桌宠"
                } else {
                    "暂时隐藏桌宠"
                },
                false,
            ),
            (0, "", false),
            (160, "随 Windows 登录启动", self.settings.autostart),
            (0, "", false),
            (199, "退出Minecraft桌宠", false),
        ];
        for (id, label, checked) in items {
            AppendMenuW(
                menu,
                if id == 0 {
                    MF_SEPARATOR
                } else {
                    MF_STRING | if checked { MF_CHECKED } else { 0 }
                },
                id as usize,
                wide(label).as_ptr(),
            );
        }
        let mut p = POINT { x: 0, y: 0 };
        GetCursorPos(&mut p);
        SetForegroundWindow(self.hwnd);
        let id = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            p.x,
            p.y,
            0,
            self.hwnd,
            null(),
        );
        DestroyMenu(menu);
        PostMessageW(self.hwnd, WM_NULL, 0, 0);
        self.menu_open = false;
        if id != 0 {
            self.command(id as u32);
        }
    }
    unsafe fn tray(&self, action: u32) {
        let mut data: NOTIFYICONDATAW = std::mem::zeroed();
        data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = 1;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = TRAY_MSG;
        data.hIcon = self.icon;
        let tip = wide(&format!("Minecraft桌宠 v{VERSION} · 右键设置 / 双击控制台"));
        data.szTip[..tip.len()].copy_from_slice(&tip);
        Shell_NotifyIconW(action, &data);
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
unsafe extern "system" fn mouse_hook(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    if code >= 0 && TEST_INPUT_ONLY && (*(l as *const MSLLHOOKSTRUCT)).flags & LLMHF_INJECTED == 0 {
        return CallNextHookEx(null_mut(), code, w, l);
    }
    if code >= 0
        && !PET_HWND.is_null()
        && matches!(
            w as u32,
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
        )
    {
        PostMessageW(PET_HWND, CLICK_MSG, 0, 0);
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
        a.tray(NIM_ADD);
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
        WM_TIMER => {
            a.tick();
            0
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
                if a.settings.keyboard_bounce {
                    a.wake();
                }
            }
            0
        }
        CLICK_MSG => {
            a.pose.click(
                a.start.elapsed().as_secs_f32(),
                a.settings.main_hand == "left",
            );
            a.wake();
            0
        }
        WM_LBUTTONDOWN => {
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
                WM_LBUTTONDBLCLK => a.show_panel(),
                _ => {}
            }
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
            if !a.mouse_hook.is_null() {
                UnhookWindowsHookEx(a.mouse_hook);
                a.mouse_hook = null_mut();
            }
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
        AUTOSTART_MSG => {
            // 自启延迟结束：把桌宠显示出来。
            a.hidden = false;
            a.next_topmost_check = 0.;
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            a.wake();
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--render-test") {
        render_test(&args);
        return;
    }
    let dir = args
        .iter()
        .find_map(|a| a.strip_prefix("--user-data-dir=").map(PathBuf::from))
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("APPDATA").unwrap()).join("minecraft-desktop-pet")
        });
    let diag = args.iter().any(|a| a == "--diagnostic");
    let from_autostart = args.iter().any(|a| a == "--autostart");
    let first = !dir.join("settings.json").exists();
    let mut app = Box::new(App::new(dir.clone(), diag));
    if let Some(e) = &app.store.read_error {
        report_error(e);
        return;
    }
    if let Err(e) = autostart::apply_priority(&app.settings.startup_priority) {
        app.notice = e;
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
            WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
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
        app.mouse_hook =
            SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), GetModuleHandleW(null()), 0);
        app.tray(NIM_ADD);
        GetCursorPos(&mut app.cursor);
        app.draw(0., 0.05);
        app.constrain();
        app.move_to();
        // 开机自启可以延迟出现，避免和别的启动项抢开机的头几秒。
        let startup_delay = if from_autostart {
            app.settings.autostart_delay
        } else {
            0
        };
        if startup_delay > 0 {
            app.hidden = true;
            let target = hwnd as usize;
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(startup_delay as u64));
                PostMessageW(target as HWND, AUTOSTART_MSG, 0, 0);
            });
        } else {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        // Low-priority fallback for Windows-owned modal loops (menus/file picker).
        SetTimer(hwnd, 1, 16, None);
        app.save();
        if !app.store.notice.is_empty() {
            app.notice = app.store.notice.clone();
        }
        if first || args.iter().any(|a| a == "--settings") || !app.store.notice.is_empty() {
            app.show_panel();
        }
        if diag {
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
