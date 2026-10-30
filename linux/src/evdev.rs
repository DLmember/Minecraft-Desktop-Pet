//! 备选全局键盘来源：直接读取 /dev/input/event*（需要当前用户在 input 组）。
//! Wayland 原生窗口的按键不经过 XWayland，XRecord 看不到；evdev 在内核层读取，
//! 不受合成器限制。只读、不抓取设备，其他程序照常收到按键。

use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::input::Msg;

const EV_KEY: u16 = 1;
const KEY_A: usize = 30;
const KEY_Z: usize = 44;
const KEY_SPACE: usize = 57;

fn ioc_read(nr: u64, len: usize) -> libc::c_ulong {
    ((2u64 << 30) | ((len as u64) << 16) | ((b'E' as u64) << 8) | nr) as libc::c_ulong
}

fn bit(bits: &[u8], n: usize) -> bool {
    bits.get(n / 8).is_some_and(|b| b & (1 << (n % 8)) != 0)
}

/// 有字母键和空格的才算键盘（排除电源键、耳机按键、鼠标等）。
fn is_keyboard(fd: i32) -> bool {
    let mut types = [0u8; 4];
    let mut keys = [0u8; 96];
    unsafe {
        if libc::ioctl(fd, ioc_read(0x20, types.len()), types.as_mut_ptr()) < 0
            || !bit(&types, EV_KEY as usize)
        {
            return false;
        }
        if libc::ioctl(fd, ioc_read(0x20 + EV_KEY as u64, keys.len()), keys.as_mut_ptr()) < 0 {
            return false;
        }
    }
    bit(&keys, KEY_A) && bit(&keys, KEY_Z) && bit(&keys, KEY_SPACE)
}

struct Device {
    fd: i32,
    held: HashSet<u8>,
}

impl Drop for Device {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

/// evdev 键码 + 8 = X11 键码（XWayland/evdev 键盘模型），与 XRecord 路径共用映射。
fn x11_code(code: u16) -> Option<u8> {
    (code < 248).then(|| (code + 8) as u8)
}
fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// 扫描 /dev/input：打开新出现的键盘，记住非键盘设备避免反复打开。
fn scan(devices: &mut HashMap<PathBuf, Device>, ignored: &mut HashSet<PathBuf>) -> usize {
    ignored.retain(|p| p.exists());
    let mut denied = 0;
    let Ok(dir) = std::fs::read_dir("/dev/input") else { return 0 };
    for entry in dir.flatten() {
        let path = entry.path();
        let is_event = path
            .file_name()
            .is_some_and(|n| n.as_bytes().starts_with(b"event"));
        if !is_event || devices.contains_key(&path) || ignored.contains(&path) {
            continue;
        }
        let Ok(c) = CString::new(path.as_os_str().as_bytes()) else { continue };
        let fd = unsafe {
            libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK | libc::O_CLOEXEC)
        };
        if fd < 0 {
            match errno() {
                libc::EACCES | libc::EPERM => denied += 1,
                _ => {
                    ignored.insert(path);
                }
            }
            continue;
        }
        if is_keyboard(fd) {
            devices.insert(path, Device { fd, held: HashSet::new() });
        } else {
            unsafe { libc::close(fd) };
            ignored.insert(path);
        }
    }
    denied
}

fn status(devices: usize, denied: usize) -> Result<usize, String> {
    if devices > 0 {
        Ok(devices)
    } else if denied > 0 {
        Err("evdev 未启用：无权读取 /dev/input（sudo usermod -aG input $USER 后重新登录）".into())
    } else {
        Err("evdev 未启用：没有找到键盘设备".into())
    }
}

/// 读完一个设备当前所有事件；返回 false 表示设备已拔出或出错。
fn drain(device: &mut Device, tx: &Sender<Msg>) -> bool {
    const SIZE: usize = std::mem::size_of::<libc::input_event>();
    let mut buf = [0u8; SIZE * 64];
    loop {
        let n = unsafe { libc::read(device.fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n < 0 {
            let e = errno();
            return e == libc::EAGAIN || e == libc::EINTR;
        }
        if n == 0 {
            return false;
        }
        for chunk in buf[..n as usize].chunks_exact(SIZE) {
            let ev: libc::input_event = unsafe { std::ptr::read_unaligned(chunk.as_ptr().cast()) };
            if ev.type_ != EV_KEY || ev.code >= 0x100 {
                continue; // 只要键盘键；鼠标按键（0x110+）仍由 XRecord 负责
            }
            let Some(code) = x11_code(ev.code) else { continue };
            // 0 松开、1 按下、2 自动重复（与 Windows 低级钩子一样当作按下转发）
            let down = ev.value != 0;
            if down {
                device.held.insert(code);
            } else {
                device.held.remove(&code);
            }
            let _ = tx.send(Msg::EvdevKey { code, down });
        }
    }
}

pub fn run(tx: Sender<Msg>, stop: Arc<AtomicBool>) {
    let mut devices: HashMap<PathBuf, Device> = HashMap::new();
    let mut ignored = HashSet::new();
    let mut next_scan = Instant::now();
    let mut reported: Option<Result<usize, String>> = None;
    while !stop.load(Ordering::Relaxed) {
        if Instant::now() >= next_scan {
            let denied = scan(&mut devices, &mut ignored);
            next_scan = Instant::now() + Duration::from_secs(2);
            let now = status(devices.len(), denied);
            if reported.as_ref() != Some(&now) {
                let _ = tx.send(Msg::EvdevReady(now.clone()));
                reported = Some(now);
            }
        }
        if devices.is_empty() {
            std::thread::sleep(Duration::from_millis(200));
            continue;
        }
        let paths: Vec<PathBuf> = devices.keys().cloned().collect();
        let mut fds: Vec<libc::pollfd> = paths
            .iter()
            .map(|p| libc::pollfd { fd: devices[p].fd, events: libc::POLLIN, revents: 0 })
            .collect();
        let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 200) };
        if ready <= 0 {
            continue;
        }
        for (path, pfd) in paths.iter().zip(&fds) {
            if pfd.revents == 0 {
                continue;
            }
            let alive = pfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) == 0
                && devices.get_mut(path).is_some_and(|d| drain(d, &tx));
            if !alive {
                // 拔出键盘时补发松开，避免人物一直按着键
                if let Some(device) = devices.remove(path) {
                    for &code in &device.held {
                        let _ = tx.send(Msg::EvdevKey { code, down: false });
                    }
                }
                next_scan = Instant::now();
            }
        }
    }
}
