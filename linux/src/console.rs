//! GTK3 控制台：与 macOS 版对应的六页设置界面，直接编辑核心的设置 JSON。

use std::cell::RefCell;
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;
use serde_json::{json, Value};

use crate::platform;
use crate::settings;
use crate::App;

pub struct Console {
    pub window: gtk::Window,
    health: gtk::Label,
    skin_summary: gtk::Label,
    online_status: gtk::Label,
    startup_status: gtk::Label,
    pending: Rc<RefCell<Option<std::sync::mpsc::Receiver<Result<crate::online::Fetched, String>>>>>,
}

impl Console {
    pub fn update(&self, app: &App) {
        self.window
            .set_title(&format!("Minecraft桌宠 v{} · Linux", platform::version()));
        self.health.set_text(&format!(
            "{}\n{}\n收到按下 {} / 松开 {} · 全局点击 {}",
            app.input.status,
            app.input.evdev_status,
            app.input.key_downs,
            app.input.key_ups,
            app.input.clicks
        ));
        let skin = settings::get_str(&app.settings, "skinName");
        self.skin_summary.set_text(&format!(
            "当前皮肤：{}\n支持 64×64、64×32 及等比例高清 PNG（最大 1024×1024、8 MB）。",
            if skin.is_empty() { "内置" } else { &skin }
        ));
        self.online_status.set_text(&app.online_status);
        self.startup_status.set_text(&app.startup_status);
    }
}

fn fmt(key: &str, value: f64) -> String {
    if key == "scale" {
        format!("{value:.2}")
    } else {
        format!("{value:.1}")
    }
}

fn note(parent: &gtk::Box, text: &str) {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_line_wrap(true);
    label.style_context().add_class("dim-label");
    parent.pack_start(&label, false, false, 0);
}

fn page(notebook: &gtk::Notebook, title: &str) -> gtk::Box {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(14);
    content.set_margin_end(14);
    let scroll = gtk::ScrolledWindow::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    scroll.set_policy(gtk::PolicyType::Never, gtk::PolicyType::Automatic);
    scroll.add(&content);
    notebook.append_page(&scroll, Some(&gtk::Label::new(Some(title))));
    content
}

fn current_text(value: &Value, key: &str) -> String {
    match settings::get(value, key) {
        Value::String(text) => text,
        Value::Number(number) => number.to_string(),
        _ => String::new(),
    }
}

fn add_check(
    parent: &gtk::Box,
    weak: &Weak<RefCell<App>>,
    initial: &Value,
    key: &str,
    title: &str,
    enabled: bool,
) {
    let button = gtk::CheckButton::with_label(title);
    button.set_active(settings::get_bool(initial, key));
    button.set_sensitive(enabled);
    let weak = weak.clone();
    let key = key.to_string();
    button.connect_toggled(move |b| {
        let Some(rc) = weak.upgrade() else { return };
        let mut app = rc.borrow_mut();
        settings::set(&mut app.settings, &key, json!(b.is_active()));
        app.changed();
        if key == "autostart" {
            app.apply_startup_settings();
            app.save();
        }
    });
    parent.pack_start(&button, false, false, 0);
}

fn add_combo(
    parent: &gtk::Box,
    weak: &Weak<RefCell<App>>,
    initial: &Value,
    key: &str,
    title: &str,
    items: &[(&str, &str)],
    numeric: bool,
    enabled: bool,
) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.set_width_chars(20);
    let combo = gtk::ComboBoxText::new();
    for (id, text) in items {
        combo.append(Some(id), text);
    }
    let current = current_text(initial, key);
    if !current.is_empty() {
        combo.set_active_id(Some(&current));
    }
    combo.set_hexpand(true);
    combo.set_sensitive(enabled);
    row.pack_start(&label, false, false, 0);
    row.pack_start(&combo, true, true, 0);
    parent.pack_start(&row, false, false, 0);

    let weak = weak.clone();
    let key = key.to_string();
    combo.connect_changed(move |c| {
        let Some(id) = c.active_id() else { return };
        let Some(rc) = weak.upgrade() else { return };
        let mut app = rc.borrow_mut();
        let id = id.to_string();
        let value = if numeric {
            json!(id.parse::<u32>().unwrap_or(0))
        } else {
            json!(id)
        };
        settings::set(&mut app.settings, &key, value);
        app.changed();
        if key == "startupPriority" {
            app.apply_startup_settings();
            app.save();
        }
    });
}

