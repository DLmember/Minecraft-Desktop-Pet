//! 系统托盘（StatusNotifierItem，KDE/GNOME AppIndicator 扩展均支持）。
//! 对应 Windows 版 Shell_NotifyIcon：左键打开控制台，右键弹出菜单；
//! 关闭控制台只是隐藏到托盘，程序继续运行。

use std::sync::mpsc::{channel, Receiver, Sender};

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, StandardItem};
use ksni::{Icon, MenuItem, ToolTip};

#[derive(Clone, Copy, Debug)]
pub enum TrayCmd {
    Console,
    ClickThrough(bool),
    Above(bool),
    ToggleHidden,
    Recenter,
    ImportSkin,
    Quit,
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct TrayState {
    pub click_through: bool,
    pub above: bool,
    pub hidden: bool,
}

struct PetTray {
    tx: Sender<TrayCmd>,
    state: TrayState,
    icons: Vec<Icon>,
    version: String,
}

fn load_icons() -> Vec<Icon> {
    let Ok(img) = image::load_from_memory(include_bytes!("../packaging/minecraft-desktop-pet.png"))
    else {
        return Vec::new();
    };
    [22u32, 32, 48, 64, 128]
        .iter()
        .map(|&size| {
            let rgba = img
                .resize_exact(size, size, image::imageops::FilterType::Lanczos3)
                .into_rgba8();
            // StatusNotifierItem 要求 ARGB32、网络字节序
            let data = rgba
                .pixels()
                .flat_map(|p| [p[3], p[0], p[1], p[2]])
                .collect();
            Icon { width: size as i32, height: size as i32, data }
        })
        .collect()
}

impl PetTray {
    fn send(&self, cmd: TrayCmd) {
        let _ = self.tx.send(cmd);
    }
}

impl ksni::Tray for PetTray {
    fn id(&self) -> String {
        "minecraft-desktop-pet".into()
    }

    fn title(&self) -> String {
        "Minecraft桌宠".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ToolTip {
        ToolTip {
            title: format!("Minecraft桌宠 v{}", self.version),
            description: "左键打开控制台 / 右键菜单".into(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayCmd::Console);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let s = self.state;
        vec![
            StandardItem {
                label: "打开控制台…".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayCmd::Console)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: "完全鼠标穿透".into(),
                checked: s.click_through,
                activate: Box::new(|t: &mut Self| {
                    t.state.click_through = !t.state.click_through;
                    t.send(TrayCmd::ClickThrough(t.state.click_through));
                }),
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: "保持置顶".into(),
                checked: s.above,
                activate: Box::new(|t: &mut Self| {
                    t.state.above = !t.state.above;
                    t.send(TrayCmd::Above(t.state.above));
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: if s.hidden { "显示桌宠" } else { "暂时隐藏桌宠" }.into(),
                activate: Box::new(|t: &mut Self| {
                    t.state.hidden = !t.state.hidden;
                    t.send(TrayCmd::ToggleHidden);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "移到屏幕中央".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayCmd::Recenter)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "导入 PNG 皮肤…".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayCmd::ImportSkin)),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "退出 Minecraft桌宠".into(),
                activate: Box::new(|t: &mut Self| t.send(TrayCmd::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub struct Tray {
    handle: Handle<PetTray>,
    rx: Receiver<TrayCmd>,
    state: TrayState,
}

impl Tray {
    /// 没有 StatusNotifierWatcher（例如未装 AppIndicator 扩展的 GNOME）时返回错误。
    pub fn start(state: TrayState, version: &str) -> Result<Self, String> {
        let (tx, rx) = channel();
        let tray = PetTray { tx, state, icons: load_icons(), version: version.into() };
        let handle = tray.spawn().map_err(|e| format!("托盘不可用：{e}"))?;
        Ok(Self { handle, rx, state })
    }

    /// 设置从别处（控制台、右键菜单）改变时同步托盘菜单的勾选状态。
    pub fn sync(&mut self, state: TrayState) {
        if state != self.state {
            self.state = state;
            self.handle.update(|t| t.state = state);
        }
    }

    pub fn poll(&self) -> Vec<TrayCmd> {
        self.rx.try_iter().collect()
    }

    pub fn alive(&self) -> bool {
        !self.handle.is_closed()
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let _ = self.handle.shutdown();
    }
}
