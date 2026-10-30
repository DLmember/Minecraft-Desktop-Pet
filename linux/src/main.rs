//! Minecraft 桌宠 · Linux 前端。
//! 复用 native/ 的原始 Rust 渲染器；本程序负责 X11 透明窗口、全局输入与 GTK 控制台。

mod autostart;
mod capture;
mod console;
mod evdev;
mod core;
mod input;
mod instance;
mod menu;
mod online;
mod petwin;
mod pointer;
mod platform;
mod settings;
mod tray;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::glib;
use gtk::prelude::*;
use serde_json::json;

use core::Core;
use input::{Input, InputEvent};
use petwin::{PetWindow, WinEvent};

pub struct App {
    pub core: Core,
    pub settings: serde_json::Value,
    pub pet: PetWindow,
    pub input: Input,
    pub key_map: petwin::KeyMap,
    pub shift_codes: Vec<u8>,
    pub shift_held: bool,
    start: Instant,
    last_tick: Instant,
    last_frame: Instant,
    awake_until: f32,
    last_cursor: (i16, i16),
    pub opacity: f32,
    pub hidden: bool,
    frame: Vec<u8>,
    pub console: Option<console::Console>,
    pub data_dir: PathBuf,
    save_generation: u64,
    last_local_click: f64,
    held: HashMap<u8, u32>,
    missing: HashMap<u8, u8>,
    pub online_status: String,
    pub startup_status: String,
    activate: std::sync::mpsc::Receiver<()>,
    pub tray: Option<tray::Tray>,
    pub quit: bool,
}

#[derive(Default)]
struct Args {
    data_dir: Option<PathBuf>,
    console: bool,
    autostart: bool,
    smoke: bool,
    render_png: Option<PathBuf>,
    capture: Option<(u32, PathBuf)>,
    probe_input: Option<f32>,
    probe_pointer: Option<f32>,
    import_skin: Option<PathBuf>,
    fetch_skin: Option<String>,
}

impl App {
    pub fn now(&self) -> f32 {
        self.start.elapsed().as_secs_f32()
    }

    pub fn wake(&mut self) {
        self.awake_until = self.now() + 0.85;
    }

    pub fn fps(&self) -> f32 {
        let fps = settings::get_f64(&self.settings, "fps");
        if fps < 30.0 {
            60.0
        } else {
            fps as f32
        }
    }

    pub fn scale(&self) -> f32 {
        settings::get_f64(&self.settings, "scale").clamp(0.65, 1.8) as f32
    }

    pub fn flag(&self, key: &str) -> bool {
        settings::get_bool(&self.settings, key)
    }

    pub fn alert(&self, text: &str) {
        let dialog = gtk::MessageDialog::new(
            None::<&gtk::Window>,
            gtk::DialogFlags::MODAL,
            gtk::MessageType::Warning,
            gtk::ButtonsType::Ok,
            text,
        );
        dialog.set_title("Minecraft桌宠");
        dialog.connect_response(|d, _| d.close());
        dialog.show_all();
    }

    pub fn show_core_error(&self) {
        let message = self.core.error();
        self.alert(if message.is_empty() {
            "操作失败。"
        } else {
            &message
        });
    }

    fn apply_window(&mut self) {
        let scale = self.scale();
        let width = (360.0 * scale).round().clamp(120.0, 3000.0) as u16;
        let height = (420.0 * scale).round().clamp(140.0, 3500.0) as u16;
        let (x, y) = self.pet.position();
        self.pet.move_resize(x, y, width, height);
        self.pet.set_above(self.flag("alwaysOnTop"));
        self.pet.set_click_through(self.flag("clickThrough"));
        if self.hidden {
            self.pet.unmap();
        } else {
            self.pet.map();
        }
        self.wake();
    }

    pub fn preview(&mut self) {
        if !self.core.preview(&self.settings) {
            let message = self.core.error();
            if !message.is_empty() {
                self.alert(&format!("设置预览失败：{message}"));
            }
        }
    }

