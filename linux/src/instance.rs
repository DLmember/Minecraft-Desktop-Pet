//! 单实例：第二次启动时只通知已有实例打开控制台，避免出现两只桌宠。

use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

pub struct Single {
    pub path: PathBuf,
    pub rx: Receiver<()>,
}

pub enum Claim {
    /// 本进程持有实例锁。
    Primary(Single),
    /// 已有实例在运行，并已请求它打开控制台。
    Secondary,
}

fn socket_path(data_dir: &Path) -> PathBuf {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data_dir.hash(&mut hasher);
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(format!("minecraft-pet-{}-{:x}.sock", unsafe { libc::getuid() }, hasher.finish()))
}

pub fn claim(data_dir: &Path) -> Result<Claim, String> {
    let path = socket_path(data_dir);
    if let Ok(mut stream) = UnixStream::connect(&path) {
        let _ = stream.write_all(b"open\n");
        return Ok(Claim::Secondary);
    }
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)
        .map_err(|e| format!("无法创建单实例套接字 {}：{e}", path.display()))?;
    if let Ok(meta) = std::fs::metadata(&path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = std::fs::set_permissions(&path, perms);
    }
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("instance".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut buffer = [0u8; 32];
                let n = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..n]);
                if request.trim() == "open" && tx.send(()).is_err() {
                    return;
                }
            }
        })
        .ok();
    Ok(Claim::Primary(Single { path, rx }))
}