fn add_slider(
    parent: &gtk::Box,
    weak: &Weak<RefCell<App>>,
    initial: &Value,
    key: &str,
    title: &str,
    min: f64,
    max: f64,
    step: f64,
) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let label = gtk::Label::new(Some(title));
    label.set_xalign(0.0);
    label.set_width_chars(20);
    let current = settings::get_f64(initial, key);
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, min, max, step);
    scale.set_value(current);
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    let value = gtk::Label::new(Some(&fmt(key, current)));
    value.set_width_chars(6);
    value.set_xalign(1.0);
    row.pack_start(&label, false, false, 0);
    row.pack_start(&scale, true, true, 0);
    row.pack_start(&value, false, false, 0);
    parent.pack_start(&row, false, false, 0);

    let weak = weak.clone();
    let key = key.to_string();
    let value_label = value.clone();
    scale.connect_value_changed(move |s| {
        let v = s.value();
        value_label.set_text(&fmt(&key, v));
        let Some(rc) = weak.upgrade() else { return };
        let mut app = rc.borrow_mut();
        settings::set(&mut app.settings, &key, json!(v));
        app.changed();
    });
}

fn action_button(parent: &gtk::Box, title: &str, action: impl Fn() + 'static) {
    let button = gtk::Button::with_label(title);
    button.set_halign(gtk::Align::Start);
    button.connect_clicked(move |_| action());
    parent.pack_start(&button, false, false, 2);
}