    pub fn save(&mut self) {
        let previous = self.settings.clone();
        if !self.core.apply(&self.settings) {
            let message = self.core.error();
            // 核心可能拒绝并清理了非法值，回读真实设置，避免界面与文件不一致。
            self.settings = self.core.settings().unwrap_or(previous);
            self.refresh_console();
            self.alert(&format!("保存设置失败：{message}"));
            return;
        }
        self.refresh_console();
    }

    /// 控件变化后的统一入口：即时预览、应用窗口属性、防抖保存。
    pub fn changed(&mut self) {
        self.preview();
        if !self.flag("keyboardEnabled") {
            self.core.clear_keys();
            self.held.clear();
        }
        self.apply_window();
        self.refresh_console();
        self.save_generation += 1;
        let generation = self.save_generation;
        let weak = APP_WEAK.with(|cell| cell.borrow().clone());
        glib::timeout_add_local_once(Duration::from_millis(250), move || {
            if let Some(rc) = weak.upgrade() {
                let mut app = rc.borrow_mut();
                if app.save_generation == generation {
                    app.save();
                }
            }
        });
    }

    /// 登录启动与进程优先级需要立即落到系统，而不是只写设置。
    pub fn apply_startup_settings(&mut self) {
        let enabled = self.flag("autostart");
        let mut notes = Vec::new();
        match autostart::set_enabled(enabled, &autostart::exe_path(), &self.data_dir) {
            Ok(()) => notes.push(if enabled {
                "登录启动：已启用".to_string()
            } else {
                "登录启动：已关闭".to_string()
            }),
            Err(e) => notes.push(e),
        }
        let priority = settings::get_str(&self.settings, "startupPriority");
        match platform::apply_priority(&priority) {
            Ok(()) => notes.push(format!("进程优先级：{priority}")),
            Err(e) => notes.push(e),
        }
        notes.push("登录启动与延迟在下次登录时生效。".to_string());
        self.startup_status = notes.join(" · ");
        self.refresh_console();
    }

    pub fn refresh_console(&mut self) {
        if let Some(console) = &self.console {
            console.update(self);
        }
    }

    pub fn save_position(&mut self) {
        let (x, y) = self.pet.position();
        settings::set(&mut self.settings, "position", json!({ "x": x, "y": y }));
        self.save();
    }

    fn constrain(&mut self) {
        let (sw, sh) = self.pet.screen_size();
        let (mut x, mut y) = self.pet.position();
        let width = self.pet.width as i16;
        x = x.clamp(-width + 60, sw as i16 - 60);
        y = y.clamp(0, (sh as i16 - 40).max(0));
        self.pet.move_resize(x, y, self.pet.width, self.pet.height);
    }

