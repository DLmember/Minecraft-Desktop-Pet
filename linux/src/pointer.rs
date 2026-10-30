//! Wayland 会话下的全局指针位置。
//!
//! XWayland 只在指针位于某个 X11 窗口之上时才更新 `XQueryPointer` 的位置；
//! 指针移到 Wayland 原生窗口或桌面上时，查询结果会停在最后一次的值，
//! 表现为“只有靠近桌宠才跟手”。因此在 KDE 上临时加载一个 KWin 脚本，
//! 把 `workspace.cursorPos` 定时发到本进程的 D-Bus 服务，作为全局指针来源；
//! 拿不到数据（X11 会话、其它桌面、脚本被卸载）时回退到 XQueryPointer。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SERVICE: &str = "org.kde.MinecraftPet.Pointer";
const PLUGIN: &str = "minecraft-pet-pointer";
/// 超过这个时间没有收到脚本上报，就认为桥接失效（KWin 重启等）。
const FRESH: Duration = Duration::from_millis(1500);

const SCRIPT: &str = r#"
// X11 根窗口用物理像素，KWin 的 cursorPos 是逻辑坐标，需乘当前输出的缩放比。
const timer = new QTimer();
timer.interval = 33;
timer.timeout.connect(function () {
    const out = workspace.activeScreen;
    const p = workspace.cursorPos;
    const dpr = (out && out.devicePixelRatio) ? out.devicePixelRatio : 1;
    callDBus("org.kde.MinecraftPet.Pointer", "/Pointer",
             "org.kde.MinecraftPet.Pointer", "Update",
             Math.round(p.x * dpr), Math.round(p.y * dpr));
});
timer.start();
"#;

#[derive(Clone, Copy)]
struct Sample {
    x: i16,
    y: i16,
    at: Instant,
}

#[derive(Clone, Default)]
pub struct Latest(Arc<Mutex<Option<Sample>>>);

impl Latest {
    pub fn get(&self) -> Option<(i16, i16)> {
        let sample = *self.0.lock().ok()?.as_ref()?;
        (sample.at.elapsed() < FRESH).then_some((sample.x, sample.y))
    }
}

struct PointerService {
    latest: Latest,
}

#[zbus::interface(name = "org.kde.MinecraftPet.Pointer")]
impl PointerService {
    fn update(&self, x: i32, y: i32) {
        if let Ok(mut slot) = self.latest.0.lock() {
            *slot = Some(Sample {
                x: x.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                y: y.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                at: Instant::now(),
            });
        }
    }
}

pub struct Bridge {
    latest: Latest,
    conn: zbus::blocking::Connection,
    script: PathBuf,
}

impl Bridge {
    /// 只在 KDE Wayland 下有意义；失败时返回 None，调用方回退到 XQueryPointer。
    pub fn start() -> Result<Self, String> {
        if std::env::var_os("WAYLAND_DISPLAY").is_none()
            && std::env::var("XDG_SESSION_TYPE").as_deref() != Ok("wayland")
        {
            return Err("不是 Wayland 会话，直接使用 XQueryPointer".into());
        }
        let latest = Latest::default();
        let conn = zbus::blocking::connection::Builder::session()
            .map_err(|e| format!("无法连接会话总线：{e}"))?
            .name(SERVICE)
            .map_err(|e| format!("无法占用 {SERVICE}：{e}"))?
            .serve_at("/Pointer", PointerService { latest: latest.clone() })
            .map_err(|e| format!("无法导出指针服务：{e}"))?
            .build()
            .map_err(|e| format!("无法建立指针服务：{e}"))?;

        let runtime = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let script = runtime.join(format!("{PLUGIN}.js"));
        std::fs::write(&script, SCRIPT).map_err(|e| format!("无法写入 {script:?}：{e}"))?;

        let proxy = zbus::blocking::Proxy::new(
            &conn,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        )
        .map_err(|e| format!("无法连接 KWin 脚本接口：{e}"))?;
        // 上次异常退出可能残留同名脚本，先卸载再加载，避免定时器叠加
        let _: Result<bool, _> = proxy.call("unloadScript", &(PLUGIN,));
        let path = script.to_string_lossy().to_string();
        let _: i32 = proxy
            .call("loadScript", &(path.as_str(), PLUGIN))
            .map_err(|e| format!("KWin 拒绝加载脚本：{e}"))?;
        let _: () = proxy
            .call("start", &())
            .map_err(|e| format!("KWin 拒绝启动脚本：{e}"))?;

        Ok(Self { latest, conn, script })
    }

    pub fn latest(&self) -> Latest {
        self.latest.clone()
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        // 退出时卸载脚本，避免它在 KWin 里留下一个指向已消失服务的定时器
        if let Ok(proxy) = zbus::blocking::Proxy::new(
            &self.conn,
            "org.kde.KWin",
            "/Scripting",
            "org.kde.kwin.Scripting",
        ) {
            let _: Result<bool, _> = proxy.call("unloadScript", &(PLUGIN,));
        }
        let _ = std::fs::remove_file(&self.script);
    }
}
