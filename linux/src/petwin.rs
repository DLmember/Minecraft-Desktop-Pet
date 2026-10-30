//! X11 桌面宠物窗口：32 位 ARGB 透明、EWMH 置顶/跳过任务栏、鼠标穿透、
//! 拖动、全局快捷键与控制台入口。

use x11rb::connection::{Connection, RequestConnection};
use x11rb::wrapper::ConnectionExt as WrapperExt;
use x11rb::protocol::shape::{ConnectionExt as ShapeExt, SK, SO};
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageData, ClientMessageEvent, ClipOrdering, ColormapAlloc,
    ConfigureWindowAux, ConnectionExt as XprotoExt, CreateGCAux, CreateWindowAux, EventMask,
    ImageFormat, PropMode, Rectangle, VisualClass, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use crate::core::{FRAME_H, FRAME_W};

const TITLE: &str = "Minecraft桌宠";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinEvent {
    /// 左键按下：挥动主手（每次物理点击只触发一次）
    Click,
    /// 双击：打开/聚焦控制台
    DoubleClick,
    /// 右键：打开菜单
    ContextMenu,
    /// 拖动结束：保存位置
    DragFinished,
    /// Ctrl+Alt+P：切换控制台
    Hotkey,
    /// 需要重绘（例如重新映射）
    Redraw,
    /// 窗口管理器请求关闭
    Close,
}

#[derive(Default, Clone)]
pub struct KeyMap {
    pub keysym_of: std::collections::HashMap<u8, u32>,
    pub shift_codes: Vec<u8>,
}

struct Atoms {
    wm_state: u32,
    above: u32,
    skip_taskbar: u32,
    skip_pager: u32,
    sticky: u32,
    wm_type: u32,
    utility: u32,
    motif: u32,
    net_name: u32,
    utf8: u32,
    net_pid: u32,
    wm_delete: u32,
    wm_protocols: u32,
}

pub struct PetWindow {
    conn: RustConnection,
    screen_num: usize,
    root: u32,
    pub win: u32,
    gc: u32,
    atoms: Atoms,
    pub width: u16,
    pub height: u16,
    pub x: i16,
    pub y: i16,
    click_through: bool,
    above: bool,
    mapped: bool,
    dragging: Option<(i16, i16)>,
    drag_moved: bool,
    last_press: Option<(f64, i16, i16)>,
    hotkey: Option<u8>,
    buf: Vec<u8>,
    mapping_stale: bool,
    /// Wayland 下由 KWin 脚本上报的全局指针位置，优先于 XQueryPointer
    pointer_source: Option<crate::pointer::Latest>,
}