    pub fn recenter(&mut self) {
        let (sw, sh) = self.pet.screen_size();
        let x = (sw as i16 - self.pet.width as i16) / 2;
        let y = (sh as i16 - self.pet.height as i16) / 2;
        self.pet
            .move_resize(x, y, self.pet.width, self.pet.height);
        self.save_position();
        self.wake();
    }

    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
        if hidden {
            self.pet.unmap();
        } else {
            self.pet.map();
        }
        self.wake();
        self.refresh_console();
    }

    pub fn toggle_console(&mut self, rc: &Rc<RefCell<App>>) {
        if let Some(console) = &self.console {
            if console.window.is_visible() {
                console.window.hide();
            } else {
                console.window.show_all();
                console.window.present();
            }
            return;
        }
        self.open_console(rc);
    }

    pub fn open_console(&mut self, rc: &Rc<RefCell<App>>) {
        if let Some(console) = &self.console {
            console.window.show_all();
            console.window.present();
            return;
        }
        let weak = Rc::downgrade(rc);
        let console = console::build(weak, &self.settings, &self.data_dir);
        console.update(self);
        console.window.show_all();
        console.window.present();
        self.console = Some(console);
    }

    pub fn close_console(&mut self) {
        if let Some(console) = self.console.take() {
            console.window.close();
        }
    }

    pub fn import_skin(&mut self, path: &Path) {
        if !self.core.import(path) {
            self.show_core_error();
            return;
        }
        self.reload_settings();
        self.wake();
    }

    pub fn reload_settings(&mut self) {
        if let Some(value) = self.core.settings() {
            self.settings = value;
        }
        self.apply_window();
        self.refresh_console();
    }

    pub fn apply_fetched(&mut self, fetched: online::Fetched) -> Result<(), String> {
        let result = import_fetched(&mut self.core, &self.data_dir, fetched);
        if result.is_ok() {
            self.reload_settings();
            self.wake();
        }
        result
    }

    pub fn reset_settings(&mut self) {
        if !self.core.reset() {
            self.show_core_error();
            return;
        }
        let _ = autostart::set_enabled(false, &autostart::exe_path(), &self.data_dir);
        let _ = platform::apply_priority("normal");
        self.reload_settings();
        self.recenter();
        self.close_console();
        self.startup_status.clear();
    }

    fn point_in_window(&self, x: i16, y: i16) -> bool {
        let (wx, wy) = self.pet.position();
        x >= wx
            && y >= wy
            && x < wx + self.pet.width as i16
            && y < wy + self.pet.height as i16
    }

    fn tray_state(&self) -> tray::TrayState {
        tray::TrayState {
            click_through: self.flag("clickThrough"),
            above: self.flag("alwaysOnTop"),
            hidden: self.hidden,
        }
    }

    fn set_flag(&mut self, key: &str, on: bool) {
        settings::set(&mut self.settings, key, json!(on));
        self.changed();
        self.save();
    }

    /// 处理托盘命令，并把其他入口改动的设置同步回托盘菜单。
    fn handle_tray(&mut self, rc: &Rc<RefCell<App>>) {
        let Some(tray) = &self.tray else { return };
        let cmds = tray.poll();
        for cmd in cmds {
            match cmd {
                tray::TrayCmd::Console => {
                    self.set_hidden(false);
                    self.open_console(rc);
                }
                tray::TrayCmd::ClickThrough(on) => self.set_flag("clickThrough", on),
                tray::TrayCmd::Above(on) => self.set_flag("alwaysOnTop", on),
                tray::TrayCmd::ToggleHidden => {
                    let next = !self.hidden;
                    self.set_hidden(next);
                }
                tray::TrayCmd::Recenter => self.recenter(),
                tray::TrayCmd::ImportSkin => {
                    // tick 正持有 App 的可变借用，对话框放到下一轮主循环再打开
                    let rc = rc.clone();
                    glib::idle_add_local_once(move || menu::import_dialog(&rc));
                }
                tray::TrayCmd::Quit => self.quit = true,
            }
        }
        let state = self.tray_state();
        if let Some(tray) = &mut self.tray {
            if tray.alive() {
                tray.sync(state);
            } else {
                // 托盘宿主退出（例如 plasmashell 崩溃）：打开控制台，保证还有入口
                self.tray = None;
                self.open_console(rc);
            }
        }
    }

    pub fn swing(&mut self) {
        let now = self.now();
        self.core.click(now);
        self.last_local_click = now as f64;
        self.wake();
    }

    fn handle_window_events(&mut self, rc: &Rc<RefCell<App>>, events: Vec<WinEvent>) {
        for event in events {
            match event {
                WinEvent::Click => self.swing(),
                WinEvent::DoubleClick => self.toggle_console(rc),
                WinEvent::ContextMenu => menu::show(rc),
                WinEvent::DragFinished => self.save_position(),
                WinEvent::Hotkey => self.toggle_console(rc),
                WinEvent::Redraw => {
                    let opacity = self.opacity;
                    self.pet.present(&self.frame, opacity);
                }
                WinEvent::Close => self.quit = true,
            }
        }
    }

    fn handle_input(&mut self) {
        let now = self.now();
        for event in self.input.poll() {
            match event {
                InputEvent::Key { code, down } => {
                    if self.shift_codes.contains(&code) {
                        self.shift_held = down;
                    }
                    let vk = self
                        .key_map
                        .keysym_of
                        .get(&code)
                        .copied()
                        .and_then(vk_for_keysym);
                    if down {
                        self.wake();
                        self.missing.remove(&code);
                    }
                    if !self.flag("keyboardEnabled") {
                        continue;
                    }
                    if let Some(vk) = vk {
                        self.core.key(vk, down, now);
                        if down {
                            self.held.insert(code, vk);
                        } else {
                            self.held.remove(&code);
                        }
                    }
                }
                InputEvent::Button => {
                    let (px, py) = self.pet.pointer().unwrap_or((-1, -1));
                    let on_pet = self.point_in_window(px, py) && !self.flag("clickThrough");
                    if !on_pet && (now as f64 - self.last_local_click) > 0.12 {
                        self.swing();
                    }
                }
            }
        }
    }

    /// 周期性核对真实键盘状态，修复丢失的 key-up（例如切换 VT / 锁屏）。
    fn recover_keys(&mut self) {
        if self.held.is_empty() {
            return;
        }
        let now = self.now();
        for (&code, &vk) in self.held.clone().iter() {
            let down = self
                .input
                .evdev_down(code)
                .unwrap_or_else(|| self.pet.key_down(code));
            if down {
                self.missing.remove(&code);
                continue;
            }
            let misses = self.missing.entry(code).or_insert(0);
            *misses += 1;
            if *misses >= 2 {
                self.core.key(vk, false, now);
                self.held.remove(&code);
                self.missing.remove(&code);
            }
        }
    }

    fn tick(&mut self, rc: &Rc<RefCell<App>>) {
        let instant = Instant::now();
        let dt = (instant - self.last_tick).as_secs_f32().clamp(0.0005, 0.1);
        let resumed = instant - self.last_frame > Duration::from_secs(1);
        self.last_tick = instant;

        if resumed {
            self.core.clear_keys();
            self.held.clear();
            self.missing.clear();
            self.wake();
        }
        self.last_frame = instant;

        while self.activate.try_recv().is_ok() {
            self.set_hidden(false);
            self.open_console(rc);
            self.pet.raise();
        }

        self.handle_tray(rc);

        self.pet.drag_tick();
        let events = self.pet.poll(self.now() as f64);
        self.handle_window_events(rc, events);
        if self.pet.mapping_stale() {
            self.key_map = self.pet.key_map();
            self.shift_codes = self.key_map.shift_codes.clone();
        }
        self.handle_input();
        self.recover_keys();

        if self.quit {
            gtk::main_quit();
            return;
        }

        let (px, py) = self.pet.pointer().unwrap_or(self.last_cursor);
        if (px, py) != self.last_cursor {
            self.last_cursor = (px, py);
            self.wake();
        }
        let (wx, wy) = self.pet.position();
        let width = self.pet.width.max(1) as f32;
        let height = self.pet.height.max(1) as f32;
        let rel_x = (px - wx) as f32;
        let rel_y = (py - wy) as f32;
        let over = self.point_in_window(px, py)
            && self.core.hit(rel_x / width, rel_y / height);

        let target = if self.flag("clickThrough") && over {
            0.25
        } else {
            1.0
        };
        if std::env::var_os("PET_DEBUG_POINTER").is_some() {
            eprintln!(
                "ptr=({px},{py}) win=({wx},{wy}) {width}x{height} over={over} ct={} opacity={:.3}",
                self.flag("clickThrough"),
                self.opacity
            );
        }
        let fading = (target - self.opacity).abs() > 0.002;
        if fading {
            self.opacity += (target - self.opacity) * (dt / 0.045).min(1.0);
        } else {
            self.opacity = target;
        }

        let now = self.now();
        let awake = now <= self.awake_until;
        if !self.hidden && (awake || fading) {
            if awake {
                let shift = self.shift_held;
                let opacity = self.opacity;
                let mut frame = std::mem::take(&mut self.frame);
                if self.core.render_into(
                    rel_x,
                    rel_y,
                    width,
                    height,
                    shift,
                    now,
                    dt,
                    &mut frame,
                ) {
                    self.frame = frame;
                }
                self.pet.present(&self.frame, opacity);
            } else {
                let opacity = self.opacity;
                self.pet.present(&self.frame, opacity);
            }
        }

    }

    fn awake(&self) -> bool {
        self.now() <= self.awake_until + 0.02
    }
}

