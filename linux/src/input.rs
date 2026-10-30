//! 全局输入：XRecord 被动监听所有客户端的按键/按键释放（等同 Windows 低级钩子、
//! macOS CGEventTap 的只读模式）。不需要 root，也不需要抢焦点。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use x11rb::connection::Connection;
use x11rb::protocol::record::{self, ConnectionExt as RecordExt};

#[derive(Clone, Debug)]
pub enum InputEvent {
    Key { code: u8, down: bool },
    Button,
}

pub enum Msg {
    Ready(Result<(), String>),
    Key { code: u8, down: bool },
    Button,
    EvdevReady(Result<usize, String>),
    EvdevKey { code: u8, down: bool },
}

pub struct Input {
    rx: Receiver<Msg>,
    stop: Arc<AtomicBool>,
    pub ready: bool,
    pub status: String,
    /// evdev 打开了至少一个键盘：键盘事件以它为准，XRecord 只负责鼠标按键。
    pub evdev: bool,
    pub evdev_status: String,
    evdev_held: std::collections::HashSet<u8>,
    pub key_downs: u64,
    pub key_ups: u64,
    pub clicks: u64,
}

impl Input {
    pub fn start() -> Self {
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let evdev_tx = tx.clone();
        std::thread::Builder::new()
            .name("xrecord".into())
            .spawn(move || run(tx, flag))
            .ok();
        let flag = stop.clone();
        std::thread::Builder::new()
            .name("evdev".into())
            .spawn(move || crate::evdev::run(evdev_tx, flag))
            .ok();
        Self {
            rx,
            stop,
            ready: false,
            status: "正在连接 XRecord…".into(),
            evdev: false,
            evdev_status: "正在检查 /dev/input…".into(),
            evdev_held: Default::default(),
            key_downs: 0,
            key_ups: 0,
            clicks: 0,
        }
    }

    /// evdev 启用时用它自己的按住集合核对 key-up；否则返回 None，由 XQueryKeymap 核对。
    pub fn evdev_down(&self, code: u8) -> Option<bool> {
        self.evdev.then(|| self.evdev_held.contains(&code))
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn poll(&mut self) -> Vec<InputEvent> {
        let mut out = Vec::new();
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Ready(Ok(())) => {
                    self.ready = true;
                    self.status = "XRecord 已连接：全局按键与点击监听中".into();
                }
                Msg::Ready(Err(e)) => {
                    self.ready = false;
                    self.status = e;
                }
                Msg::EvdevReady(Ok(n)) => {
                    self.evdev = true;
                    self.evdev_status = format!("evdev 已启用：读取 {n} 个键盘（含 Wayland 原生窗口）");
                }
                Msg::EvdevReady(Err(e)) => {
                    // 键盘全部拔出：补发松开后回退到 XRecord
                    for code in self.evdev_held.drain() {
                        out.push(InputEvent::Key { code, down: false });
                    }
                    self.evdev = false;
                    self.evdev_status = e;
                }
                Msg::Key { .. } if self.evdev => {}
                Msg::EvdevKey { code, down } | Msg::Key { code, down } => {
                    if self.evdev {
                        if down {
                            self.evdev_held.insert(code);
                        } else {
                            self.evdev_held.remove(&code);
                        }
                    }
                    if down {
                        self.key_downs += 1;
                    } else {
                        self.key_ups += 1;
                    }
                    out.push(InputEvent::Key { code, down });
                }
                Msg::Button => {
                    self.clicks += 1;
                    out.push(InputEvent::Button);
                }
            }
        }
        out
    }
}

impl Drop for Input {
    fn drop(&mut self) {
        self.stop();
    }
}

fn range(first: u8, last: u8) -> record::Range8 {
    record::Range8 { first, last }
}

fn ext(first: u16, last: u16) -> record::ExtRange {
    record::ExtRange {
        major: range(0, 0),
        minor: record::Range16 { first, last },
    }
}

fn run(tx: Sender<Msg>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match listen(&tx, &stop) {
            Ok(()) => {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
            }
            Err(e) => {
                let _ = tx.send(Msg::Ready(Err(e)));
            }
        }
        for _ in 0..50 {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn listen(tx: &Sender<Msg>, stop: &Arc<AtomicBool>) -> Result<(), String> {
    let (conn, _) = x11rb::connect(None).map_err(|e| format!("无法连接 X11：{e}"))?;
    conn.record_query_version(1, 13)
        .map_err(|e| format!("XRecord 查询失败：{e}"))?
        .reply()
        .map_err(|_| "X 服务器未提供 Record 扩展（无法监听全局键盘）".to_string())?;

    let ctx = conn.generate_id().map_err(|e| e.to_string())?;
    let r = record::Range {
        core_requests: range(0, 0),
        core_replies: range(0, 0),
        ext_requests: ext(0, 0),
        ext_replies: ext(0, 0),
        delivered_events: range(0, 0),
        // KeyPress(2) .. ButtonRelease(5)
        device_events: range(2, 5),
        errors: range(0, 0),
        client_started: false,
        client_died: false,
    };
    // 空的 client_specs = 记录所有客户端。
    conn.record_create_context(ctx, 0, &[], &[r])
        .map_err(|e| format!("XRecord 创建上下文失败：{e}"))?;
    conn.flush().map_err(|e| e.to_string())?;

    let cookie = conn
        .record_enable_context(ctx)
        .map_err(|e| format!("XRecord 启用失败：{e}"))?;
    let _ = tx.send(Msg::Ready(Ok(())));

    let mut last: Option<(u8, u8, u32)> = None;
    for reply in cookie {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let reply = match reply {
            Ok(r) => r,
            Err(_) => break,
        };
        // 0 = FromServer, 1 = FromClient；其余为控制类别。
        if reply.category > 1 || reply.client_swapped {
            continue;
        }
        for chunk in reply.data.chunks_exact(32) {
            let kind = chunk[0] & 0x7f;
            let detail = chunk[1];
            let time = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            if last == Some((kind, detail, time)) {
                continue;
            }
            last = Some((kind, detail, time));
            match kind {
                2 => {
                    let _ = tx.send(Msg::Key {
                        code: detail,
                        down: true,
                    });
                }
                3 => {
                    let _ = tx.send(Msg::Key {
                        code: detail,
                        down: false,
                    });
                }
                // 只算真实按键（左/中/右/侧键）；4–7 是滚轮，Windows 版轮询按键也不会把滚动当点击
                4 if matches!(detail, 1..=3 | 8 | 9) => {
                    let _ = tx.send(Msg::Button);
                }
                _ => {}
            }
        }
    }
    Ok(())
}