impl PetWindow {
    pub fn new(width: u16, height: u16, x: i16, y: i16) -> Result<Self, String> {
        let (conn, screen_num) =
            x11rb::connect(None).map_err(|e| format!("无法连接 X11 显示：{e}"))?;
        let screen = conn.setup().roots[screen_num].clone();
        let root = screen.root;

        let (depth, visual) = screen
            .allowed_depths
            .iter()
            .find(|d| d.depth == 32)
            .and_then(|d| {
                d.visuals
                    .iter()
                    .find(|v| v.class == VisualClass::TRUE_COLOR)
                    .map(|v| (d.depth, v.visual_id))
            })
            .ok_or_else(|| {
                "当前 X 服务器没有 32 位 ARGB 视觉。请启用桌面合成器（如 picom、KWin、Mutter）后重试。"
                    .to_string()
            })?;

        let win = conn.generate_id().map_err(|e| e.to_string())?;
        let cmap = conn.generate_id().map_err(|e| e.to_string())?;
        conn.create_colormap(ColormapAlloc::NONE, cmap, root, visual)
            .map_err(|e| e.to_string())?;

        let event_mask = EventMask::EXPOSURE
            | EventMask::BUTTON_PRESS
            | EventMask::BUTTON_RELEASE
            | EventMask::POINTER_MOTION
            | EventMask::STRUCTURE_NOTIFY
            | EventMask::KEY_PRESS
            | EventMask::KEY_RELEASE
            | EventMask::ENTER_WINDOW
            | EventMask::LEAVE_WINDOW;

        conn.create_window(
            depth,
            win,
            root,
            x,
            y,
            width,
            height,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &CreateWindowAux::new()
                .background_pixel(0)
                .border_pixel(0)
                .colormap(cmap)
                .event_mask(event_mask),
        )
        .map_err(|e| e.to_string())?;

        let gc = conn.generate_id().map_err(|e| e.to_string())?;
        conn.create_gc(gc, win, &CreateGCAux::new().foreground(0).graphics_exposures(0))
            .map_err(|e| e.to_string())?;

        let atom = |name: &str| -> u32 {
            conn.intern_atom(false, name.as_bytes())
                .ok()
                .and_then(|c| c.reply().ok())
                .map_or(0, |r| r.atom)
        };
        let atoms = Atoms {
            wm_state: atom("_NET_WM_STATE"),
            above: atom("_NET_WM_STATE_ABOVE"),
            skip_taskbar: atom("_NET_WM_STATE_SKIP_TASKBAR"),
            skip_pager: atom("_NET_WM_STATE_SKIP_PAGER"),
            sticky: atom("_NET_WM_STATE_STICKY"),
            wm_type: atom("_NET_WM_WINDOW_TYPE"),
            utility: atom("_NET_WM_WINDOW_TYPE_UTILITY"),
            motif: atom("_MOTIF_WM_HINTS"),
            net_name: atom("_NET_WM_NAME"),
            utf8: atom("UTF8_STRING"),
            net_pid: atom("_NET_WM_PID"),
            wm_delete: atom("WM_DELETE_WINDOW"),
            wm_protocols: atom("WM_PROTOCOLS"),
        };

        let mut pet = Self {
            conn,
            screen_num,
            root,
            win,
            gc,
            atoms,
            width,
            height,
            x,
            y,
            click_through: false,
            above: true,
            mapped: false,
            dragging: None,
            drag_moved: false,
            last_press: None,
            hotkey: None,
            buf: Vec::new(),
            mapping_stale: false,
            pointer_source: None,
        };
        pet.set_properties()?;
        pet.set_size_hints();
        pet.grab_hotkey();
        pet.conn.flush().ok();
        Ok(pet)
    }