thread_local! {
    /// 供 changed() 的防抖保存回调升级到 App（只存 Weak，避免引用环）。
    static APP_WEAK: RefCell<std::rc::Weak<RefCell<App>>> = RefCell::new(std::rc::Weak::new());
}

fn vk_for_keysym(keysym: u32) -> Option<u32> {
    Some(match keysym {
        0x20 => 32,
        0xff0d | 0xff8d => 13,
        0xff08 => 8,
        0xff09 => 9,
        0xff1b => 27,
        0xffe1 => 160,
        0xffe2 => 161,
        0xffe3 => 162,
        0xffe4 => 163,
        0xffe9 => 164,
        0xffea => 165,
        0xffeb => 91,
        0xffec => 92,
        0xffe5 => 20,
        0xff51 => 37,
        0xff52 => 38,
        0xff53 => 39,
        0xff54 => 40,
        0xff50 => 36,
        0xff57 => 35,
        0xff55 => 33,
        0xff56 => 34,
        0xffff => 46,
        0xff63 => 45,
        0xffbe..=0xffc9 => 112 + (keysym - 0xffbe),
        0xff95..=0xff9e => 96 + (keysym - 0xff95),
        0xffaa => 106,
        0xffab => 107,
        0xffad => 109,
        0xffaf => 111,
        0xffae => 110,
        // 可打印 ASCII（含标点）与 Windows 虚拟键一致
        0x21..=0x7e => match keysym {
            0x3b => 186,
            0x3d => 187,
            0x2c => 188,
            0x2d => 189,
            0x2e => 190,
            0x2f => 191,
            0x60 => 192,
            0x5b => 219,
            0x5c => 220,
            0x5d => 221,
            0x27 => 222,
            c => u32::from((c as u8).to_ascii_uppercase()),
        },
        _ => return None,
    })
}

