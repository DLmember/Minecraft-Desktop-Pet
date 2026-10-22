//! 随 Windows 登录自动启动。
//!
//! 自启写入当前用户的 HKCU\Software\Microsoft\Windows\CurrentVersion\Run，
//! 不需要管理员权限，也不依赖启动文件夹快捷方式。注册表里保存的是
//! “可执行文件 + --autostart”，因此程序被移动或换名字后，下次启动会
//! 通过 sync 自动改写为新的路径。
use std::path::{Path, PathBuf};

pub const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
pub const RUN_VALUE: &str = "MinecraftDesktopPetBlockbench";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn default_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|p| PathBuf::from(p).join("minecraft-desktop-pet-blockbench"))
}

/// 登录时执行的命令行：`"exe" --autostart [--user-data-dir="..."]`。
///
/// 只有使用 --user-data-dir 指定了非默认目录时才附加该参数，避免自启后
/// 读到另一份设置。路径统一加引号，兼容含空格或中文的目录。
pub fn command_line(user_dir: &Path) -> String {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("Minecraft桌宠.exe"));
    let mut line = format!("\"{}\" --autostart", exe.display());
    if default_dir().as_deref() != Some(user_dir) {
        line.push_str(&format!(" --user-data-dir=\"{}\"", user_dir.display()));
    }
    line
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegQueryValueExW, RegSetValueExW, HKEY,
            HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
        },
    };

    const ERROR_FILE_NOT_FOUND: u32 = 2;

    unsafe fn open_key(write: bool) -> Result<HKEY, String> {
        let mut key: HKEY = null_mut();
        let mut disposition = 0u32;
        let access = if write {
            KEY_SET_VALUE | KEY_QUERY_VALUE
        } else {
            KEY_QUERY_VALUE
        };
        let rc = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(RUN_KEY).as_ptr(),
            0,
            null(),
            REG_OPTION_NON_VOLATILE,
            access,
            null(),
            &mut key,
            &mut disposition,
        );
        if rc != ERROR_SUCCESS {
            return Err(format!("无法打开自启注册表项（错误 {rc}）"));
        }
        Ok(key)
    }

    /// 注册表里是否已经有本程序的自启项（不看内容，只判断存在）。
    pub fn is_registered() -> bool {
        unsafe {
            let Ok(key) = open_key(false) else {
                return false;
            };
            let rc = RegQueryValueExW(
                key,
                wide(RUN_VALUE).as_ptr(),
                null(),
                null_mut(),
                null_mut(),
                null_mut(),
            );
            RegCloseKey(key);
            rc == ERROR_SUCCESS
        }
    }

    /// 写入或删除自启项。enabled 为真时总是重写为当前路径与数据目录。
    pub fn set_enabled(enabled: bool, user_dir: &Path) -> Result<(), String> {
        unsafe {
            let key = open_key(true)?;
            let result = if enabled {
                let data: Vec<u16> = command_line(user_dir)
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                let rc = RegSetValueExW(
                    key,
                    wide(RUN_VALUE).as_ptr(),
                    0,
                    REG_SZ,
                    data.as_ptr() as *const u8,
                    (data.len() * 2) as u32,
                );
                if rc == ERROR_SUCCESS {
                    Ok(())
                } else {
                    Err(format!("写入开机自启失败（错误 {rc}）"))
                }
            } else {
                let rc = RegDeleteValueW(key, wide(RUN_VALUE).as_ptr());
                if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND {
                    Ok(())
                } else {
                    Err(format!("取消开机自启失败（错误 {rc}）"))
                }
            };
            RegCloseKey(key);
            result
        }
    }

    /// 启动时对齐注册表：开启则刷新路径，关闭则清掉可能残留的旧项。
    pub fn sync(enabled: bool, user_dir: &Path) {
        if enabled {
            let _ = set_enabled(true, user_dir);
        } else if is_registered() {
            let _ = set_enabled(false, user_dir);
        }
    }
}

#[cfg(windows)]
pub use win::{set_enabled, sync};