    fn set_properties(&self) -> Result<(), String> {
        let conn = &self.conn;
        // WM_NAME 只能是 Latin-1，中文放进 _NET_WM_NAME。
        conn.change_property8(
            PropMode::REPLACE,
            self.win,
            AtomEnum::WM_NAME,
            AtomEnum::STRING,
            b"Minecraft Desktop Pet",
        )
        .map_err(|e| e.to_string())?;
        if self.atoms.net_name != 0 {
            conn.change_property8(
                PropMode::REPLACE,
                self.win,
                self.atoms.net_name,
                self.atoms.utf8,
                TITLE.as_bytes(),
            )
            .map_err(|e| e.to_string())?;
        }
        conn.change_property8(
            PropMode::REPLACE,
            self.win,
            AtomEnum::WM_CLASS,
            AtomEnum::STRING,
            b"minecraft-pet\0MinecraftDesktopPet\0",
        )
        .map_err(|e| e.to_string())?;
        if self.atoms.wm_type != 0 {
            conn.change_property32(
                PropMode::REPLACE,
                self.win,
                self.atoms.wm_type,
                AtomEnum::ATOM,
                &[self.atoms.utility],
            )
            .map_err(|e| e.to_string())?;
        }
        if self.atoms.net_pid != 0 {
            conn.change_property32(
                PropMode::REPLACE,
                self.win,
                self.atoms.net_pid,
                AtomEnum::CARDINAL,
                &[std::process::id()],
            )
            .map_err(|e| e.to_string())?;
        }
        // 无边框（MOTIF hints）
        if self.atoms.motif != 0 {
            conn.change_property32(
                PropMode::REPLACE,
                self.win,
                self.atoms.motif,
                self.atoms.motif,
                &[2u32, 0, 0, 0, 0],
            )
            .map_err(|e| e.to_string())?;
        }
        // 点击不抢焦点：InputHint + input=false
        conn.change_property32(
            PropMode::REPLACE,
            self.win,
            AtomEnum::WM_HINTS,
            AtomEnum::WM_HINTS,
            &[1u32, 0, 0, 0, 0, 0, 0, 0, 0],
        )
        .map_err(|e| e.to_string())?;
        conn.change_property32(
            PropMode::REPLACE,
            self.win,
            self.atoms.wm_protocols,
            AtomEnum::ATOM,
            &[self.atoms.wm_delete],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn grab_hotkey(&mut self) {
        let map = self.key_map();
        let code = map
            .keysym_of
            .iter()
            .find(|(_, sym)| **sym == 0x50 /* XK_P */)
            .map(|(c, _)| *c);
        let Some(code) = code else { return };
        let base = 4u16 | 8u16; // ControlMask | Mod1Mask
        for extra in [0u16, 2, 16, 18] {
            let _ = self.conn.grab_key(
                false,
                self.root,
                x11rb::protocol::xproto::ModMask::from(base | extra),
                code,
                x11rb::protocol::xproto::GrabMode::ASYNC,
                x11rb::protocol::xproto::GrabMode::ASYNC,
            );
        }
        self.conn.flush().ok();
        self.hotkey = Some(code);
    }

    pub fn screen_size(&self) -> (u16, u16) {
        let screen = &self.conn.setup().roots[self.screen_num];
        (screen.width_in_pixels, screen.height_in_pixels)
    }

    pub fn key_map(&self) -> KeyMap {
        let mut map = KeyMap::default();
        // 8..255 一次取回，避免逐键往返。
        let Ok(cookie) = self.conn.get_keyboard_mapping(8, 248) else {
            return map;
        };
        let Ok(reply) = cookie.reply() else { return map };
        let per = reply.keysyms_per_keycode as usize;
        if per == 0 {
            return map;
        }
        for (i, row) in reply.keysyms.chunks(per).enumerate() {
            let code = 8 + i as u8;
            let sym = row[0];
            map.keysym_of.insert(code, sym);
            if sym == 0xffe1 || sym == 0xffe2 {
                map.shift_codes.push(code);
            }
        }
        map
    }

    pub fn mapping_stale(&mut self) -> bool {
        std::mem::take(&mut self.mapping_stale)
    }

    pub fn keyboard_state(&self) -> [u8; 32] {
        self.conn
            .query_keymap()
            .ok()
            .and_then(|c| c.reply().ok())
            .map_or([0u8; 32], |r| r.keys)
    }

    pub fn key_down(&self, code: u8) -> bool {
        let keys = self.keyboard_state();
        keys[(code / 8) as usize] & (1 << (code % 8)) != 0
    }

    pub fn pointer(&self) -> Option<(i16, i16)> {
        if let Some(pos) = self.pointer_source.as_ref().and_then(|s| s.get()) {
            return Some(pos);
        }
        let reply = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        Some((reply.root_x, reply.root_y))
    }

    pub fn set_pointer_source(&mut self, latest: crate::pointer::Latest) {
        self.pointer_source = Some(latest);
    }

    pub fn position(&self) -> (i16, i16) {
        (self.x, self.y)
    }

    pub fn map(&mut self) {
        if self.mapped {
            return;
        }
        self.conn.map_window(self.win).ok();
        self.conn.flush().ok();
        self.mapped = true;
        self.set_wm_state(true);
        self.apply_above();
        self.conn.flush().ok();
    }

    pub fn unmap(&mut self) {
        if !self.mapped {
            return;
        }
        self.conn.unmap_window(self.win).ok();
        self.conn.flush().ok();
        self.mapped = false;
    }

    pub fn is_mapped(&self) -> bool {
        self.mapped
    }

    fn send_state(&self, action: u32, first: u32, second: u32) {
        if self.atoms.wm_state == 0 {
            return;
        }
        let data = ClientMessageData::from([action, first, second, 1, 0]);
        let event = ClientMessageEvent::new(32, self.win, self.atoms.wm_state, data);
        let _ = self.conn.send_event(
            false,
            self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        );
    }

    fn set_wm_state(&self, add: bool) {
        let action = if add { 1 } else { 0 };
        for atom in [
            self.atoms.skip_taskbar,
            self.atoms.skip_pager,
            self.atoms.sticky,
        ] {
            if atom != 0 {
                self.send_state(action, atom, 0);
            }
        }
    }

    fn apply_above(&self) {
        if self.atoms.above == 0 {
            return;
        }
        self.send_state(if self.above { 1 } else { 0 }, self.atoms.above, 0);
    }

    pub fn set_above(&mut self, on: bool) {
        self.above = on;
        self.apply_above();
        self.conn.flush().ok();
    }

    /// 写入 ICCCM 尺寸提示。带 USPosition/PPosition 时，KWin 等窗口管理器
    /// 会在映射时采用我们给的坐标，而不是用自己的放置策略（否则保存的位置失效）。
    fn set_size_hints(&self) {
        const US_POSITION: u32 = 1;
        const US_SIZE: u32 = 2;
        const P_POSITION: u32 = 4;
        const P_SIZE: u32 = 8;
        const P_MIN_SIZE: u32 = 16;
        const P_MAX_SIZE: u32 = 32;
        const STATIC_GRAVITY: u32 = 10;
        let (w, h) = (self.width as u32, self.height as u32);
        let hints = [
            US_POSITION | US_SIZE | P_POSITION | P_SIZE | P_MIN_SIZE | P_MAX_SIZE,
            self.x as u32,
            self.y as u32,
            w,
            h,
            w, // min_width
            h, // min_height
            w, // max_width
            h, // max_height
            0, 0, 0, 0, 0, 0, 0, 0, STATIC_GRAVITY,
        ];
        let _ = self.conn.change_property32(
            PropMode::REPLACE,
            self.win,
            AtomEnum::WM_NORMAL_HINTS,
            AtomEnum::WM_SIZE_HINTS,
            &hints,
        );
    }

    pub fn set_click_through(&mut self, on: bool) {
        if self.click_through == on {
            return;
        }
        self.click_through = on;
        if on {
            let _ = self.conn.shape_rectangles(
                SO::SET,
                SK::INPUT,
                ClipOrdering::UNSORTED,
                self.win,
                0,
                0,
                &[],
            );
        } else {
            let rect = [Rectangle {
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
            }];
            let _ = self.conn.shape_rectangles(
                SO::SET,
                SK::INPUT,
                ClipOrdering::UNSORTED,
                self.win,
                0,
                0,
                &rect,
            );
        }
        self.conn.flush().ok();
    }

    pub fn click_through(&self) -> bool {
        self.click_through
    }

    pub fn move_resize(&mut self, x: i16, y: i16, width: u16, height: u16) {
        let resized = width != self.width || height != self.height;
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height;
        let _ = self.conn.configure_window(
            self.win,
            &ConfigureWindowAux::new()
                .x(x as i32)
                .y(y as i32)
                .width(width as u32)
                .height(height as u32),
        );
        if resized && !self.click_through {
            let rect = [Rectangle {
                x: 0,
                y: 0,
                width,
                height,
            }];
            let _ = self.conn.shape_rectangles(
                SO::SET,
                SK::INPUT,
                ClipOrdering::UNSORTED,
                self.win,
                0,
                0,
                &rect,
            );
        }
        // 映射前的坐标要同步进 WM_NORMAL_HINTS，否则窗口管理器会忽略它
        if !self.mapped {
            self.set_size_hints();
        }
        self.conn.flush().ok();
    }

    /// 把 450×525 的 BGRA 预乘帧按窗口尺寸最近邻缩放后送入 X11。
    pub fn present(&mut self, frame: &[u8], opacity: f32) {
        let (dw, dh) = (self.width as usize, self.height as usize);
        if dw == 0 || dh == 0 {
            return;
        }
        self.buf.resize(dw * dh * 4, 0);        let alpha = opacity.clamp(0.0, 1.0);
        for y in 0..dh {
            let sy = y * FRAME_H / dh;
            let src_row = sy * FRAME_W;
            let dst_row = y * dw;
            for x in 0..dw {
                let sx = x * FRAME_W / dw;
                let si = (src_row + sx) * 4;
                let di = (dst_row + x) * 4;
                // core 帧与 X11 32 位 ARGB visual（小端）同为 BGRA 内存布局，无需交换通道
                if alpha >= 0.999 {
                    self.buf[di..di + 4].copy_from_slice(&frame[si..si + 4]);
                } else {
                    // 预乘 alpha：所有通道同比缩放
                    for c in 0..4 {
                        self.buf[di + c] = (frame[si + c] as f32 * alpha) as u8;
                    }
                }
            }
        }
        self.blit();
    }

    fn blit(&mut self) {
        let row = self.width as usize * 4;
        if row == 0 {
            return;
        }
        let max = self.conn.maximum_request_bytes().saturating_sub(64);
        let rows = (max / row).max(1).min(self.height as usize);
        let mut y = 0usize;
        while y < self.height as usize {
            let n = rows.min(self.height as usize - y);
            let start = y * row;
            let end = start + n * row;
            let _ = self.conn.put_image(
                ImageFormat::Z_PIXMAP,
                self.win,
                self.gc,
                self.width,
                n as u16,
                0,
                y as i16,
                0,
                32,
                &self.buf[start..end],
            );
            y += n;
        }
        self.conn.flush().ok();
    }

    pub fn poll(&mut self, now: f64) -> Vec<WinEvent> {
        let mut events = Vec::new();
        loop {
            let event = match self.conn.poll_for_event() {
                Ok(Some(e)) => e,
                Ok(None) => break,
                Err(_) => break,
            };
            match event {
                Event::ButtonPress(e) => match e.detail {
                    1 => {
                        events.push(WinEvent::Click);
                        if let Some((t, x, y)) = self.last_press {
                            if now - t < 0.4
                                && (e.root_x - x).abs() <= 6
                                && (e.root_y - y).abs() <= 6
                            {
                                events.push(WinEvent::DoubleClick);
                                self.last_press = None;
                            } else {
                                self.last_press = Some((now, e.root_x, e.root_y));
                            }
                        } else {
                            self.last_press = Some((now, e.root_x, e.root_y));
                        }
                        self.dragging = Some((e.root_x - self.x, e.root_y - self.y));
                        self.drag_moved = false;
                    }
                    3 => events.push(WinEvent::ContextMenu),
                    _ => {}
                },
                Event::ButtonRelease(e) => {
                    if e.detail == 1 && self.dragging.is_some() {
                        self.dragging = None;
                        if self.drag_moved {
                            events.push(WinEvent::DragFinished);
                        }
                    }
                }
                Event::KeyPress(e) => {
                    if Some(e.detail) == self.hotkey {
                        events.push(WinEvent::Hotkey);
                    }
                }
                Event::Expose(_) => events.push(WinEvent::Redraw),
                Event::ConfigureNotify(e) => {
                    self.width = e.width.max(1);
                    self.height = e.height.max(1);
                    if self.dragging.is_none() {
                        self.x = e.x;
                        self.y = e.y;
                    }
                }
                Event::MappingNotify(_) => self.mapping_stale = true,
                Event::ClientMessage(e) => {
                    if e.data.as_data32()[0] == self.atoms.wm_delete {
                        events.push(WinEvent::Close);
                    }
                }
                Event::DestroyNotify(_) => events.push(WinEvent::Close),
                _ => {}
            }
        }
        events
    }

    /// 拖动时跟随指针（指针离开窗口也能继续拖）。
    pub fn drag_tick(&mut self) {
        let Some((ax, ay)) = self.dragging else { return };
        let Some((px, py)) = self.pointer() else { return };
        let nx = px - ax;
        let ny = py - ay;
        if (nx, ny) != (self.x, self.y) {
            self.drag_moved = true;
            self.move_resize(nx, ny, self.width, self.height);
        }
    }

    pub fn dragging(&self) -> bool {
        self.dragging.is_some()
    }

    pub fn raise(&self) {
        let _ = self.conn.configure_window(self.win, &ConfigureWindowAux::new().stack_mode(
            x11rb::protocol::xproto::StackMode::ABOVE,
        ));
        self.conn.flush().ok();
    }

    pub fn flush(&self) {
        self.conn.flush().ok();
    }

    pub fn set_scale(&mut self, width: u16, height: u16) {
        let (x, y) = (self.x, self.y);
        self.move_resize(x, y, width, height);
    }
}
