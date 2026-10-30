//! 平台相关的小工具：设置目录、进程优先级、打开文件管理器。

use std::path::{Path, PathBuf};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// $XDG_DATA_HOME/MinecraftDesktopPet，默认 ~/.local/share/MinecraftDesktopPet
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(dir).join("MinecraftDesktopPet");
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join(".local/share/MinecraftDesktopPet")
}

/// 五档优先级映射到 nice 值；负值需要 CAP_SYS_NICE，失败时保持默认。
pub fn apply_priority(value: &str) -> Result<(), String> {
    let nice = match value {
        "idle" => 19,
        "below" => 10,
        "above" => -5,
        "high" => -10,
        // 默认档位不调用 setpriority：保留登录会话本来就有的调度优先级。
        _ => return Ok(()),
    };
    let rc = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, nice) };
    if rc != 0 {
        if nice < 0 {
            return Err(format!(
                "无法把渲染优先级设为 {value}（nice {nice} 需要 CAP_SYS_NICE），已保持当前调度优先级。"
            ));
        }
        return Err(format!(
            "无法调整进程优先级：{}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

pub fn open_path(path: &Path) {
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}

pub fn open_url(url: &str) {
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}