fn schedule(rc: Rc<RefCell<App>>) {
    let delay = {
        let app = rc.borrow();
        if app.awake() {
            Duration::from_secs_f32((1.0 / app.fps()).max(0.001))
        } else {
            Duration::from_millis(50)
        }
    };
    let next = rc.clone();
    glib::timeout_add_local_once(delay, move || {
        {
            let mut app = rc.borrow_mut();
            app.tick(&next);
        }
        schedule(next);
    });
}

/// 把在线获取的皮肤/披风写入临时文件并交给核心导入。
fn import_fetched(
    core: &mut Core,
    data_dir: &Path,
    fetched: online::Fetched,
) -> Result<(), String> {
    let stamp = format!(
        "{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );
    let skin_path = data_dir.join(format!("download-skin-{stamp}.png"));
    let cape_path = data_dir.join(format!("download-cape-{stamp}.png"));
    std::fs::write(&skin_path, &fetched.skin).map_err(|e| e.to_string())?;
    if let Some(cape) = &fetched.cape {
        std::fs::write(&cape_path, cape).map_err(|e| e.to_string())?;
    }
    let cape_opt = fetched.cape.as_ref().map(|_| cape_path.as_path());
    let ok = core.import_profile(
        &skin_path,
        cape_opt,
        Some(&fetched.name),
        Some(&fetched.model),
    );
    let message = core.error();
    let _ = std::fs::remove_file(&skin_path);
    let _ = std::fs::remove_file(&cape_path);
    if ok {
        Ok(())
    } else if message.is_empty() {
        Err("纹理无法识别。".into())
    } else {
        Err(message)
    }
}

/// 无界面模式：导入本地皮肤或在线获取（用于脚本与自检）。
fn headless(args: &Args) {
    let dir = args
        .data_dir
        .clone()
        .unwrap_or_else(platform::data_dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("无法创建设置目录 {}：{e}", dir.display());
        std::process::exit(1);
    }
    let mut core = match Core::new(&dir) {
        Ok(core) => core,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    if let Some(path) = &args.import_skin {
        if !core.import(path) {
            eprintln!("导入失败：{}", core.error());
            std::process::exit(1);
        }
    }
    if let Some(name) = &args.fetch_skin {
        let user_agent = format!("MinecraftDesktopPet/{}-Linux", platform::version());
        let result = online::fetch(name, &user_agent)
            .and_then(|fetched| import_fetched(&mut core, &dir, fetched));
        if let Err(e) = result {
            eprintln!("获取失败：{e}");
            std::process::exit(1);
        }
    }
    let value = core.settings().unwrap_or_else(|| json!({}));
    println!("皮肤：{}", settings::get_str(&value, "skinName"));
    println!("披风：{}", settings::get_str(&value, "capePath"));
}

fn smoke(args: &Args) {
    let dir = args
        .data_dir
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("minecraft-pet-smoke"));
    std::fs::create_dir_all(&dir).ok();
    let mut core = match Core::new(&dir) {
        Ok(core) => core,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let mut frame = vec![0u8; core::FRAME_W * core::FRAME_H * 4];
    let mut opaque = 0usize;
    for step in 0..12 {
        let now = step as f32 / 30.0;
        if step == 4 {
            core.click(now);
        }
        if step == 6 {
            core.key(81, true, now);
        }
        if !core.render_into(180.0, 200.0, 360.0, 420.0, false, now, 1.0 / 30.0, &mut frame) {
            eprintln!("渲染失败");
            std::process::exit(1);
        }
    }
    for pixel in frame.chunks_exact(4) {
        if pixel[3] != 0 {
            opaque += 1;
        }
    }
    let png = args
        .render_png
        .clone()
        .unwrap_or_else(|| dir.join("smoke-frame.png"));
    // core 帧为预乘 BGRA，PNG 需要直通 RGBA
    let rgba: Vec<u8> = frame
        .chunks_exact(4)
        .flat_map(|p| {
            let a = p[3] as u32;
            let c = |v: u8| if a == 0 { 0 } else { (v as u32 * 255 / a).min(255) as u8 };
            [c(p[2]), c(p[1]), c(p[0]), p[3]]
        })
        .collect();
    if let Err(e) = image::save_buffer(
        &png,
        &rgba,
        core::FRAME_W as u32,
        core::FRAME_H as u32,
        image::ColorType::Rgba8,
    ) {
        eprintln!("无法写出 PNG：{e}");
    }
    let report = json!({
        "version": platform::version(),
        "width": core::FRAME_W,
        "height": core::FRAME_H,
        "opaquePixels": opaque,
        "frame": png,
        "settings": core.settings(),
    });
    println!("{}", serde_json::to_string_pretty(&report).unwrap_or_default());
}

fn parse_window_id(text: &str) -> Option<u32> {
    let text = text.trim();
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => text.parse::<u32>().ok(),
    }
}

fn probe_pointer(seconds: f32) {
    let bridge = match pointer::Bridge::start() {
        Ok(bridge) => bridge,
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };
    let latest = bridge.latest();
    use x11rb::connection::Connection as _;
    use x11rb::protocol::xproto::ConnectionExt as _;
    let (conn, screen_num) = match x11rb::connect(None) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("无法连接 X11：{e}");
            return;
        }
    };
    let root = conn.setup().roots[screen_num].root;
    let start = Instant::now();
    while start.elapsed().as_secs_f32() < seconds {
        let x11 = conn
            .query_pointer(root)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| (r.root_x, r.root_y));
        println!(
            "t={:.1}s kwin={:?} x11={:?}",
            start.elapsed().as_secs_f32(),
            latest.get(),
            x11
        );
        std::thread::sleep(Duration::from_millis(500));
    }
    println!("已停止（脚本随进程退出自动卸载）");
}

