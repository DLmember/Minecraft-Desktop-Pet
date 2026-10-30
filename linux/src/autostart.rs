//! XDG 登录自启：~/.config/autostart/minecraft-desktop-pet.desktop

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const FILE_NAME: &str = "minecraft-desktop-pet.desktop";

fn autostart_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(dir).join("autostart");
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join(".config/autostart")
}

fn entry_path() -> PathBuf {
    autostart_dir().join(FILE_NAME)
}

fn exec_quote(text: &str) -> String {
    // Desktop Entry 规范：双引号内的 " ` $ \ 需要转义。
    let mut out = String::from("\"");
    for ch in text.chars() {
        if matches!(ch, '"' | '`' | '$' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

fn entry_body(exe: &Path, data_dir: &Path) -> String {
    let mut body = String::new();
    body.push_str("[Desktop Entry]\n");
    body.push_str("Type=Application\n");
    body.push_str("Version=1.0\n");
    body.push_str("Name=Minecraft桌宠\n");
    body.push_str("Comment=Minecraft 风格的桌面宠物\n");
    let _ = writeln!(
        body,
        "Exec={} --autostart --data-dir {}",
        exec_quote(&exe.to_string_lossy()),
        exec_quote(&data_dir.to_string_lossy())
    );
    body.push_str("Terminal=false\n");
    body.push_str("X-GNOME-Autostart-enabled=true\n");
    body.push_str("StartupNotify=false\n");
    body.push_str("Categories=Utility;\n");
    body
}

pub fn is_enabled() -> bool {
    entry_path().exists()
}

pub fn set_enabled(enabled: bool, exe: &Path, data_dir: &Path) -> Result<(), String> {
    let path = entry_path();
    if !enabled {
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("无法删除自启项：{e}"))?;
        }
        return Ok(());
    }
    let dir = autostart_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("无法创建 {}：{e}", dir.display()))?;
    let body = entry_body(exe, data_dir);
    if std::fs::read_to_string(&path).ok().as_deref() == Some(body.as_str()) {
        return Ok(());
    }
    std::fs::write(&path, body).map_err(|e| format!("无法写入自启项：{e}"))
}

pub fn exe_path() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("minecraft-desktop-pet"))
}