pub fn build(weak: Weak<RefCell<App>>, initial: &Value, data_dir: &Path) -> Console {
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title(&format!("Minecraft桌宠 v{} · Linux", platform::version()));
    window.set_default_size(680, 760);
    window.connect_delete_event(|w, _| {
        w.hide();
        glib::Propagation::Stop
    });

    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    root.set_margin_top(14);
    root.set_margin_bottom(10);
    root.set_margin_start(16);
    root.set_margin_end(16);

    let title = gtk::Label::new(None);
    title.set_markup("<b>MINECRAFT  桌宠</b>");
    title.set_xalign(0.0);
    root.pack_start(&title, false, false, 0);
    let subtitle = gtk::Label::new(Some(
        "原版 Rust 渲染器 · X11 透明窗口 · GTK3 控制台 · 设置自动保存",
    ));
    subtitle.set_xalign(0.0);
    subtitle.style_context().add_class("dim-label");
    root.pack_start(&subtitle, false, false, 0);

    let notebook = gtk::Notebook::new();
    notebook.set_vexpand(true);
    root.pack_start(&notebook, true, true, 4);

    // ---- 基本 ----
    let basic = page(&notebook, "基本");
    add_combo(
        &basic,
        &weak,
        initial,
        "posture",
        "姿态",
        &[("standing", "站立"), ("sitting", "坐下"), ("custom", "自定义")],
        false,
        true,
    );
    add_combo(
        &basic,
        &weak,
        initial,
        "armModel",
        "手臂模型",
        &[("classic", "经典 · 4 像素"), ("slim", "纤细 · 3 像素")],
        false,
        true,
    );
    add_combo(
        &basic,
        &weak,
        initial,
        "fps",
        "帧率上限",
        &[
            ("30", "30"),
            ("60", "60"),
            ("90", "90"),
            ("120", "120"),
            ("144", "144"),
            ("165", "165"),
            ("240", "240"),
        ],
        true,
        true,
    );
    add_slider(&basic, &weak, initial, "scale", "桌宠缩放", 0.65, 1.8, 0.05);
    add_check(&basic, &weak, initial, "alwaysOnTop", "始终置顶", true);
    add_check(
        &basic,
        &weak,
        initial,
        "clickThrough",
        "完全鼠标穿透（悬停淡化，Ctrl+Alt+P 打开控制台）",
        true,
    );
    add_check(&basic, &weak, initial, "trackMouse", "头部跟随鼠标", true);
    add_check(&basic, &weak, initial, "outerLayer", "显示皮肤外层", true);
    add_check(
        &basic,
        &weak,
        initial,
        "keyboardEnabled",
        "全局键盘互动 / Shift 潜行",
        true,
    );
    add_combo(
        &basic,
        &weak,
        initial,
        "typingMode",
        "打字动作",
        &[("precise", "按键位置精准敲击"), ("alternating", "双手交替")],
        false,
        true,
    );
    add_combo(
        &basic,
        &weak,
        initial,
        "mainHand",
        "点击挥动主手",
        &[
            ("right", "右手"),
            ("left", "左手"),
            ("mapped", "按映射"),
            ("reverse", "反向映射"),
        ],
        false,
        true,
    );
    add_slider(
        &basic,
        &weak,
        initial,
        "typingSquash",
        "打字下压幅度 %",
        0.0,
        30.0,
        0.5,
    );
    note(
        &basic,
        "取消穿透后可拖动桌宠；双击或右键打开控制台。静止时暂停渲染。",
    );

    // ---- 输入与聊天 ----
    let input_page = page(&notebook, "输入与聊天");
    add_check(
        &input_page,
        &weak,
        initial,
        "hypQuickChat",
        "hyp 快速队伍聊天：# → /p chat （Linux 暂不支持）",
        false,
    );
    note(
        &input_page,
        "Linux 版使用 XRecord 被动监听全局按键：只读、不需要 root、不抢焦点。\
         它只能看到 X11 客户端的按键，因此 Wayland 原生窗口（以及运行在 Wayland 的终端）里的输入不会产生打字动画；\
         XWayland 下运行的窗口不受影响。",
    );
    note(
        &input_page,
        "hyp 快速聊天需要拦截并注入按键，XRecord 无法消费事件，因此 Linux 版暂不提供（设置项保留，切回其他平台仍然有效）。",
    );
    let preview = gtk::Button::with_label("预览一次挥臂");
    preview.set_halign(gtk::Align::Start);
    let weak_preview = weak.clone();
    preview.connect_clicked(move |_| {
        if let Some(rc) = weak_preview.upgrade() {
            rc.borrow_mut().swing();
        }
    });
    input_page.pack_start(&preview, false, false, 4);
    let health = gtk::Label::new(Some("正在连接 XRecord…"));
    health.set_xalign(0.0);
    health.set_line_wrap(true);
    input_page.pack_start(&health, false, false, 4);
    note(
        &input_page,
        "只用于角色动作，不保存或上传输入内容；按键仅影响手臂动画与下压幅度。",
    );

    // ---- 视角 ----
    let head = page(&notebook, "视角");
    let items: [(&str, &str, f64, f64); 9] = [
        ("headSensitivityX", "水平灵敏度 %", 0.0, 300.0),
        ("headSensitivityY", "垂直灵敏度 %", 0.0, 300.0),
        ("headRangeX", "水平范围 °", 0.0, 85.0),
        ("headRangeY", "垂直范围 °", 0.0, 85.0),
        ("headCenterX", "水平中心 °", -60.0, 60.0),
        ("headCenterY", "垂直中心 °", -60.0, 60.0),
        ("modelRotation.0", "整体旋转 X °", -180.0, 180.0),
        ("modelRotation.1", "整体旋转 Y °", -180.0, 180.0),
        ("modelRotation.2", "整体旋转 Z °", -180.0, 180.0),
    ];
    for (key, title, min, max) in items {
        add_slider(&head, &weak, initial, key, title, min, max, 0.5);
    }
    note(&head, "头部跟随以全局鼠标位置为准，支持多个显示器。");

    // ---- 自定义姿态 ----
    let pose = page(&notebook, "自定义姿态");
    add_combo(
        &pose,
        &weak,
        initial,
        "customBase",
        "自定义基础姿态",
        &[("standing", "站姿"), ("sitting", "坐姿")],
        false,
        true,
    );
    note(
        &pose,
        "在“基本”页把姿态设为“自定义”后生效。站姿与坐姿分别保存，下面为各部位的 X / Y / Z 旋转角度。",
    );
    let parts: [(&str, &str); 6] = [
        ("head", "头部"),
        ("body", "身体"),
        ("leftArm", "左臂"),
        ("rightArm", "右臂"),
        ("leftLeg", "左腿"),
        ("rightLeg", "右腿"),
    ];
    for (group, expander_title) in [
        ("customPose", "自定义站姿"),
        ("customSittingPose", "自定义坐姿"),
    ] {
        let expander = gtk::Expander::new(Some(expander_title));
        expander.set_expanded(false);
        let grid = gtk::Box::new(gtk::Orientation::Vertical, 2);
        grid.set_margin_top(6);
        grid.set_margin_start(10);
        for (part, name) in parts {
            for (axis, axis_name) in ["X", "Y", "Z"].iter().enumerate() {
                add_slider(
                    &grid,
                    &weak,
                    initial,
                    &format!("{group}.{part}.{axis}"),
                    &format!("{name} {axis_name} °"),
                    -180.0,
                    180.0,
                    0.5,
                );
            }
        }
        expander.add(&grid);
        pose.pack_start(&expander, false, false, 2);
    }

    // ---- 皮肤与关于 ----
    let skins = page(&notebook, "皮肤与关于");
    add_check(
        &skins,
        &weak,
        initial,
        "showCape",
        "显示正版披风（随正版皮肤一起获取）",
        true,
    );
    let skin_summary = gtk::Label::new(None);
    skin_summary.set_xalign(0.0);
    skin_summary.set_line_wrap(true);
    skins.pack_start(&skin_summary, false, false, 4);

    let window_for_dialog = window.clone();
    let weak_import = weak.clone();
    action_button(&skins, "导入本地 PNG 皮肤…", move || {
        let Some(rc) = weak_import.upgrade() else { return };
        let dialog = gtk::FileChooserNative::new(
            Some("选择 PNG 皮肤"),
            Some(&window_for_dialog),
            gtk::FileChooserAction::Open,
            Some("导入"),
            Some("取消"),
        );
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("PNG 图片"));
        filter.add_pattern("*.png");
        dialog.add_filter(filter);
        let response = dialog.run();
        let path = dialog.filename();
        dialog.destroy();
        if response == gtk::ResponseType::Accept {
            if let Some(path) = path {
                let mut app = rc.borrow_mut();
                app.import_skin(&path);
            }
        }
    });

    let online_row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let entry = gtk::Entry::new();
    entry.set_placeholder_text(Some("Java 版用户名或 UUID"));
    entry.set_hexpand(true);
    let fetch = gtk::Button::with_label("获取皮肤与披风");
    online_row.pack_start(&entry, true, true, 0);
    online_row.pack_start(&fetch, false, false, 0);
    skins.pack_start(&online_row, false, false, 0);

    let online_status = gtk::Label::new(None);
    online_status.set_xalign(0.0);
    online_status.set_line_wrap(true);
    skins.pack_start(&online_status, false, false, 4);

    let data_dir_owned = data_dir.to_path_buf();
    action_button(&skins, "打开设置文件夹", move || {
        platform::open_path(&data_dir_owned)
    });

    let weak_reset = weak.clone();
    action_button(&skins, "恢复默认设置和内置皮肤", move || {
        if let Some(rc) = weak_reset.upgrade() {
            rc.borrow_mut().reset_settings();
        }
    });

    action_button(&skins, "打开项目主页", || {
        platform::open_url("https://github.com/DLmember/Minecraft-Desktop-Pet")
    });

    note(
        &skins,
        &format!(
            "Minecraft桌宠 v{} · Linux\n独立粉丝作品，与 Mojang Studios / Microsoft 无关联。原项目 MIT 许可。",
            platform::version()
        ),
    );

    // ---- 启动与性能 ----
    let startup = page(&notebook, "启动与性能");
    add_check(
        &startup,
        &weak,
        initial,
        "autostart",
        "登录桌面后自动启动",
        true,
    );
    add_combo(
        &startup,
        &weak,
        initial,
        "autostartDelay",
        "登录启动延迟",
        &[
            ("0", "立即"),
            ("5", "5 秒"),
            ("15", "15 秒"),
            ("30", "30 秒"),
            ("60", "60 秒"),
            ("120", "120 秒"),
        ],
        true,
        true,
    );
    add_combo(
        &startup,
        &weak,
        initial,
        "startupPriority",
        "渲染进程优先级",
        &[
            ("idle", "空闲（最低）"),
            ("below", "较低"),
            ("normal", "正常（默认）"),
            ("above", "较高（需要权限）"),
            ("high", "高（需要权限）"),
        ],
        false,
        true,
    );
    let startup_status = gtk::Label::new(None);
    startup_status.set_xalign(0.0);
    startup_status.set_line_wrap(true);
    startup.pack_start(&startup_status, false, false, 4);
    note(
        &startup,
        "登录启动通过 ~/.config/autostart 下的 .desktop 项实现；延迟只影响登录启动，手动启动立即显示。\
         优先级映射为 nice 值，负值（较高/高）需要 CAP_SYS_NICE 或 root，失败时会提示并保持原优先级。",
    );

    let hint = gtk::Label::new(Some(
        "关闭窗口即隐藏到托盘，单击托盘图标可重新打开；穿透时也可用 Ctrl+Alt+P。",
    ));
    hint.set_xalign(0.0);
    hint.style_context().add_class("dim-label");
    root.pack_start(&hint, false, false, 0);

    window.add(&root);

    let console = Console {
        window,
        health,
        skin_summary,
        online_status,
        startup_status,
        pending: Rc::new(RefCell::new(None)),
    };

    // 在线获取：工作线程下载，主线程轮询结果。
    let entry_for_fetch = entry.clone();
    let weak_fetch = weak.clone();
    let pending = console.pending.clone();
    fetch.connect_clicked(move |_| {
        let Some(rc) = weak_fetch.upgrade() else { return };
        let text = entry_for_fetch.text().to_string();
        let text = text.trim().to_string();
        if !crate::online::valid_name(&text) && crate::online::compact_uuid(&text).is_none() {
            let mut app = rc.borrow_mut();
            app.online_status = "请输入有效的 Java 版用户名或 UUID。".into();
            app.refresh_console();
            return;
        }
        {
            let mut app = rc.borrow_mut();
            app.online_status = "正在连接官方皮肤服务…".into();
            app.refresh_console();
        }
        let (tx, rx) = std::sync::mpsc::channel();
        *pending.borrow_mut() = Some(rx);
        let user_agent = format!("MinecraftDesktopPet/{}-Linux", platform::version());
        std::thread::spawn(move || {
            let _ = tx.send(crate::online::fetch(&text, &user_agent));
        });
        let poll_pending = pending.clone();
        let poll_weak = weak_fetch.clone();
        glib::timeout_add_local(Duration::from_millis(120), move || {
            let result = {
                let mut slot = poll_pending.borrow_mut();
                match slot.as_ref().map(|rx| rx.try_recv()) {
                    Some(Ok(result)) => {
                        *slot = None;
                        Some(result)
                    }
                    Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                        *slot = None;
                        Some(Err("官方皮肤服务连接中断。".to_string()))
                    }
                    _ => None,
                }
            };
            let Some(result) = result else {
                return glib::ControlFlow::Continue;
            };
            if let Some(rc) = poll_weak.upgrade() {
                let mut app = rc.borrow_mut();
                match result {
                    Ok(fetched) => {
                        let name = fetched.name.clone();
                        let has_cape = fetched.cape.is_some();
                        match app.apply_fetched(fetched) {
                            Ok(()) => {
                                app.online_status = format!(
                                    "已载入：{name} · {}",
                                    if has_cape {
                                        "皮肤与披风已保存"
                                    } else {
                                        "该账号未装备披风"
                                    }
                                )
                            }
                            Err(e) => {
                                app.online_status = format!("获取失败，保留原皮肤与披风：{e}")
                            }
                        }
                    }
                    Err(e) => app.online_status = format!("获取失败，保留原皮肤与披风：{e}"),
                }
                app.refresh_console();
            }
            glib::ControlFlow::Break
        });
    });

    // 标签由 App::open_console 在 build 之后立即刷新（此处 App 正被可变借用）。
    console
}