fn probe_input(seconds: f32) {
    let mut input = Input::start();
    let start = Instant::now();
    let mut count = 0usize;
    while start.elapsed().as_secs_f32() < seconds {
        for event in input.poll() {
            count += 1;
            match event {
                InputEvent::Key { code, down } => println!("key code={code} down={down}"),
                InputEvent::Button => println!("button"),
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    println!("{}", input.status);
    println!("{}", input.evdev_status);
    println!("共收到 {count} 个事件");
}

fn parse_args() -> Args {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--data-dir" => args.data_dir = iter.next().map(PathBuf::from),
            "--console" | "--show-settings" => args.console = true,
            "--autostart" => args.autostart = true,
            "--smoke-test" => args.smoke = true,
            "--render-png" => args.render_png = iter.next().map(PathBuf::from),
            "--capture" => {
                let id = iter.next().and_then(|text| parse_window_id(&text));
                let path = iter.next().map(PathBuf::from);
                match (id, path) {
                    (Some(id), Some(path)) => args.capture = Some((id, path)),
                    _ => eprintln!("用法：--capture <窗口ID> <输出.png>"),
                }
            }
            "--probe-pointer" => {
                args.probe_pointer = iter
                    .next()
                    .and_then(|text| text.parse::<f32>().ok())
                    .or(Some(5.0));
            }
            "--probe-input" => {
                args.probe_input = iter
                    .next()
                    .and_then(|text| text.parse::<f32>().ok())
                    .or(Some(3.0));
            }
            "--import-skin" => args.import_skin = iter.next().map(PathBuf::from),
            "--fetch-skin" => args.fetch_skin = iter.next(),
            "--version" | "-V" => {
                println!("Minecraft桌宠 {}", platform::version());
                std::process::exit(0);
            }
            "--help" | "-h" => {
                println!(
                    "Minecraft桌宠 {} (Linux)\n\
                     用法: minecraft-desktop-pet [--console] [--data-dir DIR] [--autostart]\n\
                     \n\
                     Ctrl+Alt+P 打开/隐藏控制台；双击或右键桌宠也能打开控制台。",
                    platform::version()
                );
                std::process::exit(0);
            }
            other => eprintln!("忽略未知参数：{other}"),
        }
    }
    args
}

fn main() {
    let args = parse_args();
    if args.smoke {
        smoke(&args);
        return;
    }
    if let Some((window, path)) = args.capture.clone() {
        if let Err(e) = capture::capture(window, &path) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(seconds) = args.probe_pointer {
        probe_pointer(seconds);
        return;
    }
    if let Some(seconds) = args.probe_input {
        probe_input(seconds);
        return;
    }
    if args.import_skin.is_some() || args.fetch_skin.is_some() {
        headless(&args);
        return;
    }
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!(
            "Minecraft桌宠需要 X11 显示（Wayland 会话请通过 XWayland 运行：DISPLAY 必须存在）。"
        );
        std::process::exit(1);
    }
    // 桌宠窗口、XRecord 与弹出菜单都基于 X11；让 GTK 也走 XWayland，
    // 避免菜单在 Wayland 下拿不到全局指针位置。
    if std::env::var_os("GDK_BACKEND").is_none() {
        std::env::set_var("GDK_BACKEND", "x11");
    }
    if gtk::init().is_err() {
        eprintln!("无法初始化 GTK3。请安装 gtk3 运行库。");
        std::process::exit(1);
    }

    let data_dir = args
        .data_dir
        .clone()
        .unwrap_or_else(platform::data_dir);
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        eprintln!("无法创建设置目录 {}：{e}", data_dir.display());
        std::process::exit(1);
    }
    let (instance_path, activate_rx) = match instance::claim(&data_dir) {
        Ok(instance::Claim::Primary(single)) => (Some(single.path), single.rx),
        Ok(instance::Claim::Secondary) => {
            println!("Minecraft桌宠已在运行，已请求它打开控制台。");
            return;
        }
        Err(e) => {
            eprintln!("{e}");
            (None, std::sync::mpsc::channel().1)
        }
    };

    let core = match Core::new(&data_dir) {
        Ok(core) => core,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let settings_value = core.settings().unwrap_or_else(|| json!({}));

    let scale = settings::get_f64(&settings_value, "scale").clamp(0.65, 1.8) as f32;
    let width = (360.0 * scale).round() as u16;
    let height = (420.0 * scale).round() as u16;
    let pet = match PetWindow::new(width, height, 0, 0) {
        Ok(pet) => pet,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };

    // Wayland：XWayland 只在指针位于 X11 窗口上时更新位置，靠 KWin 脚本补全屏跟随。
    let pointer_bridge = match pointer::Bridge::start() {
        Ok(bridge) => Some(bridge),
        Err(e) => {
            // X11 会话下不需要桥接，只有 Wayland 才值得提示
            if std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
                || std::env::var_os("WAYLAND_DISPLAY").is_some()
            {
                eprintln!("{e}");
            }
            None
        }
    };

    let key_map = pet.key_map();
    let shift_codes = key_map.shift_codes.clone();
    let saved = settings::get(&settings_value, "position");
    let (sw, sh) = pet.screen_size();
    let (x, y) = match (saved.get("x").and_then(|v| v.as_f64()), saved.get("y").and_then(|v| v.as_f64())) {
        (Some(x), Some(y)) => (x as i16, y as i16),
        _ => (
            (sw as i16 - width as i16 - 40).max(0),
            (sh as i16 - height as i16 - 30).max(0),
        ),
    };

    let mut app = App {
        core,
        settings: settings_value,
        pet,
        input: Input::start(),
        key_map,
        shift_codes,
        shift_held: false,
        start: Instant::now(),
        last_tick: Instant::now(),
        last_frame: Instant::now(),
        awake_until: 1.0,
        last_cursor: (-1, -1),
        opacity: 1.0,
        hidden: false,
        frame: vec![0u8; core::FRAME_W * core::FRAME_H * 4],
        console: None,
        data_dir: data_dir.clone(),
        save_generation: 0,
        last_local_click: -1.0,
        held: HashMap::new(),
        missing: HashMap::new(),
        online_status: "仅主动获取时连接 Mojang 官方资料与纹理服务。".into(),
        startup_status: String::new(),
        activate: activate_rx,
        tray: None,
        quit: false,
    };
    app.pet.move_resize(x, y, width, height);
    if let Some(bridge) = &pointer_bridge {
        app.pet.set_pointer_source(bridge.latest());
    }
    app.constrain();
    app.apply_window();

    if let Err(e) = platform::apply_priority(&settings::get_str(&app.settings, "startupPriority")) {
        eprintln!("{e}");
    }
    if autostart::is_enabled() != app.flag("autostart") {
        // 设置与桌面文件保持一致（移动可执行文件后自动纠正路径）。
        if let Err(e) = autostart::set_enabled(
            app.flag("autostart"),
            &autostart::exe_path(),
            &app.data_dir,
        ) {
            eprintln!("{e}");
        }
    }

    let rc = Rc::new(RefCell::new(app));
    APP_WEAK.with(|cell| {
        *cell.borrow_mut() = Rc::downgrade(&rc);
    });

    // 托盘：与 Windows 版一致，控制台关闭后隐藏到托盘。
    {
        let mut app = rc.borrow_mut();
        let state = app.tray_state();
        match tray::Tray::start(state, &platform::version()) {
            Ok(tray) => app.tray = Some(tray),
            Err(e) => eprintln!("{e}"),
        }
    }

    // 与 Windows 版相同：托盘不可用时打开控制台，避免用户找不到入口。
    let needs_console = {
        let app = rc.borrow();
        args.console || (app.tray.is_none() && !args.autostart)
    };
    if needs_console {
        rc.borrow_mut().open_console(&rc);
    }

    // 登录启动延迟：先隐藏，到点再显示（手动启动立即显示）。
    let delay = if args.autostart {
        settings::get(&rc.borrow().settings, "autostartDelay")
            .as_u64()
            .unwrap_or(0)
            .min(120)
    } else {
        0
    };
    if delay > 0 {
        rc.borrow_mut().pet.unmap();
        let wake = rc.clone();
        glib::timeout_add_local_once(Duration::from_secs(delay), move || {
            let mut app = wake.borrow_mut();
            app.pet.map();
            app.wake();
        });
    }

    schedule(rc.clone());

    for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        let quit_rc = rc.clone();
        glib::unix_signal_add_local(signal, move || {
            quit_rc.borrow_mut().quit = true;
            gtk::main_quit();
            glib::ControlFlow::Continue
        });
    }

    gtk::main();

    {
        let mut app = rc.borrow_mut();
        app.save_position();
        app.input.stop();
    }
    if let Some(path) = instance_path {
        let _ = std::fs::remove_file(path);
    }
}
