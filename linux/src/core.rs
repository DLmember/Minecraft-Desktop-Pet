//! 对 native crate 的 C ABI 做一层安全封装。
//! 渲染与动画全部来自原始 Rust 核心，Linux 前端只负责窗口、输入与设置界面。

use serde_json::Value;
use std::ffi::{CStr, CString};
use std::path::Path;

pub const FRAME_W: usize = 450;
pub const FRAME_H: usize = 525;

pub struct Core {
    raw: *mut pet_core::PetCore,
}

// 只在 GTK 主线程使用；显式声明避免误传给工作线程。
impl Core {
    pub fn new(dir: &Path) -> Result<Self, String> {
        let c = CString::new(dir.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        let raw = unsafe { pet_core::pet_create(c.as_ptr()) };
        if raw.is_null() {
            Err(format!(
                "无法读取设置或创建设置目录：{}",
                dir.display()
            ))
        } else {
            Ok(Self { raw })
        }
    }

    pub fn settings(&self) -> Option<Value> {
        unsafe {
            let p = pet_core::pet_settings(self.raw);
            if p.is_null() {
                None
            } else {
                serde_json::from_str(CStr::from_ptr(p).to_str().ok()?).ok()
            }
        }
    }

    pub fn error(&self) -> String {
        unsafe {
            let p = pet_core::pet_error(self.raw);
            if p.is_null() {
                String::new()
            } else {
                CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        }
    }

    fn call(
        &self,
        value: &Value,
        f: unsafe extern "C" fn(*mut pet_core::PetCore, *const std::ffi::c_char) -> bool,
    ) -> bool {
        let Ok(json) = CString::new(value.to_string()) else {
            return false;
        };
        unsafe { f(self.raw, json.as_ptr()) }
    }

    pub fn preview(&self, value: &Value) -> bool {
        self.call(value, pet_core::pet_preview)
    }

    pub fn apply(&self, value: &Value) -> bool {
        self.call(value, pet_core::pet_apply)
    }

    pub fn reset(&self) -> bool {
        unsafe { pet_core::pet_reset(self.raw) }
    }

    pub fn key(&mut self, key: u32, down: bool, now: f32) {
        unsafe { pet_core::pet_key(self.raw, key, down, now) }
    }

    pub fn clear_keys(&mut self) {
        unsafe { pet_core::pet_clear_keys(self.raw) }
    }

    pub fn click(&mut self, now: f32) {
        unsafe { pet_core::pet_click(self.raw, now) }
    }

    /// 渲染一帧并拷贝到 `out`（RGBA，预乘 alpha，450×525）。
    pub fn render_into(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        shift: bool,
        now: f32,
        dt: f32,
        out: &mut [u8],
    ) -> bool {
        let p = unsafe { pet_core::pet_render(self.raw, x, y, width, height, shift, now, dt) };
        if p.is_null() || out.len() < FRAME_W * FRAME_H * 4 {
            return false;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(p, out.as_mut_ptr(), FRAME_W * FRAME_H * 4);
        }
        true
    }

    pub fn hit(&self, x: f32, y: f32) -> bool {
        unsafe { pet_core::pet_hit(self.raw, x, y) }
    }

    pub fn import(&mut self, skin: &Path) -> bool {
        self.import_profile(skin, None, None, None)
    }

    pub fn import_profile(
        &mut self,
        skin: &Path,
        cape: Option<&Path>,
        name: Option<&str>,
        model: Option<&str>,
    ) -> bool {
        let skin = CString::new(skin.to_string_lossy().as_bytes()).unwrap_or_default();
        let to_c = |p: Option<&Path>| {
            p.and_then(|p| CString::new(p.to_string_lossy().as_bytes()).ok())
        };
        let cape = to_c(cape);
        let name_c = name.and_then(|s| CString::new(s.as_bytes()).ok());
        let model_c = model.and_then(|s| CString::new(s.as_bytes()).ok());
        unsafe {
            pet_core::pet_import_profile(
                self.raw,
                skin.as_ptr(),
                cape.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
                name_c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
                model_c.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            )
        }
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        unsafe { pet_core::pet_destroy(self.raw) }
    }
}
